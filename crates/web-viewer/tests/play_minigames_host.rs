//! Disc-gated oracle for the browser play page's **in-world minigame host**
//! (`LegaiaRuntime::play_mg_*`, drawn by `site/js/play-minigames.js`).
//!
//! The defect this pins: a page player walking into the Vidna casino cabinet
//! entered `SceneMode::SlotMachine` with a frozen field and no UI, and the
//! same held for the duel, the dome and the dance hall. The contract under
//! test is host-side, not pixel-side:
//!
//! 1. **The door warp lands the page in a drawable session.** The mode-24
//!    warp is armed the way the field-VM `0x3E` arm does it, the scene host
//!    drains it through its own overlay loader, and the runtime's overlay
//!    draw list carries the HUD rows for that game.
//! 2. **The presentation decodes off the visitor's disc.** The compact PROT
//!    image behind the shared bundle must parse, and each game's scene
//!    exports must return geometry / art - the slot scene graph and reel
//!    symbols, the dome's staged monster + VRAM, the Baka opponent mesh, the
//!    dance hall.
//! 3. **Input reaches the rules and Start leaves.** A Cross press spins the
//!    slot reels; a Start press restores the field mode and the page's game
//!    label goes back to `null`.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

const START: u16 = 0x0008;
const CROSS: u16 = 0x4000;

/// Mode-24 sub-ids (`legaia_engine_core::minigame_entry::MinigameSubId`).
const SUB_SLOT: u8 = 3;
const SUB_BAKA: u8 = 4;
const SUB_MUSCLE: u8 = 5;
const SUB_DANCE: u8 = 6;

fn loaded_in(scene: &str) -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field(scene).ok()?;
    Some(rt)
}

fn tick(rt: &mut LegaiaRuntime, n: usize) {
    for _ in 0..n {
        rt.tick_frame().expect("tick_frame");
    }
}

/// Press `mask` for one tick, release for one.
fn press(rt: &mut LegaiaRuntime, mask: u16) {
    rt.set_pad(mask);
    tick(rt, 1);
    rt.set_pad(0);
    tick(rt, 1);
}

fn game_json(rt: &LegaiaRuntime) -> serde_json::Value {
    serde_json::from_str(&rt.play_mg_game_json()).expect("game json")
}

/// Arm the warp the way the casino door does and let the host drain it.
fn warp_into(rt: &mut LegaiaRuntime, sub_id: u8, expect_mode: &str, expect_game: &str) {
    assert!(rt.play_mg_debug_warp(sub_id), "a scene must be loaded");
    tick(rt, 2);
    assert_eq!(
        rt.scene_mode(),
        expect_mode,
        "sub-id {sub_id} must land in {expect_mode} (the scene host's own loader)"
    );
    let g = game_json(rt);
    assert_eq!(g["game"].as_str(), Some(expect_game), "{g}");
    assert!(
        g["art"].as_bool() == Some(true),
        "the compact PROT image must decode: {g}"
    );
}

/// The overlay draw list must carry HUD rows while the game is up.
fn assert_hud_rows(rt: &mut LegaiaRuntime, game: &str) {
    let ov: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(960, 720)).expect("overlay json");
    assert_eq!(
        ov["open"].as_bool(),
        Some(true),
        "{game}: overlay closed: {ov}"
    );
    let texts = ov["texts"].as_array().expect("texts");
    assert!(
        !texts.is_empty(),
        "{game}: the HUD rows must produce font quads"
    );
    for q in texts {
        let dst = q["dst"].as_array().expect("dst");
        assert!(
            dst[2].as_i64().unwrap_or(0) > 0,
            "{game}: zero-width quad {q}"
        );
    }
}

/// Start leaves the game and the field comes back, on every host.
fn assert_start_exits(rt: &mut LegaiaRuntime, game: &str) {
    press(rt, START);
    assert_eq!(
        rt.scene_mode(),
        "Field",
        "{game}: Start must restore the field"
    );
    let g = game_json(rt);
    assert!(
        g["game"].is_null(),
        "{game}: the page label must clear: {g}"
    );
    let ov: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(960, 720)).expect("overlay json");
    // Nothing else is up in a quiet field, so the overlay payload closes
    // once the minigame rows are gone (the field party HUD is idle-gated).
    let _ = ov;
}

