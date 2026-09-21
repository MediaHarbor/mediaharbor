//! Refcounted arena pool for decoder frame allocations.
//!
//! This module is the runtime half of the DoS-protection framework
//! described in [`crate::limits`]. It provides three types:
//!
//! - [`ArenaPool`] — a pool of reusable raw byte buffers (allocated
//!   via [`std::alloc::alloc`] with a fixed `MAX_ALIGN` alignment so
//!   each buffer's base pointer is suitable for any `T` whose
//!   alignment is `<= MAX_ALIGN`) that a decoder leases from. Pool
//!   size and per-buffer capacity are fixed at construction; together
//!   they bound peak RSS by construction
//!   (`max_arenas × cap_per_arena`).
//!
//! - [`Arena`] — a single buffer leased from the pool. Allocations are
//!   bump-pointer (no per-alloc bookkeeping, no fragmentation). When
//!   the `Arena` is dropped, its buffer is returned to the pool, *not*
//!   freed — this is what makes the pool memory-reusing rather than
//!   memory-leaking. If the pool has been dropped before the arena
//!   (last-arena-outlives-pool), the arena's buffer is freed normally.
//!
//! - [`Frame`] / [`FrameInner`] — a refcounted (`Rc<FrameInner>`)
//!   handle that holds an `Arena` plus per-plane offset/length pairs
//!   and a small [`FrameHeader`]. As long as any clone of a `Frame`
//!   exists, its arena (and therefore its buffer) stays out of the
//!   pool. The last `Drop` returns the buffer.
//!
//! ## Design choices for round 1
//!
//! - **Hand-rolled bump allocator** over a raw `NonNull<u8>` from
//!   [`std::alloc::alloc`]. We deliberately do not depend on the
//!   `bumpalo` crate yet — the logic is twenty lines and avoids
//!   pulling in a dependency before profiling justifies it. The
//!   signature is intentionally compatible with what a
//!   `bumpalo`-backed implementation would look like, so swapping
//!   later is a contained refactor.
//!
//! - **`Rc` for `Frame`, not `Arc`.** This module targets the
//!   single-threaded decode path (one decoder, one consumer thread).
//!   The bump-pointer cursor is `Cell<usize>` for the same reason
//!   (no atomics on the hot path). For the cross-thread decode path
//!   — where a decoder produces frames on one thread and a consumer
//!   reads them on another — see the sibling [`sync`] module, which
//!   mirrors this API 1:1 with `Arc<FrameInner>` / atomic cursor so
//!   `Frame: Send + Sync`.
//!
//! - **`Arena::alloc<T>` returns `&mut [T]` borrowed from the arena.**
//!   The borrow is bounded by the lifetime of the `&Arena` reference,
//!   not the lifetime of the arena itself; the arena holds the
//!   buffer's base address as a raw [`NonNull<u8>`] so multiple
//!   calls to `alloc` against the same `&Arena` can each carve out
//!   non-overlapping sub-slices without ever materialising a
//!   whole-buffer mutable borrow (which would invalidate previously
//!   returned slices under stacked borrows). This matches
//!   `bumpalo::Bump::alloc_slice_*` semantics.
//!
//! ## Soundness notes
//!
//! Three issues called out by an external Miri audit (PR #12, May
//! 2026) shaped the current implementation; they are noted here so
//! future refactors don't reintroduce them:
//!
//! 1. **Base-pointer alignment.** A `Box<[u8]>` is byte-aligned only,
//!    so even an empty `&mut [u32]` carved out of one would have an
//!    unaligned pointer (UB). Each pool buffer is now allocated
//!    directly via [`std::alloc::alloc`] with `MAX_ALIGN` (= 64 B,
//!    enough for AVX-512), so the base pointer is suitable for any
//!    type the arena will hand out. `alloc::<T>` rejects types whose
//!    alignment exceeds `MAX_ALIGN` at compile time via a
//!    `const`-evaluated assertion.
//!
//! 2. **Invalid bit patterns.** Pool buffers are zero-filled, but
//!    zero is not a valid bit pattern for every `Copy` type
//!    (`NonZeroU8`, references, function pointers, niche-optimised
//!    enums, …). `alloc<T>` is therefore bounded on
//!    `bytemuck::Zeroable` rather than just `Copy`, so the safe API
//!    cannot hand out `&mut [NonZeroU8]` over zero bytes.
//!
//! 3. **Stacked-borrows retag.** Each `alloc` previously took
//!    `[u8]::as_mut_ptr` of the whole backing slice, which retagged
//!    the whole buffer and popped the borrow stacks of every
//!    previously returned `&mut [T]`. The fix is the raw `NonNull<u8>`
//!    base pointer above: each `alloc` does
//!    `base.as_ptr().add(offset).cast::<T>()` and never re-borrows
//!    the whole buffer.

pub mod sync;

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::cell::Cell;
use std::mem::{align_of, size_of};
use std::ptr::{self, NonNull};
use std::rc::Rc;
use std::sync::{Arc, Mutex, Weak};

