//! The line dialect the managed Python downloaders print.
//!
//! gamdl and votify are by the same author and share an output vocabulary — the same
//! `[Track n/N]` headers, `Downloading "…"` / `Skipping "…": reason` lines and
//! `[download] NN%` bars. Folding one line at a time into [`BatchState`] keeps the
//! parsing synchronous and testable; [`super::driver`] owns the process and the clock.

use std::collections::VecDeque;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use crate::downloads::BatchProgress;
use crate::errors::{MhError, MhResult};
use crate::services::common::cli::driver::{IdleAction, LineEffect, ProcessDriver};
use crate::subprocess::{strip_ansi, LineSource};

/// Silence long enough to mean the tool is wedged rather than working.
const STALL_TIMEOUT: Duration = Duration::from_secs(300);

const STALL_MESSAGE: &str = "The downloader stopped responding (no output for 5 minutes) and was stopped. It may be waiting on input or stuck — check your cookies/credentials and try again.";

/// How many trailing lines to keep for the "it exited non-zero, what did it say?"
/// message. Enough to carry a Python traceback's tail without holding the whole run.
const TAIL_LINES: usize = 5;

static PROGRESS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[download\]\s+(\d+(?:\.\d+)?)%").unwrap());

/// Both spellings of the same "item n of N" header: gamdl counts tracks, votify counts
/// the URLs it was handed.
static ITEM_COUNT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(?:Track|URL)\s+(\d+)/(\d+)\]").unwrap());

static FOUND_TRACKS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Found\s+(\d+)\s+tracks?").unwrap());

static DOWNLOADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"Downloading\s+"(.+)""#).unwrap());

static SKIPPING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"Skipping\s+"(.+?)":\s*(.+)"#).unwrap());

static FINISHED_ERRORS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Finished with (\d+) error").unwrap());

static OAUTH_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https://accounts\.spotify\.com/authorize[^\s]+").unwrap());

static DOES_NOT_EXIST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"does not exist").unwrap());

#[derive(Debug, Clone)]
pub enum InteractivePromptType {
    CookiesDialog,
    WvdDialog,
    OAuthUrl(String),
    InteractiveSelection(String),
}

/// Spot a line where the CLI has stopped to ask a terminal question. These tools run
/// with no attached TTY, so anything they wait on here would hang until the stall
/// timeout — every prompt has to become an error the user can act on instead.
pub fn detect_interactive_prompt(line: &str) -> Option<InteractivePromptType> {
    if DOES_NOT_EXIST_RE.is_match(line) && line.to_lowercase().contains("press enter to continue") {
        if line.to_lowercase().contains("cookies") {
            return Some(InteractivePromptType::CookiesDialog);
        } else {
            return Some(InteractivePromptType::WvdDialog);
        }
    }

    if line.contains("Click on the following link to login:") {
        return None; // The URL will be on a following line
    }
    if let Some(caps) = OAUTH_URL_RE.captures(line) {
        return Some(InteractivePromptType::OAuthUrl(caps[0].to_string()));
    }

    if line.contains("Select which") && (line.contains("to download") || line.contains("codec")) {
        return Some(InteractivePromptType::InteractiveSelection(
            line.to_string(),
        ));
    }

    None
}

/// Running tally of a batch, folded from the CLI's output.
pub struct BatchState {
    total: u32,
    completed: u32,
    current_track: String,
    skipped: Vec<(String, String)>,
    /// Percent banked from items that are no longer current, and the current item's
    /// last-seen percent. Together they are the batch's position between item
    /// boundaries, which is all the sum over per-track percentages ever computed.
    settled_pct: f32,
    current_pct: f32,
    /// The three things `finish` needs from the output, kept as it streams rather than
    /// by retaining every line of a run that can print megabytes.
    error_count: Option<u32>,
    tail: VecDeque<String>,
    last_complaint: Option<String>,
}

impl Default for BatchState {
    fn default() -> Self {
        Self::new()
    }
}

