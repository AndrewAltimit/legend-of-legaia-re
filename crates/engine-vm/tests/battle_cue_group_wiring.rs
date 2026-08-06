//! The Item / Spirit band's presentation chain, driven through the action
//! state machine rather than called directly.
//!
//! One committed action has to walk `0x3C -> 0x3D -> 0x3E -> 0x3F` and, on
//! the way, seed `+0x1E8`/`+0x1E9` from a disc table, fire its cast cue, call
//! the applier with the class and tier (not the staged anim), expand the cue
//! group its `(class, tier)` selects, and land every spawn in a sink. These
//! assert that whole path, because each link was individually correct before
//! and the chain still did nothing.

use legaia_engine_vm::battle_action::{
    ActionCategory, ActionState, BattleActionCtx, BattleActionHost, BattleActor, step,
};
use legaia_engine_vm::battle_cue_group::{CUE_GROUP_STRIDE, CueSpawn};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Rec {
    Applier {
        class: u8,
        tier: u8,
        target: u8,
        party_index: u8,
    },
    Cue {
        slot: u8,
        spawn: CueSpawn,
    },
    Sfx(u16),
    ItemGive(u8),
}

struct Host {
    actors: Vec<BattleActor>,
    /// `item_id -> (class, tier)`.
    item_effects: std::collections::HashMap<u8, (u8, u8)>,
    spell_class: std::collections::HashMap<u8, u8>,
    spell_sub: std::collections::HashMap<u8, u8>,
    groups: Vec<u8>,
    sfx_map: Vec<u8>,
    log: Vec<Rec>,
    /// Whether `cue_tables` reports a table at all.
    tables_installed: bool,
}

impl Host {
    fn new() -> Self {
        // Group 3 (class 3 status cure): one actor cue + one effect cue whose
        // SFX-map byte is set. Group 6 (class 4 revive): one effect cue with
        // no sound. Group 8 (class 7 tier 1): one effect cue with sound.
        let mut groups = vec![0u8; CUE_GROUP_STRIDE * 16];
        let put = |g: &mut Vec<u8>, id: usize, cues: &[u8]| {
            g[id * CUE_GROUP_STRIDE] = cues.len() as u8;
            for (i, c) in cues.iter().enumerate() {
                g[id * CUE_GROUP_STRIDE + 1 + i] = *c;
            }
        };
        put(&mut groups, 5, &[0x82, 0x07]);
        put(&mut groups, 6, &[0x09]);
        put(&mut groups, 8, &[0x0A]);
        let mut sfx_map = vec![0u8; 32];
        sfx_map[0x07] = 0x33;
        sfx_map[0x0A] = 0x41;
        // 0x09 stays silent.
        Self {
            actors: vec![BattleActor::default(); 8],
            item_effects: std::collections::HashMap::new(),
            spell_class: std::collections::HashMap::new(),
            spell_sub: std::collections::HashMap::new(),
            groups,
            sfx_map,
            log: Vec::new(),
            tables_installed: true,
        }
    }

    fn with_party(mut self, n: usize) -> Self {
        for a in self.actors.iter_mut().take(n) {
            a.liveness = 100;
            a.hp = 100;
        }
        for a in self.actors.iter_mut().skip(3) {
            a.liveness = 60;
            a.hp = 60;
        }
        let _ = n;
        self
    }

    fn take(&mut self) -> Vec<Rec> {
        std::mem::take(&mut self.log)
    }
}

impl BattleActionHost for Host {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.actors.get(slot as usize)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.actors.get_mut(slot as usize)
    }
    fn roster_character_id(&self, slot: u8) -> u8 {
        if slot < 3 { slot + 1 } else { 4 }
    }
    fn item_effect_class_pair(&self, id: u8) -> Option<(u8, u8)> {
        self.item_effects.get(&id).copied()
    }
    fn spell_class_byte(&self, id: u8) -> Option<u8> {
        self.spell_class.get(&id).copied()
    }
    fn spell_sub_class_byte(&self, id: u8) -> Option<u8> {
        self.spell_sub.get(&id).copied()
    }
    fn cue_tables(&self) -> Option<(&[u8], &[u8])> {
        self.tables_installed
            .then_some((self.groups.as_slice(), self.sfx_map.as_slice()))
    }
    fn spawn_cue(&mut self, slot: u8, spawn: CueSpawn) {
        self.log.push(Rec::Cue { slot, spawn });
    }
    fn one_shot_sfx(&mut self, id: u16) {
        self.log.push(Rec::Sfx(id));
    }
    fn cast_item_give(&mut self, voice_arg: u8) {
        self.log.push(Rec::ItemGive(voice_arg));
    }
    fn apply_damage(&mut self, class: u8, tier: u8, target: u8, party_index: u8) {
        self.log.push(Rec::Applier {
            class,
            tier,
            target,
            party_index,
        });
    }
}