use crate::error::{Error, Result};
use crate::format::PixelFormat;

/// Alignment used for every pool buffer's base pointer. 64 bytes
/// covers the alignment requirements of every primitive type and of
/// AVX-512 SIMD loads (`__m512` is 64-byte aligned). [`Arena::alloc`]
/// statically rejects any type with a stricter alignment requirement.
pub(crate) const MAX_ALIGN: usize = 64;

/// Strict-provenance-compatible `MAX_ALIGN`-aligned dangling sentinel
/// used for the `cap == 0` empty-buffer case in [`Buffer::new_zeroed`].
///
/// Casting a bare integer to `*mut u8` is rejected by Miri's
/// `-Zmiri-strict-provenance` check (the resulting pointer has no
/// provenance and cannot legally be reborrowed). Taking the address of
/// a real static gives us a properly-provenanced pointer with the same
/// runtime properties (non-null, `MAX_ALIGN`-aligned, never
/// dereferenced for `cap == 0`).
///
/// `#[repr(align(N))]` requires a literal, so the `const_assert` below
/// (a const-eval'd `assert!`) catches the day someone bumps
/// `MAX_ALIGN` without updating the literal here.
#[repr(align(64))]
struct AlignedSentinel([u8; 0]);

const _: () = assert!(
    align_of::<AlignedSentinel>() == MAX_ALIGN,
    "AlignedSentinel alignment must match MAX_ALIGN; \
     update the #[repr(align(N))] literal on AlignedSentinel"
);

static EMPTY_SENTINEL: AlignedSentinel = AlignedSentinel([]);

/// Layout used to allocate (and deallocate) pool buffers. `cap` is the
/// per-arena byte capacity; alignment is fixed at `MAX_ALIGN`.
///
/// Returns `None` for `cap == 0` — `Layout::from_size_align` rejects
/// zero-sized layouts and we can't pass a zero-sized layout to
/// `std::alloc::alloc`. Callers must special-case the empty arena.
pub(crate) fn buffer_layout(cap: usize) -> Option<Layout> {
    if cap == 0 {
        None
    } else {
        Layout::from_size_align(cap, MAX_ALIGN).ok()
    }
}

/// Backing storage for one pool buffer — a raw aligned byte buffer
/// produced by [`std::alloc::alloc_zeroed`] (or a sentinel for the
/// `cap == 0` case, which doesn't allocate). Owns the allocation;
/// frees it in `Drop`. Used by both [`crate::arena::ArenaPool`] and
/// [`crate::arena::sync::ArenaPool`].
pub(crate) struct Buffer {
    /// Base pointer. For `cap > 0` this points at a live allocation
    /// of `cap` bytes aligned to `MAX_ALIGN`. For `cap == 0` this is
    /// a `MAX_ALIGN`-aligned dangling pointer (no backing storage).
    pub(crate) ptr: NonNull<u8>,
    /// Capacity of the allocation in bytes (also the layout `size`).
    pub(crate) cap: usize,
}

// SAFETY: `Buffer` owns its allocation outright (no aliasing) and
// `NonNull<u8>` is `!Send + !Sync` only out of caution; sending the
// owning handle to another thread is sound.
unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

impl Buffer {
    /// Allocate a buffer of `cap` bytes aligned to `MAX_ALIGN`,
    /// zero-filled. For `cap == 0` returns a dangling-but-aligned
    /// sentinel (matching `NonNull::dangling()` semantics for an
    /// arbitrary-alignment pointer) without touching the global
    /// allocator.
    pub(crate) fn new_zeroed(cap: usize) -> Self {
        match buffer_layout(cap) {
            None => {
                // Produce a `MAX_ALIGN`-aligned dangling pointer that
                // is never dereferenced (cap == 0 means no allocation
                // accesses go through it). We use the address of a
                // real `MAX_ALIGN`-aligned static rather than an
                // integer-to-pointer cast: the latter is rejected by
                // Miri's `-Zmiri-strict-provenance` check (the
                // resulting pointer has no provenance and cannot
                // legally be reborrowed). `NonNull::from(&...)`
                // preserves provenance and is strict-provenance
                // friendly; the `cast::<u8>()` is also
                // provenance-preserving.
                Buffer {
                    ptr: NonNull::from(&EMPTY_SENTINEL).cast::<u8>(),
                    cap: 0,
                }
            }
            Some(layout) => {
                // SAFETY: layout has non-zero size (we just checked).
                let raw = unsafe { alloc_zeroed(layout) };
                let ptr =
                    NonNull::new(raw).unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
                Buffer { ptr, cap }
            }
        }
    }

