//! How a game process ended, in the words the desktop programs show a
//! player. The game launcher (`legaia-engine` with no subcommand) and Legaia
//! Explorer both start the game as a child process and report a failed exit
//! through this module, so the two say the same thing: [`exit_headline`]
//! for how the process ended, [`failure_detail`] for the reason its log
//! gives (a panic's message, the root of the `Error:` chain the engine exits
//! with, or its last `ERROR` record).
//!
//! The one part of the crate that needs `std` (an exit status, owned
//! strings); the palette above stays `no_std`.

use std::format;
use std::string::{String, ToString};
use std::vec::Vec;

/// One line on how the game process ended: its exit code, the signal it
/// died of, or the Windows exception code a crash leaves.
pub fn exit_headline(status: &std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            // Something outside the game ended it (a shutdown, a task
            // manager, a kill): not the game crashing.
            if sig == 15 {
                return "The game was closed from outside (SIGTERM).".into();
            }
            if sig == 9 {
                return "The game was killed from outside (SIGKILL).".into();
            }
            let name = match sig {
                4 => "SIGILL",
                6 => "SIGABRT",
                7 => "SIGBUS",
                8 => "SIGFPE",
                11 => "SIGSEGV",
                _ => "",
            };
            return if name.is_empty() {
                format!("The game crashed (signal {sig}).")
            } else {
                format!("The game crashed ({name}).")
            };
        }
    }
    match status.code() {
        // An NTSTATUS exception code (0xC0000005 access violation, ...).
        Some(c) if (c as u32) >= 0xC000_0000 => {
            format!("The game crashed (exception 0x{:08X}).", c as u32)
        }
        Some(c) => format!("The game quit with error code {c}."),
        None => "The game stopped unexpectedly.".into(),
    }
}

/// The exit status of a game program that has already told the player what
/// went wrong in a message box: the `Legend of Legaia` stub (a program with no
/// console) shows its own box for any other failure, and two boxes for one
/// failure read as two failures.
pub const ALREADY_REPORTED_EXIT: i32 = 3;

/// What a program says when its window could not be opened: what happened,
/// the fix a player can make, then `detail` (the graphics library's own
/// words) for a bug report.
pub fn window_failure_message(detail: &str) -> String {
    std::format!(
        "Legend of Legaia could not open its window.\n\n\
         The game draws through your graphics driver, and the driver did not offer \
         what it needs. Installing the latest driver for your graphics card usually \
         fixes this. Over a remote desktop, try again at the computer itself.\n\n\
         Details: {detail}"
    )
}

/// The reason a failed game's log gives, phrased for a desktop program's message: a
/// panic's message, else the root of the `Error:` chain the engine exits
/// with, else its last `ERROR` record. Paths shrink to their file name and
/// the line to [`DETAIL_MAX`] characters. `None` when the log holds no
/// failure at all: a crash leaves only the chatter before it, which would
/// read as the reason.
pub fn failure_detail(log: &str) -> Option<String> {
    let lines: Vec<&str> = log.lines().map(str::trim_end).collect();
    let found = if let Some(i) = lines.iter().rposition(|l| l.contains("panicked at")) {
        // Since Rust 1.73 the message is the line after "panicked at
        // file:line:col:"; before, it followed on the same line.
        let next = lines.get(i + 1).map(|l| l.trim()).filter(|l| {
            !l.is_empty() && !l.starts_with("note:") && !l.starts_with("stack backtrace")
        });
        let msg = match next {
            Some(m) => m.to_string(),
            None => {
                let l = lines[i];
                l[l.find("panicked at").unwrap_or(0)..].to_string()
            }
        };
        format!("It panicked: {msg}")
    } else if let Some(i) = lines.iter().rposition(|l| l.starts_with("Error:")) {
        // `Error: top`, then `Caused by:` and `    0: ...` / `    1: ...`:
        // the last cause is the root ("No such file or directory").
        let causes: Vec<&str> = lines[i + 1..]
            .iter()
            .take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with("Caused by"))
            .map(|l| l.trim())
            .filter(|t| !t.is_empty() && *t != "Caused by:")
            .map(|t| match t.split_once(": ") {
                Some((n, rest)) if n.parse::<u32>().is_ok() => rest,
                _ => t,
            })
            .collect();
        let top = lines[i].trim_start_matches("Error:").trim();
        match causes.last() {
            Some(root) => format!(
                "{}: {}",
                capitalise_first(&shorten_paths(top)),
                shorten_paths(root)
            ),
            None => capitalise_first(&shorten_paths(top)),
        }
    } else {
        // An output hiccup is logged as an error and the game plays on: under a
        // loaded machine it is the last record of many a crash that has
        // nothing to do with it.
        let l = lines
            .iter()
            .rev()
            .find(|l| l.contains(" ERROR ") && !l.contains("buffer underrun"))?;
        capitalise_first(&shorten_paths(strip_log_prefix(l)))
    };
    Some(clip(&found, DETAIL_MAX))
}

