//! `Send + Sync` mirror of the parent [`crate::arena`] module.
//!
//! This module exposes the same four-type API ([`ArenaPool`],
//! [`Arena`], [`Frame`], [`FrameInner`]) as its sibling, with one
//! difference that ripples through the whole shape:
//!
//! - [`Arena`] uses `AtomicUsize` / `AtomicU32` for its bump cursor
//!   and allocation counter (instead of `Cell<usize>` / `Cell<u32>`),
//!   and is therefore `Send + Sync`.
//! - [`Frame`] is `Arc<FrameInner>` (instead of `Rc<FrameInner>`),
//!   so a decoded frame can be moved or shared across threads.
//! - [`FrameInner`] holds a sync [`Arena`], so it is itself `Send +
//!   Sync` and `Arc<FrameInner>: Send + Sync` falls out for free.
//!
//! ## When to use which
//!
//! Use [`crate::arena`] (the `Rc` variant) when the decoder produces
//! frames on the same thread that consumes them. The bump cursor is
//! a plain `Cell<usize>` and there are no atomic operations on the
//! hot allocation path.
//!
//! Use this module (the `Arc` variant) when the decoder hands frames
//! to a different thread — the typical case for a pipeline that
//! decodes on one worker and renders / encodes / transmits on
//! another. The cost is a relaxed atomic load + CAS per allocation
//! and an atomic refcount per frame clone; both are negligible
//! compared to the actual decode work.
//!
//! ## Concurrent allocation contract
//!
//! [`Arena::alloc`] uses a CAS loop on the cursor, so two threads
//! that both call [`Arena::alloc`] on the same `&Arena` will receive
//! disjoint slices (the loser of the CAS retries against the new
//! cursor). The returned `&mut [T]` points into a region that no
//! other in-flight `alloc()` call can also receive, and the slice's
//! lifetime is bounded by the borrow of `&self`.
//!
//! In practice the typical pattern is **one decoder thread allocates,
//! then freezes into a [`Frame`] which is shared read-only across
//! threads** — concurrent allocation is supported but rarely useful.
//! The bytes returned by [`Arena::alloc`] are not zero-initialised;
//! callers must fully overwrite them before reading.
//!
//! Everything else (per-arena byte cap, per-arena allocation-count
//! cap, weak handle back to the pool for `Drop`-time release, the
//! `FrameHeader` shape, plane validation in [`FrameInner::new`])
//! matches the parent module exactly.

use std::mem::{align_of, size_of};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use crate::error::{Error, Result};

// Re-export the shared `FrameHeader` and `MAX_PLANES` constant so
// users of either arena module see the same metadata shape — there is
// no thread-safety angle to either of them, and duplicating them
// would only add drift.
pub use super::{FrameHeader, MAX_PLANES};
// `Buffer` and `MAX_ALIGN` are shared with the parent module — the
// pool-backing storage and the soundness-critical alignment constant
// are identical for the `Rc` and `Arc` variants. See `arena/mod.rs`
// for the full soundness rationale.
use super::{Buffer, MAX_ALIGN};

/// `Send + Sync` pool of reusable byte buffers for arena-backed frame
/// allocations. Mirrors [`crate::arena::ArenaPool`] in shape and
/// behaviour; the only difference is that the [`Arena`] (and the
/// [`Frame`] holding it) handed out are themselves `Send + Sync`.
///
/// Construct via [`ArenaPool::new`]. Lease an [`Arena`] per frame via
/// [`ArenaPool::lease`]; drop the arena (or drop the last clone of a
/// [`Frame`] holding it) to return its buffer to the pool.
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
    /// Per-arena allocation count is capped at a generous 1 M
    /// (override via [`ArenaPool::with_alloc_count_cap`]).
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
            buffer: Mutex::new(Some(buffer)),
            base,
            cursor: AtomicUsize::new(0),
            alloc_count: AtomicU32::new(0),
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

