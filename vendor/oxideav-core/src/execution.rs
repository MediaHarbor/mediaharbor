//! Runtime hints passed from the executor to codecs and filters.
//!
//! An [`ExecutionContext`] carries advisory information — today only a
//! thread budget — that codecs can use to tune their internal
//! parallelism. Codecs that don't care can ignore it; the default trait
//! method on [`Decoder`](../../oxideav_codec/trait.Decoder.html) /
//! [`Encoder`](../../oxideav_codec/trait.Encoder.html) is a no-op.
//!
//! # Threading contract
//!
//! The context is the **single threading authority** for a codec:
//!
//! * A codec runs **serial until told otherwise** — before
//!   `set_execution_context` is called (or when it never is), internal
//!   fan-out is one worker.
//! * Every internal fan-out is bounded through
//!   [`ExecutionContext::effective_workers`], never by querying the host
//!   directly. Host-derived budgets are the *caller's* decision, made by
//!   constructing the context with [`ExecutionContext::auto`].
//! * Threading stays optional: a codec with no internal parallelism
//!   simply keeps the default no-op trait method, and callers must
//!   always work with a codec that runs serial regardless of the budget
//!   they granted.

/// Advisory runtime information handed to a codec after construction.
///
/// The struct is deliberately tiny for now. New fields can be added
/// without breaking API consumers that already construct the value via
/// [`ExecutionContext::serial`] or [`ExecutionContext::with_threads`].
#[derive(Clone, Debug)]
pub struct ExecutionContext {
    /// Advisory cap on how many threads a codec may use for its own
    /// internal parallelism (slice-parallel decode, GOP-parallel decode,
    /// etc.). Always `≥ 1`. `1` means "caller requests serial execution
    /// from this codec" — obey it unless you have a very good reason.
    pub threads: usize,
}

impl ExecutionContext {
    /// Ask the codec to run strictly single-threaded.
    pub const fn serial() -> Self {
        Self { threads: 1 }
    }

    /// Budget the codec to at most `threads` internal workers. Values
    /// below 1 are clamped up to 1.
    pub fn with_threads(threads: usize) -> Self {
        Self {
            threads: threads.max(1),
        }
    }

    /// Derive the budget from the host:
    /// [`std::thread::available_parallelism`], falling back to `1` when
    /// the host refuses to answer.
    ///
    /// This is the **caller-side** convenience for "use the machine".
    /// Codecs never call it — they receive whatever budget the caller
    /// chose and bound their fan-out with [`Self::effective_workers`].
    pub fn auto() -> Self {
        let threads = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1);
        Self { threads }
    }

    /// Bound a codec-internal fan-out: the number of workers to spawn
    /// for `work_units` independent units of work under this budget.
    ///
    /// Returns `min(self.threads, work_units)`, and never less than 1
    /// (`work_units == 0` still yields 1 so degenerate inputs stay on
    /// the plain serial path). This is the one clamp codecs use for
    /// every slice-/tile-/field-/GOP-parallel dispatch; querying host
    /// parallelism directly from codec code is out of contract.
    pub fn effective_workers(&self, work_units: usize) -> usize {
        self.threads.min(work_units).max(1)
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::serial()
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionContext;

    #[test]
    fn serial_is_one_thread_and_default() {
        assert_eq!(ExecutionContext::serial().threads, 1);
        assert_eq!(ExecutionContext::default().threads, 1);
    }

    #[test]
    fn with_threads_clamps_up_to_one() {
        assert_eq!(ExecutionContext::with_threads(0).threads, 1);
        assert_eq!(ExecutionContext::with_threads(1).threads, 1);
        assert_eq!(ExecutionContext::with_threads(8).threads, 8);
    }

    #[test]
    fn auto_is_at_least_one() {
        assert!(ExecutionContext::auto().threads >= 1);
    }

    #[test]
    fn effective_workers_clamps_both_sides() {
        let ctx = ExecutionContext::with_threads(4);
        assert_eq!(ctx.effective_workers(0), 1);
        assert_eq!(ctx.effective_workers(1), 1);
        assert_eq!(ctx.effective_workers(3), 3);
        assert_eq!(ctx.effective_workers(4), 4);
        assert_eq!(ctx.effective_workers(64), 4);

        let serial = ExecutionContext::serial();
        assert_eq!(serial.effective_workers(64), 1);
        assert_eq!(serial.effective_workers(0), 1);
    }
}