/// Drive one committed action from `SpiritPreArm` to the end of
/// `SpiritFireDamage`, returning everything the host recorded.
fn run_band(host: &mut Host, slot: u8) -> Vec<Rec> {
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = slot;
    ctx.action_state = ActionState::SpiritPreArm.as_byte();
    // 0x3C -> 0x3D.
    step(host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::SpiritWait.as_byte());
    // 0x3D holds until the staged anim settles; settle it.
    let queued = host.actors[slot as usize].queued_anim;
    host.actors[slot as usize].current_anim = queued;
    step(host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::SpiritFire.as_byte());
    // 0x3E holds until the clip drains.
    host.actors[slot as usize].current_anim = 0;
    step(host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::SpiritFireDamage.as_byte());
    // 0x3F counts down 0x20 frames at dt = 1.
    for _ in 0..0x21 {
        if ctx.action_state != ActionState::SpiritFireDamage.as_byte() {
            break;
        }
        step(host, &mut ctx);
    }
    assert_eq!(ctx.action_state, ActionState::SpiritPostDamage.as_byte());
    host.take()
}

/// A status-cure item: the class-3 site, group 5, one actor cue and one
/// sounded effect cue - and the applier sees the descriptor, not the anim.
#[test]
fn an_item_action_expands_its_cue_group_and_every_cue_lands_in_a_sink() {
    let mut host = Host::new().with_party(3);
    host.item_effects.insert(0x21, (3, 0));
    host.actors[1].action_category = ActionCategory::Item.as_byte();
    host.actors[1].params[0] = 0x21;
    host.actors[1].active_target = 4;
    // A staged anim byte the pre-`+0x1E8` port used to pass as the class.
    host.actors[1].queued_anim_b = 0x77;
    host.actors[1].facing_angle = 0x300;

    let log = run_band(&mut host, 1);

    // The seed reached the actor.
    assert_eq!(host.actors[1].cast_class, 3);
    assert_eq!(host.actors[1].cast_sub_class, 0);

    // The applier got (class, tier, target, roster index) - the roster index
    // is `DAT_8007BD10[1] - 1 == 1`, not the battle slot as a party slot.
    assert!(
        log.contains(&Rec::Applier {
            class: 3,
            tier: 0,
            target: 4,
            party_index: 1,
        }),
        "{log:?}"
    );
    assert!(
        !log.iter()
            .any(|r| matches!(r, Rec::Applier { class: 0x77, .. })),
        "the staged anim must not reach the applier"
    );

    // Group 5's two cues both reached a sink, placed on the target slot.
    let cues: Vec<&Rec> = log
        .iter()
        .filter(|r| matches!(r, Rec::Cue { .. }))
        .collect();
    assert_eq!(cues.len(), 2, "{log:?}");
    assert_eq!(
        *cues[0],
        Rec::Cue {
            slot: 4,
            spawn: CueSpawn::Actor { id: 2, yaw: 0 },
        }
    );
    let Rec::Cue {
        slot,
        spawn: CueSpawn::Effect { id, sfx, tint, .. },
    } = cues[1]
    else {
        panic!("second cue is the effect arm: {:?}", cues[1]);
    };
    assert_eq!(*slot, 4);
    assert_eq!(*id, 0x07);
    assert_eq!(*sfx, Some(0x33), "the SFX-map byte rides the cue");
    assert!(
        tint.is_some(),
        "the class-3 site's tint is not neutral, so the spawn recolours"
    );
}

