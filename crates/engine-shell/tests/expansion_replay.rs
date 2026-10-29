//! Expansion replays: `j-replay-v1` files whose `[[switch]]` rows carry the
//! session changes the pad stream cannot - the active expansion pack (id and
//! content hash), the action-movement switch, and the mode seat and battle
//! and map entries - each run twice and required to give bit-identical state
//! traces.
//!
//! Every replay goes through its TOML text before it runs, so what is
//! measured is the file form, not the in-memory builder. Each test also has a
//! control that must move the trace (another seed, or the switch removed), so
//! an identical pair cannot come from a trace that ignores the input.
//!
//! A no-pack family pins the identity: a retail fight traces the same on a
//! world that never saw a pack, one that activated and dropped one, and one
//! fed the retail-valued switches, and a world that never saw a pack writes
//! no expansion block into its save.
//!
//! The battle, poker and golf replays run on a synthetic `World` with no disc.
//! The pack-map walk needs the base scene's geometry and is disc-gated: it
//! skips and passes without `LEGAIA_DISC_BIN` and an extracted tree.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_core::world::{ACTION_JUMP_BUTTON, World};
use legaia_engine_expansion::modes::ModeStage;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::ModeView;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_shell::replay::{
    PackShelf, ReplayFile, ReplayMeta, ReplaySeat, SwitchChange, apply_switches_at, pack_switch,
};
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const DOWN: u16 = PadButton::Down as u16;

/// The repo's committed replay fixtures.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/replays/expansion")
        .join(name)
}

/// The replay as its file form reads back.
fn through_text(replay: &ReplayFile) -> ReplayFile {
    let text = replay.to_toml_string().expect("serialise");
    let back = ReplayFile::from_toml_str(&text).expect("parse back");
    assert_eq!(&back, replay, "the file form carries every row");
    back
}

/// One trace line: everything a determinism break in the expansion layer
/// would move.
fn sample(w: &World) -> String {
    let player = w.player_actor_slot.map(|s| {
        let ms = &w.actors[s as usize].move_state;
        (ms.world_x, ms.world_y, ms.world_z, ms.y_rot)
    });
    let monsters: Vec<(u16, u16)> = w.actors[1..]
        .iter()
        .filter(|a| a.active && a.battle_monster_id.is_some())
        .map(|a| (a.battle_monster_id.unwrap_or(0), a.battle.hp))
        .collect();
    let seat = w.expansion.mode_seat.as_deref().map(|s| match &s.stage {
        ModeStage::Picker { cursor, .. } => format!("picker@{cursor}"),
        ModeStage::Playing { mode, quit_prompt } => {
            format!("play:{:016x}:{quit_prompt}", mode.state_hash())
        }
        ModeStage::Results { rows, .. } => format!("results:{}", rows.len()),
    });
    let view = match w.expansion.mode_seat.as_deref().and_then(|s| s.view()) {
        Some(ModeView::Poker(v)) => format!("hand={}", v.hand),
        Some(ModeView::Golf(v)) => format!("hole={} card={:?}", v.hole, v.scorecard),
        Some(_) => "other".into(),
        None => String::new(),
    };
    let hp: u32 = w.actors.iter().map(|a| u32::from(a.battle.hp)).sum();
    format!(
        "{} {:?} rng={:08x} gold={} coins={} hp={hp} pack={:?} am={} player={player:?} \
         mon={monsters:?} seat={seat:?} view={view} map={:?} cards={} victory={}",
        w.frame,
        w.mode,
        w.rng_state,
        w.party.money,
        w.minigames.casino_coins,
        w.expansion.pack.as_ref().map(|p| p.manifest.id.as_str()),
        w.toggles.action_movement,
        w.expansion.map.as_ref().map(|m| m.map_id.as_str()),
        w.expansion.pending_card_rewards.len(),
        w.battle.victory.is_some(),
    )
}

