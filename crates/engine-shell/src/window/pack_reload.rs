//! The native window's expansion-pack hot reload: `End`
//! ([`legaia_engine_session::hotkeys::HostHotkey::ReloadPack`]) and the
//! `--expansion-watch` file watcher.
//!
//! The reload is the play page's (`LegaiaRuntime::reload_expansion`, the
//! workbench's 3D preview and Play here) on this host: read the pack again
//! from where `--expansion` named it, check it, activate it through the one
//! activation path, and - when the player stands on one of its maps - re-enter
//! that map with them seated where they stood
//! (`SceneHost::enter_expansion_map_seated`), the follow camera's framing kept
//! and no place-name banner. Both hosts read the seat through
//! `SceneHost::expansion_reload_seat` and word the outcome through
//! `hotkeys::pack_reload_notice`.
//!
//! The watcher polls modification times (no file-system notification
//! dependency): a pack folder is a few dozen small files, so a walk every
//! half second costs nothing, and a change is reloaded once the tree has
//! held still for one more poll, so an editor's save-as-rename lands as one
//! reload.

use super::*;
use legaia_engine_scene_host::scene_host::ExpansionReloadSeat;
use legaia_engine_session::hotkeys::{PackReloadOutcome, pack_reload_notice};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often the watcher looks at the pack.
const POLL: Duration = Duration::from_millis(500);
/// How long a reload the game was too busy for waits before it is tried
/// again.
const BUSY_RETRY: Duration = Duration::from_millis(750);

/// The pack the window reloads, and the watcher over it.
#[derive(Debug)]
pub(in crate::window) struct PackReload {
    /// The `--expansion` operands, re-read on every reload.
    launch: ExpansionLaunch,
    /// `Some` under `--expansion-watch`.
    watch: Option<PackWatch>,
}

/// The `--expansion-watch` state.
#[derive(Debug)]
struct PackWatch {
    /// The resolved pack folder or `.lxpack` file.
    path: PathBuf,
    /// The tree signature last reloaded (or seen at start).
    applied: u64,
    /// A signature that differs from `applied`, waiting for the tree to
    /// hold still one poll.
    pending: Option<u64>,
    /// The next poll.
    next: Instant,
    /// A reload the game was busy for, tried again at this time.
    retry: Option<Instant>,
}

impl PackReload {
    /// The reload source for `launch`, watching it when `launch.watch`.
    pub(in crate::window) fn new(launch: ExpansionLaunch) -> Self {
        let watch = launch
            .watch
            .then(|| launch.resolve_path().ok())
            .flatten()
            .map(|path| PackWatch {
                applied: tree_signature(&path),
                path,
                pending: None,
                next: Instant::now() + POLL,
                retry: None,
            });
        if let Some(w) = watch.as_ref() {
            log::info!(
                "expansion: watching {} - an edit reloads the pack in place",
                w.path.display()
            );
        }
        Self { launch, watch }
    }

    /// Whether the watcher wants a reload now: a change that held still for
    /// one poll, or a busy reload's retry.
    fn due(&mut self, now: Instant) -> bool {
        let Some(w) = self.watch.as_mut() else {
            return false;
        };
        if w.retry.is_some_and(|t| now >= t) {
            w.retry = None;
            return true;
        }
        if now < w.next {
            return false;
        }
        w.next = now + POLL;
        let sig = tree_signature(&w.path);
        if sig == w.applied {
            w.pending = None;
            return false;
        }
        if w.pending == Some(sig) {
            w.pending = None;
            w.applied = sig;
            return true;
        }
        w.pending = Some(sig);
        false
    }
}

impl PlayWindowApp {
    /// The per-frame watcher step: reload when the watched pack changed.
    pub(in crate::window) fn poll_pack_watch(&mut self) {
        let now = Instant::now();
        let due = self.pack_reload.as_mut().is_some_and(|r| r.due(now));
        if due {
            self.reload_expansion_pack("file change");
        }
    }

    /// `End`, or a watched change: reload the active pack in place and say
    /// how it went on the notice line.
    pub(in crate::window) fn reload_expansion_pack(&mut self, why: &str) {
        let outcome = self.reload_expansion_outcome();
        if outcome == PackReloadOutcome::Busy
            && let Some(w) = self.pack_reload.as_mut().and_then(|r| r.watch.as_mut())
        {
            w.retry = Some(Instant::now() + BUSY_RETRY);
        }
        let notice = pack_reload_notice(&outcome);
        match &outcome {
            PackReloadOutcome::Refused(e) => log::warn!("expansion: reload ({why}): {e}"),
            _ => log::info!("expansion: reload ({why}): {notice}"),
        }
        self.show_notice(notice);
    }

