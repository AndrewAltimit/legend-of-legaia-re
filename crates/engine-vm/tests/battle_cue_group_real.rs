//! Disc-gated: the cue-group expander (`FUN_801E22C8`) over the **real** two
//! tables it reads, now that both are disc-parsed.
//!
//! `legaia_asset::move_power::EffectAuxTables` carries the SFX map at
//! `0x801F6418` and, since the group region at `0x801F6470` was added to it,
//! the `[count][id; 4]` records the expander indexes. This test is the
//! composition oracle for the pair - it is **not** a wire: retail's caller is
//! the damage primitive `FUN_800402F4`, which picks the group id per
//! damage-kind branch and is not ported. See the `NOT WIRED` note on
//! `battle_cue_group::expand_cue_group`.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::move_power::{CUE_ACTOR_FLAG, EffectAuxTables};
use legaia_engine_vm::battle_cue_group::{CUE_TINT_NEUTRAL, CueSpawn, CueTables, expand_cue_group};

fn aux_tables() -> Option<EffectAuxTables> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let mut prot = None;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base).join("PROT.DAT");
        if p.is_file() {
            prot = Some(p);
            break;
        }
    }
    let Some(prot) = prot else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return None;
    };
    let mut archive = legaia_prot::archive::Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(legaia_asset::battle_camera_table::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");
    Some(EffectAuxTables::parse(&bytes).expect("aux tables parse off the real overlay"))
}

/// Every group id retail's eleven `jal 0x801E22C8` sites pass expands, and each
/// spawn's payload resolves in the tables the expander read it from.
#[test]
fn the_real_cue_groups_expand_through_the_port() {
    let Some(aux) = aux_tables() else { return };
    let tables = CueTables {
        groups: aux.cue_group_bytes(),
        sfx_map: aux.sfx(),
    };

    let mut spawns = 0usize;
    let mut actor_cues = 0usize;
    let mut sounded = 0usize;
    for id in 0..=0x0Cu8 {
        let out = expand_cue_group(CUE_TINT_NEUTRAL, 0, 0x200, id, &tables);
        let (count, _) = aux.cue_group(id).expect("group in range");
        assert_eq!(
            out.spawns.len(),
            count as usize,
            "group {id} expanded to the wrong spawn count"
        );
        spawns += out.spawns.len();
        for spawn in &out.spawns {
            match *spawn {
                CueSpawn::Actor { id: actor_id, yaw } => {
                    assert_eq!(actor_id & CUE_ACTOR_FLAG, 0, "the flag bit is stripped");
                    assert_eq!(yaw, 0x200, "the actor arm passes the unbiased heading");
                    actor_cues += 1;
                }
                CueSpawn::Effect {
                    id: cue,
                    sfx,
                    effect_index,
                    tint,
                } => {
                    assert_eq!(effect_index, cue, "one id indexes both effect tables");
                    assert_eq!(sfx, aux.effect_sfx(cue).filter(|&s| s != 0));
                    assert_eq!(tint, None, "the neutral tint recolours nothing");
                    if sfx.is_some() {
                        sounded += 1;
                    }
                }
            }
        }
    }
    // Non-vacuous on all three counts: real records, real actor cues, and real
    // sound ids coming back out of the SFX map.
    assert!(spawns >= 20, "only {spawns} spawns across the 13 groups");
    assert!(actor_cues >= 10, "only {actor_cues} actor cues");
    assert!(sounded > 0, "no group's effect cue resolved a sound id");
}