impl BatchState {
    pub fn new() -> Self {
        Self {
            total: 0,
            completed: 0,
            current_track: String::from("Unknown Track"),
            skipped: Vec::new(),
            settled_pct: 0.0,
            current_pct: 0.0,
            error_count: None,
            tail: VecDeque::with_capacity(TAIL_LINES),
            last_complaint: None,
        }
    }

    fn snapshot(&self, percent: f32) -> BatchProgress {
        BatchProgress {
            completed: self.completed,
            total: self.total,
            current_track: self.current_track.clone(),
            percent,
            ..Default::default()
        }
    }

    /// Batch position from settled items alone, held below 100 so the bar cannot claim
    /// completion before the process has actually exited.
    fn settled_percent(&self) -> f32 {
        if self.total > 0 {
            (self.completed as f32 / self.total as f32 * 100.0).min(99.0)
        } else {
            0.0
        }
    }

    fn remember(&mut self, line: &str) {
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line.to_string());
        if line.contains("[CRITICAL") || line.contains("[ERROR") {
            self.last_complaint = Some(line.to_string());
        }
    }

    fn skip_report(&self) -> String {
        self.skipped
            .iter()
            .map(|(t, r)| format!("\"{}\" skipped: {}", t, r))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn fold(&mut self, raw: &str) -> LineEffect<BatchProgress> {
        self.remember(raw);

        let stripped = strip_ansi(raw);
        let line = stripped.trim_start_matches("Error: ").trim();

        if let Some(prompt) = detect_interactive_prompt(line) {
            return match prompt {
                InteractivePromptType::InteractiveSelection(_) => LineEffect::Fatal(
                    MhError::Subprocess("Interactive selection not supported".into()),
                ),
                InteractivePromptType::CookiesDialog => LineEffect::Fatal(MhError::Auth(
                    "The cookies file is missing or invalid. Import valid cookies in Settings and try again.".into(),
                )),
                InteractivePromptType::WvdDialog => LineEffect::Fatal(MhError::Auth(
                    "The Widevine device (.wvd) file is missing or invalid. Add it in Settings and try again.".into(),
                )),
                // The login URL is already in the log the user can read; there is no
                // stdin to answer it on, so the run continues and either succeeds from
                // a cached token or fails on its own.
                InteractivePromptType::OAuthUrl(_) => LineEffect::None,
            };
        }

        let mut effect = LineEffect::None;

        if let Some(caps) = ITEM_COUNT_RE.captures(line) {
            self.total = caps[2].parse().unwrap_or(self.total);
            let idx: u32 = caps[1].parse().unwrap_or(1);
            self.completed = idx.saturating_sub(1);
            effect = LineEffect::important(self.snapshot(self.settled_percent()));
        }

        if let Some(caps) = FOUND_TRACKS_RE.captures(line) {
            self.total = caps[1].parse().unwrap_or(self.total);
        }

        if let Some(caps) = DOWNLOADING_RE.captures(line) {
            // The outgoing item keeps whatever percent it reached.
            self.settled_pct += self.current_pct;
            self.current_pct = 0.0;
            self.current_track = caps[1].trim().to_string();
        }

        if let Some(caps) = SKIPPING_RE.captures(line) {
            self.skipped
                .push((caps[1].trim().to_string(), caps[2].trim().to_string()));
            self.completed = self.completed.saturating_add(1).min(self.total);
            // A skipped item contributes nothing, not the fraction it had reached.
            self.current_pct = 0.0;
        }

        if line.contains("Finished with") {
            self.completed = self.total;
            self.settled_pct = 0.0;
            self.current_pct = 0.0;
            if let Some(caps) = FINISHED_ERRORS_RE.captures(line) {
                self.error_count = caps[1].parse().ok();
            }
        }

        if let Some(caps) = PROGRESS_RE.captures(line) {
            let pct: f32 = caps[1].parse().unwrap_or(0.0);
            self.current_pct = pct;

            let summed = self.settled_pct + self.current_pct;
            let overall = if self.total > 0 {
                (summed / (self.total as f32 * 100.0) * 100.0).min(100.0)
            } else {
                pct
            };
            effect = LineEffect::progress(self.snapshot(overall));
        }

        effect
    }
}

