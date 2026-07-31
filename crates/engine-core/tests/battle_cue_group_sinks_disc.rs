//! Disc-gated: a committed battle item's cue group reaches the world's spawn
//! and SFX sinks - the engine half of the `FUN_801E22C8` wiring.
//!
//! `crates/engine-vm/tests/battle_cue_group_real.rs` proves the selection and
//! the expansion against the real tables; this proves the other end, that
//! each expanded [`CueSpawn`] lands somewhere a host drains. The two arms go
//! to the two paths retail's own arms go to:
//!
//! * `CueSpawn::Actor` -> `World::try_spawn_effect` (`FUN_801DFDF0`, the 2D
//!   effect pool),
//! * `CueSpawn::Effect` -> `World::spawn_action_table_effect` (`FUN_80050ED4`
//!   over the `0x801F6324` prototypes), plus the SFX-map byte into
//!   `World::battle_sfx_cues`.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::move_power::MovePowerCatalog;
use legaia_engine_core::world::World;
use legaia_engine_vm::battle_action::{ActionCategory, ActionState};
use legaia_engine_vm::battle_cue_group::{CueSpawn, CueTables, cue_group_for, expand_cue_group};
use legaia_prot::archive::Archive;

fn extracted() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

fn overlay_0898(dir: &std::path::Path) -> Vec<u8> {
    let mut archive = Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry");
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read 0898");
    bytes
}

#[test]
fn a_committed_items_cue_group_reaches_the_world_spawn_and_sfx_sinks() {
    let Some(dir) = extracted() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let overlay = overlay_0898(&dir);
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS");
    let items =
        legaia_asset::item_effect::ItemEffectTable::from_scus(&scus).expect("item-effect table");

    let mut world = World::new();
    world.move_power = Some(MovePowerCatalog::from_overlay_0898(&overlay).expect("catalog"));
    world.move_power_overlay = Some(Arc::from(overlay.as_slice()));
    world.set_item_effects(items.clone());
    // A 32-entry effect catalog so the actor-cue arm has a pool to spawn in
    // (the real efect.dat is not loaded here).
    {
        use legaia_engine_vm::effect_vm::{EffectCatalog, EffectScript};
        let entries: Vec<_> = (0..32)
            .map(|_| {
                (
                    EffectScript {
                        child_count: 1,
                        flags: 0,
                        spread: 0,
                        body: vec![],
                    },
                    vec![],
                )
            })
            .collect();
        world.effect_catalog = EffectCatalog::new(entries);
    }

    let aux = world
        .move_power
        .as_ref()
        .unwrap()
        .aux_tables()
        .cloned()
        .expect("aux tables off the real overlay");

    // Pick a battle-usable item whose site names a group holding at least one
    // effect cue **and** at least one sounded cue, so all three sinks are
    // exercised by one action.
    let tables = CueTables {
        groups: aux.cue_group_bytes(),
        sfx_map: aux.sfx(),
    };
    let Some((item_id, site, plan)) = (0..=0xFFu8).find_map(|id| {
        let eff = items.effect(id)?;
        if !eff.battle_usable() {
            return None;
        }
        let site = cue_group_for(eff.class, eff.tier)?;
        if site.per_target {
            return None;
        }
        let plan = expand_cue_group(site.tint, site.actor_state, 0, site.group, &tables);
        let sounded = plan
            .spawns
            .iter()
            .any(|s| matches!(s, CueSpawn::Effect { sfx: Some(_), .. }));
        let actor_cue = plan
            .spawns
            .iter()
            .any(|s| matches!(s, CueSpawn::Actor { .. }));
        (sounded && actor_cue).then_some((id, site, plan))
    }) else {
        eprintln!("[skip] no single-target battle item names a group with both cue kinds");
        return;
    };

    // Seat a 3-party / 2-monster formation and commit the item action.
    world.party_count = 3;
    for slot in 0..5u8 {
        let a = world
            .actors
            .get_mut(slot as usize)
            .expect("actor table is 8 wide");
        a.battle.liveness = 100;
        a.battle.hp = 100;
        a.battle.max_hp = 100;
    }
    world.actors[1].battle.action_category = ActionCategory::Item.as_byte();
    world.actors[1].battle.params[0] = item_id;
    world.actors[1].battle.active_target = 3;
    world.battle_ctx.active_actor = 1;
    world.battle_ctx.action_state = ActionState::SpiritPreArm.as_byte();
    world.battle_sfx_cues.clear();
    let pool_before = world.effect_pool.active_count();

    for _ in 0..0x60 {
        if world.battle_ctx.action_state == ActionState::SpiritPostDamage.as_byte() {
            break;
        }
        // Settle the two anim waits so the band reaches `0x3F`.
        if world.battle_ctx.action_state == ActionState::SpiritWait.as_byte() {
            let q = world.actors[1].battle.queued_anim;
            world.actors[1].battle.current_anim = q;
        } else if world.battle_ctx.action_state == ActionState::SpiritFire.as_byte() {
            world.actors[1].battle.current_anim = 0;
        }
        world.step_battle();
    }
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::SpiritPostDamage.as_byte(),
        "the item band did not reach the applier"
    );

    let eff = items.effect(item_id).expect("descriptor");
    assert_eq!(world.actors[1].battle.cast_class, eff.class);
    assert_eq!(world.actors[1].battle.cast_sub_class, eff.tier);

    // Actor cues reached the 2D pool.
    let actor_cues = plan
        .spawns
        .iter()
        .filter(|s| matches!(s, CueSpawn::Actor { .. }))
        .count();
    assert_eq!(
        world.effect_pool.active_count() - pool_before,
        actor_cues,
        "item {item_id:#04x} group {}: each actor cue spawns one pool master",
        site.group
    );

    // Two different sounds reached the host-drained SFX queue, in retail's
    // order: the cast cue first (state `0x3D`, `FUN_8004FCC8`, seated on the
    // *caster*), then the group's sounded effect cues (state `0x3F`,
    // `FUN_80058490`, seated where the group was placed).
    let char_kind = world.party_roster_slot(1) as u8 + 1;
    let cast = legaia_engine_vm::battle_cast_cue::cast_audio_cue(
        1, char_kind, eff.class, item_id, eff.tier,
    );
    let mut want: Vec<u16> = Vec::new();
    if let legaia_engine_vm::battle_cast_cue::CastCueOutcome::Sfx(id) = cast {
        want.push(id);
    }
    let group_sounds: Vec<u16> = plan
        .spawns
        .iter()
        .filter_map(|s| match s {
            CueSpawn::Effect { sfx: Some(v), .. } => Some(u16::from(*v)),
            _ => None,
        })
        .collect();
    assert!(!group_sounds.is_empty(), "fixture picked a silent group");
    want.extend_from_slice(&group_sounds);
    let got: Vec<u16> = world.battle_sfx_cues.iter().map(|c| c.kind).collect();
    assert_eq!(got, want, "item {item_id:#04x} group {}", site.group);
    let target = world.actors[1].battle.active_target;
    assert!(
        world.battle_sfx_cues[got.len() - group_sounds.len()..]
            .iter()
            .all(|c| c.actor_slot == target),
        "group cues are seated on the slot the group was placed at"
    );

    // The actor-state word the site passes landed on the placed slot.
    assert_eq!(
        world.actors[target as usize].battle.render_color,
        site.actor_state
    );
}