/// Drive `seat` through `replay`: per frame, its switches, then its pad,
/// then one `tick`; one [`sample`] line after each.
fn run<S: ReplaySeat>(
    seat: &mut S,
    replay: &ReplayFile,
    shelf: &PackShelf,
    mut tick: impl FnMut(&mut S),
) -> Vec<String> {
    let pads = replay.expand_pad_stream();
    let mut out = Vec::with_capacity(pads.len());
    for frame in 0..replay.meta.frames {
        apply_switches_at(seat, replay, frame, shelf).expect("switch applies");
        seat.replay_world().set_pad(pads[frame as usize]);
        tick(seat);
        out.push(sample(seat.replay_world()));
    }
    out
}

fn tick_world(w: &mut World) {
    w.tick();
}

// ---------------------------------------------------------------------------
// A pack battle with waves
// ---------------------------------------------------------------------------

/// A three-wave formation over a synthetic catalog's Wolf (9) and Rat (10).
fn wave_pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"wavereplay\"\nname = \"Wave Replay\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert(
        "battle.toml".into(),
        br#"
[monsters.dire_wolf]
base = { kind = "monster", id = 9 }
name = "Dire Wolf"
exp = 1000
gold = 400

[formations.gauntlet]
monsters = ["dire_wolf"]

[[formations.gauntlet.waves]]
monsters = ["dire_wolf", { kind = "monster", id = 10 }]

[[formations.gauntlet.waves]]
monsters = [{ kind = "monster", id = 10 }]
arrival = "refill"
count = 2
max_standing = 1
"#
        .to_vec(),
    );
    Arc::new(ExpansionPack::from_files(&files).expect("the wave pack parses"))
}

/// One capable party member in the field, with the catalog the pack's
/// monsters build on.
fn battle_world(rng_seed: u32) -> World {
    let mut w = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    w.mode = SceneMode::Field;
    w.actors[0].active = true;
    w.actors[0].battle.hp = 900;
    w.actors[0].battle.max_hp = 900;
    w.actors[0].battle.liveness = 1;
    w.battle.set_battle_attack(0, 80);
    let mut cat = MonsterCatalog::new();
    let mut wolf = MonsterDef::new(9, "Wolf", 35, 12);
    wolf.exp = 20;
    wolf.gold = 40;
    cat.insert(wolf);
    let mut rat = MonsterDef::new(10, "Rat", 20, 6);
    rat.exp = 30;
    rat.gold = 50;
    cat.insert(rat);
    w.set_monster_catalog(cat);
    w.tables
        .formation_table
        .insert(FormationDef::new(1, vec![FormationSlot::new(9)]));
    w.rng_state = rng_seed;
    w
}

fn battle_replay(rng_seed: u32) -> ReplayFile {
    let mut r = ReplayFile::new(ReplayMeta::new(6000).with_rng_seed(rng_seed));
    r.push_switch(0, pack_switch(&wave_pack()).unwrap());
    r.push_switch(
        2,
        SwitchChange::Battle {
            formation: "gauntlet".into(),
        },
    );
    through_text(&r)
}

fn battle_trace(replay: &ReplayFile) -> Vec<String> {
    let mut shelf = PackShelf::new();
    shelf.add(wave_pack()).unwrap();
    let mut w = battle_world(replay.meta.rng_seed);
    run(&mut w, replay, &shelf, tick_world)
}

#[test]
fn a_pack_battle_with_waves_replays_bit_identical() {
    let replay = battle_replay(0x0BAD_5EED);
    let a = battle_trace(&replay);
    let b = battle_trace(&replay);
    assert_eq!(a, b, "two runs of one battle replay diverged");
    // The fight happened and ended: a battle, every wave's monsters seated
    // (two distinct ids), one victory, and the field back.
    assert!(a.iter().any(|l| l.contains(" Battle ")), "the fight starts");
    assert!(
        a.iter().any(|l| l.contains("victory=true")),
        "it ends in victory"
    );
    assert!(
        a.iter().any(|l| l.contains("mon=[(10,")),
        "a later wave seats"
    );
    assert!(a.last().unwrap().contains(" Field "), "the field returns");
    // Control: another seed deals the fight differently.
    assert_ne!(a, battle_trace(&battle_replay(0x1234_5678)));
}

