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
//! Arguments are passed through, and the engine's exit status is returned,
//! except one: a lone file argument is what the desktop hands a program a
//! file was opened with ("Open with", or a file dropped on its icon), and
//! the engine would read it as an unknown subcommand. It goes to the
//! launcher as `--open <file>`, which takes it as if it were dropped on the
//! launcher's window ([`engine_args`]).
//! docs/tooling/releases.md records why this is a separate stub and not
//! `windows_subsystem` on the engine itself.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::Path;
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

/// Off Windows the message always goes to standard error, and when that is
/// not a terminal (the program was double-clicked in a file manager) to a
/// message box as well: otherwise a failed double-click does nothing at all.
#[cfg(not(windows))]
fn report(msg: &str) {
    use std::io::IsTerminal;
    eprintln!("{msg}");
    if !std::io::stderr().is_terminal() {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Legend of Legaia")
            .set_description(msg)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}

/// What the player reads when the engine program cannot be started at all.
fn start_failure_message(engine: &Path, e: &std::io::Error) -> String {
    format!(
        "Legend of Legaia could not start {} ({e}).\n\n\
         It ships in the {TOOLS_DIR} folder of the release. If the folder is there, \
         extract the whole archive again: a copy that stopped part way, or a security \
         program that removed the file, leaves it unusable.",
        engine.display()
    )
}

/// What the player reads when the engine program ended with a failure it
/// did not report itself: how it ended in the words the launcher and
/// Explorer use for a failed game ([`legaia_app_theme::game_exit`]), the
/// reason its output gives (`log`, the tail of its standard error), and how
/// to keep the report.
#[cfg_attr(not(windows), allow(dead_code))]
fn exit_failure_message(headline: &str, log: &str) -> String {
    let mut text = String::from(headline);
    if let Some(why) = legaia_app_theme::game_exit::failure_detail(log) {
        text.push_str("\n\n");
        text.push_str(&why);
    }
    text.push_str(
        "\n\nPress Ctrl+C to copy this message for a bug report. Running \
         legaia-engine from a command prompt shows its whole log.",
    );
    text
}

/// Run the engine as this process (Unix): `exec` only returns on failure.
#[cfg(unix)]
fn run(mut cmd: Command, engine: &std::path::Path) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let e = cmd.exec();
    report(&start_failure_message(engine, &e));
    ExitCode::FAILURE
}

/// How much of the engine's standard error [`run`] keeps for the message.
#[cfg(not(unix))]
const LOG_TAIL: usize = 16 * 1024;

/// Run the engine as a child and hand back its exit status. Its standard
/// error (where its log goes) is read as it runs, the last [`LOG_TAIL`]
/// bytes kept, so a failure can say why.
#[cfg(not(unix))]
fn run(mut cmd: Command, engine: &std::path::Path) -> ExitCode {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    cmd.stderr(std::process::Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            report(&start_failure_message(engine, &e));
            return ExitCode::FAILURE;
        }
    };
    let tail = Arc::new(Mutex::new(Vec::<u8>::new()));
    if let Some(mut err) = child.stderr.take() {
        let tail = Arc::clone(&tail);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut t = tail.lock().unwrap_or_else(|p| p.into_inner());
                t.extend_from_slice(&buf[..n]);
                let over = t.len().saturating_sub(LOG_TAIL);
                t.drain(..over);
            }
        });
    }
    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => {
            report(&start_failure_message(engine, &e));
            return ExitCode::FAILURE;
        }
    };
    match status.code() {
        Some(0) => ExitCode::SUCCESS,
        // The engine has already shown its own message box.
        Some(c) if c == legaia_app_theme::game_exit::ALREADY_REPORTED_EXIT => {
            ExitCode::from(u8::try_from(c).unwrap_or(1))
        }
        code => {
            // Let the reader catch the last lines the engine wrote.
            std::thread::sleep(std::time::Duration::from_millis(100));
            let log = String::from_utf8_lossy(&tail.lock().unwrap_or_else(|p| p.into_inner()))
                .into_owned();
            let headline = legaia_app_theme::game_exit::exit_headline(&status);
            report(&exit_failure_message(&headline, &log));
            ExitCode::from(code.and_then(|c| u8::try_from(c).ok()).unwrap_or(1))
        }
    }
}

/// The engine's arguments for this program's own: unchanged, except that a
/// lone argument naming a file (`is_file`) becomes `--open <file>`.
fn engine_args(args: Vec<OsString>, is_file: impl Fn(&Path) -> bool) -> Vec<OsString> {
    match args.as_slice() {
        [one] if !one.to_string_lossy().starts_with('-') && is_file(Path::new(one)) => {
            vec![OsString::from("--open"), one.clone()]
        }
        _ => args,
    }
}

fn main() -> ExitCode {
    let Some(engine) = std::env::current_exe().ok().and_then(|me| find_engine(&me)) else {
        report(&format!(
            "Legend of Legaia cannot find {}.\n\nIt ships in the {TOOLS_DIR} folder of the \
             release: keep that folder beside this program, as the archive has it. Moving \
             only this program somewhere else (to the desktop, say) leaves the folder behind; \
             make a shortcut to it instead.",
            engine_file_name()
        ));
        return ExitCode::FAILURE;
    };
    let mut cmd = Command::new(&engine);
    cmd.args(engine_args(
        std::env::args_os().skip(1).collect(),
        Path::is_file,
    ));
    configure(&mut cmd);
    run(cmd, &engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_lone_file_opens_in_the_launcher() {
        let file = |_: &Path| true;
        assert_eq!(
            engine_args(args(&["/g/Legaia.bin"]), file),
            args(&["--open", "/g/Legaia.bin"])
        );
    }

    /// The stub's own failures read like the launcher's: the shared exit
    /// headline, the reason from the engine's output, no bare jargon.
    #[test]
    fn failure_messages_name_the_reason_and_the_folder() {
        let log = "[2026-01-01T00:00:00Z INFO  legaia_engine] launcher: start\n\
                   Error: launcher event loop\n\nCaused by:\n    no display found\n";
        let text = exit_failure_message("The game quit with error code 1.", log);
        assert!(
            text.starts_with("The game quit with error code 1."),
            "{text}"
        );
        assert!(text.contains("no display found"), "{text}");
        assert!(text.contains("Ctrl+C"), "{text}");
        assert!(!text.contains("exit status"), "{text}");
        let quiet = exit_failure_message("The game crashed (SIGSEGV).", "");
        assert!(
            quiet.starts_with("The game crashed (SIGSEGV).\n\nPress"),
            "{quiet}"
        );
        let e = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let start = start_failure_message(Path::new("/g/command-line-tools/legaia-engine"), &e);
        assert!(
            start.contains(TOOLS_DIR) && start.contains("extract"),
            "{start}"
        );
    }

    #[test]
    fn everything_else_passes_through() {
        let file = |p: &Path| p.extension().is_some();
        // No arguments: the launcher.
        assert!(engine_args(args(&[]), file).is_empty());
        // A subcommand, a flag, or a word that names no file.
        assert_eq!(engine_args(args(&["config"]), file), args(&["config"]));
        assert_eq!(engine_args(args(&["--help"]), file), args(&["--help"]));
        assert_eq!(
            engine_args(args(&["play-window", "--disc", "/g/l.bin"]), file),
            args(&["play-window", "--disc", "/g/l.bin"])
        );
        // A flag-looking name is never taken for a file.
        assert_eq!(engine_args(args(&["-x.bin"]), file), args(&["-x.bin"]));
    }
}