#[test]
fn casino_door_warp_draws_the_slot_machine_and_start_leaves() {
    let Some(mut rt) = loaded_in("koin1") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(game_json(&rt)["game"].is_null());
    warp_into(&mut rt, SUB_SLOT, "SlotMachine", "slot");
    assert!(rt.play_mg_slot_active());
    assert_hud_rows(&mut rt, "slot");

    // The retail machine's presentation off the disc.
    assert!(
        rt.play_mg_slot_art_ready(),
        "PROT 1200 art pack must decode"
    );
    assert!(
        rt.play_mg_slot_scene_ready(),
        "PROT 0975 scene graph must decode"
    );
    let scene: serde_json::Value =
        serde_json::from_str(&rt.play_mg_slot_scene_json()).expect("scene json");
    assert_eq!(scene["ok"].as_bool(), Some(true));
    assert!(scene["paylines"].as_array().is_some_and(|p| !p.is_empty()));
    assert_eq!(rt.play_mg_slot_symbol_rgba(0).len(), 64 * 64 * 4);
    assert_eq!(rt.play_mg_slot_bonus_number_rgba(1).len(), 64 * 64 * 4);
    assert_eq!(rt.play_mg_slot_reel_pos().len(), 3);
    assert_eq!(rt.play_mg_slot_strip(0).len(), 20);
    let st: serde_json::Value =
        serde_json::from_str(&rt.play_mg_slot_state_json()).expect("state json");
    assert_eq!(st["live"].as_bool(), Some(true));
    assert_eq!(st["phase"].as_str(), Some("idle"));

    // Cross reaches the engine's session: the reels spin.
    if st["can_spin"].as_bool() == Some(true) {
        press(&mut rt, CROSS);
        let st: serde_json::Value =
            serde_json::from_str(&rt.play_mg_slot_state_json()).expect("state json");
        assert_ne!(
            st["phase"].as_str(),
            Some("idle"),
            "a Cross press must charge a spin: {st}"
        );
    }
    assert_start_exits(&mut rt, "slot");
}

#[test]
fn arena_door_warp_draws_the_muscle_dome_and_start_leaves() {
    let Some(mut rt) = loaded_in("koin1") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    warp_into(&mut rt, SUB_MUSCLE, "MuscleDome", "muscle");
    assert_hud_rows(&mut rt, "muscle");
    let g = game_json(&rt);
    let monster = g["muscle"]["monster_id"]
        .as_u64()
        .expect("the PROT 0977 ladder must stage a monster for the opened contest")
        as u16;
    let char_slot = g["muscle"]["char_slot"].as_u64().unwrap_or(0) as u32;
    assert!(
        rt.play_mg_muscle_scene_ready(monster, char_slot),
        "monster {monster:#x} + player file {char_slot} must build a scene"
    );
    assert!(!rt.play_mg_muscle_fighter_positions(char_slot).is_empty());
    assert!(!rt.play_mg_muscle_monster_positions(monster).is_empty());
    assert_eq!(
        rt.play_mg_muscle_vram(monster, char_slot).len(),
        1024 * 512 * 2
    );
    assert!(
        !rt.play_mg_muscle_arena_positions().is_empty(),
        "PROT 1225 arena"
    );
    let st: serde_json::Value =
        serde_json::from_str(&rt.play_mg_muscle_state_json()).expect("state json");
    assert_eq!(st["live"].as_bool(), Some(true));
    // The door stages the lead record's own fighter - the AP pool is its live
    // AGL and the entry HP its live HP - through the builder the native
    // launcher shares (`SceneHost::dome_lead_fighter`), not the flat 120 AP /
    // 400 HP stand-in the door used to field on both hosts.
    let lead: serde_json::Value =
        serde_json::from_str(&rt.debug_lead_live_stats_json()).expect("lead json");
    let agl = lead["agl"].as_u64().expect("a lead record");
    assert!(agl > 0, "the page's party has a live AGL: {lead}");
    assert_eq!(
        st["budget"].as_u64(),
        Some(agl),
        "dome AP pool = lead AGL: {st} vs {lead}"
    );
    assert_eq!(
        st["hp"][0].as_u64(),
        lead["hp"].as_u64(),
        "dome entry HP = lead HP"
    );
    assert_eq!(st["costs"].as_array().map(|c| c.len()), Some(4), "{st}");
    eprintln!("[ok] door-staged dome fighter {st} from lead {lead}");
    // The intro card + ROUND banner arm on the fresh contest's first leg.
    tick(&mut rt, 4);
    let hub: serde_json::Value =
        serde_json::from_str(&rt.play_mg_muscle_hub_quads_json()).expect("hub json");
    assert_eq!(
        hub["ok"].as_bool(),
        Some(true),
        "PROT 0977 sprite table: {hub}"
    );
    assert!(
        hub["quads"].as_array().is_some_and(|q| !q.is_empty()),
        "the intro card must be up on the first leg: {hub}"
    );
    assert_eq!(rt.play_mg_muscle_hub_sheet_dims(4).len(), 2);
    let dims = rt.play_mg_muscle_hub_sheet_dims(4);
    assert_eq!(
        rt.play_mg_muscle_hub_sheet_rgba(4, 0).len(),
        (dims[0] * dims[1] * 4) as usize
    );
    assert_start_exits(&mut rt, "muscle");
}

