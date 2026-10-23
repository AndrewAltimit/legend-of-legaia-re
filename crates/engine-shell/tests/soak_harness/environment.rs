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

/// `rel` under the workspace root or the nearest ancestor that has it. A
/// worktree under `.claude/worktrees/` carries none of the gitignored data
/// (`extracted/`, `saves/library/`), and its enclosing checkout does - the
/// same walk up the tree `full_game_ladder` and `retail-compare.py` take.
pub(super) fn up_the_tree(rel: &str, probe: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    repo_root()
        .ancestors()
        .map(|root| root.join(rel))
        .find(|p| probe(p))
}

/// The save library the `@<save>` runs read: `$LEGAIA_SAVES_LIBRARY`, else
/// `saves/library/` up the tree.
pub(super) fn saves_library() -> Option<PathBuf> {
    match std::env::var_os("LEGAIA_SAVES_LIBRARY") {
        Some(p) => Some(PathBuf::from(p)).filter(|p| p.is_dir()),
        None => up_the_tree("saves/library", Path::is_dir),
    }
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
    let complete = |d: &Path| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists();
    candidates.extend(up_the_tree("extracted", complete));
    for d in candidates {
        if complete(&d) {
            return Some(Source::Extracted(d));
        }
    }
    Some(Source::Disc(disc))
}

pub(super) fn open_session(src: &Source) -> NativeSession {
    open_session_armed(
        src,
        Arming {
            host: true,
            retail: false,
        },
    )
}

/// The session configuration a run plays under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Arming {
    /// The hosts' faithful-play arming ([`arm_host_play_defaults`]).
    pub(super) host: bool,
    /// The hosts' retail mode (`--retail` / the play page's retail mode):
    /// the persisted options pinned (`OptionsState::pin_retail`, pushed
    /// through `options::apply_to_world`) and then every fidelity toggle
    /// (`WorldToggles::pin_retail`), as `SessionSwitches::apply` does - the
    /// hop landing guard down, the retail tile windows, no flash slew.
    pub(super) retail: bool,
}

impl Arming {
    /// Read back a replay's `# soak-arming` value.
    pub(super) fn from_tag(tag: Option<&str>) -> Self {
        let tag = tag.unwrap_or("bare");
        let (base, retail) = match tag.strip_suffix("-retail") {
            Some(b) => (b, true),
            None => (tag, false),
        };
        Self {
            host: base == "host",
            retail,
        }
    }
}

/// [`open_session`], with the hosts' faithful-play arming applied only when
/// `arm.host` is set, and their retail mode when `arm.retail` is.
pub(super) fn open_session_armed(src: &Source, arm: Arming) -> NativeSession {
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
    if arm.host {
        arm_host_play_defaults(&mut session);
    }
    if arm.retail {
        pin_host_retail_mode(&mut session);
    }
    session
}

/// The hosts' retail mode on a session: the options screen's engine knobs
/// pinned and pushed into the world, then the fidelity toggles pinned.
pub(super) fn pin_host_retail_mode(session: &mut NativeSession) {
    let mut opts = legaia_engine_menus::options::OptionsState::default();
    opts.pin_retail();
    let w = &mut session.host.world;
    legaia_engine_core::options::apply_to_world(&opts, w);
    w.toggles.pin_retail();
    debug_assert!(w.toggles.retail_deviations().is_empty());
}

/// The faithful-play arming both play hosts apply on top of a bare
/// `BootSession` (`play-window`'s defaults in `window/run.rs`, the browser
/// page's `enter_field_core`): the default options' world knobs, field talk
/// through the inline field-VM runner,
/// retail's leading-edge wall footprint, solid NPC bodies and per-step
/// terrain follow. `BootSession` already sets the NPCs' ambient walk.
///
/// Without it the soak played a configuration no host ships: a conversation
/// typed only its first segment and never ran its record, and the
/// candidate-centre wall test let a diagonal press wedge the player between
/// sub-cells the retail footprint never reaches (`town0d`'s cupboard nook at
/// (94, 17), reported `confined`).
pub(super) fn arm_host_play_defaults(session: &mut NativeSession) {
    // Both hosts push their persisted options' simulation knobs onto the
    // world every tick (`BootSession::apply_option_knobs`); a fresh options
    // file is `OptionsState::default()`. A bare `World` keeps its own
    // defaults instead - `retail_static_window` off among them, while both
    // hosts start with it on - so the soak pushes the hosts' defaults once.
    // Nothing a run does reseeds these knobs (they live on `World` from its
    // construction), and a run never opens the options screen, so one push
    // stands for the hosts' per-tick re-assert.
    let defaults = legaia_engine_menus::options::OptionsState::default();
    session.apply_option_knobs(&defaults, None);
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
