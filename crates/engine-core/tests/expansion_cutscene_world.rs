//! Expansion-pack cutscenes run end to end through `World::tick`: a
//! synthetic pack's cutscene locks the player, brings a hidden NPC on and
//! walks it, says a line on the faithful dialogue runner, branches on the
//! option the pad picked, raises pack flags and ends on a fight that the
//! pack's fight path starts; a seen cutscene skips with Start.
//!
//! Synthetic world and pack: no disc, no Sony bytes. The map's collision
//! image is the one the scene host would seat.

use std::sync::Arc;

use legaia_engine_core::world::World;
use legaia_engine_expansion::map::ActiveMap;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::expansion_map::{BaseInputs, build_field_map_image, floor_lut};
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::cutscene::seen_flag;
use legaia_expansion_format::{ExpansionPack, PackFiles};
use legaia_scene_formats::man_section::ActorPlacement;

const CROSS: u16 = PadButton::Cross as u16;
const START: u16 = PadButton::Start as u16;
const UP: u16 = PadButton::Up as u16;
const HIDE: i16 = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;

const MAP: &str = r#"
name = "Hall"
base = { scene = { kind = "scene", name = "town01" } }
[tiles]
origin = [16, 16]
rows = ["........", "........", "........", "........", "........", "........"]
[tiles.legend]
"." = {}

[[spawns]]
id = "start"
tile = [2, 4]

[[npcs]]
id = "guide"
model = { kind = "npc", scene = "town01", index = 3 }
tile = [6, 1]
present = { flags = ["guide_here"] }
"#;

const DIALOG: &str = r#"
[lines.offer]
text = "Will you fight?"
choices = [{ label = "Yes" }, { label = "No" }]

[lines.hello]
text = "Hello."

[lines.alpha_banner]
text = """
The Alpha,
Lord of the Hall"""
"#;

const BATTLE: &str = r#"
[monsters.wolf]
base = { kind = "monster", id = 9 }
name = "Wolf"

[formations.ambush]
monsters = ["wolf"]

[bosses.alpha]
formation = "ambush"
intro = "alpha_intro"
"#;

const CUTSCENES: &str = r#"
[cutscenes.meet]
map = "hall"
letterbox = true
steps = [
  { do = "fade", to = "black" },
  { do = "camera", focus = { tile = [6, 1] }, zoom = 1.5 },
  { do = "fade", to = "clear", frames = 10 },
  { do = "show", npc = "guide" },
  { do = "move", actor = "guide", to = [3, 3] },
  { do = "face", actor = "player", toward = "guide" },
  { do = "say", line = "offer", on_choice = ["fight", ""] },
  { do = "set_flag", flag = "declined" },
  { do = "end" },
  { do = "label", name = "fight" },
  { do = "set_flag", flag = "agreed" },
  { do = "battle", formation = "ambush" },
]

[cutscenes.alpha_intro]
map = "hall"
steps = [
  { do = "camera", focus = { tile = [6, 1] }, zoom = 0.8, frames = 20 },
  { do = "banner", line = "alpha_banner" },
  { do = "wait", frames = 60 },
]

[cutscenes.walk]
map = "hall"
steps = [
  { do = "move", actor = "player", to = [5, 4] },
  { do = "set_flag", flag = "walked" },
  { do = "wait", frames = 120 },
  { do = "set_flag", flag = "waited" },
]
"#;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"cs\"\nname = \"Cutscenes\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert("maps/hall.toml".into(), MAP.as_bytes().to_vec());
    files.insert("dialog.toml".into(), DIALOG.as_bytes().to_vec());
    files.insert("battle.toml".into(), BATTLE.as_bytes().to_vec());
    files.insert("cutscenes.toml".into(), CUTSCENES.as_bytes().to_vec());
    let pack = ExpansionPack::from_files(&files).expect("the pack parses");
    let issues = legaia_expansion_format::validate::validate(&pack);
    assert!(
        !legaia_expansion_format::validate::has_errors(&issues),
        "{issues:?}"
    );
    Arc::new(pack)
}

