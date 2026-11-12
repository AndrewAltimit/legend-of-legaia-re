//! Explorer's own file browser: the folder list a controller can work,
//! where the desktop's file dialog (`rfd`) wants a mouse or a keyboard.
//!
//! It opens in place of the dialog while a controller was the last thing
//! pressed (`App`'s `pad_active`), when the dialog never appeared (a
//! desktop without a file-dialog portal hands back nothing at once), and
//! from the start page's *Browse in this window* link. It lists the folder's
//! subfolders and the files it was opened for
//! (`legaia_app_theme::folder_list`, the listing the game launcher's finder
//! shares), the way up, and the usual places.
//!
//! | Key | Pad | What it does |
//! |---|---|---|
//! | Up / Down, Page Up / Page Down, Home / End | D-pad | move |
//! | Enter, Right | Cross, Right | open the folder, take the file (or the folder) |
//! | Backspace, Left | Left | the folder above |
//! | Esc | Circle | close without choosing |

use std::path::{Path, PathBuf};

use eframe::egui;
use legaia_app_theme::folder_list::{self, EntryKind};

/// How many entries of one folder the browser reads.
const ENTRY_LIMIT: usize = 4096;
/// The rows Page Up / Page Down move.
const PAGE: usize = 10;

/// What the browser was opened to pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// A disc image (`.bin` / `.cue`) to open.
    Disc,
    /// A folder `legaia-extract` (or the Extract view) wrote, to open.
    ExtractedFolder,
    /// The folder the Extract view writes into.
    OutputFolder,
}

impl Purpose {
    pub fn title(self) -> &'static str {
        match self {
            Self::Disc => "Find your disc image",
            Self::ExtractedFolder => "Find an extracted folder",
            Self::OutputFolder => "Choose where to extract to",
        }
    }

    /// Whether the pick is a folder (the list then starts with *Use this
    /// folder*).
    pub fn picks_folder(self) -> bool {
        !matches!(self, Self::Disc)
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Disc => &["bin", "cue"],
            _ => &[],
        }
    }

    /// The line under the title: what to look for.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Disc => {
                "Your disc image is a .bin file (or the .cue beside it). Folders open; the \
                 Places below jump to the usual folders."
            }
            Self::ExtractedFolder => {
                "Open the folder legaia-extract or the Extract view wrote, then choose \
                 Use this folder."
            }
            Self::OutputFolder => {
                "Open the folder the extraction should write into, then choose Use this \
                 folder. An empty folder is best."
            }
        }
    }
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// Take the folder on show (folder purposes).
    UseThisFolder,
    /// The folder above.
    Up(PathBuf),
    Folder {
        name: String,
        path: PathBuf,
    },
    File {
        name: String,
        path: PathBuf,
    },
    /// One of the usual places (home, Downloads, a drive).
    Place {
        label: String,
        path: PathBuf,
    },
}

/// A key the browser acts on: the keyboard's, or a controller's (Explorer
/// turns a pad's buttons into these keys, `crate::gamepad`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
    /// Enter, Right: open the row.
    Open,
    /// Backspace, Left: the folder above.
    Parent,
    /// Esc: close.
    Cancel,
}

/// What a step of the browser came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Still open.
    Open,
    /// Closed without a choice.
    Cancelled,
    /// The file or folder chosen.
    Picked(PathBuf),
}

#[derive(Debug, Clone)]
pub struct FileBrowser {
    pub purpose: Purpose,
    dir: PathBuf,
    rows: Vec<Row>,
    /// The index of the first place row (the "Places" heading goes above it).
    places_at: usize,
    selected: usize,
    /// Why the folder could not be read.
    error: Option<String>,
    /// The selection moved by a key: scroll it into view this frame.
    reveal: bool,
}

