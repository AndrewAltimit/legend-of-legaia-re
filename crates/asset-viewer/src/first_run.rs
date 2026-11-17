//! What a start with no view named and nothing to browse does when nobody is
//! at a terminal: a double-click in a file manager or File Explorer. The
//! terminal start keeps its printed guide ([`super::NO_EXTRACTION`]); a
//! double-click gets a desktop dialog instead, since its text would land
//! nowhere (Linux) or in a console window that closes with the program
//! (Windows).
//!
//! The dialog says what the viewer reads and offers the two ways on: pick
//! the extracted folder (or the folder holding it), which the viewer then
//! browses, or start Legaia Explorer, which reads the disc image itself.

use std::path::{Path, PathBuf};

/// The dialog's title.
const TITLE: &str = "Legend of Legaia asset viewer";

/// The dialog's text: what the viewer needs and the two ways on.
pub(crate) const NOTHING_TO_OPEN: &str = "\
The asset viewer shows the files legaia-extract unpacks from your disc; it \
does not read a disc image itself, and found no extracted folder beside it.

Choose folder: pick the extracted folder (the one holding PROT.DAT), or the \
folder that holds it.

Legaia Explorer: browse, preview and extract your disc image with buttons, \
no terminal needed.";

const CHOOSE: &str = "Choose folder...";
const EXPLORER: &str = "Legaia Explorer";
const CLOSE: &str = "Close";

/// Whether nobody is at a terminal to read a printed guide: standard error
/// is no terminal (Linux, macOS), or the program is its console's only
/// process, so the console was made for it by a double-click (Windows).
pub(crate) fn double_clicked() -> bool {
    #[cfg(windows)]
    {
        windows_console::sole_process()
    }
    #[cfg(not(windows))]
    {
        use std::io::IsTerminal;
        !std::io::stderr().is_terminal()
    }
}

/// On Windows, let go of the console a double-click made for the program,
/// so its window closes while the dialog is up rather than sitting empty
/// behind it. The console flashes for a moment first: a console-subsystem
/// program always gets one from File Explorer (`docs/tooling/releases.md`,
/// "Windows: the console window").
pub(crate) fn release_console() {
    #[cfg(windows)]
    windows_console::free();
}

/// The PROT.DAT a picked folder names: the folder's own, or that of an
/// `extracted` folder inside it.
pub(crate) fn prot_dat_in(dir: &Path) -> Option<PathBuf> {
    [dir.join("PROT.DAT"), dir.join("extracted").join("PROT.DAT")]
        .into_iter()
        .find(|p| p.is_file())
}

/// Ask the player what to open. `Some` is the PROT.DAT to browse; `None`
/// means the dialog was closed, or Legaia Explorer was started in the
/// viewer's place.
pub(crate) fn ask() -> Option<PathBuf> {
    let mut text = NOTHING_TO_OPEN.to_string();
    loop {
        let choice = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title(TITLE)
            .set_description(&text)
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                CHOOSE.into(),
                EXPLORER.into(),
                CLOSE.into(),
            ))
            .show();
        match choice {
            rfd::MessageDialogResult::Custom(c) if c == CHOOSE => {
                let Some(dir) = rfd::FileDialog::new()
                    .set_title("The extracted folder, or the folder holding it")
                    .pick_folder()
                else {
                    continue;
                };
                if let Some(prot) = prot_dat_in(&dir) {
                    return Some(prot);
                }
                text = format!(
                    "{} holds no PROT.DAT, and no extracted folder with one.\n\n{NOTHING_TO_OPEN}",
                    dir.display()
                );
            }
            rfd::MessageDialogResult::Custom(c) if c == EXPLORER => match start_explorer() {
                Ok(()) => return None,
                Err(why) => text = format!("{why}\n\n{NOTHING_TO_OPEN}"),
            },
            _ => return None,
        }
    }
}

/// Start Legaia Explorer from the release archive (or the build folder)
/// this viewer sits in.
fn start_explorer() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let Some(explorer) = legaia_app_theme::release_layout::find_explorer(&exe) else {
        return Err(
            "Legaia Explorer was not found beside this program; it ships at the top of the \
             release archive."
                .into(),
        );
    };
    std::process::Command::new(&explorer)
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            format!(
                "Legaia Explorer ({}) did not start: {e}",
                explorer.display()
            )
        })
}

#[cfg(windows)]
mod windows_console {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        fn FreeConsole() -> i32;
    }

    /// Whether this process is alone on its console (File Explorer made the
    /// console for it); a terminal's shell shares it.
    pub(super) fn sole_process() -> bool {
        let mut ids = [0u32; 2];
        // SAFETY: the buffer holds `ids.len()` process ids, as passed.
        let n = unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) };
        n == 1
    }

    pub(super) fn free() {
        // SAFETY: no arguments; detaching from a console is always allowed.
        unsafe {
            FreeConsole();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picked folder is the extracted folder itself, or the one above it.
    #[test]
    fn a_picked_folder_finds_its_prot_dat() {
        let root =
            std::env::temp_dir().join(format!("asset-viewer-first-run-{}", std::process::id()));
        let extracted = root.join("extracted");
        std::fs::create_dir_all(&extracted).unwrap();
        assert_eq!(prot_dat_in(&root), None);
        std::fs::write(extracted.join("PROT.DAT"), b"").unwrap();
        assert_eq!(prot_dat_in(&root), Some(extracted.join("PROT.DAT")));
        assert_eq!(prot_dat_in(&extracted), Some(extracted.join("PROT.DAT")));
        std::fs::remove_dir_all(&root).ok();
    }
}
