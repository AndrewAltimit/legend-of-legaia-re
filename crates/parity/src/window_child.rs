//! Spawning a `play-window` child that survives the X11 startup race.
//!
//! On an X11 display, `winit` 0.30's `EventLoop::new` selects XInput2 raw
//! pointer / key events on the root window and only then makes its first
//! libXi call (`XIQueryDevice`). That first call initialises libXi's
//! per-display state: `_XiCheckExtInit` `malloc`s the extension record and
//! fills its `vers` pointer from an `XGetExtensionVersion` round trip. A raw
//! event read off the socket inside that round trip is converted on the spot
//! (`_XEnq` -> `XInputWireToCookie` -> `wireToRawEvent`), and the converter
//! asks `_XiCheckVersion`, which dereferences the not-yet-written `vers`. When
//! the allocator handed back a recycled chunk, that is garbage and the
//! process dies with `SIGSEGV` before it has logged a line.
//!
//! Anything that moves a pointer or presses a key on the display during
//! those few hundred microseconds is enough: a person at the desktop, a
//! remote-desktop input device. The defect is in libXi (as shipped by the
//! distribution) and `winit`'s ordering, not in the engine, so the capture
//! drivers retry exactly that death once instead of reading it as a failed
//! frame. `docs/tooling/host-drift.md` carries the full account.

use std::io;
use std::process::{Command, Output};

/// The line `play-window` logs once its event loop exists, i.e. once the
/// startup race is behind it. A `SIGSEGV` with this line on stderr is a
/// different crash and is never retried.
pub const EVENT_LOOP_OPEN_MARKER: &str = "play-window: event loop open";

/// `SIGSEGV`'s number on every platform the window runs on.
#[cfg(unix)]
const SIGSEGV: i32 = 11;

/// Whether `out` is a child that died of the X11 startup race: killed by
/// `SIGSEGV` before its event loop was open.
pub fn died_in_startup_race(out: &Output) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        out.status.signal() == Some(SIGSEGV)
            && !String::from_utf8_lossy(&out.stderr).contains(EVENT_LOOP_OPEN_MARKER)
    }
    #[cfg(not(unix))]
    {
        let _ = out;
        false
    }
}

/// [`Command::output`], run exactly once more (with the reason on stderr)
/// when the first child died in the X11 startup race. Any other outcome,
/// including a second startup death, is returned as it is.
pub fn output_retrying_startup_race(cmd: &mut Command, label: &str) -> io::Result<Output> {
    let out = cmd.output()?;
    if !died_in_startup_race(&out) {
        return Ok(out);
    }
    eprintln!(
        "[retry] {label}: play-window died of SIGSEGV before its event loop opened (the \
         libXi first-use race against XInput2 raw events, docs/tooling/host-drift.md); \
         running it once more"
    );
    cmd.output()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn output(status: ExitStatus, stderr: &str) -> Output {
        Output {
            status,
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn a_segfault_before_the_marker_is_the_startup_race() {
        // A raw wait status of 11 is "killed by signal 11, no core flag".
        assert!(died_in_startup_race(&output(
            ExitStatus::from_raw(SIGSEGV),
            ""
        )));
        // With the core-dump bit set (0x80), as the CI runner reports it.
        assert!(died_in_startup_race(&output(
            ExitStatus::from_raw(SIGSEGV | 0x80),
            ""
        )));
    }

    #[test]
    fn a_segfault_after_the_marker_is_not_retried() {
        let stderr = format!("[INFO] {EVENT_LOOP_OPEN_MARKER}\n[INFO] scene entered\n");
        assert!(!died_in_startup_race(&output(
            ExitStatus::from_raw(SIGSEGV),
            &stderr
        )));
    }

    #[test]
    fn other_exits_are_not_retried() {
        // Exit code 1 (wait status 1 << 8) and SIGABRT (6).
        assert!(!died_in_startup_race(&output(
            ExitStatus::from_raw(1 << 8),
            ""
        )));
        assert!(!died_in_startup_race(&output(ExitStatus::from_raw(6), "")));
        assert!(!died_in_startup_race(&output(ExitStatus::from_raw(0), "")));
    }

    #[test]
    fn a_child_that_dies_twice_is_returned_after_one_retry() {
        // `kill -SEGV $$` dies of SIGSEGV with no marker on stderr: the
        // startup-race shape, every time. Each run appends a line, so the
        // file counts the launches: one retry, then the death is returned.
        let runs = std::env::temp_dir().join(format!(
            "legaia-window-child-retry-{}.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&runs);
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("echo run >> \"$1\"; kill -SEGV $$")
            .arg("sh")
            .arg(&runs);
        let out = output_retrying_startup_race(&mut cmd, "test").expect("spawn sh");
        let launches = std::fs::read_to_string(&runs).unwrap_or_default();
        let _ = std::fs::remove_file(&runs);
        assert_eq!(out.status.signal(), Some(SIGSEGV));
        assert_eq!(launches.lines().count(), 2, "exactly one retry");
    }
}
