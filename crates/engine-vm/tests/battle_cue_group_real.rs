//! Disc-gated: the cue-group chain over the **real** tables it reads - the
//! two the expander indexes and the one that selects which group an action
//! reaches.
//!
//! `legaia_asset::move_power::EffectAuxTables` carries the SFX map at
//! `0x801F6418` and the `[count][id; 4]` group records at `0x801F6470`;
//! `legaia_asset::item_effect::ItemEffectTable` carries the `(class, tier)`
//! descriptors at `0x800752C0` that `cue_group_for` turns into a group id.
//! The first test is the composition oracle for the expander pair; the second
//! walks every real battle-usable item through the selection and asserts the
//! group it names is a real record; the third drives the whole thing through
//! the action state machine, which is where it runs in production.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::move_power::{CUE_ACTOR_FLAG, EffectAuxTables};
use legaia_engine_vm::battle_action::{
    ActionCategory, ActionState, BattleActionCtx, BattleActionHost, BattleActor, step,
};
use legaia_engine_vm::battle_cue_group::{
    CUE_TINT_NEUTRAL, CueSpawn, CueTables, cue_group_for, expand_cue_group,
};

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    eprintln!("[skip] extracted/PROT.DAT missing");
    None
}

fn item_effects() -> Option<legaia_asset::item_effect::ItemEffectTable> {
    let scus = extracted_dir()?.join("SCUS_942.54");
    if !scus.is_file() {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return None;
    }
    let bytes = std::fs::read(&scus).expect("read SCUS_942.54");
    legaia_asset::item_effect::ItemEffectTable::from_scus(&bytes)
}

