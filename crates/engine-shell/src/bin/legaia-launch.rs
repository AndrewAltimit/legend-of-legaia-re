//! The double-click entry: the release archive's `Legend of Legaia.exe`
//! (Windows) and `Legend of Legaia` (Linux), a stub that starts the
//! `legaia-engine` executable from the archive's `command-line-tools` folder
//! (`legaia_app_theme::release_layout::find_engine`; beside the stub in a
//! `cargo build`).
//!
//! It exists so the archive's top folder holds only what a player opens,
//! with the engine and every other command-line program one folder down.
//! On Windows it also hides the console: `legaia-engine.exe` is a console
//! program so its many CLI subcommands keep ordinary terminal behaviour (the
//! shell waits for it, pipes and exit codes work, output lands before the
//! next prompt), and a console program started from File Explorer gets a
//! console window of its own. Being a `windows`-subsystem program this stub
//! never gets one, and it starts the engine with `CREATE_NO_WINDOW`, so
//! neither does the engine nor the game process the launcher spawns (that
//! one inherits the hidden console). On Unix it replaces itself with the
//! engine (`exec`), so a terminal sees the engine's own output and exit.
//!
//! Arguments are passed through, and the engine's exit status is returned.
//! docs/tooling/releases.md records why this is a separate stub and not
//! `windows_subsystem` on the engine itself.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::{Command, ExitCode};

use legaia_app_theme::release_layout::{TOOLS_DIR, engine_file_name, find_engine};

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

/// Run the engine as this process (Unix): `exec` only returns on failure.
#[cfg(unix)]
fn run(mut cmd: Command, engine: &std::path::Path) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let e = cmd.exec();
    report(&format!("Cannot start {}: {e}", engine.display()));
    ExitCode::FAILURE
}

/// Run the engine as a child and hand back its exit status.
#[cfg(not(unix))]
fn run(mut cmd: Command, engine: &std::path::Path) -> ExitCode {
    match cmd.status() {
        Ok(status) => match status.code() {
            Some(0) => ExitCode::SUCCESS,
            Some(c) => {
                report(&format!(
                    "The engine stopped with an error (exit status {c}).\n\n\
                     Run {} from a command prompt to see its log.",
                    engine.display()
                ));
                ExitCode::from(u8::try_from(c).unwrap_or(1))
            }
            None => ExitCode::FAILURE,
        },
        Err(e) => {
            report(&format!("Cannot start {}: {e}", engine.display()));
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let Some(engine) = std::env::current_exe().ok().and_then(|me| find_engine(&me)) else {
        report(&format!(
            "Cannot find {}.\n\nIt ships in the {TOOLS_DIR} folder of the release; \
             keep that folder beside this program.",
            engine_file_name()
        ));
        return ExitCode::FAILURE;
    };
    let mut cmd = Command::new(&engine);
    cmd.args(std::env::args_os().skip(1));
    configure(&mut cmd);
    run(cmd, &engine)
}