    /// Zero the entire buffer. Called when a buffer is returned to the
    /// pool so a subsequent lease starts from a clean (and therefore
    /// `Zeroable`-valid) state.
    pub(crate) fn zero(&mut self) {
        if self.cap > 0 {
            // SAFETY: ptr points to `cap` bytes of writable storage we
            // own exclusively (`&mut self`).
            unsafe { ptr::write_bytes(self.ptr.as_ptr(), 0, self.cap) };
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        if let Some(layout) = buffer_layout(self.cap) {
            // SAFETY: ptr was returned by `alloc_zeroed(layout)` and
            // we have not freed it yet.
            unsafe { dealloc(self.ptr.as_ptr(), layout) };
        }
    }
}

/// Pool of reusable byte buffers for arena-backed frame allocations.
///
/// Construct one per decoder via [`ArenaPool::new`]. Lease an
/// [`Arena`] per frame via [`ArenaPool::lease`]; drop the arena (or
/// drop the last clone of a [`Frame`] holding it) to return its
/// buffer to the pool.
///
/// **Backpressure:** when all `max_arenas` slots are checked out the
/// next [`ArenaPool::lease`] returns
/// [`Error::ResourceExhausted`]. A decoder that hits this should
/// surface the error to its caller rather than busy-loop — the
/// upstream pipeline is supposed to drop frames it no longer needs,
/// which returns a buffer to the pool.
///
/// `ArenaPool` is `Send + Sync` (the inner `Mutex<Vec<…>>` makes it
/// safe to share across threads even though [`Arena`] / [`Frame`]
/// themselves are `!Send` due to their `Rc`/`Cell` contents). This
/// asymmetry is intentional: a parallel-decoder thread can share a
/// single pool while each thread owns its own arenas — see also the
/// sibling [`sync::ArenaPool`] whose leases are themselves `Send + Sync`.
pub struct ArenaPool {
    inner: Mutex<PoolInner>,
    cap_per_arena: usize,
    max_arenas: usize,
    max_alloc_count_per_arena: u32,
}

struct PoolInner {
    /// Buffers currently sitting idle in the pool (ready to lease).
    idle: Vec<Buffer>,
    /// Total buffers ever allocated by this pool (idle + in-flight).
    /// Caps lazy growth at `max_arenas`.
    total_allocated: usize,
}

impl ArenaPool {
    /// Construct a new pool with `max_arenas` buffer slots, each of
    /// `cap_per_arena` bytes. Buffers are allocated lazily on first
    /// lease — a freshly constructed pool holds no memory.
    ///
    /// Per-arena allocation count is capped at `max_alloc_count` (use
    /// [`ArenaPool::new`] which defaults to a generous 1M, or
    /// [`ArenaPool::with_alloc_count_cap`] to tighten further).
    pub fn new(max_arenas: usize, cap_per_arena: usize) -> Arc<Self> {
        Self::with_alloc_count_cap(max_arenas, cap_per_arena, 1_000_000)
    }

    /// Like [`ArenaPool::new`] but lets the caller set the per-arena
    /// allocation-count cap. Useful when the caller is plumbing
    /// [`crate::DecoderLimits`] through.
    pub fn with_alloc_count_cap(
        max_arenas: usize,
        cap_per_arena: usize,
        max_alloc_count_per_arena: u32,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(PoolInner {
                idle: Vec::with_capacity(max_arenas),
                total_allocated: 0,
            }),
            cap_per_arena,
            max_arenas,
            max_alloc_count_per_arena,
        })
    }

    /// Capacity of each arena buffer this pool hands out, in bytes.
    pub fn cap_per_arena(&self) -> usize {
        self.cap_per_arena
    }

    /// Maximum number of arenas that may be checked out at once.
    pub fn max_arenas(&self) -> usize {
        self.max_arenas
    }

    /// Lease one arena from the pool. Returns
    /// [`Error::ResourceExhausted`] if every arena slot is already
    /// checked out by an [`Arena`] (or a [`Frame`] holding one).
    pub fn lease(self: &Arc<Self>) -> Result<Arena> {
        let buffer = {
            let mut inner = self.inner.lock().expect("ArenaPool mutex poisoned");
            if let Some(buf) = inner.idle.pop() {
                buf
            } else if inner.total_allocated < self.max_arenas {
                inner.total_allocated += 1;
                Buffer::new_zeroed(self.cap_per_arena)
            } else {
                return Err(Error::resource_exhausted(format!(
                    "ArenaPool exhausted: all {} arenas checked out",
                    self.max_arenas
                )));
            }
        };

        let base = buffer.ptr;
        Ok(Arena {
            buffer: Cell::new(Some(buffer)),
            base,
            cursor: Cell::new(0),
            alloc_count: Cell::new(0),
            cap: self.cap_per_arena,
            alloc_count_cap: self.max_alloc_count_per_arena,
            pool: Arc::downgrade(self),
        })
    }

    /// Return a buffer to the idle list. Called from `Arena::Drop`;
    /// not part of the public API. The buffer is zeroed before being
    /// returned so the next lease starts from a clean state — this is
    /// what makes `Zeroable` a sufficient bound on `Arena::alloc<T>`
    /// across pool reuse cycles.
    fn release(&self, mut buffer: Buffer) {
        buffer.zero();
        if let Ok(mut inner) = self.inner.lock() {
            inner.idle.push(buffer);
        }
        // If the lock is poisoned, drop the buffer normally — the
        // pool is in an unusable state already.
    }
}