/// The most of a failure reason [`failure_detail`] returns (the launcher's
/// help box holds about four lines of it).
pub const DETAIL_MAX: usize = 72;

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max - 3).collect();
    t.push_str("...");
    t
}

/// Every absolute path in `s` cut to its file name (`/a/b/game.bin` ->
/// `game.bin`): a message box is narrow, and the folder is the log's or the disc's.
/// A path with spaces in it shrinks from its last space-free piece.
fn shorten_paths(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let unix = w.starts_with('/') && w[1..].contains('/');
            let win = w.len() > 3 && w.as_bytes()[1] == b':' && w[2..].starts_with('\\');
            if unix || win {
                w.rsplit(['/', '\\']).next().unwrap_or(w)
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// How many of the log's last lines a report carries.
pub const REPORT_LOG_LINES: usize = 40;
/// A longer log line is cut to this many characters.
const REPORT_LINE_MAX: usize = 300;

/// The text a player pastes into a bug report about a failed game: the
/// program, its version, the system, how the game ended, the reason, and the
/// end of the log. `home` is cut out of every line (a path under it reads
/// `~/...`), so a report does not name the account it came from.
pub fn report_text(
    program: &str,
    version: &str,
    headline: &str,
    detail: Option<&str>,
    log: Option<&str>,
    home: Option<&str>,
) -> String {
    let mut out = format!(
        "{program} {version} on {} {}\n{headline}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if let Some(d) = detail {
        out.push_str(d);
        out.push('\n');
    }
    if let Some(log) = log {
        out.push_str("--- end of the log ---\n");
        let lines: Vec<&str> = log.lines().collect();
        for line in &lines[lines.len().saturating_sub(REPORT_LOG_LINES)..] {
            let cut: String = line.chars().take(REPORT_LINE_MAX).collect();
            match home.filter(|h| !h.is_empty()) {
                Some(h) => out.push_str(&cut.replace(h, "~")),
                None => out.push_str(&cut),
            }
            out.push('\n');
        }
    }
    out
}

/// The account's home folder, as [`report_text`] takes it.
pub fn home_folder() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
}

/// `[2026-01-01T00:00:00Z INFO  module] text` -> `text`.
pub fn strip_log_prefix(line: &str) -> &str {
    if line.starts_with('[')
        && let Some(end) = line.find("] ")
    {
        return &line[end + 2..];
    }
    line
}

fn capitalise_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window that never opened says what to do first and keeps the
    /// library's words for a bug report; the game's own exit line (what the
    /// launcher and Explorer read back from the log) fits the detail length
    /// whole, with the fix in it.
    #[test]
    fn a_window_failure_names_the_fix() {
        let m = window_failure_message("no adapter");
        let (fix, detail) = (
            m.find("latest driver").unwrap(),
            m.find("no adapter").unwrap(),
        );
        assert!(fix < detail, "{m}");
        let log = "Error: could not open the game window: update your graphics driver\n";
        let d = failure_detail(log).unwrap();
        assert!(d.contains("update your graphics driver"), "{d}");
        assert!(!d.ends_with("..."), "{d}");
    }

    #[test]
    fn a_report_carries_the_end_of_the_log_without_the_home_folder() {
        let log: String = (0..100)
            .map(|n| format!("line {n} /home/someone/disc.bin\n"))
            .collect();
        let r = report_text(
            "legaia-engine",
            "1.2.3",
            "The game crashed.",
            Some("why"),
            Some(&log),
            Some("/home/someone"),
        );
        assert!(r.starts_with("legaia-engine 1.2.3 on "), "{r}");
        assert!(r.contains("The game crashed.\nwhy\n"), "{r}");
        assert!(r.contains("line 99 ~/disc.bin"), "{r}");
        assert!(!r.contains("line 59 "), "only the last lines: {r}");
        assert!(!r.contains("/home/someone"), "{r}");
    }

    #[test]
    fn a_report_needs_no_log() {
        let r = report_text("x", "1", "It stopped.", None, None, None);
        assert!(r.ends_with("It stopped.\n"), "{r}");
    }

    #[test]
    fn failure_detail_reads_the_engine_error_chain() {
        let log = "[t INFO  m] loading\n\
                   Error: open disc image /tmp/x/disc/Legaia.bin\n\
                   \n\
                   Caused by:\n    \
                   0: open disc image /tmp/x/disc/Legaia.bin\n    \
                   1: No such file or directory (os error 2)\n";
        assert_eq!(
            failure_detail(log).as_deref(),
            Some("Open disc image Legaia.bin: No such file or directory (os error 2)")
        );
        // A bare `Error:` with no chain.
        assert_eq!(
            failure_detail("Error: no adapter\n").as_deref(),
            Some("No adapter")
        );
    }

    #[test]
    fn failure_detail_reads_a_panic_message() {
        let log = "[t INFO  m] ok\n\
                   thread 'main' panicked at crates/x/src/lib.rs:10:5:\n\
                   index out of bounds\n\
                   note: run with `RUST_BACKTRACE=1`\n";
        assert_eq!(
            failure_detail(log).as_deref(),
            Some("It panicked: index out of bounds")
        );
    }

    #[test]
    fn failure_detail_is_none_for_a_crash_log() {
        // A killed process leaves only its chatter: no reason to show.
        let log = "[t INFO  m] light 11: pos (1, 2, 3)\n[t INFO  m] 165 meshes uploaded\n";
        assert_eq!(failure_detail(log), None);
        assert_eq!(
            failure_detail("[t INFO  m] a\n[t ERROR m] no VRAM\n[t INFO  m] bye\n").as_deref(),
            Some("No VRAM")
        );
        let long = format!("Error: {}\n", "x".repeat(300));
        assert_eq!(failure_detail(&long).unwrap().chars().count(), DETAIL_MAX);
    }

    #[cfg(unix)]
    #[test]
    fn an_audio_underrun_is_not_a_reason() {
        let log = "[t ERROR legaia_engine_audio::native_out] audio output error: \
                   A buffer underrun or overrun occurred.";
        assert_eq!(failure_detail(log), None);
    }

    #[test]
    fn exit_headline_names_signals_and_codes() {
        use std::os::unix::process::ExitStatusExt;
        let st = |raw| std::process::ExitStatus::from_raw(raw);
        assert_eq!(
            exit_headline(&st(9)),
            "The game was killed from outside (SIGKILL)."
        );
        assert_eq!(
            exit_headline(&st(15)),
            "The game was closed from outside (SIGTERM)."
        );
        assert_eq!(exit_headline(&st(11)), "The game crashed (SIGSEGV).");
        assert_eq!(
            exit_headline(&st(1 << 8)),
            "The game quit with error code 1."
        );
    }
}