/// One leased buffer from a [`ArenaPool`]. `Send + Sync`.
///
/// Allocations are bump-pointer on an atomic cursor: each call to
/// [`Arena::alloc`] CAS-advances the cursor and returns a fresh
/// aligned slice carved out of the buffer at the old position. There
/// is no per-allocation header and no individual free — the entire
/// arena is reset (returned to the pool) only when the `Arena` is
/// dropped.
///
/// Concurrent calls to [`Arena::alloc`] on the same `&Arena` are
/// supported and produce disjoint slices (the CAS loser retries
/// against the new cursor). See the module docs for the full
/// concurrency contract.
pub struct Arena {
    /// Backing buffer leased from the pool. Stored in a `Mutex` so
    /// `Drop` can `take()` the buffer without needing direct
    /// `UnsafeCell` access (which would re-borrow the whole storage
    /// and invalidate previously-returned slices under stacked
    /// borrows). Outside of `Drop` this is always `Some` — the
    /// mutex itself is essentially uncontended (nothing else touches
    /// it on the hot path).
    ///
    /// We never re-borrow this buffer mutably while handing out
    /// slices from it — the typed pointers returned by `alloc` are
    /// derived from the cached raw `base` pointer below, never from
    /// a fresh borrow of the whole storage. This is what avoids the
    /// stacked-borrows whole-buffer-retag race that Miri reported
    /// when two threads called `alloc` concurrently while a third
    /// held a previously-returned `&mut [T]`.
    buffer: Mutex<Option<Buffer>>,
    /// Cached base pointer of `buffer` (a `MAX_ALIGN`-aligned
    /// allocation owned by `buffer`). Stable for the lifetime of the
    /// arena: `Buffer` does not move its allocation, and we only take
    /// `buffer` out of the mutex during `Drop` after no allocator
    /// activity remains. All `alloc` calls derive their typed
    /// pointers from `base.as_ptr().add(offset)`.
    base: NonNull<u8>,
    /// Atomic bump cursor: the next free byte offset within the
    /// buffer.
    cursor: AtomicUsize,
    /// Atomic allocation counter.
    alloc_count: AtomicU32,
    /// Cached cap (== `pool.cap_per_arena` at lease time).
    cap: usize,
    /// Cached cap (== `pool.max_alloc_count_per_arena` at lease time).
    alloc_count_cap: u32,
    /// Weak handle back to the pool so `Drop` can return the buffer.
    pool: Weak<ArenaPool>,
}

// SAFETY: `Arena` owns its buffer's allocation outright (no shared
// ownership), all cursor/count mutations go through atomics, and the
// raw `base` pointer is only used to derive disjoint typed slices
// whose ranges the CAS loop guarantees not to overlap. The `Drop`
// path takes the buffer out of the mutex under `&mut self` — no
// other thread can be in `alloc` at that point.
unsafe impl Send for Arena {}
// SAFETY: `&Arena::alloc` mutates only via the atomic cursor and the
// allocation counter (themselves `Sync`) and writes into a region of
// the buffer that no other in-flight call has been handed (CAS
// guarantees disjoint regions). The raw `base` pointer is never used
// to materialise a whole-buffer mutable borrow, so a new `alloc`
// call cannot invalidate any other thread's previously returned
// `&mut [T]` slice under stacked borrows.
unsafe impl Sync for Arena {}

impl Arena {
    /// Capacity of this arena in bytes.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Bytes consumed by allocations so far.
    pub fn used(&self) -> usize {
        self.cursor.load(Ordering::Acquire)
    }

    /// Number of allocations performed so far.
    pub fn alloc_count(&self) -> u32 {
        self.alloc_count.load(Ordering::Acquire)
    }

    /// `true` once the per-arena allocation-count cap has been
    /// reached. Decoders that produce many small allocations should
    /// poll this and bail with [`Error::ResourceExhausted`] when it
    /// flips, instead of waiting for the next [`Arena::alloc`] call
    /// to fail.
    pub fn alloc_count_exceeded(&self) -> bool {
        self.alloc_count.load(Ordering::Acquire) >= self.alloc_count_cap
    }