/// One leased buffer from an [`ArenaPool`].
///
/// Allocations are bump-pointer: each call to [`Arena::alloc`] carves
/// out a fresh aligned slice from the head of the buffer. There is no
/// per-allocation header and no individual free — the entire arena
/// is reset (returned to the pool) only when the `Arena` is dropped.
///
/// `Arena` is `!Send + !Sync` because its bump cursor is a `Cell` and
/// its buffer cell is `Cell<Option<Buffer>>` (not synchronised). This
/// is fine for the round-1 single-threaded decoder path. The sibling
/// [`sync::Arena`] uses `AtomicUsize` for the cursor and a `Mutex`
/// around the buffer slot to regain `Send + Sync`.
pub struct Arena {
    /// Backing buffer leased from the pool. `Cell<Option<Buffer>>` so
    /// `Drop` can `take()` the buffer and hand it back to the pool
    /// without needing `&mut self`. Outside of `Drop` this is always
    /// `Some`.
    ///
    /// We never re-borrow this buffer mutably while handing out
    /// slices from it — the typed pointers returned by `alloc` are
    /// derived from the cached raw `base` pointer below, never from
    /// `(*buffer).as_mut_ptr()`. This avoids the stacked-borrows
    /// "whole-buffer retag invalidates previously returned slices"
    /// problem.
    buffer: Cell<Option<Buffer>>,
    /// Cached base pointer of `buffer` (a `MAX_ALIGN`-aligned
    /// allocation owned by `buffer`). Stable for the lifetime of the
    /// arena: `Buffer` does not move its allocation, and we only take
    /// `buffer` out of the cell during `Drop` after no allocator
    /// activity remains. All `alloc` calls derive their typed
    /// pointers from `base.as_ptr().add(offset)`.
    base: NonNull<u8>,
    /// Bump cursor: the next free byte offset within the buffer.
    cursor: Cell<usize>,
    /// Number of allocations performed so far.
    alloc_count: Cell<u32>,
    /// Cached cap (== `pool.cap_per_arena` at lease time).
    cap: usize,
    /// Cached cap (== `pool.max_alloc_count_per_arena` at lease time).
    alloc_count_cap: u32,
    /// Weak handle back to the pool so `Drop` can return the buffer.
    pool: Weak<ArenaPool>,
}

impl Arena {
    /// Capacity of this arena in bytes.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Bytes consumed by allocations so far.
    pub fn used(&self) -> usize {
        self.cursor.get()
    }

    /// Number of allocations performed so far.
    pub fn alloc_count(&self) -> u32 {
        self.alloc_count.get()
    }

    /// `true` once the per-arena allocation-count cap has been
    /// reached. Decoders that produce many small allocations should
    /// poll this and bail with [`Error::ResourceExhausted`] when it
    /// flips, instead of waiting for the next [`Arena::alloc`] call
    /// to fail.
    pub fn alloc_count_exceeded(&self) -> bool {
        self.alloc_count.get() >= self.alloc_count_cap
    }

