//! Expansion boss scripts on a disc-free world: a scripted pack monster
//! walks its phases (banner lines, stat changes, a summon into a free seat),
//! telegraphs a charged move, holds on once at 1 HP, shrugs off a status it
//! is immune to, and picks deterministically; a retail fight with a scripted
//! pack active plays exactly as without one.

use std::sync::Arc;

use super::*;
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const PACK_TOML: &str = r#"
id = "bosspack"
name = "Boss Pack"
version = "0.1.0"
"#;

const DIALOG_TOML: &str = r#"
[lines.howl]
text = "The Dread Wolf howls!"

[lines.charge]
text = """
The Dread Wolf
crouches low..."""

[lines.hold]
text = "It will not fall!"
"#;

/// A Dread Wolf (on retail 9): swings at the lowest-HP member, goes savage
/// below half (attack doubled, a Rat summoned), and below a fifth only
/// telegraphs its second swing; immune to Sleep, with a last stand.
const BATTLE_TOML: &str = r#"
[monsters.dread_wolf]
base = { kind = "monster", id = 9 }
name = "Dread Wolf"
hp = 600
exp = 100

[monsters.dread_wolf.script]
target = "lowest_hp"
immune = ["sleep"]
last_stand = { line = "hold" }

[[monsters.dread_wolf.script.phases]]
moves = [{ attack = 0 }]

[[monsters.dread_wolf.script.phases]]
when = { hp_below_pct = 50 }
line = "howl"
attack = { mul = 2.0 }
flash = { color = [255, 0, 0], frames = 10 }
tint = { color = [0, 0, 40], frames = 4 }
summon = [{ kind = "monster", id = 10 }]

[[monsters.dread_wolf.script.phases]]
when = { hp_below_pct = 20 }
moves = [{ attack = 1, hits = 2, charge = true, charge_line = "charge" }]

[formations.den]
label = "Den"
monsters = ["dread_wolf"]
"#;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), BATTLE_TOML.as_bytes().to_vec());
    files.insert("dialog.toml".into(), DIALOG_TOML.as_bytes().to_vec());
    Arc::new(ExpansionPack::from_files(&files).expect("synthetic pack parses"))
}

/// Two members (one weaker) against a catalog holding a Wolf (9) with two
/// swings and a Rat (10); retail's formation row `1` is one Wolf.
fn world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 2,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Field;
    for (s, hp) in [(0usize, 9000u16), (1, 7000)] {
        world.actors[s].active = true;
        world.actors[s].battle.hp = hp;
        world.actors[s].battle.max_hp = 9000;
        world.actors[s].battle.liveness = 1;
        world.battle.set_battle_attack(s as u8, 60);
    }
    let mut cat = MonsterCatalog::new();
    let mut wolf = MonsterDef::new(9, "Wolf", 35, 12);
    wolf.action_entries = vec![6, 7];
    wolf.action_costs = vec![10, 10];
    wolf.exp = 20;
    cat.insert(wolf);
    let mut rat = MonsterDef::new(10, "Rat", 20, 6);
    rat.exp = 30;
    cat.insert(rat);
    world.set_monster_catalog(cat);
    world
        .tables
        .formation_table
        .insert(FormationDef::new(1, vec![FormationSlot::new(9)]));
    world.rng_state = 0x0BAD_5EED;
    world
}

fn enter(w: &mut World) {
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            return;
        }
        w.tick();
    }
    panic!("the fight never started");
}

#[derive(Debug, Default, PartialEq)]
struct Watch {
    banners: Vec<String>,
    /// Monster ids seated after the opening, in order.
    arrivals: Vec<u16>,
    /// Each time the wolf's phase moved.
    phases: Vec<usize>,
    wolf_min_hp: u16,
    rng: u32,
}

fn fight(w: &mut World) -> Watch {
    enter(w);
    let mut watch = Watch {
        wolf_min_hp: u16::MAX,
        ..Default::default()
    };
    let mut alive = [true; 8];
    for _ in 0..60_000 {
        if let Some(b) = &w.battle.message_banner
            && watch.banners.last() != Some(&b.text)
        {
            watch.banners.push(b.text.clone());
        }
        if let Some((_, run)) = w.expansion.battle.boss_runs.get(&2)
            && let Some(p) = run.phase
            && watch.phases.last() != Some(&p)
        {
            watch.phases.push(p);
        }
        for (s, was) in alive.iter_mut().enumerate().skip(3) {
            let a = &w.actors[s];
            let now = a.active && a.battle.liveness != 0 && a.battle_monster_id.is_some();
            if now && !*was {
                watch.arrivals.push(a.battle_monster_id.unwrap());
            }
            *was = now;
        }
        if w.actors[2].battle_monster_id == Some(9) {
            watch.wolf_min_hp = watch.wolf_min_hp.min(w.actors[2].battle.hp);
        }
        w.tick();
        if w.mode != SceneMode::Battle {
            watch.rng = w.rng_state;
            return watch;
        }
    }
    panic!("the fight never ended: {watch:?}");
}

