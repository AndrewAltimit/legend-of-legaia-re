//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

/// Thin shim that opens a `play-window` session with the pad-capture
/// hook armed. Identical UX to `play-window`; the only added behaviour
/// is that every pad-mask transition is appended to a `Vec<PadEvent>`
/// on `PlayWindowApp` and flushed to `out` as a `j-replay-v1` file on
/// window close.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_record(
    out: &Path,
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    save_dir: &Path,
    scenario: Option<&str>,
    rng_seed: u32,
) -> Result<()> {
    cmd_play_window_with_record(
        scene,
        extracted_root,
        disc,
        enable_audio,
        world_map,
        None,
        false,
        save_dir,
        None,
        None,
        false,
        false,
        false,
        None,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
        None,
        false,
        false, // dynamic_lighting: replays stay on the faithful render
        Some(RecordTarget {
            out: out.to_path_buf(),
            scenario: scenario.map(str::to_string),
            rng_seed,
        }),
    )
}

/// Bundle of "where to write the captured replay + how to label it".
/// Threaded through into [`PlayWindowApp::record_log`] so the keyboard
/// handler can append events and the close handler can flush.
pub(super) struct RecordTarget {
    out: std::path::PathBuf,
    scenario: Option<String>,
    rng_seed: u32,
}

/// Frames between periodic durability checkpoints when no pad input is
/// arriving (~1 s at the 100 Hz sim rate). A pad transition checkpoints
/// on the next redraw regardless of this interval.
const CHECKPOINT_EVERY_FRAMES: u64 = 100;

/// Per-tick recorded-pad-event buffer + flush state. Lives on
/// [`PlayWindowApp`] when the user invoked the `record` subcommand;
/// `None` for plain `play-window` runs so the keyboard handler pays
/// nothing in the common case.
///
/// Durability: the replay file is not only written on clean window
/// close - `observe_frame` checkpoints the complete file to disk
/// whenever there are unwritten events (and at least once every
/// [`CHECKPOINT_EVERY_FRAMES`] frames to keep `meta.frames` fresh), so
/// a session killed by Ctrl-C / SIGTERM still leaves a valid replay
/// covering everything up to the last checkpoint. Only SIGKILL between
/// checkpoints can lose the final ~second.
pub(super) struct RecordLog {
    out_path: std::path::PathBuf,
    events: Vec<PadEvent>,
    /// Previous pad value the log saw. The keyboard handler dedups so a
    /// stream of "press, press, press" key events from auto-repeat
    /// collapses to a single PadEvent.
    last_pad: u16,
    scenario: Option<String>,
    rng_seed: u32,
    /// Highest frame index observed during the run. Used to populate
    /// `meta.frames` so the on-disk file faithfully describes the
    /// recorded duration.
    last_frame: u64,
    /// `meta.frames` value of the last on-disk checkpoint (`None` =
    /// nothing written yet - the first `observe_frame` writes).
    checkpointed_frame: Option<u64>,
    /// Events recorded since the last successful on-disk write.
    dirty: bool,
    /// Once the final flush has run, additional Close events become
    /// no-ops (winit can deliver CloseRequested + the loop's exit drop
    /// both).
    flushed: bool,
}

impl RecordLog {
    pub(super) fn from_target(target: RecordTarget) -> Self {
        Self {
            out_path: target.out,
            events: Vec::new(),
            last_pad: 0,
            scenario: target.scenario,
            rng_seed: target.rng_seed,
            last_frame: 0,
            checkpointed_frame: None,
            dirty: false,
            flushed: false,
        }
    }

    /// Record a pad transition iff `pad` differs from the previously
    /// logged value. Caller is responsible for emitting events in
    /// frame-ascending order (the keyboard handler always does).
    pub(super) fn record_transition(&mut self, frame: u64, pad: u16) {
        if pad == self.last_pad {
            return;
        }
        self.events.push(PadEvent { frame, pad });
        self.last_pad = pad;
        self.dirty = true;
        if frame > self.last_frame {
            self.last_frame = frame;
        }
    }

    /// Note the frame counter advanced past `frame` without a pad
    /// change (keeps `meta.frames` honest when the user closes the
    /// window with no input held), and opportunistically checkpoint the
    /// replay file to disk so an interrupted session stays recoverable.
    pub(super) fn observe_frame(&mut self, frame: u64) {
        if frame > self.last_frame {
            self.last_frame = frame;
        }
        if self.flushed {
            return;
        }
        let due = self.dirty
            || self
                .checkpointed_frame
                .is_none_or(|c| self.last_frame >= c + CHECKPOINT_EVERY_FRAMES);
        if !due {
            return;
        }
        match self.write_file() {
            Ok(_) => {
                self.dirty = false;
                self.checkpointed_frame = Some(self.last_frame);
            }
            Err(e) => log::warn!(
                "record: checkpoint write to {} failed: {e:#}",
                self.out_path.display()
            ),
        }
    }

    /// Serialize the current state as a complete, valid `j-replay-v1`
    /// file at `out_path`. Returns `(event count, frames covered)`.
    ///
    /// Writes via a `.tmp` sibling + rename so a kill mid-write never
    /// leaves a truncated replay - the previous checkpoint survives.
    fn write_file(&self) -> Result<(usize, u64)> {
        let meta = ReplayMeta {
            schema: legaia_engine_shell::replay::REPLAY_SCHEMA_V1.to_string(),
            scenario: self.scenario.clone(),
            rng_seed: self.rng_seed,
            frames: self.last_frame,
        };
        let mut file = ReplayFile::new(meta);
        file.events = self.events.clone();
        file.validate()?;
        let mut tmp_name = self
            .out_path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "replay".into());
        tmp_name.push(".tmp");
        let tmp = self.out_path.with_file_name(tmp_name);
        file.write_to(&tmp)?;
        std::fs::rename(&tmp, &self.out_path).with_context(|| {
            format!(
                "rename replay checkpoint {} -> {}",
                tmp.display(),
                self.out_path.display()
            )
        })?;
        Ok((file.events.len(), file.meta.frames))
    }

    /// Final flush to disk on window close. Idempotent.
    pub(super) fn flush(&mut self) -> Result<()> {
        if self.flushed {
            return Ok(());
        }
        let (events, frames) = self.write_file()?;
        self.flushed = true;
        self.dirty = false;
        eprintln!(
            "record: wrote {} event(s) covering {} frame(s) -> {}",
            events,
            frames,
            self.out_path.display()
        );
        Ok(())
    }
}
