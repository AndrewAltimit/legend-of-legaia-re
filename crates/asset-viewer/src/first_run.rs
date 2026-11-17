//! What a start with no view named and nothing to browse does when nobody is
//! at a terminal: a double-click in a file manager or File Explorer. The
//! terminal start keeps its printed guide ([`super::NO_EXTRACTION`]); a
//! double-click gets a desktop dialog instead, since its text would land
//! nowhere.
//!
//! On Windows the viewer is a `windows`-subsystem program, so a double-click
//! opens no console window at all; [`attach_terminal`] reaches the terminal
//! it was typed into, if any, so `--help`, the printed guide and errors still
//! print there (`docs/tooling/releases.md`, "Windows: the console window").
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

/// Connect standard output and error to the terminal the viewer was started
/// from, before anything prints. Only Windows has work to do: a
/// `windows`-subsystem program starts with no console, so it attaches to its
/// parent's (a `cmd.exe` or PowerShell window) and points the missing
/// standard handles at it. Output redirected to a file or a pipe arrives as
/// ready handles and is left alone. Returns whether a terminal or a redirect
/// is there to print to: `false` is a double-click.
pub(crate) fn attach_terminal() -> bool {
    #[cfg(windows)]
    {
        windows_console::attach()
    }
    #[cfg(not(windows))]
    {
        use std::io::IsTerminal;
        std::io::stderr().is_terminal()
    }
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
    use core::ffi::c_void;

    type Handle = *mut c_void;
    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const FILE_TYPE_DISK: u32 = 1;
    const FILE_TYPE_PIPE: u32 = 3;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn GetStdHandle(which: u32) -> Handle;
        fn SetStdHandle(which: u32, handle: Handle) -> i32;
        fn GetFileType(handle: Handle) -> u32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
    }

    /// Whether `which` already leads to a file or a pipe (`asset-viewer
    /// --help > help.txt`, `| more`): the parent handed it over at start.
    fn redirected(which: u32) -> bool {
        // SAFETY: plain handle queries; a null or stale handle answers
        // FILE_TYPE_UNKNOWN.
        unsafe {
            let h = GetStdHandle(which);
            !h.is_null()
                && h as isize != -1
                && matches!(GetFileType(h), FILE_TYPE_DISK | FILE_TYPE_PIPE)
        }
    }

    pub(super) fn attach() -> bool {
        let streams = [
            (STD_INPUT_HANDLE, "CONIN$"),
            (STD_OUTPUT_HANDLE, "CONOUT$"),
            (STD_ERROR_HANDLE, "CONOUT$"),
        ];
        let redirects = streams.map(|(which, _)| redirected(which));
        // SAFETY: no arguments beyond the documented parent-process id.
        let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0;
        if attached {
            for ((which, device), redirect) in streams.into_iter().zip(redirects) {
                if redirect {
                    continue;
                }
                let name: Vec<u16> = device.encode_utf16().chain(Some(0)).collect();
                // SAFETY: `name` is NUL-terminated UTF-16 that outlives the
                // call; the handle is kept for the process's lifetime.
                unsafe {
                    let h = CreateFileW(
                        name.as_ptr(),
                        GENERIC_READ | GENERIC_WRITE,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        core::ptr::null_mut(),
                        OPEN_EXISTING,
                        0,
                        core::ptr::null_mut(),
                    );
                    if h as isize != -1 {
                        SetStdHandle(which, h);
                    }
                }
            }
        }
        attached || redirects[1] || redirects[2]
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