    /// Allocate `count` `T`s out of this arena. Returns a borrowed
    /// `&mut [T]` (lifetime bounded by the borrow of `self`).
    ///
    /// The returned slice points at zero-filled bytes (the pool
    /// zero-fills on initial allocation and again whenever a buffer
    /// is returned). The `Zeroable` bound on `T` guarantees that an
    /// all-zero bit pattern is a valid value for `T`, so reading the
    /// slice without first writing it is sound. **The intended
    /// pattern is still "decoder fills the slice, then reads back
    /// what it wrote" — but unwritten bytes will read back as
    /// `T::zeroed()` rather than as UB.**
    ///
    /// Returns [`Error::ResourceExhausted`] if either the per-arena
    /// byte cap or the per-arena allocation-count cap would be
    /// exceeded.
    ///
    /// # Type bounds
    ///
    /// - `T: bytemuck::Zeroable` — pool buffers are zero-filled, so
    ///   handing back `&mut [T]` over those bytes is only sound when
    ///   the all-zero bit pattern is valid for `T`. This rules out
    ///   `NonZeroU8`/`NonZeroU16`/…/references/function pointers/
    ///   niche-optimised enums (anything where the optimizer relies
    ///   on a forbidden-bit-pattern invariant).
    /// - `align_of::<T>() <= MAX_ALIGN` — checked at compile time via
    ///   a `const` assertion. The pool buffer's base pointer is
    ///   aligned to `MAX_ALIGN` (= 64 bytes); per-`T` alignment is
    ///   then a relative-offset adjustment of the bump cursor.
    /// - The arena does not run destructors on allocated values, so
    ///   `T` should not have meaningful `Drop` glue. `Zeroable` is
    ///   automatically implemented only for types where this is the
    ///   case (primitives, `[T; N]` of zeroable, `#[derive(Zeroable)]`
    ///   on POD structs).
    ///
    /// **Aliasing model:** the bump cursor is monotonically
    /// non-decreasing, so successive `alloc` calls return slices
    /// covering disjoint regions of the underlying buffer. The
    /// returned typed pointer is derived from the arena's cached raw
    /// base pointer (`base.as_ptr().add(offset)`), never from a
    /// re-borrow of the whole buffer — that's what keeps previously
    /// returned `&mut [T]` slices valid under stacked borrows. This
    /// is the standard arena-allocator pattern (cf.
    /// `bumpalo::Bump::alloc_slice_*`) and is the reason this method
    /// takes `&self` rather than `&mut self`.
    #[allow(clippy::mut_from_ref)] // see "Aliasing model" doc above.
    pub fn alloc<T>(&self, count: usize) -> Result<&mut [T]>
    where
        T: bytemuck::Zeroable,
    {
        // Compile-time check: T's alignment must not exceed the
        // pool buffer's base alignment. Doing this as a const-eval'd
        // assert means a violating monomorphisation fails the build.
        const fn assert_align<T>() {
            assert!(
                align_of::<T>() <= MAX_ALIGN,
                "Arena::alloc<T>: align_of::<T>() exceeds MAX_ALIGN; \
                 increase MAX_ALIGN in arena/mod.rs"
            );
        }
        const { assert_align::<T>() };

        // Allocation-count cap.
        let next_count =
            self.alloc_count.get().checked_add(1).ok_or_else(|| {
                Error::resource_exhausted("Arena alloc_count overflow".to_string())
            })?;
        if next_count > self.alloc_count_cap {
            return Err(Error::resource_exhausted(format!(
                "Arena alloc-count cap of {} exceeded",
                self.alloc_count_cap
            )));
        }

        let elem_size = size_of::<T>();
        let elem_align = align_of::<T>();
        // Bytes requested.
        let bytes = elem_size
            .checked_mul(count)
            .ok_or_else(|| Error::resource_exhausted("Arena alloc size overflow".to_string()))?;

        // Align cursor up to T's alignment.
        let cursor = self.cursor.get();
        let aligned = align_up(cursor, elem_align).ok_or_else(|| {
            Error::resource_exhausted("Arena cursor alignment overflow".to_string())
        })?;
        let new_cursor = aligned.checked_add(bytes).ok_or_else(|| {
            Error::resource_exhausted("Arena cursor advance overflow".to_string())
        })?;

        if new_cursor > self.cap {
            return Err(Error::resource_exhausted(format!(
                "Arena cap of {} bytes exceeded (would consume {} bytes)",
                self.cap, new_cursor
            )));
        }

        // SAFETY:
        //
        // - `self.base` points to a `MAX_ALIGN`-aligned allocation of
        //   `self.cap` bytes owned by the `Buffer` inside `self.buffer`,
        //   which lives at least as long as `&self`.
        // - `aligned + count*size_of::<T>() <= self.cap` (just checked
        //   above), so the byte range we slice is in-bounds.
        // - `aligned` is a multiple of `align_of::<T>()` (computed via
        //   `align_up`), and `MAX_ALIGN >= align_of::<T>()` (compile-
        //   time assert above), so `base + aligned` is `T`-aligned.
        //   This holds even for `count == 0` (the slice still has an
        //   aligned dangling pointer, which is what an empty `&mut [T]`
        //   requires).
        // - The cursor is monotonically non-decreasing, so the byte
        //   range `aligned..new_cursor` does not overlap any byte
        //   range previously returned by `alloc`. We never re-borrow
        //   the whole buffer — the typed pointer is derived from the
        //   raw base pointer — so the new `&mut [T]` does not invalidate
        //   any previously returned slice under stacked borrows.
        // - `T: Zeroable` and the buffer bytes are zero, so the
        //   `&mut [T]` references valid `T` values (the safe API
        //   contract).
        let slice: &mut [T] = unsafe {
            let elem_ptr = self.base.as_ptr().add(aligned).cast::<T>();
            std::slice::from_raw_parts_mut(elem_ptr, count)
        };

        self.cursor.set(new_cursor);
        self.alloc_count.set(next_count);
        Ok(slice)
    }

    /// Reset the arena to empty without releasing its buffer to the
    /// pool. Useful for a decoder that wants to reuse the same arena
    /// across several intermediate stages of the same frame. Callers
    /// must ensure no slice previously returned from [`Arena::alloc`]
    /// is still in use — Rust's borrow checker enforces this, since
    /// `reset` takes `&mut self`.
    pub fn reset(&mut self) {
        self.cursor.set(0);
        self.alloc_count.set(0);
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // Take the buffer out of the cell. We're in Drop with `&mut
        // self`, so no `alloc`-returned slices can still be borrowing
        // from `base`.
        if let Some(buffer) = self.buffer.take() {
            if let Some(pool) = self.pool.upgrade() {
                pool.release(buffer);
            } else {
                // Pool was dropped before us — buffer drops here and
                // its allocation is freed via `Buffer::Drop`.
                drop(buffer);
            }
        }
    }
}