/// The usual places, each a folder that exists: the home folder,
/// Downloads, Desktop, Documents, then the drives.
pub fn places() -> Vec<(String, PathBuf)> {
    let home = dirs::home_dir();
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let named = [
        ("Home folder", home.clone()),
        (
            "Downloads",
            dirs::download_dir().or_else(|| home.as_ref().map(|h| h.join("Downloads"))),
        ),
        (
            "Desktop",
            dirs::desktop_dir().or_else(|| home.as_ref().map(|h| h.join("Desktop"))),
        ),
        (
            "Documents",
            dirs::document_dir().or_else(|| home.as_ref().map(|h| h.join("Documents"))),
        ),
    ];
    let named = named
        .into_iter()
        .filter_map(|(l, d)| d.map(|d| (l.to_string(), d)));
    for (label, dir) in named.chain(folder_list::drive_places()) {
        if dir.is_dir() && !out.iter().any(|(_, p)| *p == dir) {
            out.push((label, dir));
        }
    }
    out
}

impl FileBrowser {
    /// Open on `start` (a folder, or a file whose folder is used), else
    /// Downloads, else the home folder, else the top of the file system.
    pub fn open(purpose: Purpose, start: Option<&Path>) -> Self {
        let start = start
            .map(|p| {
                if p.is_dir() {
                    p.to_path_buf()
                } else {
                    p.parent().map(Path::to_path_buf).unwrap_or_default()
                }
            })
            .filter(|d| d.is_dir())
            .or_else(|| {
                dirs::download_dir()
                    .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
                    .filter(|d| d.is_dir())
            })
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self::open_in(purpose, start, places())
    }

