//! The Windows double-click entry: a GUI-subsystem stub that starts the
//! `legaia-engine` executable sitting next to it with no console window.
//!
//! `legaia-engine.exe` is a console program so its many CLI subcommands keep
//! ordinary terminal behaviour (the shell waits for it, pipes and exit codes
//! work, output lands before the next prompt). A console program started
//! from Explorer gets a console window of its own, though, so the release
//! archive also ships this stub as `Legend of Legaia.exe`: being a
//! `windows`-subsystem program it never gets a console, and it starts the
//! engine with `CREATE_NO_WINDOW`, so neither does the engine nor the game
//! process the launcher spawns (that one inherits the hidden console).
//!
//! Arguments are passed through, and the engine's exit status is returned.
//! On other platforms there is no console to hide; the stub just runs the
//! engine. docs/tooling/releases.md records why this is a separate stub and
//! not `windows_subsystem` on the engine itself.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::{Command, ExitCode};

#[cfg(windows)]
const ENGINE: &str = "legaia-engine.exe";
#[cfg(not(windows))]
const ENGINE: &str = "legaia-engine";

fn engine_path() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    Some(me.parent()?.join(ENGINE))
}

#[cfg(windows)]
fn configure(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: the console-subsystem child gets a console that is
    // never shown.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure(_cmd: &mut Command) {}

/// A GUI-subsystem program has nowhere to print, so failures go to a dialog.
#[cfg(windows)]
fn report(msg: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(msg), wide("Legend of Legaia"));
    const MB_ICONERROR: u32 = 0x10;
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn report(msg: &str) {
    eprintln!("{msg}");
}

fn main() -> ExitCode {
    let Some(engine) = engine_path() else {
        report("Cannot locate this program's own folder.");
        return ExitCode::FAILURE;
    };
    let mut cmd = Command::new(&engine);
    cmd.args(std::env::args_os().skip(1));
    configure(&mut cmd);
    match cmd.status() {
        Ok(status) => match status.code() {
            Some(0) => ExitCode::SUCCESS,
            Some(c) => {
                report(&format!(
                    "The engine stopped with an error (exit status {c}).\n\n\
                     Run {ENGINE} from a command prompt to see its log."
                ));
                ExitCode::from(u8::try_from(c).unwrap_or(1))
            }
            None => ExitCode::FAILURE,
        },
        Err(e) => {
            report(&format!(
                "Cannot start {}: {e}\n\nKeep this program in the same folder as {ENGINE}.",
                engine.display()
            ));
            ExitCode::FAILURE
        }
    }
}
