//! Driving a spawned downloader to completion.
//!
//! Every downloader MediaHarbor shells out to — gamdl, votify, yt-dlp, OrpheusDL —
//! is watched the same way: poll its output, decide what each line means, notice when
//! it has gone quiet, and turn its exit code into a result. Only the *deciding* differs
//! per tool, so that is the half a [`ProcessDriver`] supplies and this is the half
//! written once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::errors::{MhError, MhResult};
use crate::subprocess::{LineSource, SubprocessHandle};

/// Floor between progress events. A tool that redraws its bar in place produces these
/// far faster than the UI can use them.
const PROGRESS_THROTTLE: Duration = Duration::from_millis(250);

/// What one output line means for the run.
pub enum LineEffect<P> {
    /// Nothing to report.
    None,
    /// Publish this progress. `important` bypasses the throttle window — an item
    /// boundary has to land, a redrawn percentage does not.
    Progress { progress: P, important: bool },
    /// Kill the child and fail the run with this.
    Fatal(MhError),
}

impl<P> LineEffect<P> {
    pub fn progress(progress: P) -> Self {
        Self::Progress {
            progress,
            important: false,
        }
    }

    pub fn important(progress: P) -> Self {
        Self::Progress {
            progress,
            important: true,
        }
    }
}

/// What to do when nothing has been printed for a while.
pub enum IdleAction {
    /// Keep waiting.
    Wait,
    /// The tool is blocked on stdin. These lines are the question to put to the user.
    Prompt(Vec<String>),
    /// Give up: kill the child and fail with this message.
    Stall(String),
}

/// The per-tool half of driving a subprocess: what its output means.
pub trait ProcessDriver: Send {
    /// What a fold of this tool's output reports to the UI.
    type Progress: Send + 'static;

    /// Fold one output line.
    fn feed(&mut self, source: LineSource, line: &str) -> LineEffect<Self::Progress>;

    /// Called on every tick where nothing arrived, with the time since the last
    /// output. Owning this decision per tool is what lets yt-dlp vary its ceiling with
    /// aria2c and post-processing state, and OrpheusDL wait on a prompt, without the
    /// loop knowing about either.
    fn on_idle(&mut self, _idle: Duration) -> IdleAction {
        IdleAction::Wait
    }

    /// Turn the exit code into the most specific error the output can justify, or into
    /// the closing progress. `None` for a tool whose last real line already said all
    /// there was to say.
    fn finish(&mut self, exit_code: i32) -> MhResult<Option<Self::Progress>>;
}

/// How a driver's prompts get answered. Absent when the tool has no way to be answered,
/// which turns any prompt into a failure rather than a hang.
pub struct Prompts<'a> {
    pub replies: &'a mut mpsc::Receiver<String>,
    pub ask: &'a (dyn Fn(Vec<String>) + Send + Sync),
}

/// A spawned child and the channel carrying its output.
pub struct Running<'a> {
    pub handle: &'a mut SubprocessHandle,
    pub lines: &'a mut mpsc::Receiver<(LineSource, String)>,
    /// How long to block on the channel before giving the driver an idle tick.
    pub poll: Duration,
    pub prompts: Option<Prompts<'a>>,
}

impl<'a> Running<'a> {
    pub fn new(
        handle: &'a mut SubprocessHandle,
        lines: &'a mut mpsc::Receiver<(LineSource, String)>,
        poll: Duration,
    ) -> Self {
        Self {
            handle,
            lines,
            poll,
            prompts: None,
        }
    }

    pub fn answered_by(mut self, prompts: Prompts<'a>) -> Self {
        self.prompts = Some(prompts);
        self
    }
}

/// Run a spawned child to completion under `driver`.
pub async fn drive<D: ProcessDriver>(
    driver: &mut D,
    mut run: Running<'_>,
    cancel: &Arc<AtomicBool>,
    on_progress: impl Fn(D::Progress),
    on_log: impl Fn(String),
) -> MhResult<()> {
    let mut last_emit = Instant::now();
    let mut last_output = Instant::now();

    loop {
        if cancel.load(Ordering::Relaxed) {
            run.handle.kill().await;
            return Err(MhError::Cancelled);
        }

        match tokio::time::timeout(run.poll, run.lines.recv()).await {
            Ok(Some((source, line))) => {
                last_output = Instant::now();
                let effect = driver.feed(source, &line);
                on_log(line);

                match effect {
                    LineEffect::None => {}
                    LineEffect::Progress {
                        progress,
                        important,
                    } => {
                        if important || last_emit.elapsed() >= PROGRESS_THROTTLE {
                            last_emit = Instant::now();
                            on_progress(progress);
                        }
                    }
                    LineEffect::Fatal(e) => {
                        run.handle.kill().await;
                        return Err(e);
                    }
                }
            }
            Ok(None) => break,
            Err(_elapsed) => match driver.on_idle(last_output.elapsed()) {
                IdleAction::Wait => {}
                IdleAction::Stall(message) => {
                    run.handle.kill().await;
                    return Err(MhError::Subprocess(message));
                }
                IdleAction::Prompt(question) => {
                    answer_prompt(&mut run, question).await?;
                    last_output = Instant::now();
                }
            },
        }
    }

    let exit_code = run.handle.child.wait().await?.code().unwrap_or(-1);
    if let Some(closing) = driver.finish(exit_code)? {
        on_progress(closing);
    }
    Ok(())
}

/// Put the driver's question to the user and write the answer to the child's stdin.
/// A dismissed prompt writes nothing and lets the tool time out on its own terms.
async fn answer_prompt(run: &mut Running<'_>, question: Vec<String>) -> MhResult<()> {
    let Some(prompts) = run.prompts.as_mut() else {
        run.handle.kill().await;
        return Err(MhError::Subprocess(format!(
            "The downloader is waiting for input it cannot be given: {}",
            question.join(" ")
        )));
    };

    (prompts.ask)(question);
    if let (Some(reply), Some(stdin)) = (prompts.replies.recv().await, run.handle.stdin.as_mut()) {
        stdin.write_all(reply.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
    }
    Ok(())
}
