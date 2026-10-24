//! A panic on any thread of a running game ends the game.
//!
//! The play window runs code of ours on threads it does not own - the audio
//! mixer inside cpal's output callback, the controller reader - and a panic
//! there kills only that thread: the window plays on, silent or deaf to the
//! pad, and the launcher waiting on the process never learns anything
//! happened. With [`install`], the panic message prints as usual (the
//! launcher's log picks it up, `failure_detail`) and the process aborts,
//! which the launcher reports as a crash.
//!
//! What this is not about: a `kill -SEGV` / `kill -ABRT` sent to a running
//! game can be survived, and that is not a hang. The Rust runtime's
//! stack-overflow handler owns SIGSEGV and, for a signal that is not a guard
//! page fault, restores the default action and returns (a real fault then
//! faults again and kills the process; a sent one is simply spent). NVIDIA's
//! shader compiler (`libnvidia-gpucomp`) installs a SIGABRT handler of its
//! own the first time it compiles a shader with no disk cache to read; a real
//! `abort()` still ends the process, since glibc re-raises with the default
//! action when a handler returns.

/// Whether a panic on the thread named `name` should end the process: any
/// thread but the main one, whose panic already unwinds out of `main` and
/// exits with status 101.
pub(crate) fn is_fatal(name: Option<&str>) -> bool {
    name != Some("main")
}

/// Make a panic on any non-main thread abort the process, after the
/// standard hook has printed it.
pub(crate) fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        if is_fatal(std::thread::current().name()) {
            eprintln!("Error: a game thread panicked; the game stops so the crash is reported");
            std::process::abort();
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_non_main_thread_panic_is_fatal() {
        assert!(!is_fatal(Some("main")));
        assert!(is_fatal(Some("cpal_alsa_out")));
        assert!(is_fatal(None));
    }

    /// The child half of the test below: installs the hook and panics on
    /// a spawned thread while the main thread waits forever.
    #[test]
    fn child_panics_on_a_background_thread() {
        if std::env::var_os("LEGAIA_FATAL_PANIC_CHILD").is_none() {
            return;
        }
        install();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(|| panic!("mixer index out of range"))
            .unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    /// A background-thread panic ends the process instead of leaving the
    /// main thread running.
    #[cfg(unix)]
    #[test]
    fn a_background_thread_panic_ends_the_process() {
        use std::os::unix::process::ExitStatusExt;
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fatal_panic::tests::child_panics_on_a_background_thread",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("LEGAIA_FATAL_PANIC_CHILD", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.signal(), Some(6), "{out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("mixer index out of range"), "{err}");
    }
}
