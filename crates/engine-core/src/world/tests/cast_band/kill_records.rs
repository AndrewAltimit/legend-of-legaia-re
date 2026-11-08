//! The Seru instant kills' Monsters Defeated credit, driven through the live
//! band (`World::tick`) on a monster that carries a reaction ladder, so the
//! death arm of `FUN_8004AD80` runs wherever the kill stages a knockdown.
//! Each kill counts once, from the module's own arm, except Ultimate Rave's,
//! which also stages the knockdown and reaches the death arm.

use super::*;
use legaia_actor_formats::monster_archive::{MonsterAnimation, PartPose};

const NIGHTO: u8 = 0x85;
const ALURU: u8 = 0x8E;
const KEMARO: u8 = 0x90;
const ULTIMATE_RAVE: u8 = 0x96;
const JEDO: u8 = 0x9D;

fn clip(action_id: u8, frames: usize) -> MonsterAnimation {
    MonsterAnimation {
        variant_track: Default::default(),
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: frames,
        frames: (0..frames)
            .map(|_| {
                vec![PartPose {
                    tx: 0,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 0,
                }]
            })
            .collect(),
    }
}

/// `seru_cast_world` casting `spell` on one monster with `hp`, its clip bank
/// holding an idle, a flinch (tag 2) and a knockdown (tag 4).
fn kill_world(spell: u8, hp: u16) -> World {
    let mut world = seru_cast_world();
    world.battle.set_battle_magic(0, 999);
    world.actors[0].battle.mp = 999;
    let mut list = world.party.roster.members[0].spell_list();
    list.ids[0] = spell;
    list.levels[0] = 9;
    world.party.roster.members[0].set_spell_list(list);
    world.actors[1].battle_monster_id = Some(1);
    world.actors[1].battle.hp = hp;
    world.actors[1].battle.max_hp = hp.max(1);
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 8];
    clips[0] = Some(clip(0, 8));
    clips[1] = Some(clip(2, 8));
    clips[2] = Some(clip(4, 8));
    world.set_actor_battle_action_clips(1, std::sync::Arc::new(clips));
    if world.tables.spell_catalog.get(spell).is_none() {
        world
            .tables
            .spell_catalog
            .insert(legaia_engine_battle::spells::SpellDef {
                id: spell,
                name: format!("{spell:02X}"),
                mp_cost: 10,
                element: legaia_engine_battle::spells::SpellElement::Neutral,
                target: legaia_engine_battle::spells::SpellTarget::OneEnemy,
                effect: legaia_engine_battle::spells::SpellEffect::Damage {
                    base_power: 20,
                    element: legaia_engine_battle::spells::SpellElement::Neutral,
                },
                anim_id: 0x25 + (spell - 0x81),
                effect_class: 0,
            });
    }
    world.battle.spell_menu = world.build_battle_spell_session(0);
    world
}

/// Run the cast to the next member's turn and some frames past it, so a
/// knockdown the kill staged plays out and reaches the death arm.
fn run_cast(world: &mut World) {
    confirm_cast(world);
    let mut done = 0;
    for _ in 0..0x1800 {
        world.set_pad(0);
        let _ = world.tick();
        if world.take_pending_summon_spawn().is_some() {
            world.seat_summon_actor(9);
        }
        if world.battle.command.is_some() || world.battle_ctx.active_actor != 0 {
            done += 1;
            if done > 0x100 {
                break;
            }
        }
    }
}

fn defeated(world: &World) -> u32 {
    world.party.roster.members[0]
        .battle_records()
        .monsters_defeated
}

/// The first seed whose cast of `spell` leaves the victim at `hp == 0`
/// (`want_kill`) or standing, with what the world looks like after it.
fn first_cast(spell: u8, hp: u16, want_kill: bool) -> World {
    for seed in 0..1024u32 {
        let mut w = kill_world(spell, hp);
        w.rng_state = seed;
        run_cast(&mut w);
        if (w.actors[1].battle.hp == 0) == want_kill {
            return w;
        }
    }
    panic!("no seed gave spell {spell:#04x} kill={want_kill}");
}

/// Nighto's kill counts once, from arm 0 (`0x801F6F0C`): its victim goes
/// into the defeat fade without a knockdown, so the death arm never runs.
/// The cast folds no damage, so a confused victim keeps every HP, as
/// retail's Gobu Gobu keeps its 76.
#[test]
fn nighto_counts_its_kill_once_and_deals_no_damage() {
    use legaia_engine_battle::magic_xp::spell_xp;
    let w = first_cast(NIGHTO, 9000, true);
    assert_eq!(defeated(&w), 1);
    assert_eq!(
        spell_xp(&w.party.roster.members[0], 0),
        0xC,
        "the kill leg's XP"
    );
    assert_eq!(w.actors[1].battle_reaction, None, "no knockdown staged");

    let w = first_cast(NIGHTO, 9000, false);
    assert_eq!(w.actors[1].battle.hp, 9000, "no damage off the kill leg");
    assert_eq!(defeated(&w), 0);
    let xp = spell_xp(&w.party.roster.members[0], 0);
    let want = match w.casting.module_nighto_outcome {
        Some(o) => o.arm0_credit().spell_xp,
        None => xp,
    };
    assert!(
        xp == want && (xp == 4 || xp == 8),
        "resist or confuse XP, read {xp}"
    );
}