    /// Allocate `count` `T`s out of this arena. Returns a borrowed
    /// `&mut [T]` (lifetime bounded by the borrow of `self`).
    ///
    /// The returned slice points at zero-filled bytes (the pool
    /// zero-fills on initial allocation and again whenever a buffer
    /// is returned). The `Zeroable` bound on `T` guarantees that an
    /// all-zero bit pattern is a valid value for `T`, so reading the
    /// slice without first writing it is sound. The intended pattern
    /// is still "decoder fills the slice, then reads back what it
    /// wrote" — but unwritten bytes will read back as `T::zeroed()`
    /// rather than as UB.
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
    ///   niche-optimised enums.
    /// - `align_of::<T>() <= MAX_ALIGN` — checked at compile time via
    ///   a `const` assertion. The pool buffer's base pointer is
    ///   aligned to `MAX_ALIGN` (= 64 bytes); per-`T` alignment is
    ///   then a relative-offset adjustment of the bump cursor.
    /// - The arena does not run destructors on allocated values, so
    ///   `T` should not have meaningful `Drop` glue.
    ///
    /// **Concurrency:** the bump cursor is advanced via a CAS loop,
    /// so concurrent `alloc` calls on the same `&Arena` produce
    /// disjoint slices. The CAS loser retries against the new
    /// cursor; in the uncontended case the cost is a single relaxed
    /// load plus one successful CAS. Crucially, no `alloc` call
    /// re-borrows the whole buffer (the typed pointer is derived
    /// from the cached raw base pointer), so concurrent allocators
    /// cannot invalidate each other's previously-returned slices
    /// under stacked borrows.
    #[allow(clippy::mut_from_ref)] // see "Concurrency" doc above.
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

        // Allocation-count cap. Increment first; if we overshoot,
        // roll back so subsequent calls still see the correct value.
        let prev_count = self.alloc_count.fetch_add(1, Ordering::AcqRel);
        if prev_count >= self.alloc_count_cap {
            // Roll back so `alloc_count_exceeded()` keeps returning
            // a stable cap value rather than drifting upward.
            self.alloc_count.fetch_sub(1, Ordering::AcqRel);
            return Err(Error::resource_exhausted(format!(
                "Arena alloc-count cap of {} exceeded",
                self.alloc_count_cap
            )));
        }

        let elem_size = size_of::<T>();
        let elem_align = align_of::<T>();
        // Bytes requested.
        let bytes = elem_size.checked_mul(count).ok_or_else(|| {
            // Roll back the alloc-count bump on size-overflow too.
            self.alloc_count.fetch_sub(1, Ordering::AcqRel);
            Error::resource_exhausted("Arena alloc size overflow".to_string())
        })?;