/// A field world standing on the hall, its NPC seated as the scene host
/// seats one (and hidden: its `present` gate is shut).
fn hall_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    // Pack lines run on the faithful dialogue runner, as both hosts run it.
    w.toggles.use_vm_dialogue = true;
    w.spawn_actor(0).active = true;
    w.install_field_player(0);
    let pack = pack();
    w.activate_expansion(pack.clone()).expect("activates");
    let def = &pack.maps["hall"];
    let grid = def.tile_grid().unwrap();
    let image = build_field_map_image(def, &grid, BaseInputs::default()).unwrap();
    w.terrain.collision_grid = image[0x4000..0x8000].to_vec();
    w.terrain.floor_height_lut = floor_lut(def, None).map(i16::wrapping_neg);
    let (px, pz) = def.tile_centre(2, 4);
    w.actors[0].move_state.world_x = px as i16;
    w.actors[0].move_state.world_z = pz as i16;
    w.expansion.map = Some(ActiveMap::new(
        "hall",
        Arc::new(def.clone()),
        Arc::new(grid),
        (2, 4),
        |_| false,
    ));
    let (gx, gz) = def.tile_centre(6, 1);
    let guide = ActorPlacement {
        index: 1,
        record_offset: 0,
        local_count: 0,
        model_index: 3,
        special_model: false,
        anim_id: 1,
        tile_x: (gx >> 7) as u8,
        tile_z: (gz >> 7) as u8,
        world_x: gx as i16,
        world_z: gz as i16,
        script_pc0: 0,
    };
    w.install_expansion_npcs(&[guide], &[0], &[false]);
    w.expansion_hide_absent_npcs();
    w
}

fn frame(w: &mut World, pad: u16) {
    w.set_pad(pad);
    w.tick();
}

fn player(w: &World) -> (i16, i16) {
    let m = &w.actors[0].move_state;
    (m.world_x, m.world_z)
}

fn flag(w: &World, f: &str) -> bool {
    w.expansion.quests.progress.flags.is_set(f)
}

#[test]
fn a_cutscene_locks_the_player_moves_the_npc_branches_and_starts_its_fight() {
    let mut w = hall_world();
    let def = w.expansion.map.as_ref().unwrap().def.clone();
    assert_eq!(w.npcs.positions[&1], (HIDE, HIDE), "absent until shown");

    assert!(w.expansion_play_cutscene("meet"));
    frame(&mut w, 0);
    assert!(w.expansion_cutscene_active());
    assert!(w.script_context_engages_player(), "the player is held");
    // Black, with the shot on the guide's tile and the bars sliding in.
    let (quads, _) = w.expansion_cutscene_screen();
    assert_eq!(quads, vec![(0xFF_FFFF, 2)], "a full subtractive fade");
    let (pose, _) = w.expansion_cutscene_camera().expect("the camera moved");
    let (gx, gz) = def.tile_centre(6, 1);
    assert_eq!(pose.focus, [gx as f32, gz as f32]);
    assert_eq!(pose.zoom, 1.5);

    // Held Up walks nobody while the cutscene runs.
    let before = player(&w);
    let mut opened = None;
    for f in 0..600 {
        frame(&mut w, UP);
        if w.dialog.inline.is_some() {
            opened = Some(f);
            break;
        }
    }
    assert!(opened.is_some(), "the line opened");
    assert_eq!(player(&w), before, "the pad did not walk the player");
    // The guide came on and walked to its stop, and the player turned to it.
    let (tx, tz) = def.tile_centre(3, 3);
    assert_eq!(w.npcs.positions[&1], (tx as i16, tz as i16));
    let h = w.actors[0].move_state.render_26 & 0x0FFF;
    assert!(
        (1024..=2048).contains(&h),
        "the player faces up and right, toward the guide: {h}"
    );
    assert!(w.expansion_cutscene_screen().1 > 0, "letterbox up");

    // Confirm through the line and its menu: option 0 is the fight branch.
    let mut n = 0;
    while w.expansion_cutscene_active() && n < 2000 {
        frame(&mut w, if n % 2 == 0 { CROSS } else { 0 });
        n += 1;
    }
    assert!(!w.expansion_cutscene_active(), "the cutscene ended");
    assert!(flag(&w, "agreed") && !flag(&w, "declined"));
    assert!(flag(&w, &seen_flag("meet")), "seen once it ended");
    // The fight it ended on starts through the pack's fight path.
    for _ in 0..10 {
        if w.expansion.quests.fights.is_empty() {
            break;
        }
        frame(&mut w, 0);
    }
    assert!(w.expansion.quests.fights.is_empty(), "the fight was taken");
    assert!(
        w.encounters.session.as_ref().is_some_and(|s| !matches!(
            s.phase(),
            legaia_engine_battle::encounter::EncounterPhase::Idle
        )),
        "the encounter was triggered"
    );
}

/// A boss with an `intro` plays it as its fight is called - the shot, the
/// title banner - and the fight starts as the cutscene ends.
#[test]
fn a_boss_intro_plays_its_banner_and_then_starts_the_fight() {
    let mut w = hall_world();
    assert!(w.start_expansion_boss("alpha"), "the call is taken");
    assert!(w.expansion_cutscene_active(), "the intro plays first");
    let triggered = |w: &World| {
        w.encounters.session.as_ref().is_some_and(|s| {
            !matches!(
                s.phase(),
                legaia_engine_battle::encounter::EncounterPhase::Idle
            )
        })
    };
    assert!(!triggered(&w), "no fight yet");
    let mut banner = None;
    let mut n = 0;
    while w.expansion_cutscene_active() && n < 600 {
        frame(&mut w, 0);
        if banner.is_none() {
            banner = w.cutscene.text_balloon.as_ref().map(|b| b.text.clone());
        }
        n += 1;
    }
    assert!(!w.expansion_cutscene_active(), "the intro ended");
    assert_eq!(
        banner.as_deref(),
        Some(&b"The Alpha, Lord of the Hall"[..]),
        "the title banner rose with the line's rows joined"
    );
    for _ in 0..10 {
        if triggered(&w) {
            break;
        }
        frame(&mut w, 0);
    }
    assert!(triggered(&w), "the fight followed the intro");
    assert_eq!(
        w.expansion.battle.active_boss.as_deref(),
        Some("alpha"),
        "as the boss"
    );
}

