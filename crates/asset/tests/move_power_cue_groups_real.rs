//! Disc-gated: the cue-group table (`0x801F6470`) parses out of the real
//! PROT 0898 (battle-action overlay) entry, and its member ids resolve in the
//! two sibling effect tables the same struct already carries.
//!
//! `FUN_801E22C8` indexes this table by group id and expands each record's
//! `[count][id; 4]` into spawns: an id with bit `0x80` set is an actor cue, and
//! anything else indexes the SFX map at `0x801F6418` and the prototype table at
//! `0x801F6324`. The check that matters is the cross-table one - a shifted base
//! would still yield records that look like counts and ids, but their ids would
//! not land inside the sibling tables.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is absent (the
//! workspace disc-gated convention).

use std::path::PathBuf;

use legaia_asset::move_power::{
    CUE_ACTOR_FLAG, CUE_GROUP_STRIDE, CUE_GROUP_TABLE_LEN, EffectAuxTables,
};
use legaia_prot::archive::Archive;

/// PROT index of the battle-action overlay, the entry all three aux tables
/// live in.
const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::battle_camera_table::BATTLE_ACTION_OVERLAY_PROT_INDEX;

fn extracted_prot() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

fn aux_tables() -> Option<EffectAuxTables> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let prot = extracted_prot().or_else(|| {
        eprintln!("[skip] extracted/PROT.DAT missing");
        None
    })?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");
    Some(EffectAuxTables::parse(&bytes).expect("aux tables parse off the real overlay"))
}

#[test]
fn cue_groups_parse_off_the_real_overlay() {
    let Some(aux) = aux_tables() else { return };

    let mut populated = 0usize;
    for id in 0..CUE_GROUP_TABLE_LEN as u8 {
        let (count, ids) = aux.cue_group(id).expect("every group id is in range");
        assert!(
            (count as usize) < CUE_GROUP_STRIDE,
            "group {id} names {count} cues, more than the record holds"
        );
        if count > 0 {
            populated += 1;
        }
        for &cue in &ids[..count as usize] {
            if cue & CUE_ACTOR_FLAG != 0 {
                continue;
            }
            assert!(
                aux.effect_sfx(cue).is_some() && aux.effect_proto(cue).is_some(),
                "group {id} names effect cue {cue:#04x}, which is outside the sibling tables"
            );
        }
    }
    // Non-vacuous: the table is not a run of empty records.
    assert!(
        populated >= 10,
        "only {populated} of {CUE_GROUP_TABLE_LEN} groups carry cues"
    );
}

/// The retail expander is handed group ids up to `0xC` by `FUN_800402F4`'s
/// eleven call sites, so every one of those has to resolve - and each must name
/// at least one actor cue, which is the arm that plays the strike pose.
#[test]
fn every_group_id_retail_passes_resolves() {
    let Some(aux) = aux_tables() else { return };

    for id in 0..=0x0Cu8 {
        let (count, ids) = aux.cue_group(id).unwrap_or_else(|| panic!("group {id}"));
        assert!(count > 0, "group {id} is empty");
        assert!(
            ids[..count as usize]
                .iter()
                .any(|c| c & CUE_ACTOR_FLAG != 0),
            "group {id} names no actor cue: {ids:02x?}"
        );
    }
}