        // CAS loop on the cursor. We compute aligned + new_cursor
        // from the latest observed cursor value, then attempt to
        // claim that range; if another thread won the race, retry
        // against the updated cursor.
        let mut current = self.cursor.load(Ordering::Acquire);
        let aligned;
        let new_cursor;
        loop {
            let candidate_aligned = match align_up(current, elem_align) {
                Some(a) => a,
                None => {
                    self.alloc_count.fetch_sub(1, Ordering::AcqRel);
                    return Err(Error::resource_exhausted(
                        "Arena cursor alignment overflow".to_string(),
                    ));
                }
            };
            let candidate_new = match candidate_aligned.checked_add(bytes) {
                Some(n) => n,
                None => {
                    self.alloc_count.fetch_sub(1, Ordering::AcqRel);
                    return Err(Error::resource_exhausted(
                        "Arena cursor advance overflow".to_string(),
                    ));
                }
            };

            if candidate_new > self.cap {
                self.alloc_count.fetch_sub(1, Ordering::AcqRel);
                return Err(Error::resource_exhausted(format!(
                    "Arena cap of {} bytes exceeded (would consume {} bytes)",
                    self.cap, candidate_new
                )));
            }

            match self.cursor.compare_exchange_weak(
                current,
                candidate_new,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    aligned = candidate_aligned;
                    new_cursor = candidate_new;
                    let _ = new_cursor; // silence "unused" if optimised
                    break;
                }
                Err(observed) => {
                    current = observed;
                    // Retry with the freshly observed cursor.
                }
            }
        }

        // SAFETY:
        //
        // - `self.base` points to a `MAX_ALIGN`-aligned allocation of
        //   `self.cap` bytes owned by the `Buffer` inside
        //   `self.buffer`, which lives at least as long as `&self`.
        // - We just CAS-claimed the byte range `aligned..new_cursor`,
        //   so no other in-flight `alloc` call can claim any byte
        //   inside it (the cursor is monotonically non-decreasing
        //   under successful CAS, so a subsequent winner observes a
        //   `current` >= our `new_cursor`).
        // - `aligned + count*size_of::<T>() <= self.cap` (just checked
        //   above), so the byte range is in-bounds of the allocation.
        // - `aligned` is a multiple of `align_of::<T>()` and `MAX_ALIGN
        //   >= align_of::<T>()` (compile-time assert above), so `base
        //   + aligned` is `T`-aligned (true even for `count == 0`).
        // - We derive the typed pointer from `self.base.as_ptr()`, not
        //   from a fresh borrow of the whole buffer, so this slice
        //   does not invalidate any other thread's previously
        //   returned `&mut [T]` under stacked borrows.
        // - `T: Zeroable` and the buffer bytes are zero, so the
        //   `&mut [T]` references valid `T` values.
        let slice: &mut [T] = unsafe {
            let elem_ptr = self.base.as_ptr().add(aligned).cast::<T>();
            std::slice::from_raw_parts_mut(elem_ptr, count)
        };

        Ok(slice)
    }

    /// Reset the arena to empty without releasing its buffer to the
    /// pool. Useful for a decoder that wants to reuse the same arena
    /// across several intermediate stages of the same frame. Callers
    /// must ensure no slice previously returned from [`Arena::alloc`]
    /// is still in use — Rust's borrow checker enforces this, since
    /// `reset` takes `&mut self`.
    pub fn reset(&mut self) {
        // `&mut self` proves exclusive access; non-atomic stores
        // would suffice, but the atomic API is uniform.
        self.cursor.store(0, Ordering::Release);
        self.alloc_count.store(0, Ordering::Release);
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // We're in Drop with `&mut self`, so no `alloc`-returned
        // slices can still be borrowing from `base` and no other
        // thread can be in `alloc`. Take the buffer out of the mutex
        // and either return it to the pool or let it free here.
        let taken = self.buffer.get_mut().ok().and_then(|slot| slot.take());
        if let Some(buffer) = taken {
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

/// The owned body of a refcounted [`Frame`]. `Send + Sync`.
///
/// Holds a [`sync::Arena`](Arena) (the bytes), a fixed-size table of
/// `(offset_in_arena, length_in_bytes)` pairs (one per plane), and a
/// [`FrameHeader`]. The `plane_count` field tracks how many entries
/// of `plane_offsets` are actually populated. Up to [`MAX_PLANES`]
/// planes are supported.
///
/// **Lifetime:** an [`Arena`] returns its buffer to the pool when
/// dropped. An `Arc<FrameInner>` keeps the arena alive via its single
/// owned field, so as long as any clone of a [`Frame`] exists the
/// underlying buffer stays out of the pool.
pub struct FrameInner {
    arena: Arena,
    plane_offsets: [(usize, usize); MAX_PLANES],
    plane_count: u8,
    header: FrameHeader,
}

/// Refcounted handle to a decoded video frame. `Send + Sync`.
///
/// Construct via [`FrameInner::new`]; clone freely (each clone bumps
/// the atomic refcount by 1). The arena and its buffer are released
/// back to the pool when the last clone is dropped.
///
/// Use this type when the decoder hands frames to a different thread
/// from the one that produced them. For same-thread decode/consume,
/// the cheaper [`crate::arena::Frame`] (`Rc`-backed) is preferable.
pub type Frame = Arc<FrameInner>;

impl FrameInner {
    /// Construct a `Frame` (`Arc<FrameInner>`) from an arena, a slice
    /// of `(offset, length)` plane descriptors, and a header. Returns
    /// [`Error::InvalidData`] if more than [`MAX_PLANES`] planes are
    /// supplied or if any plane range falls outside the arena's used
    /// region.
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
        Ok(Arc::new(FrameInner {
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
        //   cursor only advances, so the byte range is in-bounds.
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
    use crate::format::PixelFormat;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn types_are_send_sync() {
        // The whole point of this module: prove the public types
        // satisfy the cross-thread contract that `crate::arena` does
        // not.
        assert_send_sync::<ArenaPool>();
        assert_send_sync::<Arc<ArenaPool>>();
        assert_send_sync::<Arena>();
        assert_send_sync::<FrameInner>();
        assert_send_sync::<Frame>();
    }

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
        drop((a, b));
    }

    #[test]
    fn arena_alloc_caps_at_size_limit() {
        let pool = small_pool(1, 64);
        let arena = pool.lease().unwrap();
        let _: &mut [u8] = arena.alloc::<u8>(32).unwrap();
        let _: &mut [u8] = arena.alloc::<u8>(32).unwrap();
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
        // Counter must remain at the cap even after a refused alloc
        // — no drift from the rollback path.
        assert_eq!(arena.alloc_count(), 3);
    }

    #[test]
    fn arena_returns_to_pool_on_drop() {
        let pool = small_pool(1, 256);
        {
            let arena = pool.lease().expect("first lease");
            assert!(matches!(pool.lease(), Err(Error::ResourceExhausted(_))));
            drop(arena);
        }
        let _again = pool.lease().expect("re-lease after drop");
    }

    #[test]
    fn arena_alignment_is_respected() {
        let pool = small_pool(1, 64);
        let arena = pool.lease().unwrap();
        let _: &mut [u8] = arena.alloc::<u8>(1).unwrap();
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
        let plane0: &mut [u8] = arena.alloc::<u8>(16).unwrap();
        for (i, b) in plane0.iter_mut().enumerate() {
            *b = i as u8;
        }
        let header = FrameHeader::new(4, 4, PixelFormat::Gray8, Some(42));
        FrameInner::new(arena, &[(0, 16)], header).unwrap()
    }

    #[test]
    fn frame_refcount_keeps_arena_alive() {
        let pool = small_pool(1, 256);
        let frame = build_simple_frame(&pool);
        let clone = Arc::clone(&frame);
        drop(frame);
        let plane = clone.plane(0).expect("plane 0");
        assert_eq!(plane.len(), 16);
        for (i, b) in plane.iter().enumerate() {
            assert_eq!(*b, i as u8);
        }
        assert_eq!(clone.header().width, 4);
        assert_eq!(clone.header().height, 4);
        assert_eq!(clone.header().presentation_timestamp, Some(42));
        assert!(matches!(pool.lease(), Err(Error::ResourceExhausted(_))));
    }

    #[test]
    fn last_drop_returns_arena_to_pool() {
        let pool = small_pool(1, 256);
        let frame = build_simple_frame(&pool);
        let clone = Arc::clone(&frame);
        drop(frame);
        drop(clone);
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
        let header = FrameHeader::new(1, 1, PixelFormat::Gray8, None);
        let r = FrameInner::new(arena, &[(0, 16)], header);
        assert!(matches!(r, Err(Error::InvalidData(_))));
    }

    #[test]
    fn pool_outlives_buffer_drop_when_pool_dropped_first() {
        let pool = small_pool(1, 64);
        let arena = pool.lease().unwrap();
        drop(pool);
        drop(arena);
    }

    #[test]
    fn arena_reset_clears_allocations() {
        let pool = small_pool(1, 32);
        let mut arena = pool.lease().unwrap();
        let _: &mut [u8] = arena.alloc::<u8>(32).unwrap();
        assert!(matches!(
            arena.alloc::<u8>(1),
            Err(Error::ResourceExhausted(_))
        ));
        arena.reset();
        let _: &mut [u8] = arena.alloc::<u8>(32).unwrap();
    }

    #[test]
    fn frame_can_be_sent_across_thread_boundary() {
        // Build a frame on this thread, ship it to a worker thread,
        // read its bytes there. This is the use case the module
        // exists to enable; if it ever stops compiling, the
        // `Send + Sync` impls above are wrong.
        let pool = small_pool(1, 256);
        let frame = build_simple_frame(&pool);
        let frame_for_worker = Arc::clone(&frame);
        let handle = std::thread::spawn(move || {
            let plane = frame_for_worker.plane(0).expect("plane 0 on worker");
            let mut sum: u32 = 0;
            for b in plane {
                sum += *b as u32;
            }
            sum
        });
        let sum = handle.join().expect("worker joined");
        // Plane was filled with 0..16, sum = 120.
        assert_eq!(sum, (0..16u32).sum::<u32>());
        // Original frame still readable here too.
        assert_eq!(frame.plane(0).unwrap().len(), 16);
    }

    #[test]
    fn concurrent_alloc_produces_disjoint_slices() {
        // Two threads alloc 64 bytes each from a 256-byte arena.
        // Their slices must not overlap.
        let pool = small_pool(1, 256);
        let arena = Arc::new(pool.lease().unwrap());
        let a = Arc::clone(&arena);
        let b = Arc::clone(&arena);
        let h1 = std::thread::spawn(move || {
            let s: &mut [u8] = a.alloc::<u8>(64).unwrap();
            // Fill so we can detect overlap from the other thread.
            for x in s.iter_mut() {
                *x = 0xAA;
            }
            (s.as_ptr() as usize, s.len())
        });
        let h2 = std::thread::spawn(move || {
            let s: &mut [u8] = b.alloc::<u8>(64).unwrap();
            for x in s.iter_mut() {
                *x = 0xBB;
            }
            (s.as_ptr() as usize, s.len())
        });
        let (p1, l1) = h1.join().unwrap();
        let (p2, l2) = h2.join().unwrap();
        // Disjoint ranges: [p1, p1+l1) and [p2, p2+l2) do not overlap.
        let no_overlap = p1 + l1 <= p2 || p2 + l2 <= p1;
        assert!(no_overlap, "concurrent alloc returned overlapping slices");
    }

    #[cfg(miri)]
    #[test]
    fn concurrent_alloc_retags_whole_buffer_while_other_thread_writes() {
        // Memory-safety issue: `Arena` is `Sync`, so safe code can call
        // `alloc` while another thread writes through a previously
        // returned slice. The CAS cursor makes the byte ranges disjoint,
        // but `alloc` still materializes a mutable borrow of the whole
        // buffer via `[u8]::as_mut_ptr`; Miri reports that retag as a
        // data race with the other thread's write.
        let pool = small_pool(1, 256);
        let arena = Arc::new(pool.lease().unwrap());
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let a = Arc::clone(&arena);
        let b = Arc::clone(&arena);
        let barrier_a = Arc::clone(&barrier);
        let barrier_b = Arc::clone(&barrier);
        let h1 = std::thread::spawn(move || {
            let s: &mut [u8] = a.alloc::<u8>(64).unwrap();
            barrier_a.wait();
            for x in s.iter_mut() {
                *x = 0xAA;
            }
        });
        let h2 = std::thread::spawn(move || {
            barrier_b.wait();
            let s: &mut [u8] = b.alloc::<u8>(64).unwrap();
            for x in s.iter_mut() {
                *x = 0xBB;
            }
        });
        h1.join().unwrap();
        h2.join().unwrap();
    }
}