/// Aluru's kill counts once, on arm `0x11`'s kill pass (`0x801F7E48`).
#[test]
fn aluru_counts_its_kill_once() {
    let w = first_cast(ALURU, 9000, true);
    assert_eq!(defeated(&w), 1);
}

/// Kemaro's lethal roll counts once (`0x801F8810`); its victim is put back
/// on idle, not its knockdown.
#[test]
fn kemaro_counts_its_kill_once() {
    let w = first_cast(KEMARO, 1, true);
    assert_eq!(defeated(&w), 1);
}

/// Jedo off a scripted fight counts every monster seat it banishes, once
/// each (`0x801F864C`).
#[test]
fn jedo_counts_each_banished_monster_once() {
    let w = first_cast(JEDO, 9000, true);
    assert_eq!(defeated(&w), 1);
}

/// Ultimate Rave's kill counts on arm 9 (`0x801F7580`) and again in the
/// death arm: arm 10 stages the victim's knockdown (`+0x1DA = +0x1F1`) on
/// zero HP.
#[test]
fn ultimate_rave_counts_its_kill_in_the_arm_and_the_death_arm() {
    let w = first_cast(ULTIMATE_RAVE, 9000, true);
    assert_eq!(defeated(&w), 2);
    let w = first_cast(ULTIMATE_RAVE, 9000, false);
    assert_eq!(defeated(&w), 0);
}

// ---------------------------------------------------------------------------
// Disc-gated: the result lines read off the module images.
// ---------------------------------------------------------------------------

fn disc_pool() -> Option<std::sync::Arc<legaia_overlay_images::cast_effect_pool::CastEffectPool>> {
    use legaia_overlay_images::cast_effect_pool::{
        CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool,
    };
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let dir = ["extracted", "../../extracted"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file())?;
    let mut archive = legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).ok()?;
    let mut pool = CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let Some(entry) = archive.entries.get(idx as usize).cloned() else {
            continue;
        };
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_ok() {
            pool.insert(idx, &bytes);
        }
    }
    Some(std::sync::Arc::new(pool))
}

/// The bar a cast left up, as its raw line and its subject, against the
/// module's own line at `va`.
fn assert_bar(w: &World, entry: u32, va: u32, subject: Option<u8>) {
    let pool = w.casting.effect_pool.as_ref().expect("pool");
    let image = &pool.module(entry).expect("module resident").bytes;
    let off = (va - legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE) as usize;
    let tail = &image[off..];
    let line = &tail[..tail.iter().position(|&b| b == 0).expect("terminated")];
    assert!(!line.is_empty());
    let bar = w.battle.steal_caption.as_ref().expect("the bar is up");
    assert_eq!(bar.text, w.expand_name_escapes(line));
    assert_eq!(bar.subject, subject);
}

/// Aluru's kill raises the bar with the victim's name ahead of the line at
/// `0x801F8FEC`; a cast that spares the victim raises none.
#[test]
fn aluru_raises_the_annihilated_line_on_its_kill() {
    use legaia_engine_cast::cast_module_camera::{ALURU_ENTRY, ALURU_KILL_LINE};
    let Some(pool) = disc_pool() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let with_pool = |kill: bool| {
        for seed in 0..1024u32 {
            let mut w = kill_world(ALURU, 9000);
            // Level 9 always kills (`rand() % 8 < 8`); level 0 spares about
            // half the time.
            let mut list = w.party.roster.members[0].spell_list();
            list.levels[0] = if kill { 9 } else { 0 };
            w.party.roster.members[0].set_spell_list(list);
            w.casting.effect_pool = Some(pool.clone());
            w.rng_state = seed;
            confirm_cast(&mut w);
            for _ in 0..0x1800 {
                w.set_pad(0);
                let _ = w.tick();
                if w.take_pending_summon_spawn().is_some() {
                    w.seat_summon_actor(9);
                }
                if w.battle_ctx.action_state
                    == legaia_engine_battle_vm::battle_action::ActionState::DoneFadeDown.as_byte()
                {
                    break;
                }
            }
            if (w.actors[1].battle.hp == 0) == kill {
                return w;
            }
        }
        panic!("no seed gave kill={kill}");
    };
    let w = with_pool(true);
    assert_bar(&w, ALURU_ENTRY, ALURU_KILL_LINE, Some(1));
    let w = with_pool(false);
    assert!(
        w.battle.steal_caption.is_none(),
        "a spared victim raises no line"
    );
}

/// Nighto's arm 13 raises the bar for every outcome, on the line arm 0 left.
#[test]
fn nighto_raises_its_outcome_line_on_arm_13() {
    let Some(pool) = disc_pool() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..256u32 {
        let mut w = kill_world(NIGHTO, 9000);
        w.casting.effect_pool = Some(pool.clone());
        w.rng_state = seed;
        confirm_cast(&mut w);
        let mut outcome = None;
        for _ in 0..0x1800 {
            w.set_pad(0);
            let _ = w.tick();
            if w.take_pending_summon_spawn().is_some() {
                w.seat_summon_actor(9);
            }
            outcome = w.casting.module_nighto_outcome.or(outcome);
            if w.battle_ctx.action_state
                == legaia_engine_battle_vm::battle_action::ActionState::DoneFadeDown.as_byte()
            {
                break;
            }
        }
        let credit = outcome.expect("verdict drawn").arm0_credit();
        assert_bar(&w, 907, credit.line, credit.named.then_some(1));
        seen.insert(credit.line);
    }
    assert!(seen.len() >= 2, "more than one outcome reached: {seen:x?}");
}