#[test]
fn a_replay_refuses_a_pack_whose_content_moved() {
    let mut replay = battle_replay(1);
    replay.switches[0].change = SwitchChange::Pack {
        id: "wavereplay".into(),
        hash: "0".repeat(64),
    };
    let mut shelf = PackShelf::new();
    shelf.add(wave_pack()).unwrap();
    let mut w = battle_world(1);
    let err = apply_switches_at(&mut w, &replay, 0, &shelf).unwrap_err();
    assert!(format!("{err:#}").contains("differs"), "{err:#}");
    assert!(w.expansion.pack.is_none());
}

// ---------------------------------------------------------------------------
// No pack: the expansion layer is inert
// ---------------------------------------------------------------------------

/// A retail fight (formation row `1`) over `frames` ticks, after `prepare`.
fn retail_fight_trace(prepare: impl FnOnce(&mut World), replay: &ReplayFile) -> Vec<String> {
    let mut shelf = PackShelf::new();
    shelf.add(wave_pack()).unwrap();
    let mut w = battle_world(replay.meta.rng_seed);
    prepare(&mut w);
    assert!(w.force_encounter(1), "the retail formation starts");
    run(&mut w, replay, &shelf, tick_world)
}

#[test]
fn with_no_pack_the_expansion_layer_is_inert() {
    let mut replay = ReplayFile::new(ReplayMeta::new(4000).with_rng_seed(0x7777));
    for f in (10..3990).step_by(20) {
        replay.push_event(f, PadButton::Cross as u16);
        replay.push_event(f + 2, 0);
    }
    let retail = retail_fight_trace(|_| {}, &replay);
    assert!(retail.iter().any(|l| l.contains("victory=true")));
    // A pack activated and dropped again leaves nothing behind.
    let cycled = retail_fight_trace(
        |w| {
            w.activate_expansion(wave_pack()).expect("activates");
            w.deactivate_expansion();
        },
        &replay,
    );
    assert_eq!(retail, cycled, "a deactivated pack changed a retail fight");
    // The value-setting switches at their retail values are no-ops.
    let mut idle = replay.clone();
    idle.push_switch(0, SwitchChange::NoPack);
    idle.push_switch(0, SwitchChange::ActionMovement { on: false });
    assert_eq!(retail, retail_fight_trace(|_| {}, &through_text(&idle)));
    // A world that never saw a pack writes no expansion block into a save.
    use legaia_engine_save_state::WorldSaveState;
    let mut w = battle_world(1);
    w.tick();
    assert_eq!(w.save_full().ext_v2.expansion, None);
}

// ---------------------------------------------------------------------------
// Poker and golf through the World mode seat
// ---------------------------------------------------------------------------

fn field_world(rng_seed: u32) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.minigames.casino_coins = 1000;
    w.party.money = 500;
    w.rng_state = rng_seed;
    w
}

fn mode_trace(replay: &ReplayFile) -> Vec<String> {
    let mut w = field_world(replay.meta.rng_seed);
    run(&mut w, replay, &PackShelf::new(), tick_world)
}

fn load(name: &str) -> ReplayFile {
    ReplayFile::from_path(&fixture(name)).expect("the fixture parses")
}

#[test]
fn a_poker_hand_replays_bit_identical() {
    let replay = load("builtin_poker_hand.replay.toml");
    assert!(
        replay
            .switches_at(0)
            .any(|s| matches!(s, SwitchChange::Mode { id } if id == "builtin-poker")),
        "the fixture seats the built-in table"
    );
    let a = mode_trace(&replay);
    assert_eq!(
        a,
        mode_trace(&replay),
        "two runs of one poker replay diverged"
    );
    assert!(
        a.iter().any(|l| l.contains("hand=2")),
        "a hand ran to its end and the next was dealt"
    );
    let mut other = replay.clone();
    other.meta.rng_seed ^= 0x5555;
    assert_ne!(a, mode_trace(&other), "another seed deals other cards");
}