fn aux_tables() -> Option<EffectAuxTables> {
    let prot = extracted_dir()?.join("PROT.DAT");
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

/// Every **real** battle-usable item's descriptor, pushed through the site
/// selection: the group it names has to be a record the disc actually holds,
/// and the classes with no expander branch have to be silent.
#[test]
fn every_real_battle_item_selects_a_group_the_disc_holds() {
    let (Some(aux), Some(items)) = (aux_tables(), item_effects()) else {
        return;
    };
    let tables = CueTables {
        groups: aux.cue_group_bytes(),
        sfx_map: aux.sfx(),
    };

    let mut selected = 0usize;
    let mut with_spawns = 0usize;
    let mut per_target = 0usize;
    let mut classes = std::collections::BTreeSet::new();
    for id in 0..=0xFFu8 {
        let Some(eff) = items.effect(id) else {
            continue;
        };
        if !eff.battle_usable() {
            continue;
        }
        let Some(site) = cue_group_for(eff.class, eff.tier) else {
            continue;
        };
        selected += 1;
        classes.insert(eff.class);
        if site.per_target {
            per_target += 1;
        }
        // The selected group must be inside the table the disc carries, and
        // its record must round-trip through the expander.
        let (count, ids) = aux.cue_group(site.group).unwrap_or_else(|| {
            panic!(
                "item {id:#04x} class {} tier {} selected group {} - past the table",
                eff.class, eff.tier, site.group
            )
        });
        let out = expand_cue_group(site.tint, site.actor_state, 0x200, site.group, &tables);
        assert_eq!(out.spawns.len(), count as usize, "item {id:#04x}");
        if count > 0 {
            with_spawns += 1;
        }
        for (i, spawn) in out.spawns.iter().enumerate() {
            match *spawn {
                CueSpawn::Actor { id: cue, .. } => {
                    assert_eq!(cue, ids[i] & !CUE_ACTOR_FLAG);
                }
                CueSpawn::Effect { id: cue, sfx, .. } => {
                    assert_eq!(cue, ids[i]);
                    assert_eq!(sfx, aux.effect_sfx(cue).filter(|&s| s != 0));
                }
            }
        }
    }
    // Non-vacuous: real items, several distinct classes, real records, and
    // the class-1 loop arm reached by at least one of them.
    assert!(
        selected >= 10,
        "only {selected} battle items selected a site"
    );
    assert!(classes.len() >= 3, "only {classes:?} reached the expander");
    assert!(with_spawns > 0, "no selected group holds any cue");
    assert!(per_target > 0, "no real item reaches the per-slot loop arm");
}

/// The same chain, driven through the action state machine on real bytes:
/// one committed item action seeds `+0x1E8`/`+0x1E9` off the disc descriptor
/// table, calls the applier with them, and places the disc group's cues.
#[test]
fn the_state_machine_places_a_real_items_cue_group() {
    let (Some(aux), Some(items)) = (aux_tables(), item_effects()) else {
        return;
    };

    // Pick the first battle-usable item whose site names a non-empty group.
    let Some((item_id, site)) = (0..=0xFFu8).find_map(|id| {
        let eff = items.effect(id)?;
        if !eff.battle_usable() {
            return None;
        }
        let site = cue_group_for(eff.class, eff.tier)?;
        let (count, _) = aux.cue_group(site.group)?;
        (count > 0).then_some((id, site))
    }) else {
        panic!("no real battle item selects a non-empty cue group");
    };
    let (count, _) = aux.cue_group(site.group).expect("group in range");

    struct Host {
        actors: Vec<BattleActor>,
        items: legaia_asset::item_effect::ItemEffectTable,
        groups: Vec<u8>,
        sfx: Vec<u8>,
        cues: Vec<CueSpawn>,
        applier: Vec<(u8, u8, u8, u8)>,
    }
    impl BattleActionHost for Host {
        fn actor(&self, slot: u8) -> Option<&BattleActor> {
            self.actors.get(slot as usize)
        }
        fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
            self.actors.get_mut(slot as usize)
        }
        fn item_effect_class_pair(&self, id: u8) -> Option<(u8, u8)> {
            self.items.effect(id).map(|e| (e.class, e.tier))
        }
        fn cue_tables(&self) -> Option<(&[u8], &[u8])> {
            Some((&self.groups, &self.sfx))
        }
        fn spawn_cue(&mut self, _slot: u8, spawn: CueSpawn) {
            self.cues.push(spawn);
        }
        fn apply_damage(&mut self, class: u8, tier: u8, target: u8, party: u8) {
            self.applier.push((class, tier, target, party));
        }
    }

    let mut actors = vec![BattleActor::default(); 8];
    for a in actors.iter_mut() {
        a.liveness = 100;
        a.hp = 100;
    }
    actors[1].action_category = ActionCategory::Item.as_byte();
    actors[1].params[0] = item_id;
    actors[1].active_target = 4;
    let mut host = Host {
        actors,
        items,
        groups: aux.cue_group_bytes().to_vec(),
        sfx: aux.sfx().to_vec(),
        cues: Vec::new(),
        applier: Vec::new(),
    };

    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 1;
    ctx.action_state = ActionState::SpiritPreArm.as_byte();
    for _ in 0..0x40 {
        if ctx.action_state == ActionState::SpiritPostDamage.as_byte() {
            break;
        }
        // Settle whatever the band is waiting on so the walk reaches `0x3F`.
        let queued = host.actors[1].queued_anim;
        if ctx.action_state == ActionState::SpiritWait.as_byte() {
            host.actors[1].current_anim = queued;
        } else if ctx.action_state == ActionState::SpiritFire.as_byte() {
            host.actors[1].current_anim = 0;
        }
        step(&mut host, &mut ctx);
    }
    assert_eq!(
        ctx.action_state,
        ActionState::SpiritPostDamage.as_byte(),
        "the band did not reach the applier"
    );

    let eff = host.items.effect(item_id).expect("descriptor");
    assert_eq!(host.actors[1].cast_class, eff.class);
    assert_eq!(host.actors[1].cast_sub_class, eff.tier);
    assert_eq!(
        host.applier,
        vec![(eff.class, eff.tier, 4, 1)],
        "the applier sees the disc descriptor, once"
    );
    // The per-target arm walks one side of the field; this fixture seats the
    // full 8-slot table, so a party-side walk is three slots.
    let expected = if site.per_target {
        count as usize * 3
    } else {
        count as usize
    };
    assert_eq!(
        host.cues.len(),
        expected,
        "item {item_id:#04x} group {} should have placed {expected} cues",
        site.group
    );
}