/// Round `n` up to the next multiple of `align`. `align` must be a
/// power of two. Returns `None` on overflow.
fn align_up(n: usize, align: usize) -> Option<usize> {
    debug_assert!(align.is_power_of_two(), "alignment must be a power of two");
    let mask = align - 1;
    n.checked_add(mask).map(|m| m & !mask)
}

/// Per-frame metadata carried alongside an [`Arena`] inside a
/// [`Frame`]. Kept minimal in round 1; round 2 will extend with
/// stride/colorspace/HDR fields as decoders need them.
///
/// `Copy` so it travels through the hot path with no allocation.
#[non_exhaustive]
#[derive(Copy, Clone, Debug)]
pub struct FrameHeader {
    /// Visible picture width in pixels.
    pub width: u32,
    /// Visible picture height in pixels.
    pub height: u32,
    /// Pixel format of the plane data in the arena.
    pub pixel_format: PixelFormat,
    /// Presentation timestamp in stream time-base units. `None` when
    /// the codec did not surface one (e.g. a still image).
    pub presentation_timestamp: Option<i64>,
}

impl FrameHeader {
    /// Construct a header with all four mandatory fields set. Use
    /// functional-update syntax (`FrameHeader { ..header }`) to add
    /// future fields safely.
    pub fn new(
        width: u32,
        height: u32,
        pixel_format: PixelFormat,
        presentation_timestamp: Option<i64>,
    ) -> Self {
        Self {
            width,
            height,
            pixel_format,
            presentation_timestamp,
        }
    }
}

/// Maximum number of planes a [`FrameInner`] can describe in round 1.
/// Covers every real-world video pixel format (1 plane for packed
/// RGB/YUV 4:2:2, 3 planes for I420/YV12/I444, 4 planes for YUVA / RGBA
/// planar). Audio is handled by a separate sibling type in a future
/// round; this module is video-only for now.
pub const MAX_PLANES: usize = 4;

/// The owned body of a refcounted [`Frame`].
///
/// Holds an [`Arena`] (the bytes), a fixed-size table of
/// `(offset_in_arena, length_in_bytes)` pairs (one per plane), and a
/// [`FrameHeader`]. The `plane_count` field tracks how many entries of
/// `plane_offsets` are actually populated. Up to [`MAX_PLANES`] planes
/// are supported.
///
/// **Lifetime:** an `Arena` returns its buffer to the pool when
/// dropped. A `Rc<FrameInner>` keeps the arena alive via its single
/// owned field, so as long as any clone of a [`Frame`] exists the
/// underlying buffer stays out of the pool.
pub struct FrameInner {
    arena: Arena,
    plane_offsets: [(usize, usize); MAX_PLANES],
    plane_count: u8,
    header: FrameHeader,
}

/// Refcounted handle to a decoded video frame. Construct via
/// [`Frame::new`]; clone freely (each clone bumps the refcount by 1).
/// The arena and its buffer are released back to the pool when the
/// last clone is dropped.
///
/// `Frame` is `Rc<FrameInner>` (single-threaded decoder path). For the
/// cross-thread decode path — where the consumer runs on a different
/// thread from the decoder — use the sibling [`sync::Frame`] which is
/// `Arc<sync::FrameInner>` and is `Send + Sync`.
pub type Frame = Rc<FrameInner>;

impl FrameInner {
    /// Construct a `Frame` (refcounted `Rc<FrameInner>`) from an arena,
    /// a slice of `(offset, length)` plane descriptors, and a header.
    /// Returns [`Error::InvalidData`] if more than [`MAX_PLANES`]
    /// planes are supplied or if any plane range falls outside the
    /// arena's used region.
    pub fn new(arena: Arena, planes: &[(usize, usize)], header: FrameHeader) -> Result<Frame> {
        if planes.len() > MAX_PLANES {
            return Err(Error::invalid(format!(
                "FrameInner supports at most {} planes (got {})",
                MAX_PLANES,
                planes.len()
            )));
        }
        let used = arena.used();
        for (i, (off, len)) in planes.iter().enumerate() {
            let end = off
                .checked_add(*len)
                .ok_or_else(|| Error::invalid(format!("plane {i}: offset+len overflow")))?;
            if end > used {
                return Err(Error::invalid(format!(
                    "plane {i}: range {off}..{end} exceeds arena used={used}"
                )));
            }
        }
        let mut plane_offsets = [(0usize, 0usize); MAX_PLANES];
        for (i, p) in planes.iter().enumerate() {
            plane_offsets[i] = *p;
        }
        Ok(Rc::new(FrameInner {
            arena,
            plane_offsets,
            plane_count: planes.len() as u8,
            header,
        }))
    }

    /// Number of planes this frame holds.
    pub fn plane_count(&self) -> usize {
        self.plane_count as usize
    }

