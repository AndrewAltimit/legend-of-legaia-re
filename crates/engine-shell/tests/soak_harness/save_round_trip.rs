//! The save round-trip run mode: save through the real commit path partway
//! through a run, load it back and diff the two saves; plus the card-save
//! seeded runs.

use super::*;
use legaia_engine_save_state::WorldSaveState;

/// Frames between two round trips in a `+rt` run.
pub(super) const ROUND_TRIP_EVERY: u64 = 600;

/// A frame a player could open the pause menu on and save: free field
/// roam, no modal, no timeline, no dialogue.
pub(super) fn round_trip_ready(s: &NativeSession) -> bool {
    let w = &s.host.world;
    matches!(w.mode, SceneMode::Field | SceneMode::WorldMap)
        && s.field_menu.is_none()
        && w.cutscene.timeline.is_none()
        && w.dialog.current.is_none()
        && w.dialog.inline.is_none()
        && !w.name_entry_active()
        && !w.shops.shop_open
        && w.active_fmv().is_none()
        // A record that engages the player refuses the menu button with the
        // rest of the pad (`World::script_context_engages_player`).
        && !w.script_context_engages_player()
}

/// The first field two saves disagree on, named, with a short detail.
pub(super) fn save_diff(
    a: &legaia_save::SaveFile,
    b: &legaia_save::SaveFile,
) -> Option<(String, String)> {
    if a.party.members.len() != b.party.members.len() {
        return Some((
            "party.len".into(),
            format!("{} -> {}", a.party.members.len(), b.party.members.len()),
        ));
    }
    for (i, (x, y)) in a.party.members.iter().zip(&b.party.members).enumerate() {
        if let Some(off) = x.raw.iter().zip(&y.raw).position(|(p, q)| p != q) {
            return Some((
                format!("party[{i}]+{off:#05x}"),
                format!("{:#04x} -> {:#04x}", x.raw[off], y.raw[off]),
            ));
        }
    }
    macro_rules! field {
        ($name:literal, $x:expr, $y:expr) => {
            if $x != $y {
                let (dx, dy) = (format!("{:?}", $x), format!("{:?}", $y));
                let cut = |s: &str| s.chars().take(160).collect::<String>();
                return Some(($name.into(), format!("{} -> {}", cut(&dx), cut(&dy))));
            }
        };
    }
    field!("ext.story_flags", a.ext.story_flags, b.ext.story_flags);
    field!(
        "ext.story_flag_bits",
        a.ext.story_flag_bits,
        b.ext.story_flag_bits
    );
    field!("ext.money", a.ext.money, b.ext.money);
    field!("ext.inventory", a.ext.inventory, b.ext.inventory);
    field!("ext.item_slots", a.ext.item_slots, b.ext.item_slots);
    field!("ext.minigames", a.ext.minigames, b.ext.minigames);
    field!(
        "ext_v2.active_party",
        a.ext_v2.active_party,
        b.ext_v2.active_party
    );
    field!("ext_v2.per_char", a.ext_v2.per_char, b.ext_v2.per_char);
    field!(
        "ext_v2.saved_chains",
        a.ext_v2.saved_chains,
        b.ext_v2.saved_chains
    );
    field!(
        "ext_v2.field_position",
        a.ext_v2.field_position,
        b.ext_v2.field_position
    );
    field!("ext_v2", a.ext_v2, b.ext_v2);
    field!("ext", a.ext, b.ext);
    None
}

/// The host half of a save-screen pick (the play window's
/// `apply_save_commit`): a Save writes the file into the in-memory card and
/// refreshes the grid's snapshot for that block; a Load resumes the file the
/// block holds, or does nothing for an empty block (the flow refuses those
/// before committing, so that arm is defensive).
pub(super) fn apply_save_commit(
    session: &mut NativeSession,
    card: &mut SoakCard,
    commit: legaia_engine_menus::save_screen::SaveCommit,
    opts: &FieldLiveOpts,
) {
    use legaia_engine_menus::save_screen::SaveCommitKind;
    let key = (commit.port, commit.cell);
    match commit.kind {
        SaveCommitKind::Save => {
            let sf = session.host.world.save_full();
            let resume = session.current_resume();
            card.insert(key, (sf, resume));
            refresh_rack(session, card);
        }
        SaveCommitKind::Load => {
            if let Some((sf, resume)) = card.get(&key).cloned() {
                let _ = session.resume_save(sf, &resume.scene, opts);
            }
        }
    }
}

/// Re-derive the save grid's port-0 snapshots from the in-memory card.
pub(super) fn refresh_rack(session: &mut NativeSession, card: &SoakCard) {
    let mut ports: Vec<Vec<SlotSnapshot>> =
        vec![(0..15).map(SlotSnapshot::empty).collect(), Vec::new()];
    for (&(port, cell), (sf, resume)) in card.iter() {
        if let Some(slot) = ports
            .get_mut(port as usize)
            .and_then(|p| p.get_mut(cell as usize))
        {
            *slot = legaia_engine_menus::save_select::snapshot_for_save(cell, sf, resume);
        }
    }
    let rack = session.save_rack().clone();
    session.set_save_rack(rack, ports);
}

/// The harness's memory card: `(port, cell)` -> the file a Save wrote.
pub(super) type SoakCard = BTreeMap<(u8, u8), (legaia_save::SaveFile, legaia_save::SaveResume)>;

/// `"town01+rt@PRO-14"` -> `("town01+rt", Some("PRO-14"))`: the run seeds
/// the party, bag and story flags from that save on the library card
/// (`LEGAIA_SOAK_CARD`, default the playthrough card) - late-game state in
/// every scene, where the New Game party reaches none of it.
pub(super) fn split_save(label: &str) -> (&str, Option<&str>) {
    match label.split_once('@') {
        Some((scene, save)) => (scene, Some(save)),
        None => (label, None),
    }
}

/// The library card the `@<save>` runs read.
pub(super) const SOAK_CARD: &str = "playthrough-endgame-7saves.mcr";

/// Prefix of the `card save` error a run raises when no save library is
/// found at all - the one card failure the fixture tests count as a skip.
pub(super) const NO_SAVE_LIBRARY: &str = "no save library";

/// The save named `save` on the soak card, lifted into a [`legaia_save::SaveFile`].
pub(super) fn card_save(save: &str) -> Result<legaia_save::SaveFile, String> {
    let lib = saves_library().ok_or_else(|| {
        format!("{NO_SAVE_LIBRARY}: set LEGAIA_SAVES_LIBRARY or check out saves/library")
    })?;
    // `<card>:<save>` names its own card; a bare save reads
    // `LEGAIA_SOAK_CARD`, else the default.
    let (card, save) = match save.split_once(':') {
        Some((card, save)) => (card.to_string(), save),
        None => (
            std::env::var("LEGAIA_SOAK_CARD").unwrap_or_else(|_| SOAK_CARD.to_string()),
            save,
        ),
    };
    let path = lib.join("cards").join(&card);
    let mounted = legaia_save::emu::MountedCard::open(&path).map_err(|e| format!("{e:#}"))?;
    for block in 1..=15u8 {
        let Some(frame) = mounted.dir_frame(block) else {
            continue;
        };
        let name: String = frame[0x0A..0x0A + 20]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if !name.ends_with(save) {
            continue;
        }
        let Some(sc) = mounted.sc_block(block) else {
            continue;
        };
        return legaia_save::SaveFile::from_retail_sc_block(
            sc,
            legaia_save::RETAIL_SC_PARTY_RECORDS,
        )
        .map_err(|e| format!("{e:#}"));
    }
    Err(format!("{card}: no save named {save}"))
}
