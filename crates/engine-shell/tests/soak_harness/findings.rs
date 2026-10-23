//! Findings and their signatures, and the panic hook that captures a
//! panicking run's message and location.

use super::*;

// ---------------------------------------------------------------------------
// Findings
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(super) struct Finding {
    pub(super) detector: &'static str,
    /// The scene the run started in.
    pub(super) start_scene: String,
    /// The scene the world was in when the detector fired.
    pub(super) scene: String,
    pub(super) mode: String,
    pub(super) frame: u64,
    /// Detector-specific location: a panic's `file:line`, a softlock's
    /// holder, a value check's field.
    pub(super) location: String,
    pub(super) detail: String,
}

impl Finding {
    pub(super) fn signature(&self) -> String {
        format!("{}|{}|{}", self.detector, self.scene, self.location)
    }
    /// Rank bucket: lower is worse.
    pub(super) fn rank(&self) -> u8 {
        match self.detector {
            "panic" | "hang" | "tick_error" => 0,
            "softlock" | "menu_stuck" | "battle_endless" | "dropped_to_title" => {
                if self.location.starts_with("free-roam") {
                    2
                } else {
                    1
                }
            }
            "unknown_scene" | "door_loop" | "wipe_continue" => 1,
            "script_stall" | "control_starved" | "confined" | "timeline_cap" => 2,
            "battle_loop" => 1,
            _ => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Panic capture
// ---------------------------------------------------------------------------

thread_local! {
    pub(super) static LAST_PANIC: std::cell::RefCell<Option<(String, String, String)>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) static HOOK: Once = Once::new();

pub(super) fn install_panic_hook() {
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let in_soak = std::thread::current()
                .name()
                .is_some_and(|n| n.starts_with("soak-"));
            if !in_soak {
                default(info);
                return;
            }
            let loc = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "?".into());
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string payload>".into()
            };
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            let frames: Vec<&str> = bt
                .lines()
                .map(str::trim)
                .filter(|l| l.contains("legaia_") && !l.contains("soak_harness"))
                .take(6)
                .collect();
            LAST_PANIC.with(|p| {
                *p.borrow_mut() = Some((loc, msg, frames.join(" <- ")));
            });
        }));
    });
}