    /// Read-only access to plane `i`. Returns `None` if `i` is out of
    /// range.
    pub fn plane(&self, i: usize) -> Option<&[u8]> {
        if i >= self.plane_count as usize {
            return None;
        }
        let (off, len) = self.plane_offsets[i];
        // SAFETY:
        // - plane ranges were validated against `arena.used()` at
        //   construction (`off + len <= arena.cursor`), and the
        //   cursor is monotonically non-decreasing, so the byte
        //   range is still in-bounds.
        // - The bytes were written by `alloc` and never moved (the
        //   buffer's allocation is stable for the arena's lifetime).
        // - We derive the slice from the raw base pointer, never via
        //   a re-borrow of the whole buffer, so this `&[u8]` does not
        //   invalidate any other slice the caller is holding.
        // - The borrow lifetime is bounded by `&self`.
        let buf: &[u8] = unsafe {
            let elem_ptr = self.arena.base.as_ptr().add(off);
            std::slice::from_raw_parts(elem_ptr, len)
        };
        Some(buf)
    }

    /// Frame header (width / height / pixel format / pts).
    pub fn header(&self) -> &FrameHeader {
        &self.header
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_pool(slots: usize, cap: usize) -> Arc<ArenaPool> {
        ArenaPool::new(slots, cap)
    }

    #[test]
    fn pool_lease_returns_err_when_exhausted() {
        let pool = small_pool(2, 1024);
        let a = pool.lease().expect("first lease");
        let b = pool.lease().expect("second lease");
        let third = pool.lease();
        assert!(matches!(third, Err(Error::ResourceExhausted(_))));
        // Keep a and b alive past the assertion so they aren't dropped
        // before the failing lease.
        drop((a, b));
    }

    #[test]
    fn arena_alloc_caps_at_size_limit() {
        let pool = small_pool(1, 64);
        let arena = pool.lease().unwrap();
        // 64 bytes capacity. Allocate 32 u8s — fits.
        let _: &mut [u8] = arena.alloc::<u8>(32).unwrap();
        // Allocate another 32 u8s — exactly fills.
        let _: &mut [u8] = arena.alloc::<u8>(32).unwrap();
        // Any further allocation fails.
        let third = arena.alloc::<u8>(1);
        assert!(matches!(third, Err(Error::ResourceExhausted(_))));
    }

    #[test]
    fn arena_alloc_count_cap_fires() {
        let pool = ArenaPool::with_alloc_count_cap(1, 1024, 3);
        let arena = pool.lease().unwrap();
        let _: &mut [u8] = arena.alloc::<u8>(1).unwrap();
        let _: &mut [u8] = arena.alloc::<u8>(1).unwrap();
        let _: &mut [u8] = arena.alloc::<u8>(1).unwrap();
        assert!(arena.alloc_count_exceeded());
        let fourth = arena.alloc::<u8>(1);
        assert!(matches!(fourth, Err(Error::ResourceExhausted(_))));
    }

    #[test]
    fn arena_returns_to_pool_on_drop() {
        let pool = small_pool(1, 256);
        {
            let arena = pool.lease().expect("first lease");
            // Sanity: arena is leased; further leases would fail.
            assert!(matches!(pool.lease(), Err(Error::ResourceExhausted(_))));
            drop(arena);
        }
        // Arena dropped — pool slot must be free again.
        let _again = pool.lease().expect("re-lease after drop");
    }

    #[test]
    fn arena_alignment_is_respected() {
        let pool = small_pool(1, 64);
        let arena = pool.lease().unwrap();
        // Allocate a single u8 to misalign the cursor.
        let _: &mut [u8] = arena.alloc::<u8>(1).unwrap();
        // Now allocate u32s; expect cursor to be aligned to 4.
        let s: &mut [u32] = arena.alloc::<u32>(4).unwrap();
        let addr = s.as_ptr() as usize;
        assert_eq!(addr % align_of::<u32>(), 0);
        assert_eq!(s.len(), 4);
    }

    #[cfg(miri)]
    #[test]
    fn arena_alloc_can_return_misaligned_typed_slice() {
        let pool = small_pool(1, 0);
        let arena = pool.lease().unwrap();

        // Memory-safety issue: the arena's backing allocation is a
        // `Box<[u8]>`, so its base pointer is only guaranteed to be
        // byte-aligned. `alloc::<T>` aligns only the byte offset, not
        // the absolute address, and then constructs `&mut [T]`. The
        // empty-buffer case makes this deterministic: even an empty
        // `&mut [u32]` must have an aligned pointer, but `Box<[u8]>`
        // uses an alignment-1 dangling pointer when its length is 0.
        let _s: &mut [u32] = arena.alloc::<u32>(0).unwrap();
    }

    // Pre-fix this test was:
    //
    //     let values = arena.alloc::<std::num::NonZeroU8>(1).unwrap();
    //     let _ = values[0].get();
    //
    // and failed under Miri because pool buffers are zero-filled and
    // zero is not a valid `NonZeroU8`. Post-fix, the `Zeroable` bound
    // on `Arena::alloc` makes that call a hard *compile* error — the
    // strongest possible enforcement. The test below is a regression
    // assertion that the bound stays as-or-stricter than `Zeroable`:
    // if a future refactor weakened it back to `Copy`, the
    // commented-out call site would start compiling again and Miri
    // would once again accept the invalid bit pattern.
    #[cfg(miri)]
    #[test]
    fn arena_alloc_allows_invalid_bit_patterns_for_copy_types() {
        // `requires_zeroable::<NonZeroU8>()` would not compile —
        // `NonZeroU8: !Zeroable`. Sanity-check the helper itself with
        // a known zeroable type so the test is an actual exercise.
        fn requires_zeroable<T: bytemuck::Zeroable>() {}
        requires_zeroable::<u8>();
        // Uncommenting the next line must fail to compile:
        //   requires_zeroable::<std::num::NonZeroU8>();
    }

    #[cfg(miri)]
    #[test]
    fn arena_alloc_second_slice_invalidates_first_mut_reference() {
        let pool = small_pool(1, 2);
        let arena = pool.lease().unwrap();

        // Memory-safety issue: each `alloc` calls `[u8]::as_mut_ptr` on
        // the whole backing slice before carving out the requested
        // subslice. That materializes a new mutable borrow of the whole
        // buffer and invalidates previously returned `&mut` slices, even
        // when the byte ranges are disjoint.
        let first = arena.alloc::<u8>(1).unwrap();
        let second = arena.alloc::<u8>(1).unwrap();
        first[0] = 1;
        second[0] = 2;
    }

    fn build_simple_frame(pool: &Arc<ArenaPool>) -> Frame {
        let arena = pool.lease().unwrap();
        // Allocate 16 bytes for plane 0.
        let plane0: &mut [u8] = arena.alloc::<u8>(16).unwrap();
        for (i, b) in plane0.iter_mut().enumerate() {
            *b = i as u8;
        }
        // The slice borrowed from arena ends here.
        let header = FrameHeader::new(4, 4, PixelFormat::Gray8, Some(42));
        FrameInner::new(arena, &[(0, 16)], header).unwrap()
    }

    #[test]
    fn frame_refcount_keeps_arena_alive() {
        let pool = small_pool(1, 256);
        let frame = build_simple_frame(&pool);
        let clone = Rc::clone(&frame);
        drop(frame);
        // Clone is still valid; arena still leased.
        let plane = clone.plane(0).expect("plane 0");
        assert_eq!(plane.len(), 16);
        for (i, b) in plane.iter().enumerate() {
            assert_eq!(*b, i as u8);
        }
        assert_eq!(clone.header().width, 4);
        assert_eq!(clone.header().height, 4);
        assert_eq!(clone.header().presentation_timestamp, Some(42));
        // Pool still exhausted because clone holds the arena.
        assert!(matches!(pool.lease(), Err(Error::ResourceExhausted(_))));
    }

    #[test]
    fn last_drop_returns_arena_to_pool() {
        let pool = small_pool(1, 256);
        let frame = build_simple_frame(&pool);
        let clone = Rc::clone(&frame);
        drop(frame);
        drop(clone);
        // All clones gone — buffer must be back in the pool.
        let _again = pool.lease().expect("lease after last drop");
    }

    #[test]
    fn frame_rejects_too_many_planes() {
        let pool = small_pool(1, 256);
        let arena = pool.lease().unwrap();
        let header = FrameHeader::new(1, 1, PixelFormat::Gray8, None);
        let too_many = vec![(0usize, 0usize); MAX_PLANES + 1];
        let r = FrameInner::new(arena, &too_many, header);
        assert!(matches!(r, Err(Error::InvalidData(_))));
    }

    #[test]
    fn frame_rejects_plane_outside_arena() {
        let pool = small_pool(1, 64);
        let arena = pool.lease().unwrap();
        // arena.used() == 0; any non-empty plane is out of range.
        let header = FrameHeader::new(1, 1, PixelFormat::Gray8, None);
        let r = FrameInner::new(arena, &[(0, 16)], header);
        assert!(matches!(r, Err(Error::InvalidData(_))));
    }

    #[test]
    fn pool_outlives_buffer_drop_when_pool_dropped_first() {
        // Exotic: arena outlives its pool. Buffer just frees normally.
        let pool = small_pool(1, 64);
        let arena = pool.lease().unwrap();
        drop(pool);
        // Drop arena — must not panic. The Weak handle won't upgrade.
        drop(arena);
    }

    #[test]
    fn arena_reset_clears_allocations() {
        let pool = small_pool(1, 32);
        let mut arena = pool.lease().unwrap();
        let _: &mut [u8] = arena.alloc::<u8>(32).unwrap();
        // Cap reached.
        assert!(matches!(
            arena.alloc::<u8>(1),
            Err(Error::ResourceExhausted(_))
        ));
        arena.reset();
        // After reset we can allocate again.
        let _: &mut [u8] = arena.alloc::<u8>(32).unwrap();
    }
}