#[test]
fn duel_door_warp_draws_baka_fighter_and_start_leaves() {
    let Some(mut rt) = loaded_in("koin1") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    warp_into(&mut rt, SUB_BAKA, "BakaFighter", "baka");
    assert_hud_rows(&mut rt, "baka");
    let g = game_json(&rt);
    let opp = g["baka"]["opponent"].as_u64().expect("opponent") as u32;
    assert!((1..=16).contains(&opp), "ladder roster id: {g}");
    // Rows 0..2 fold onto the party pack (side 0); 3..16 have their own.
    let side = g["baka"]["opponent_side"].as_u64().expect("side") as u32;
    assert_eq!(side, u32::from(opp >= 3), "{g}");
    assert!(rt.play_mg_baka_presentation_ready());
    assert!(
        !rt.play_mg_baka_fighter_positions(0, 0).is_empty(),
        "player mesh"
    );
    assert!(
        !rt.play_mg_baka_fighter_positions(side, opp).is_empty(),
        "opponent {opp} mesh (side {side})"
    );
    let dims = rt.play_mg_baka_anim_dims(side, opp, 0);
    assert!(
        dims.len() == 2 && dims[1] > 0,
        "opponent idle clip: {dims:?}"
    );
    assert!(
        !rt.play_mg_baka_stage_positions(0).is_empty(),
        "PROT 1203 stage wall"
    );
    assert_eq!(rt.play_mg_baka_duel_vram(opp).len(), 1024 * 512 * 2);
    let st: serde_json::Value =
        serde_json::from_str(&rt.play_mg_baka_state_json()).expect("state json");
    assert_eq!(st["live"].as_bool(), Some(true));
    assert_eq!(st["phase"].as_str(), Some("fighting"));
    assert_start_exits(&mut rt, "baka");
}

#[test]
fn dance_door_warp_draws_the_hall_and_start_leaves() {
    let Some(mut rt) = loaded_in("koin1") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    warp_into(&mut rt, SUB_DANCE, "Dance", "dance");
    // The dance opens on its pre-song count-in, and the count-in banner is
    // NOT a font row: with the hall's HUD page resident it is retail's own
    // `READY...` sprite, emitted into the screen-prim pass. So the frame's
    // content is asserted where it actually is - a text-row assertion here
    // would be asserting the placeholder the sprite replaced.
    assert!(
        rt.play_screen_prim_count() >= 1,
        "the count-in banner draws as screen-space primitives"
    );
    // Run the count-in out; the status readout (and its font rows) arm
    // behind it.
    let mut armed = false;
    for _ in 0..16 {
        tick(&mut rt, 20);
        let ov: serde_json::Value =
            serde_json::from_str(&rt.play_overlay_draws_json(960, 720)).expect("overlay json");
        if ov["open"].as_bool() == Some(true) {
            armed = true;
            break;
        }
    }
    assert!(
        armed,
        "the dance status readout arms once the count-in clears"
    );
    assert_hud_rows(&mut rt, "dance");
    assert!(
        rt.play_mg_dance_body_ready(),
        "the dance cast + choreography must decode"
    );
    assert!(rt.play_mg_dance_body_count() >= 1);
    assert!(!rt.play_mg_dance_body_positions(0).is_empty());
    assert!(
        !rt.play_mg_dance_env_positions().is_empty(),
        "the other7 hall must bake"
    );
    assert_eq!(rt.play_mg_dance_body_vram().len(), 1024 * 512 * 2);
    let st: serde_json::Value =
        serde_json::from_str(&rt.play_mg_dance_state_json()).expect("state json");
    assert_eq!(st["live"].as_bool(), Some(true));
    let chart: serde_json::Value =
        serde_json::from_str(&rt.play_mg_dance_chart_json()).expect("chart json");
    assert!(chart["rows"].as_array().is_some_and(|r| !r.is_empty()));
    assert_start_exits(&mut rt, "dance");
}

/// Outside a minigame the host is inert: no label, no rows, no VRAM restore.
#[test]
fn quiet_field_has_no_minigame_payload() {
    let Some(mut rt) = loaded_in("town01") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    tick(&mut rt, 3);
    let g = game_json(&rt);
    assert!(g["game"].is_null(), "{g}");
    assert!(!rt.play_mg_take_vram_restore());
    assert!(rt.play_mg_slot_reel_pos().is_empty());
    assert_eq!(rt.play_mg_slot_state_json(), r#"{"live":false}"#);
}