    fn reload_expansion_outcome(&mut self) -> PackReloadOutcome {
        let t0 = Instant::now();
        let Some(launch) = self.pack_reload.as_ref().map(|r| r.launch.clone()) else {
            return PackReloadOutcome::NoPack;
        };
        let seat = self.session.host.expansion_reload_seat();
        if matches!(seat, ExpansionReloadSeat::Busy { .. }) || self.boot_ui.is_active() {
            return PackReloadOutcome::Busy;
        }
        let pack = match launch.load_checked() {
            Ok(p) => std::sync::Arc::new(p),
            Err(e) => return PackReloadOutcome::Refused(format!("{e:#}")),
        };
        let previous = self.session.host.world.expansion.pack.clone();
        if let Err(e) = self.session.host.activate_expansion_pack(pack.clone()) {
            // Activation leaves no pack on failure: put the running one back.
            if let Some(prev) = previous {
                let _ = self.session.host.activate_expansion_pack(prev);
            }
            return PackReloadOutcome::Refused(format!("{e:#}"));
        }
        let mut map = None;
        if let ExpansionReloadSeat::Field { map: id, seat } = seat
            && pack.maps.contains_key(&id)
        {
            // The map entry resets the follow camera; a reload keeps the
            // framing the player had.
            let camera = self.session.camera.clone();
            let opts = self.field_live_opts.clone();
            if let Err(e) = self
                .session
                .enter_expansion_map_live_seated(&id, "", Some(seat), &opts)
            {
                return PackReloadOutcome::Refused(format!("{e:#}"));
            }
            self.session.camera = camera;
            // A reload is not an arrival: no place-name banner.
            self.session.host.world.cutscene.text_balloon = None;
            self.rebuild_scene_render_state();
            map = Some(id);
        }
        PackReloadOutcome::Reloaded {
            pack: format!("{} {}", pack.manifest.id, pack.manifest.version),
            map,
            ms: t0.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
        }
    }
}

/// A signature of every file under `path` (or of the file itself): its
/// relative path, length and modification time. Dot-files and editor
/// backups (`~`, `.swp`) are left out, so an editor's scratch files do not
/// reload the pack.
fn tree_signature(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut rows: Vec<(PathBuf, u64, Option<SystemTime>)> = Vec::new();
    collect(path, path, &mut rows, 0);
    rows.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rows.hash(&mut h);
    h.finish()
}

fn collect(root: &Path, at: &Path, out: &mut Vec<(PathBuf, u64, Option<SystemTime>)>, depth: u32) {
    let Ok(meta) = std::fs::metadata(at) else {
        return;
    };
    if meta.is_file() {
        let rel = at.strip_prefix(root).unwrap_or(at).to_path_buf();
        out.push((rel, meta.len(), meta.modified().ok()));
        return;
    }
    if !meta.is_dir() || depth > 8 {
        return;
    }
    let Ok(dir) = std::fs::read_dir(at) else {
        return;
    };
    for e in dir.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name.ends_with('~') || name.ends_with(".swp") {
            continue;
        }
        collect(root, &e.path(), out, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The signature moves with an edit, ignores an editor's scratch file,
    /// and the watcher reloads once the tree held still for one poll.
    #[test]
    fn the_watcher_reloads_a_settled_edit_once() {
        let dir = std::env::temp_dir().join(format!("legaia-pack-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("maps")).unwrap();
        std::fs::write(dir.join("pack.toml"), "id = \"t\"\n").unwrap();
        std::fs::write(dir.join("maps/a.toml"), "x = 1\n").unwrap();
        let before = tree_signature(&dir);
        std::fs::write(dir.join(".a.toml.swp"), "scratch").unwrap();
        assert_eq!(tree_signature(&dir), before, "a swap file is not an edit");
        let mut r = PackReload {
            launch: ExpansionLaunch {
                pack: dir.clone(),
                map: None,
                spawn: String::new(),
                watch: true,
            },
            watch: Some(PackWatch {
                path: dir.clone(),
                applied: before,
                pending: None,
                next: Instant::now(),
                retry: None,
            }),
        };
        let t = Instant::now();
        assert!(!r.due(t), "nothing changed");
        std::fs::write(dir.join("maps/a.toml"), "x = 22\n").unwrap();
        assert_ne!(tree_signature(&dir), before);
        assert!(!r.due(t + POLL), "the first poll sees the change");
        assert!(r.due(t + POLL * 2), "the second, unchanged, reloads");
        assert!(!r.due(t + POLL * 3), "and only once");
        // A busy retry fires without a change.
        r.watch.as_mut().unwrap().retry = Some(t + POLL * 3);
        assert!(r.due(t + POLL * 3));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