#[test]
fn a_scripted_boss_walks_its_phases_to_a_last_stand() {
    let mut w = world();
    w.activate_expansion(pack()).expect("pack activates");
    assert!(w.start_expansion_battle("den"));
    let watch = fight(&mut w);
    assert_eq!(watch.phases, [0, 1, 2], "{watch:?}");
    let has = |t: &str| watch.banners.iter().any(|b| b == t);
    assert!(has("The Dread Wolf howls!"), "{:?}", watch.banners);
    assert!(has("The Dread Wolf crouches low..."), "{:?}", watch.banners);
    assert!(has("It will not fall!"), "{:?}", watch.banners);
    // The howl's order: phase banner before the charge before the stand.
    let at = |t: &str| watch.banners.iter().position(|b| b == t).unwrap();
    assert!(at("The Dread Wolf howls!") < at("The Dread Wolf crouches low..."));
    // The summoned Rat took the free seat beside the wolf.
    assert_eq!(watch.arrivals.first(), Some(&10), "{watch:?}");
    // The last stand held the wolf at 1 HP before the end.
    assert_eq!(watch.wolf_min_hp, 0);
    let rewards = w.battle.last_rewards.clone().expect("the party wins");
    // The summoned Rat joined the spoils.
    assert_eq!(rewards.xp, 100 + 30);
    // The script state is gone with the fight.
    assert!(w.expansion.battle.boss_runs.is_empty());
    assert!(w.expansion.battle.fight_scripts.is_empty());
}

#[test]
fn a_scripted_fight_replays_bit_for_bit() {
    let mut a = world();
    a.activate_expansion(pack()).unwrap();
    assert!(a.start_expansion_battle("den"));
    let mut b = world();
    b.activate_expansion(pack()).unwrap();
    assert!(b.start_expansion_battle("den"));
    assert_eq!(fight(&mut a), fight(&mut b));
}

#[test]
fn phase_changes_scale_live_stats_and_queue_the_summon() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    assert!(w.start_expansion_battle("den"));
    enter(&mut w);
    let atk = w.battle.attack[2];
    assert!(atk > 0);
    // Opening phase: the script picks swing 0 (entry 6) at the weaker
    // member, drawing no target RNG.
    let action = w.pick_monster_action(2);
    assert!(
        matches!(action, MonsterAction::Physical { target: 1 }),
        "{action:?}"
    );
    assert_eq!(w.expansion.battle.forced_strike, Some(vec![6]));
    w.arm_monster_strike_budget(2);
    assert_eq!(w.battle.monster_strike_entries, [6]);
    assert!(w.expansion.battle.forced_strike.is_none());
    // Below half: the howl doubles attack and summons a Rat into the free
    // seat as the next round opens.
    w.actors[2].battle.hp = 290;
    let _ = w.pick_monster_action(2);
    assert_eq!(w.battle.attack[2], atk * 2);
    assert_eq!(w.expansion.battle.pending_summons, [10]);
    assert_eq!(
        w.battle.message_banner.as_ref().map(|b| b.text.as_str()),
        Some("The Dread Wolf howls!")
    );
    w.expansion_boss_round_start();
    assert_eq!(w.actors[3].battle_monster_id, Some(10));
    assert_eq!(w.actors[3].battle.hp, 20);
    assert!(w.take_expansion_reseated_monsters().contains(&3));
    // Below a fifth: the charge turn waits, the next lands two swings.
    w.actors[2].battle.hp = 100;
    assert!(matches!(w.pick_monster_action(2), MonsterAction::Wait));
    assert!(matches!(
        w.pick_monster_action(2),
        MonsterAction::Physical { .. }
    ));
    assert_eq!(w.expansion.battle.forced_strike, Some(vec![7, 7]));
}

#[test]
fn a_phase_flashes_and_holds_its_tint_for_the_fight() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    assert!(w.start_expansion_battle("den"));
    enter(&mut w);
    assert_eq!(w.mode, SceneMode::Battle);
    assert!(
        w.expansion_cutscene_screen().0.is_empty(),
        "nothing before the phase"
    );
    w.actors[2].battle.hp = 290;
    let _ = w.pick_monster_action(2);
    let (quads, _) = w.expansion_cutscene_screen();
    // The flash at full strength, added; the tint not yet ramped in.
    assert!(quads.contains(&(0xFF0000, 1)), "{quads:?}");
    for _ in 0..20 {
        w.expansion_boss_frame();
    }
    // The flash is spent; the tint holds, added.
    assert_eq!(w.expansion_cutscene_screen().0, vec![(0x000028, 1)]);
    w.expansion_boss_end();
    assert!(
        w.expansion_cutscene_screen().0.is_empty(),
        "the fight's end clears it"
    );
}