/// The class-4 revive site is the one that passes `CUE_ACTOR_STATE_SKIP`, and
/// the difference is visible on the actor the SM writes.
#[test]
fn the_revive_site_leaves_the_actor_scale_word_alone() {
    let mut host = Host::new().with_party(3);
    host.item_effects.insert(0x30, (4, 1));
    host.actors[1].action_category = ActionCategory::Item.as_byte();
    host.actors[1].params[0] = 0x30;
    host.actors[1].active_target = 0;
    host.actors[0].render_blend = 0xDEAD;

    let log = run_band(&mut host, 1);
    assert!(log.iter().any(|r| matches!(
        r,
        Rec::Cue {
            slot: 0,
            spawn: CueSpawn::Effect {
                id: 0x09,
                sfx: None,
                ..
            }
        }
    )));
    assert_eq!(
        host.actors[0].render_blend, 0xDEAD,
        "the revive arm's `a1` is the skip word, so `+0x0C` is not written"
    );
    assert_eq!(host.actors[0].render_color, 0x2008_0200);

    // Contrast: the status-cure arm does write it.
    let mut host = Host::new().with_party(3);
    host.item_effects.insert(0x21, (3, 0));
    host.actors[1].action_category = ActionCategory::Item.as_byte();
    host.actors[1].params[0] = 0x21;
    host.actors[1].active_target = 0;
    host.actors[0].render_blend = 0xDEAD;
    run_band(&mut host, 1);
    assert_eq!(host.actors[0].render_blend, 0x2000);
}