#[test]
fn a_golf_hole_through_the_picker_replays_bit_identical() {
    let replay = load("builtin_golf_hole.replay.toml");
    assert!(
        replay
            .switches_at(0)
            .any(|s| matches!(s, SwitchChange::ModePicker)),
        "the fixture opens the picker"
    );
    let a = mode_trace(&replay);
    assert_eq!(
        a,
        mode_trace(&replay),
        "two runs of one golf replay diverged"
    );
    assert!(
        a.iter().any(|l| l.contains("view=hole=1 ")),
        "the first hole is played out and the second teed up"
    );
    // Control: without the picker switch the pads walk an empty field.
    let mut bare = replay.clone();
    bare.switches.clear();
    let b = mode_trace(&bare);
    assert!(b.iter().all(|l| !l.contains("seat=Some")));
    assert_ne!(a, b);
}

// ---------------------------------------------------------------------------
// A pack map walk with action-movement jumps (disc-gated)
// ---------------------------------------------------------------------------

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        return Some(PathBuf::from(d));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let d = root.join("extracted");
    (d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists()).then_some(d)
}

fn rebirth() -> Arc<ExpansionPack> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    Arc::new(ExpansionPack::from_dir(&dir).expect("the Rebirth pack parses"))
}

/// Walk into the arena (away from the stair) from the entrance spawn with jumps every half second,
/// action movement switched on at frame 1.
fn map_replay(with_action: bool) -> ReplayFile {
    let mut r = ReplayFile::new(ReplayMeta::new(480));
    r.push_switch(0, pack_switch(&rebirth()).unwrap());
    r.push_switch(
        0,
        SwitchChange::Map {
            map: "rebirth_tower_1f".into(),
            spawn: "entrance".into(),
        },
    );
    if with_action {
        r.push_switch(1, SwitchChange::ActionMovement { on: true });
    }
    for f in (20..460).step_by(30) {
        r.push_event(f, DOWN | ACTION_JUMP_BUTTON.mask());
        r.push_event(f + 8, DOWN);
    }
    r.push_event(460, 0);
    through_text(&r)
}

fn map_trace(replay: &ReplayFile, extracted: &Path) -> Vec<String> {
    let mut shelf = PackShelf::new();
    shelf.add(rebirth()).unwrap();
    let mut host = SceneHost::open_extracted(extracted).expect("open the scene host");
    host.enter_field_scene("town01", 0).expect("enter town01");
    run(&mut host, replay, &shelf, |h| {
        h.tick().expect("tick");
    })
}

#[test]
fn a_pack_map_walk_with_action_jumps_replays_bit_identical() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let replay = map_replay(true);
    let a = map_trace(&replay, &extracted);
    assert_eq!(a, map_trace(&replay, &extracted), "two runs diverged");
    assert!(
        a.iter()
            .all(|l| l.contains("map=Some(\"rebirth_tower_1f\")")),
        "the walk stays on the pack map"
    );
    // The player left the floor: some frame stands higher (smaller Y) than
    // the lowest standing height seen.
    let ys: Vec<i16> = a
        .iter()
        .filter_map(|l| {
            let p = l.split("player=Some((").nth(1)?;
            p.split(',').nth(1)?.trim().parse().ok()
        })
        .collect();
    let (lo, hi) = (ys.iter().min().copied(), ys.iter().max().copied());
    assert!(
        matches!((lo, hi), (Some(lo), Some(hi)) if hi - lo > 100),
        "the jumps rose off the floor: y in {lo:?}..{hi:?}"
    );
    // Control: the same pads with the switch off walk without jumping.
    assert_ne!(a, map_trace(&map_replay(false), &extracted));
}