impl ProcessDriver for BatchState {
    type Progress = BatchProgress;

    fn feed(&mut self, _source: LineSource, line: &str) -> LineEffect<BatchProgress> {
        self.fold(line)
    }

    fn on_idle(&mut self, idle: Duration) -> IdleAction {
        if idle > STALL_TIMEOUT {
            IdleAction::Stall(STALL_MESSAGE.to_string())
        } else {
            IdleAction::Wait
        }
    }

    /// Turn the finished run into either the closing 100 % progress or the most
    /// specific error the output can justify.
    fn finish(&mut self, exit_code: i32) -> MhResult<Option<BatchProgress>> {
        if let Some(n) = self.error_count.filter(|n| *n > 0) {
            let message = if self.skipped.is_empty() {
                format!("Finished with {} error(s)", n)
            } else {
                self.skip_report()
            };
            return Err(MhError::Subprocess(message));
        }

        if exit_code != 0 {
            return Err(MhError::Subprocess(format!(
                "Process exited with code {}: {}",
                exit_code,
                self.tail
                    .iter()
                    .rev()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            )));
        }

        if !self.skipped.is_empty() && self.completed == 0 {
            return Err(MhError::Subprocess(format!(
                "All tracks skipped: {}",
                self.skip_report()
            )));
        }

        // Nothing was ever counted, so the run cannot have produced a file even though
        // it exited cleanly. Surface whatever the tool last complained about.
        if self.total == 0 && self.completed == 0 {
            if let Some(message) = &self.last_complaint {
                return Err(MhError::Subprocess(message.trim().to_string()));
            }
        }

        Ok(Some(BatchProgress {
            completed: self.total.max(self.completed),
            total: self.total.max(1),
            current_track: self.current_track.clone(),
            percent: 100.0,
            ..Default::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fold one line, ignoring whatever it reported.
    fn feed(state: &mut BatchState, line: &str) {
        state.fold(line);
    }

    /// Fold one line that must report progress, and return it.
    fn progress(state: &mut BatchState, line: &str) -> BatchProgress {
        match state.fold(line) {
            LineEffect::Progress { progress, .. } => progress,
            LineEffect::None => panic!("expected progress from {line:?}"),
            LineEffect::Fatal(e) => panic!("expected progress from {line:?}, got {e}"),
        }
    }

    #[test]
    fn detect_cookies_dialog() {
        let line = "cookies file does not exist, press enter to continue";
        let result = detect_interactive_prompt(line);
        assert!(matches!(result, Some(InteractivePromptType::CookiesDialog)));
    }

    #[test]
    fn detect_wvd_dialog() {
        let line = "WVD file does not exist, press enter to continue";
        let result = detect_interactive_prompt(line);
        assert!(matches!(result, Some(InteractivePromptType::WvdDialog)));
    }

    #[test]
    fn detect_oauth_url() {
        let line = "https://accounts.spotify.com/authorize?client_id=abc&response_type=code";
        let result = detect_interactive_prompt(line);
        assert!(matches!(result, Some(InteractivePromptType::OAuthUrl(_))));
    }

    #[test]
    fn detect_interactive_selection() {
        let line = "Select which codec to download:";
        let result = detect_interactive_prompt(line);
        assert!(matches!(
            result,
            Some(InteractivePromptType::InteractiveSelection(_))
        ));
    }

    #[test]
    fn no_prompt_for_normal_line() {
        let line = "[download]  50.3% of 10.00MiB at 1.23MiB/s ETA 00:04";
        assert!(detect_interactive_prompt(line).is_none());
    }

    #[test]
    fn missing_cookies_is_fatal_rather_than_a_hang() {
        let mut state = BatchState::new();
        let effect = state.fold("Error: cookies file does not exist, press enter to continue");
        assert!(matches!(effect, LineEffect::Fatal(MhError::Auth(_))));
    }

    #[test]
    fn track_header_reports_settled_items_only() {
        let mut state = BatchState::new();
        let p = progress(&mut state, "[Track 3/8] Downloading");
        assert_eq!((p.completed, p.total), (2, 8));
        assert!((p.percent - 25.0).abs() < 0.01, "got {}", p.percent);
    }

    #[test]
    fn the_last_item_header_counts_it_as_unfinished() {
        let mut state = BatchState::new();
        let p = progress(&mut state, "[URL 9/9]");
        // The ninth item has started, not settled, so eight of nine are done.
        assert_eq!((p.completed, p.total), (8, 9));
        assert!(p.percent < 100.0, "got {}", p.percent);
    }

    #[test]
    fn an_item_header_can_never_report_the_batch_as_complete() {
        let mut state = BatchState::new();
        // A tool that numbers past its own total must still leave room for the finish.
        let p = progress(&mut state, "[Track 12/10]");
        assert_eq!(p.percent, 99.0);
    }

    #[test]
    fn ansi_escapes_do_not_hide_a_header() {
        let mut state = BatchState::new();
        let p = progress(&mut state, "\x1b[1;32m[Track 1/4]\x1b[0m Downloading");
        assert_eq!(p.total, 4);
    }

    #[test]
    fn in_flight_percentages_sum_across_the_batch() {
        let mut state = BatchState::new();
        feed(&mut state, "Found 4 tracks");
        feed(&mut state, "Downloading \"One\"");
        let p = progress(&mut state, "[download]  50.0% of 10.00MiB");
        // One of four tracks half done is an eighth of the batch.
        assert!((p.percent - 12.5).abs() < 0.01, "got {}", p.percent);
        assert_eq!(p.current_track, "One");
    }

    #[test]
    fn a_skip_advances_the_batch_and_drops_its_partial_progress() {
        let mut state = BatchState::new();
        feed(&mut state, "Found 2 tracks");
        feed(&mut state, "Downloading \"One\"");
        feed(&mut state, "[download]  40.0% of 10.00MiB");
        feed(&mut state, "Skipping \"One\": already exists");
        let p = progress(&mut state, "[download]  0.0% of 10.00MiB");
        assert_eq!(p.completed, 1);
        // The skipped track's 40% must not still be counted.
        assert!(p.percent < 1.0, "got {}", p.percent);
    }

    #[test]
    fn a_clean_run_finishes_at_full() {
        let mut state = BatchState::new();
        feed(&mut state, "Found 3 tracks");
        feed(&mut state, "[Track 3/3]");
        let p = state
            .finish(0)
            .expect("clean run")
            .expect("closing progress");
        assert_eq!((p.completed, p.total, p.percent), (3, 3, 100.0));
    }

    #[test]
    fn reported_errors_name_the_tracks_that_were_skipped() {
        let mut state = BatchState::new();
        feed(&mut state, "Found 2 tracks");
        feed(&mut state, "Skipping \"One\": not available in your region");
        feed(&mut state, "Finished with 1 error");
        let err = state.finish(0).unwrap_err().to_string();
        assert!(err.contains("One"), "got {err}");
        assert!(err.contains("not available in your region"), "got {err}");
    }

    #[test]
    fn a_nonzero_exit_carries_the_tail_of_the_output() {
        let mut state = BatchState::new();
        feed(&mut state, "something went wrong");
        let err = state.finish(2).unwrap_err().to_string();
        assert!(err.contains("code 2"), "got {err}");
        assert!(err.contains("something went wrong"), "got {err}");
    }

    #[test]
    fn a_clean_exit_that_downloaded_nothing_surfaces_the_last_complaint() {
        let mut state = BatchState::new();
        feed(&mut state, "[ERROR   ] Track is not available");
        let err = state.finish(0).unwrap_err().to_string();
        assert!(err.contains("Track is not available"), "got {err}");
    }
}