/// The class-1 arm's `jal` is inside a per-slot loop, and the side it walks is
/// the target byte's: anything but `9` is the party side, `9` is the monsters.
/// The gate is seat occupancy, not liveness - a downed member still gets it.
#[test]
fn the_party_wide_restore_places_its_group_once_per_seated_member() {
    let mut host = Host::new().with_party(3);
    // class 1 tier 4 -> group 5, the two-cue record.
    host.item_effects.insert(0x22, (1, 4));
    host.actors[2].liveness = 0;
    host.actors[1].action_category = ActionCategory::Item.as_byte();
    host.actors[1].params[0] = 0x22;
    host.actors[1].active_target = 8;

    let log = run_band(&mut host, 1);
    let slots: Vec<u8> = log
        .iter()
        .filter_map(|r| match r {
            Rec::Cue { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect();
    // Three seated party slots, two cues apiece - slot 2's zero HP does not
    // exclude it (retail gates on the roster byte, not `+0x14C`).
    assert_eq!(slots, vec![0, 0, 1, 1, 2, 2], "{log:?}");

    // Target `9` walks the monster side instead.
    let mut host = Host::new().with_party(3);
    host.item_effects.insert(0x22, (1, 4));
    host.actors[1].action_category = ActionCategory::Item.as_byte();
    host.actors[1].params[0] = 0x22;
    host.actors[1].active_target = 9;
    let log = run_band(&mut host, 1);
    let slots: Vec<u8> = log
        .iter()
        .filter_map(|r| match r {
            Rec::Cue { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect();
    assert_eq!(slots, vec![3, 3, 4, 4, 5, 5, 6, 6], "{log:?}");
}

/// A class with no expander branch places nothing, and neither does a host
/// with no overlay - the two silences are different and both are real.
#[test]
fn no_branch_and_no_tables_both_place_nothing() {
    // Class 6 (permanent stat-up) has an inner table that only bumps
    // counters; it never reaches the expander.
    let mut host = Host::new().with_party(3);
    host.item_effects.insert(0x40, (6, 0));
    host.actors[1].action_category = ActionCategory::Item.as_byte();
    host.actors[1].params[0] = 0x40;
    host.actors[1].active_target = 4;
    let log = run_band(&mut host, 1);
    assert!(
        log.iter()
            .any(|r| matches!(r, Rec::Applier { class: 6, .. }))
    );
    assert!(!log.iter().any(|r| matches!(r, Rec::Cue { .. })));

    // Same action, no disc tables: the applier still runs, the group does not.
    let mut host = Host::new().with_party(3);
    host.tables_installed = false;
    host.item_effects.insert(0x21, (3, 0));
    host.actors[1].action_category = ActionCategory::Item.as_byte();
    host.actors[1].params[0] = 0x21;
    host.actors[1].active_target = 4;
    let log = run_band(&mut host, 1);
    assert!(
        log.iter()
            .any(|r| matches!(r, Rec::Applier { class: 3, .. }))
    );
    assert!(!log.iter().any(|r| matches!(r, Rec::Cue { .. })));
}

/// The cast cue fires once, on the frame the staged anim settles, and its id
/// is keyed on the roster character - not the battle slot.
#[test]
fn the_cast_cue_fires_from_the_roster_character_band() {
    let mut host = Host::new().with_party(3);
    // A spell whose class byte is 2 (MP restore band) - the player leg's
    // `char_kind*0x10 + 0xF9`.
    host.spell_class.insert(0x81, 2);
    host.actors[1].action_category = ActionCategory::Magic.as_byte();
    host.actors[1].params[0] = 0x81;
    host.actors[1].active_target = 4;

    let log = run_band(&mut host, 1);
    // roster id for battle slot 1 is 2 -> base 0x20, class 2 -> +0xF9.
    assert!(log.contains(&Rec::Sfx(0x119)), "{log:?}");
    assert_eq!(
        log.iter().filter(|r| matches!(r, Rec::Sfx(_))).count(),
        1,
        "one cue per cast"
    );

    // A monster caster takes the enemy leg regardless of the class band.
    let mut host = Host::new().with_party(3);
    host.spell_class.insert(0x81, 2);
    host.actors[4].action_category = ActionCategory::Magic.as_byte();
    host.actors[4].params[0] = 0x81;
    host.actors[4].active_target = 0;
    let log = run_band(&mut host, 4);
    assert!(log.contains(&Rec::Sfx(0x20C)), "{log:?}");
}

/// The magic leg seeds both bytes from the spell record, and the tier is what
/// moves the class-2 site's group id.
#[test]
fn a_spell_seeds_its_class_and_sub_index_and_the_tier_moves_the_group() {
    let mut host = Host::new().with_party(3);
    host.spell_class.insert(0x82, 2);
    host.spell_sub.insert(0x82, 2);
    host.actors[1].action_category = ActionCategory::Magic.as_byte();
    host.actors[1].params[0] = 0x82;
    host.actors[1].active_target = 4;
    let log = run_band(&mut host, 1);
    assert_eq!(host.actors[1].cast_class, 2);
    assert_eq!(host.actors[1].cast_sub_class, 2);
    // class 2 -> group tier + 3 == 5, the two-cue record.
    assert_eq!(
        log.iter().filter(|r| matches!(r, Rec::Cue { .. })).count(),
        2,
        "{log:?}"
    );

    // Without the `+1` byte the same spell reads as tier 0 -> group 3, which
    // is empty in this fixture. This is the shape of the residual a host with
    // no `spell_sub_class_byte` carrier has.
    let mut host = Host::new().with_party(3);
    host.spell_class.insert(0x82, 2);
    host.actors[1].action_category = ActionCategory::Magic.as_byte();
    host.actors[1].params[0] = 0x82;
    host.actors[1].active_target = 4;
    let log = run_band(&mut host, 1);
    assert_eq!(host.actors[1].cast_sub_class, 0);
    assert!(!log.iter().any(|r| matches!(r, Rec::Cue { .. })));
}

/// The `0xFE` queue head takes the item-give special ahead of the class
/// dispatch, on the player leg only.
#[test]
fn the_item_give_special_routes_to_its_own_host_channel() {
    let mut host = Host::new().with_party(3);
    host.item_effects.insert(0xFE, (0, 0));
    host.actors[0].action_category = ActionCategory::Item.as_byte();
    host.actors[0].params[0] = 0xFE;
    host.actors[0].active_target = 4;
    let log = run_band(&mut host, 0);
    // roster id for battle slot 0 is 1 -> voice arg 1 + 0x19.
    assert!(log.contains(&Rec::ItemGive(0x1A)), "{log:?}");
    assert!(!log.iter().any(|r| matches!(r, Rec::Sfx(_))));
}