#[test]
fn immunity_and_last_stand() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    assert!(w.start_expansion_battle("den"));
    enter(&mut w);
    w.expansion_boss_round_start();
    use legaia_engine_battle_vm::status_effects::StatusKind;
    w.battle.status_effects.apply(2, StatusKind::Sleep);
    w.battle.status_effects.apply(2, StatusKind::Venom);
    w.expansion_boss_frame();
    assert!(!w.battle.status_effects.has(2, StatusKind::Sleep));
    assert!(w.battle.status_effects.has(2, StatusKind::Venom));
    // The first lethal blow holds at 1 HP; the second kills.
    let hp = w.actors[2].battle.hp;
    w.apply_battle_hp_delta(2, i32::from(hp) + 50);
    assert_eq!(w.actors[2].battle.hp, 1);
    assert_ne!(w.actors[2].battle.liveness, 0);
    w.apply_battle_hp_delta(2, 50);
    assert_eq!(w.actors[2].battle.hp, 0);
    assert_eq!(w.actors[2].battle.liveness, 0);
}

#[test]
fn a_scripted_spell_casts_through_the_retail_path() {
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert(
        "battle.toml".into(),
        br#"
[monsters.mage]
base = { kind = "monster", id = 9 }
mp = 500
[monsters.mage.script]
target = "lowest_hp"
[[monsters.mage.script.phases]]
moves = [{ spell = 0x20 }]
[formations.f]
monsters = ["mage"]
"#
        .to_vec(),
    );
    let p = Arc::new(ExpansionPack::from_files(&files).unwrap());
    let mut w = world();
    w.set_spell_catalog(legaia_engine_battle::spells::SpellCatalog::vanilla());
    w.activate_expansion(p).unwrap();
    assert!(w.start_expansion_battle("f"));
    enter(&mut w);
    match w.pick_monster_action(2) {
        MonsterAction::Cast { spell_id, targets } => {
            assert_eq!((spell_id, targets.as_slice()), (0x20, &[1u8][..]));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(w.actors[2].battle.params[0], 0x20);
}

fn retail_trace(w: &mut World) -> (u32, Vec<u16>, i32) {
    assert!(w.force_encounter(1));
    enter(w);
    let mut hp = Vec::new();
    for _ in 0..60_000 {
        hp.push(w.actors[2].battle.hp);
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    (w.rng_state, hp, w.party.money)
}

#[test]
fn a_retail_fight_with_a_scripted_pack_active_is_the_retail_fight() {
    let mut plain = world();
    let retail = retail_trace(&mut plain);
    assert!(plain.expansion.battle.boss_runs.is_empty());
    let mut active = world();
    active.activate_expansion(pack()).unwrap();
    // The retail row seats the retail Wolf: no script runs.
    assert_eq!(retail_trace(&mut active), retail);
    assert!(active.expansion.battle.boss_runs.is_empty());
}

#[test]
fn a_mid_round_summon_takes_its_seat_at_the_next_turn() {
    use legaia_engine_battle_loop::round_flow::RoundPhase;
    use legaia_engine_battle_vm::battle_action::ActionState;
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    assert!(w.start_expansion_battle("den"));
    enter(&mut w);
    // The wolf's own turn, below half: the howl queues the Rat.
    w.actors[2].battle.hp = 290;
    let _ = w.pick_monster_action(2);
    assert_eq!(w.expansion.battle.pending_summons, [10]);
    // The wolf's action ends mid-round: the next turn's dispatch seats the
    // Rat before it picks who acts.
    w.battle.round_flow.phase = RoundPhase::Execute;
    w.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    w.cycle_battle_turn();
    assert!(w.expansion.battle.pending_summons.is_empty());
    assert_eq!(w.actors[3].battle_monster_id, Some(10));
    assert!(w.take_expansion_reseated_monsters().contains(&3));
}

#[test]
fn an_ignore_mp_spell_casts_dry_and_spends_nothing() {
    let battle = BATTLE_TOML.replace(
        "moves = [{ attack = 0 }]",
        "moves = [{ spell = 0x51, ignore_mp = true }]",
    );
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), battle.into_bytes());
    files.insert("dialog.toml".into(), DIALOG_TOML.as_bytes().to_vec());
    let pack = Arc::new(ExpansionPack::from_files(&files).expect("pack parses"));
    let mut w = world();
    w.tables
        .spell_catalog
        .insert(legaia_engine_battle::spells::SpellDef {
            id: 0x51,
            name: "Bolt".into(),
            mp_cost: 5,
            target: legaia_engine_battle::spells::SpellTarget::OneEnemy,
            ..Default::default()
        });
    w.activate_expansion(pack).unwrap();
    assert!(w.start_expansion_battle("den"));
    enter(&mut w);
    w.actors[2].battle.mp = 0;
    let action = w.pick_monster_action(2);
    assert!(
        matches!(action, MonsterAction::Cast { spell_id: 0x51, .. }),
        "{action:?}"
    );
    // Topped up by the cost the band's debit takes back.
    assert_eq!(w.actors[2].battle.mp, 5);
}