#[test]
fn the_other_option_takes_the_other_branch_and_no_fight() {
    let mut w = hall_world();
    w.expansion_play_cutscene("meet");
    frame(&mut w, 0);
    let mut n = 0;
    let mut moved = false;
    while w.expansion_cutscene_active() && n < 3000 {
        let panel = w.dialog.inline.as_ref().and_then(|id| id.panel.as_ref());
        let menu = panel.is_some_and(|p| p.menu_active());
        let takes = panel.is_some_and(|p| p.menu_active() && p.picker_takes_input());
        let pad = if menu && !moved && !takes {
            // Hold off while the picker slides in.
            0
        } else if takes && !moved {
            moved = true;
            PadButton::Down as u16
        } else if n % 2 == 0 {
            CROSS
        } else {
            0
        };
        frame(&mut w, pad);
        n += 1;
    }
    assert!(moved, "the menu took input");
    assert!(!w.expansion_cutscene_active());
    assert!(
        flag(&w, "declined") && !flag(&w, "agreed"),
        "{:?}",
        w.expansion.quests.progress.flags
    );
    assert!(w.expansion.quests.fights.is_empty());
}

#[test]
fn a_seen_cutscene_skips_with_start_and_keeps_its_state_changes() {
    let mut w = hall_world();
    let def = w.expansion.map.as_ref().unwrap().def.clone();
    let start = player(&w);
    // Unseen: Start does nothing; the walk plays at the walk pace.
    w.expansion_play_cutscene("walk");
    frame(&mut w, START);
    frame(&mut w, 0);
    assert!(
        w.expansion_cutscene_active(),
        "not skippable before it is seen"
    );
    let mut n = 0;
    while w.expansion_cutscene_active() && n < 1000 {
        frame(&mut w, 0);
        n += 1;
    }
    let (tx, tz) = def.tile_centre(5, 4);
    assert_eq!(player(&w), (tx as i16, tz as i16));
    assert!(n > 120, "it played out: {n}");
    assert!(flag(&w, "waited") && flag(&w, &seen_flag("walk")));
    // Seen: Start lands the walk and runs the rest at once; the run holds
    // until Start is let go, so the press opens no menu.
    w.expansion.quests.progress.flags.clear("walked");
    w.expansion.quests.progress.flags.clear("waited");
    w.actors[0].move_state.world_x = start.0;
    w.actors[0].move_state.world_z = start.1;
    w.expansion_play_cutscene("walk");
    frame(&mut w, 0);
    frame(&mut w, 0);
    assert_ne!(player(&w), (tx as i16, tz as i16), "mid-walk");
    frame(&mut w, START);
    assert_eq!(player(&w), (tx as i16, tz as i16), "landed");
    assert!(flag(&w, "walked") && flag(&w, "waited"));
    assert!(
        w.script_context_engages_player(),
        "held while Start is down"
    );
    frame(&mut w, START);
    assert!(w.expansion_cutscene_active());
    frame(&mut w, 0);
    assert!(!w.expansion_cutscene_active(), "released");
    assert!(!w.script_context_engages_player());
}

#[test]
fn saves_are_refused_mid_cutscene() {
    let mut w = hall_world();
    assert!(w.host_save_allowed());
    w.expansion_play_cutscene("walk");
    frame(&mut w, 0);
    assert!(!w.host_save_allowed(), "no save mid-cutscene");
    let mut n = 0;
    while w.expansion_cutscene_active() && n < 1000 {
        frame(&mut w, 0);
        n += 1;
    }
    assert!(w.host_save_allowed());
}

#[test]
fn nothing_plays_without_a_pack_or_on_the_wrong_map() {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    assert!(!w.expansion_play_cutscene("meet"));
    assert!(!w.start_expansion_cutscene("meet"));
    frame(&mut w, START);
    assert!(!w.expansion_cutscene_active());
    assert_eq!(w.expansion_cutscene_screen(), (Vec::new(), 0));
    assert!(w.expansion_cutscene_camera().is_none());

    let mut w = hall_world();
    w.expansion.map = None;
    assert!(!w.start_expansion_cutscene("meet"), "bound to the hall");
}
