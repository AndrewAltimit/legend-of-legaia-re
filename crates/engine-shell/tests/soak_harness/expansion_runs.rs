//! Expansion runs: soak labels of the form
//! `xp:<target>[+nopack][+am][+rt][+seat=<mode id>|+seat=picker][+cards]`.
//!
//! The run activates the soak's expansion pack (the repo's Rebirth pack, or
//! `LEGAIA_SOAK_PACK`) through the hosts' one activation path, then enters
//! `<target>` - one of the pack's maps, or a retail scene played with the
//! pack active - and applies the session switches the suffixes name before
//! the first frame:
//!
//! - `+nopack`: no pack is activated (the mode seat then offers the built-in
//!   practice set).
//! - `+am`: `WorldToggles::action_movement` on; the field policy also jumps.
//! - `+seat=<id>` / `+seat=picker`: the mode seat, entered on that instance
//!   or on its picker (`World::enter_pack_mode` / `open_pack_mode_picker`).
//! - `+cards`: the card menu (`World::open_card_menu`).
//! - `+rt`: the save / load round trip of a retail `+rt` run, with the pack's
//!   own save block (quest progress, cards, the map location) in the save.
//!
//! The pause menu's Select (the Journal while a pack is active) is in the
//! policy's menu presses for every expansion run.

use super::*;

use legaia_expansion_format::ExpansionPack;

/// A parsed `xp:` label.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ExpansionRun {
    pub(super) target: String,
    pub(super) pack: bool,
    pub(super) action_movement: bool,
    pub(super) seat: Option<String>,
    pub(super) cards: bool,
    pub(super) round_trip: bool,
}

impl ExpansionRun {
    /// The label the retail suffix parsers read: the target, with `+rt`
    /// when the run round-trips its saves.
    pub(super) fn retail_label(&self) -> String {
        if self.round_trip {
            format!("{}+rt", self.target)
        } else {
            self.target.clone()
        }
    }
}

/// The `xp:` label prefix.
pub(super) const XP_PREFIX: &str = "xp:";

/// Parse an `xp:` label; `None` for any other label.
pub(super) fn split_expansion(label: &str) -> Option<ExpansionRun> {
    let rest = label.strip_prefix(XP_PREFIX)?;
    let mut parts = rest.split('+');
    let mut run = ExpansionRun {
        target: parts.next()?.to_string(),
        pack: true,
        ..Default::default()
    };
    for p in parts {
        match p {
            "nopack" => run.pack = false,
            "am" => run.action_movement = true,
            "cards" => run.cards = true,
            "rt" => run.round_trip = true,
            _ => run.seat = Some(p.strip_prefix("seat=")?.to_string()),
        }
    }
    Some(run)
}

/// The soak's expansion pack: `LEGAIA_SOAK_PACK` (a pack directory or
/// `.lxpack`), else the repo's `packs/rebirth`.
pub(super) fn soak_pack() -> Arc<ExpansionPack> {
    static PACK: std::sync::OnceLock<Arc<ExpansionPack>> = std::sync::OnceLock::new();
    PACK.get_or_init(|| {
        let path = std::env::var_os("LEGAIA_SOAK_PACK")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root().join("packs").join("rebirth"));
        let files = legaia_expansion_format::read_files(&path)
            .unwrap_or_else(|e| panic!("read the soak pack {}: {e:#}", path.display()));
        Arc::new(ExpansionPack::from_files(&files).expect("the soak pack parses"))
    })
    .clone()
}

/// The expansion run set: every map of the pack walked plain and with
/// action movement, the pack's entrance scene with action movement, the
/// mode seat on each of the pack's instances and on each built-in one (no
/// pack), the picker, and the card menu.
pub(super) fn expansion_runs() -> Vec<String> {
    let pack = soak_pack();
    let mut runs = Vec::new();
    for map in pack.maps.keys() {
        runs.push(format!("xp:{map}"));
        runs.push(format!("xp:{map}+am"));
        runs.push(format!("xp:{map}+rt"));
    }
    runs.push("xp:town01+am".into());
    for e in legaia_engine_modes::active::entries(&pack.modes) {
        runs.push(format!("xp:town01+seat={}", e.id));
    }
    let (builtin, _) = legaia_engine_modes::active::builtin_sections();
    for e in legaia_engine_modes::active::entries(builtin) {
        runs.push(format!("xp:town01+nopack+seat={}", e.id));
    }
    runs.push("xp:town01+seat=picker".into());
    if !pack.cards.cards.is_empty() {
        runs.push("xp:town01+cards".into());
    }
    runs
}

/// Whether `target` is a map of the soak pack.
pub(super) fn is_pack_map(target: &str) -> bool {
    soak_pack().maps.contains_key(target)
}

/// Activate the pack (unless `+nopack`) and enter the run's target.
pub(super) fn enter_expansion_run(
    session: &mut NativeSession,
    run: &ExpansionRun,
    opts: &FieldLiveOpts,
) -> Result<(), String> {
    if run.pack {
        session
            .host
            .activate_expansion_pack(soak_pack())
            .map_err(|e| format!("{e:#}"))?;
    }
    if run.pack && is_pack_map(&run.target) {
        session
            .enter_expansion_map_live(&run.target, "", opts)
            .map_err(|e| format!("{e:#}"))?;
    } else {
        session
            .enter_scene_live(&run.target, opts)
            .map_err(|e| format!("{e:#}"))?;
    }
    let w = &mut session.host.world;
    w.toggles.action_movement = run.action_movement;
    match run.seat.as_deref() {
        Some("picker") => w.open_pack_mode_picker(),
        Some(id) => w
            .enter_pack_mode(id)
            .map_err(|e| format!("enter mode {id}: {e}"))?,
        None => {}
    }
    if run.cards && !w.open_card_menu() {
        return Err("the card menu does not open".into());
    }
    Ok(())
}

#[test]
fn expansion_labels_parse() {
    assert_eq!(split_expansion("town01"), None);
    assert_eq!(
        split_expansion("xp:rebirth_tower_1f+am"),
        Some(ExpansionRun {
            target: "rebirth_tower_1f".into(),
            pack: true,
            action_movement: true,
            ..Default::default()
        })
    );
    assert_eq!(
        split_expansion("xp:town01+nopack+seat=builtin-golf"),
        Some(ExpansionRun {
            target: "town01".into(),
            pack: false,
            seat: Some("builtin-golf".into()),
            ..Default::default()
        })
    );
    assert_eq!(split_expansion("xp:town01+bogus"), None);
    // The run set covers each built-in mode and the pack's maps.
    let runs = expansion_runs();
    for id in [
        "builtin-poker",
        "builtin-golf",
        "builtin-race",
        "builtin-duel",
    ] {
        assert!(runs.iter().any(|r| r.ends_with(id)), "{id} in {runs:?}");
    }
    assert!(runs.iter().any(|r| r.ends_with("+am")));
    for r in &runs {
        assert!(split_expansion(r).is_some(), "{r} parses");
        assert!(!r.contains('@') && !r.contains("+mg"), "{r} is unambiguous");
    }
}