    /// Open on `dir` with `places` below the listing.
    pub fn open_in(purpose: Purpose, dir: PathBuf, places: Vec<(String, PathBuf)>) -> Self {
        let mut b = Self {
            purpose,
            dir: PathBuf::new(),
            rows: Vec::new(),
            places_at: 0,
            selected: 0,
            error: None,
            reveal: true,
        };
        b.show_dir(dir, places);
        b
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    fn places(&self) -> Vec<(String, PathBuf)> {
        self.rows[self.places_at..]
            .iter()
            .filter_map(|r| match r {
                Row::Place { label, path } => Some((label.clone(), path.clone())),
                _ => None,
            })
            .collect()
    }

    /// List `dir`, selecting its first folder or file (past *Use this
    /// folder* and the way up), or the first row when it has none.
    fn show_dir(&mut self, dir: PathBuf, places: Vec<(String, PathBuf)>) {
        let mut rows = Vec::new();
        if self.purpose.picks_folder() {
            rows.push(Row::UseThisFolder);
        }
        if let Some(up) = dir.parent() {
            rows.push(Row::Up(up.to_path_buf()));
        }
        let lead = rows.len();
        self.error = None;
        match folder_list::list(&dir, self.purpose.extensions(), ENTRY_LIMIT) {
            Ok(entries) => rows.extend(entries.into_iter().map(|e| match e.kind {
                EntryKind::Folder => Row::Folder {
                    name: e.name,
                    path: e.path,
                },
                EntryKind::File => Row::File {
                    name: e.name,
                    path: e.path,
                },
            })),
            Err(e) => self.error = Some(format!("This folder cannot be read: {e}")),
        }
        let listed = rows.len();
        self.places_at = listed;
        rows.extend(
            places
                .into_iter()
                .map(|(label, path)| Row::Place { label, path }),
        );
        self.selected = if listed > lead {
            lead
        } else if self.purpose.picks_folder() {
            0
        } else {
            lead.saturating_sub(1)
        };
        self.rows = rows;
        self.dir = dir;
        self.reveal = true;
    }

    fn go(&mut self, dir: PathBuf) {
        let places = self.places();
        self.show_dir(dir, places);
    }

    /// Act on one key.
    pub fn nav(&mut self, nav: Nav) -> Outcome {
        let last = self.rows.len().saturating_sub(1);
        match nav {
            Nav::Up => self.selected = self.selected.saturating_sub(1),
            Nav::Down => self.selected = (self.selected + 1).min(last),
            Nav::PageUp => self.selected = self.selected.saturating_sub(PAGE),
            Nav::PageDown => self.selected = (self.selected + PAGE).min(last),
            Nav::First => self.selected = 0,
            Nav::Last => self.selected = last,
            Nav::Cancel => return Outcome::Cancelled,
            Nav::Parent => {
                if let Some(up) = self.dir.parent() {
                    let from = self.dir.clone();
                    self.go(up.to_path_buf());
                    // Land on the folder just left.
                    if let Some(i) = self
                        .rows
                        .iter()
                        .position(|r| matches!(r, Row::Folder { path, .. } if *path == from))
                    {
                        self.selected = i;
                    }
                }
            }
            Nav::Open => return self.activate(self.selected),
        }
        self.reveal = true;
        Outcome::Open
    }

    /// Open (or take) row `i`: a click, or Enter on it.
    pub fn activate(&mut self, i: usize) -> Outcome {
        self.selected = i;
        match self.rows.get(i).cloned() {
            Some(Row::UseThisFolder) => Outcome::Picked(self.dir.clone()),
            Some(Row::File { path, .. }) => Outcome::Picked(path),
            Some(Row::Up(path) | Row::Folder { path, .. } | Row::Place { path, .. }) => {
                self.go(path);
                Outcome::Open
            }
            None => Outcome::Open,
        }
    }

    /// The keys this frame, taken from egui's input so the window under the
    /// browser does not also act on them.
    fn take_keys(ctx: &egui::Context) -> Vec<Nav> {
        use egui::{Key, Modifiers};
        let keys = [
            (Key::ArrowUp, Nav::Up),
            (Key::ArrowDown, Nav::Down),
            (Key::PageUp, Nav::PageUp),
            (Key::PageDown, Nav::PageDown),
            (Key::Home, Nav::First),
            (Key::End, Nav::Last),
            (Key::Enter, Nav::Open),
            (Key::ArrowRight, Nav::Open),
            (Key::Backspace, Nav::Parent),
            (Key::ArrowLeft, Nav::Parent),
            (Key::Escape, Nav::Cancel),
        ];
        let mut out = Vec::new();
        ctx.input_mut(|i| {
            for (key, nav) in keys {
                while i.consume_key(Modifiers::NONE, key) {
                    out.push(nav);
                }
            }
        });
        out
    }

    /// Draw the browser as a box over the window and act on this frame's
    /// keys and clicks.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome {
        for nav in Self::take_keys(ctx) {
            match self.nav(nav) {
                Outcome::Open => {}
                done => return done,
            }
        }
        let mut outcome = Outcome::Open;
        let screen = ctx.content_rect();
        let width = (screen.width() - 48.0).clamp(280.0, 640.0);
        let height = (screen.height() - 96.0).clamp(220.0, 640.0);
        let modal = egui::Modal::new(egui::Id::new("explorer_file_browser")).show(ctx, |ui| {
            ui.set_width(width);
            ui.heading(self.purpose.title());
            ui.label(egui::RichText::new(self.purpose.hint()).weak());
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(egui::RichText::new(self.dir.display().to_string()).monospace())
                    .truncate(),
            );
            if let Some(e) = &self.error {
                ui.colored_label(ui.visuals().warn_fg_color, e);
            }
            ui.separator();
            let reveal = std::mem::take(&mut self.reveal);
            let mut clicked = None;
            egui::ScrollArea::vertical()
                .max_height(height - 150.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // Rows the list's full width, their text from the left.
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        let listed_any = self.places_at
                            > self
                                .rows
                                .iter()
                                .take(self.places_at)
                                .filter(|r| matches!(r, Row::UseThisFolder | Row::Up(_)))
                                .count();
                        for (i, row) in self.rows.iter().enumerate() {
                            if i == self.places_at {
                                if !listed_any && self.error.is_none() {
                                    ui.label(
                                        egui::RichText::new(match self.purpose {
                                            Purpose::Disc => "No folders or disc images here.",
                                            _ => "No folders here.",
                                        })
                                        .weak(),
                                    );
                                }
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new("Places").strong());
                            }
                            let text = match row {
                                Row::UseThisFolder => egui::RichText::new(format!(
                                    "Use this folder ({})",
                                    self.dir.file_name().map_or_else(
                                        || self.dir.display().to_string(),
                                        |n| { n.to_string_lossy().into_owned() }
                                    )
                                ))
                                .strong(),
                                Row::Up(_) => egui::RichText::new(".. Up one folder"),
                                Row::Folder { name, .. } => egui::RichText::new(format!("{name}/")),
                                Row::File { name, .. } => egui::RichText::new(name.as_str())
                                    .strong()
                                    .color(crate::theme::accent(ui)),
                                Row::Place { label, .. } => egui::RichText::new(label.as_str()),
                            };
                            let r = ui.add(
                                egui::Button::selectable(i == self.selected, text)
                                    .min_size(egui::vec2(0.0, 22.0)),
                            );
                            if reveal && i == self.selected {
                                r.scroll_to_me(Some(egui::Align::Center));
                            }
                            if r.clicked() {
                                clicked = Some(i);
                            }
                        }
                    });
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    outcome = Outcome::Cancelled;
                }
                ui.label(
                    egui::RichText::new(
                        "Arrows move, Enter opens, Backspace or Left goes up, Esc closes. \
                         On a controller: Cross opens, Circle closes.",
                    )
                    .weak()
                    .small(),
                );
            });
            if let Some(i) = clicked {
                outcome = self.activate(i);
            }
        });
        if modal.should_close() && outcome == Outcome::Open {
            outcome = Outcome::Cancelled;
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Games/Legaia")).unwrap();
        std::fs::write(root.join("Games/Legaia/Legend of Legaia.bin"), b"x").unwrap();
        std::fs::write(root.join("Games/Legaia/Legend of Legaia.cue"), b"x").unwrap();
        std::fs::write(root.join("Games/readme.txt"), b"x").unwrap();
        dir
    }

    #[test]
    fn a_pad_walks_down_to_the_disc_and_takes_it() {
        let dir = tree();
        let root = dir.path();
        let places = vec![("Home folder".to_string(), root.to_path_buf())];
        let mut b = FileBrowser::open_in(Purpose::Disc, root.to_path_buf(), places);
        // The first folder, past the way up.
        assert_eq!(
            b.rows()[b.selected()],
            Row::Folder {
                name: "Games".into(),
                path: root.join("Games")
            }
        );
        assert_eq!(b.nav(Nav::Open), Outcome::Open);
        assert_eq!(b.dir(), root.join("Games"));
        // readme.txt is not listed; Legaia/ is.
        assert!(!b.rows().iter().any(|r| matches!(r, Row::File { .. })));
        assert_eq!(b.nav(Nav::Open), Outcome::Open);
        assert_eq!(b.dir(), root.join("Games/Legaia"));
        // Up, Down: the way up is above, the .bin is the first file.
        assert!(matches!(b.rows()[b.selected()], Row::File { .. }));
        assert_eq!(
            b.nav(Nav::Open),
            Outcome::Picked(root.join("Games/Legaia/Legend of Legaia.bin"))
        );
        // Left goes up and lands on the folder just left.
        b.nav(Nav::Parent);
        assert_eq!(b.dir(), root.join("Games"));
        assert_eq!(
            b.rows()[b.selected()],
            Row::Folder {
                name: "Legaia".into(),
                path: root.join("Games/Legaia")
            }
        );
        // The places stay below every folder.
        b.nav(Nav::Last);
        assert_eq!(
            b.rows()[b.selected()],
            Row::Place {
                label: "Home folder".into(),
                path: root.to_path_buf()
            }
        );
        assert_eq!(b.nav(Nav::Open), Outcome::Open);
        assert_eq!(b.dir(), root);
        assert_eq!(b.nav(Nav::Cancel), Outcome::Cancelled);
    }

    #[test]
    fn a_folder_pick_offers_the_folder_on_show() {
        let dir = tree();
        let root = dir.path();
        let mut b = FileBrowser::open_in(Purpose::OutputFolder, root.join("Games"), Vec::new());
        assert_eq!(b.rows()[0], Row::UseThisFolder);
        // No files in a folder pick: only Legaia/ under the way up.
        assert!(!b.rows().iter().any(|r| matches!(r, Row::File { .. })));
        b.nav(Nav::First);
        assert_eq!(b.nav(Nav::Open), Outcome::Picked(root.join("Games")));
    }
}
