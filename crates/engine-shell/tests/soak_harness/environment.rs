//! Where the soak reads its game from and writes its output: the env knobs,
//! the disc / extracted source, session boot and the report directory.

use super::*;

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

pub(super) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

pub(super) fn env_u64(key: &str) -> Option<u64> {
    let v = std::env::var(key).ok()?;
    let v = v.trim();
    if let Some(hex) = v.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        v.parse().ok()
    }
}

pub(super) fn env_flag(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Where the session reads its assets from.
#[derive(Clone, Debug)]
pub(super) enum Source {
    Extracted(PathBuf),
    Disc(PathBuf),
}

pub(super) fn source() -> Option<Source> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from);
    let Some(disc) = disc.filter(|p| p.exists()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or missing (disc-gated convention)");
        return None;
    };
    let mut candidates = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        candidates.push(PathBuf::from(d));
    }
    candidates.push(repo_root().join("extracted"));
    for d in candidates {
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(Source::Extracted(d));
        }
    }
    Some(Source::Disc(disc))
}

pub(super) fn open_session(src: &Source) -> NativeSession {
    open_session_armed(src, true)
}

/// [`open_session`], with the hosts' faithful-play arming applied only when
/// `arm` is set.
pub(super) fn open_session_armed(src: &Source, arm: bool) -> NativeSession {
    let cfg = BootConfig {
        scene: legaia_engine_session::session::DEFAULT_BOOT_SCENE.to_string(),
        enable_audio: false,
    };
    let mut session = match src {
        Source::Extracted(d) => {
            NativeSession::open(d, &cfg).expect("open NativeSession (extracted)")
        }
        Source::Disc(p) => NativeSession::open_disc(p, &cfg).expect("open NativeSession (disc)"),
    };
    if arm {
        arm_host_play_defaults(&mut session);
    }
    session
}

/// The faithful-play arming both play hosts apply on top of a bare
/// `BootSession` (`play-window`'s defaults in `window/run.rs`, the browser
/// page's `enter_field_core`): field talk through the inline field-VM runner,
/// retail's leading-edge wall footprint, solid NPC bodies and per-step
/// terrain follow. `BootSession` already sets the NPCs' ambient walk.
///
/// Without it the soak played a configuration no host ships: a conversation
/// typed only its first segment and never ran its record, and the
/// candidate-centre wall test let a diagonal press wedge the player between
/// sub-cells the retail footprint never reaches (`town0d`'s cupboard nook at
/// (94, 17), reported `confined`).
pub(super) fn arm_host_play_defaults(session: &mut NativeSession) {
    let w = &mut session.host.world;
    w.toggles.use_vm_dialogue = true;
    w.locomotion.leading_edge_wall_probes = true;
    w.locomotion.follow_terrain_height = true;
    w.npcs.solid = true;
    w.npcs.animate = true;
}

pub(super) fn out_dir(tag: &str) -> PathBuf {
    let base = std::env::var_os("LEGAIA_SOAK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target").join("soak"));
    base.join(tag)
}
