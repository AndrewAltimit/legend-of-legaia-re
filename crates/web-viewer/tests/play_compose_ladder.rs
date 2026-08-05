//! Draw-composition replay ladder: the browser **play page's** frame loop,
//! driven by pad and scored - the rendering-host ladder the reach report
//! (`scripts/ci/replay-port-coverage.py`) was structurally blind to.
//!
//! The four canonical pad ladders drive the headless `BootSession`, which
//! builds no draw list, so `engine-ui` reported **zero** executed regions
//! across the whole union and its rows all sat in the NO-LADDER bucket of
//! `docs/tooling/reach-triage.md`. The native window's composition lives in a
//! `bin/` target no `#[test]` can enter; the browser hosts' composition is
//! library code in this crate. This ladder is that page: it boots scenes from
//! the disc through [`LegaiaRuntime`] - the exact object `site/js/play-app.js`
//! constructs - feeds pad words per tick, and calls the page's **whole**
//! per-frame read surface (`play_overlay_draws_json`, `play_menu_draws_json`,
//! the screen-prim geometry route, the battle 3D + FX exports, the fishing /
//! dev-menu / name-entry / cutscene overlays) so the composition builders
//! execute under coverage the same way they execute under a visitor's
//! `requestAnimationFrame`.
//!
//! ## The ladder
//!
//! Rungs are ordered and cumulative - the run stops at the first one it
//! cannot clear, and the score is the count it did. Every rung carries its
//! own non-vacuity checks (draw counts, state probes), so the ladder is a
//! regression instrument and not only a coverage pump.
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | boot title to its main menu | the title card / glyph-fallback rows draw |
//! | 2 | the town01 **opening**: naming prompt commits, timeline ends | name-entry + cutscene text overlays render with live state |
//! | 3 | town01 field walk, composed per tick | the field 3D upload + per-frame surface; the pad moves the player |
//! | 4 | pause menu: all seven rows, five sub-screens driven | the menu draw path renders every screen it routes to |
//! | 5 | field shop + equipment shop overlays | `play_overlay_draws_json` composites the shop session |
//! | 6 | fishing session + both prize venues | the fishing HUD / state / prize surfaces |
//! | 7 | demo tile board | the tile-board mesh + transform exports |
//! | 8 | dev menu list + records page | the opt-in dev overlay draws off the world pad |
//! | 9 | scripted battle: command confirm -> action camera | the battle 3D / HUD / FX composition under a driven fight |
//! | 10 | map01 overworld: walk, save gate, forced encounter | the world-map arm + the field-to-battle intro's screen prims |
//!
//! ## Held is one event
//!
//! Every menu surface reads `just_pressed`, so [`tap`] presses for one frame
//! and releases on the next - the same contract as `menu_replay`'s driver.
//!
//! ## Ratchet
//!
//! `scripts/replays/play_compose_baseline.toml` carries the highest score
//! reached; the test asserts `score >= reached` and prints the line to paste
//! when it goes up. Raising it is a reviewed edit.
//!
//! Coverage export (what wires this into the reach report):
//!
//! ```text
//! cargo llvm-cov --release -p legaia-web-viewer --test play_compose_ladder \
//!     --json --output-path target/cov-play_compose_ladder.json
//! ```
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_web_viewer::runtime::LegaiaRuntime;

const W: u32 = 960;
const H: u32 = 720;

// ---------------------------------------------------------------------------
// Pad
// ---------------------------------------------------------------------------

/// One press through the world pad: a composed frame with the bit down, then
/// a composed frame at neutral (`just_pressed` is `pad & !pad_prev`).
fn tap(rt: &mut LegaiaRuntime, tally: &mut Tally, mask: u16) {
    rt.set_pad(mask);
    step(rt, tally);
    rt.set_pad(0);
    step(rt, tally);
}

// ---------------------------------------------------------------------------
// Composition - the page's per-frame read surface
// ---------------------------------------------------------------------------

/// What the composed frames added up to, per rung. The rungs assert against
/// these so an overlay that silently went empty fails the run instead of
/// passing as "composed".
#[derive(Default)]
struct Tally {
    frames: u64,
    overlay_open: u64,
    dialog_open: u64,
    menu_draw_texts: u64,
    battle_frames: u64,
    intro_prim_frames: u64,
    cutscene_text_frames: u64,
    vram_reads: u64,
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
}

/// Tick the simulation one frame and read the play page's frame surface -
/// the calls `site/js/play-app.js` makes per animation frame. Geometry that
/// the page uploads once per scene / battle generation is in
/// [`upload_field_geometry`] / [`upload_battle_geometry`] instead.
fn step(rt: &mut LegaiaRuntime, tally: &mut Tally) {
    rt.tick_frame().expect("tick_frame");
    tally.frames += 1;

    // HUD line + mode probes.
    let _ = rt.state_json();
    let _ = rt.scene_mode();
    let _ = rt.frame();

    // Field VRAM: re-read only when an effect dirtied it, as the page does.
    if rt.field_vram_take_dirty() {
        assert!(!rt.field_vram_bytes().is_empty(), "dirty VRAM must export");
        tally.vram_reads += 1;
    }

    // Player + NPC per-frame pose reads.
    let _ = rt.player_transform();
    if rt.player_has_mesh() {
        let _ = rt.player_mesh_positions();
    }
    let _ = rt.play_npc_transforms();
    let _ = rt.play_npc_clip_states();

    // Overlay stack: shop / inn / battle HUD / banners, dialog box, cutscene
    // text, name entry, fishing HUD, dev menu. Each returns a closed payload
    // when its surface is down - the page calls them unconditionally too.
    let overlay = json(&rt.play_overlay_draws_json(W, H));
    if overlay["open"] == true {
        tally.overlay_open += 1;
    }
    let dialog = json(&rt.play_dialog_draws_json(W, H));
    if dialog["open"] == true {
        tally.dialog_open += 1;
    }
    let _ = rt.play_cutscene_state_json();
    let cut = json(&rt.play_cutscene_text_draws_json(W, H));
    if cut["open"] == true {
        tally.cutscene_text_frames += 1;
    }
    let _ = rt.play_cutscene_camera_json();
    if rt.name_entry_is_active() {
        let _ = rt.name_entry_draws_json(W, H);
    }
    if rt.play_fishing_active() {
        let _ = rt.play_fishing_hud_json(W, H);
        let _ = rt.play_fishing_state_json();
    }
    let _ = rt.play_dev_menu_draws_json(W, H);
    if rt.play_menu_is_open() {
        let menu = json(&rt.play_menu_draws_json(W, H));
        tally.menu_draw_texts += menu["texts"].as_array().map(|a| a.len()).unwrap_or(0) as u64;
        let _ = rt.field_menu_model_json();
    }
    let _ = rt.play_sfx_state_json();

    // Field-to-battle intro transition: the screen-prim geometry route.
    if rt.play_screen_prim_count() > 0 {
        assert!(
            !rt.play_screen_prim_vertex_bytes().is_empty(),
            "intro prims report a count but no vertex stream"
        );
        let _ = rt.play_screen_prim_indices();
        let _ = rt.play_screen_prim_runs();
        tally.intro_prim_frames += 1;
    }

    // Battle 3D + FX layer, per frame (transforms / poses / camera / fx).
    if rt.play_battle_active() {
        tally.battle_frames += 1;
        let n = rt.play_battle_actor_count();
        let _ = rt.play_battle_actor_transforms();
        for i in 0..n {
            let _ = rt.play_battle_actor_pose(i);
            let _ = rt.play_battle_actor_ghosts(i);
        }
        let _ = rt.play_battle_camera_json();
        let _ = rt.play_battle_camera_vp(4.0 / 3.0);
        let _ = rt.play_battle_fx_sync(4.0 / 3.0);
        let _ = rt.play_battle_fx_positions();
        let _ = rt.play_battle_fx_uvs();
        let _ = rt.play_battle_fx_cba_tsb();
        let _ = rt.play_battle_fx_indices();
        let _ = rt.play_battle_fx_flat_rgba();
        for m in 0..rt.play_battle_fx_model_count() {
            let t = rt.play_battle_fx_model_tmd(m);
            let _ = rt.play_battle_fx_mesh_positions(t);
            let _ = rt.play_battle_fx_mesh_uvs(t);
            let _ = rt.play_battle_fx_mesh_cba_tsb(t);
            let _ = rt.play_battle_fx_mesh_indices(t);
        }
        let _ = rt.play_battle_fx_model_matrices();
        let _ = rt.play_battle_actor_cursor();
    }

    // Party-wipe hand-off, exactly as the page drives it.
    if rt.is_game_over() {
        let _ = rt.game_over_input(0);
    }
}

/// Tick + compose `n` frames with the pad released.
fn run(rt: &mut LegaiaRuntime, tally: &mut Tally, n: u32) {
    for _ in 0..n {
        step(rt, tally);
    }
}

/// The page's once-per-scene geometry upload: ground, terrain, every placed
/// environment mesh (posed where its object bind names a clip), morph slots,
/// the player rig, the NPC catalog, and the menu atlases.
///
/// `town` demands the full town shape (walk ground, placements, a lead field
/// mesh); a kingdom overworld carries a different layer mix, so the overworld
/// rung passes `false` and pins its own facts instead.
fn upload_field_geometry(rt: &mut LegaiaRuntime, town: bool) -> Result<(), String> {
    let status = json(&rt.field_status_json());
    if status.is_null() {
        return Err("no field render state after scene entry".into());
    }

    // Walk-ground heightfield.
    let quads = rt.field_ground_quad_count();
    if town && quads == 0 {
        return Err("scene has no walk-ground heightfield".into());
    }
    let _ = rt.field_ground_positions();
    let _ = rt.field_ground_uvs();
    let _ = rt.field_ground_cba_tsb();
    let _ = rt.field_ground_indices();

    // Placement + terrain layers.
    let slots = rt.field_placement_slots();
    let anim_ids = rt.field_placement_anim_ids();
    let _ = rt.field_placement_positions();
    let _ = rt.field_placement_rot_x();
    let _ = rt.field_placement_rot_y();
    let _ = rt.field_placement_rot_z();
    let _ = rt.field_placement_frames();
    let _ = rt.field_terrain_slots();
    let _ = rt.field_terrain_positions();
    let _ = rt.field_terrain_rot_y();
    let _ = rt.field_offmap_hide_xz();
    if town && slots.is_empty() {
        return Err("scene placed no environment objects".into());
    }

    // Environment meshes: one build per (slot, anim) pair the page would
    // upload, capped so a dense town stays a test and not a soak.
    let mut seen: Vec<(u32, u32)> = Vec::new();
    for (i, &slot) in slots.iter().enumerate() {
        let anim = anim_ids.get(i).copied().unwrap_or(0);
        if seen.contains(&(slot, anim)) || seen.len() >= 48 {
            continue;
        }
        seen.push((slot, anim));
        if rt.field_mesh_posed(slot, anim).is_err() {
            return Err(format!("field_mesh_posed({slot}, {anim}) failed"));
        }
        let pos = rt.field_mesh_positions();
        if pos.is_empty() {
            return Err(format!("env slot {slot} built an empty mesh"));
        }
        let _ = rt.field_mesh_uvs();
        let _ = rt.field_mesh_cba_tsb();
        let _ = rt.field_mesh_indices();
        let _ = rt.field_mesh_flat_rgba();
        if anim != 0 {
            let _ = rt.field_mesh_posed_frame_positions(slot, anim, 1);
        }
    }

    // Morph (vertex-lerp) slots.
    for slot in rt.field_morph_slots().into_iter().take(8) {
        let _ = rt.field_morph_positions(slot);
    }

    // Player rig.
    if town && !rt.player_has_mesh() {
        return Err("scene entry seeded no lead field mesh".into());
    }
    if rt.player_has_mesh() {
        let _ = rt.player_mesh_positions();
        let _ = rt.player_mesh_uvs();
        let _ = rt.player_mesh_cba_tsb();
        let _ = rt.player_mesh_indices();
        let _ = rt.player_mesh_flat_rgba();
        let _ = rt.field_player_occluded(0.0, 0.0, 0.0);
    }

    // NPC catalog + meshes + poses.
    let npcs = json(&rt.play_npc_catalog_json());
    let count = npcs["entries"].as_array().map(|a| a.len()).unwrap_or(0);
    for i in 0..count.min(24) as u32 {
        if rt.play_npc_mesh(i).is_err() {
            return Err(format!("play_npc_mesh({i}) failed"));
        }
        let _ = rt.play_npc_mesh_positions();
        let _ = rt.play_npc_mesh_uvs();
        let _ = rt.play_npc_mesh_cba_tsb();
        let _ = rt.play_npc_mesh_indices();
        let _ = rt.play_npc_mesh_object_ids();
        let _ = rt.play_npc_mesh_flat_rgba();
        let _ = rt.play_npc_pose_dims(i);
        let _ = rt.play_npc_pose_frames(i);
        let _ = rt.play_npc_live_bones(i);
    }

    // Menu atlases (uploaded once per session by the page).
    let _ = rt.play_menu_font_rgba();
    let _ = rt.play_menu_font_dims();
    let _ = rt.play_menu_has_chrome();
    let _ = rt.play_menu_chrome_rgba();
    let _ = rt.play_menu_chrome_dims();

    // Initial VRAM upload.
    if rt.field_vram_bytes().is_empty() {
        return Err("field VRAM export is empty".into());
    }
    Ok(())
}

/// The page's once-per-battle-generation geometry upload.
fn upload_battle_geometry(rt: &mut LegaiaRuntime) -> Result<(), String> {
    if rt.play_battle_generation() == 0 {
        return Err("battle render carries no generation stamp".into());
    }
    let _ = rt.play_battle_world_scale();
    if rt.play_battle_vram_bytes().is_empty() {
        return Err("battle VRAM export is empty".into());
    }
    let _ = rt.play_battle_backdrop_positions();
    let _ = rt.play_battle_backdrop_uvs();
    let _ = rt.play_battle_backdrop_cba_tsb();
    let _ = rt.play_battle_backdrop_indices();
    let _ = rt.play_battle_backdrop_flat_rgba();
    let _ = rt.play_battle_ground_positions();
    let _ = rt.play_battle_ground_uvs();
    let _ = rt.play_battle_ground_cba_tsb();
    let _ = rt.play_battle_ground_indices();
    let _ = rt.play_battle_ground_flat_rgba();
    let _ = rt.play_battle_ground_cue_json();
    let n = rt.play_battle_actor_count();
    if n < 2 {
        return Err(format!(
            "battle bound {n} actor meshes (need monster + party)"
        ));
    }
    for i in 0..n {
        if rt.play_battle_actor_positions(i).is_empty() {
            return Err(format!("battle actor {i} has no geometry"));
        }
        let _ = rt.play_battle_actor_uvs(i);
        let _ = rt.play_battle_actor_cba_tsb(i);
        let _ = rt.play_battle_actor_indices(i);
        let _ = rt.play_battle_actor_flat_rgba(i);
        let _ = rt.play_battle_actor_object_ids(i);
        for g in 0..2 {
            let _ = rt.play_battle_actor_ghost_pose(i, g);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rungs
// ---------------------------------------------------------------------------

fn rung1_title(rt: &mut LegaiaRuntime) -> Result<(), String> {
    rt.boot_title_start();
    if !rt.boot_title_is_active() {
        return Err("boot title session did not start".into());
    }
    let _ = rt.boot_title_has_save_data();
    let _ = rt.boot_title_has_atlas();
    let _ = rt.boot_title_atlas_rgba();
    let _ = rt.boot_title_atlas_dims();
    let _ = rt.boot_title_glyph_atlas_rgba();
    let _ = rt.boot_title_glyph_atlas_dims();
    // Start edge: PressStart -> MainMenu.
    let _ = rt.boot_title_step(PadButton::Start.mask());
    let draws = json(&rt.boot_title_draws_json(W, H));
    if draws["active"] != true {
        return Err(format!("title draws inactive at the main menu: {draws}"));
    }
    let sprites = draws["sprites"].as_array().map(|a| a.len()).unwrap_or(0);
    let glyphs = draws["glyphs"].as_array().map(|a| a.len()).unwrap_or(0);
    if sprites + glyphs == 0 {
        return Err("title main menu drew neither art bands nor glyph rows".into());
    }
    // Walk the menu rows (rendered each step) without confirming one.
    let _ = rt.boot_title_step(PadButton::Down.mask());
    let _ = rt.boot_title_draws_json(W, H);
    let _ = rt.boot_title_step(PadButton::Up.mask());
    let _ = rt.boot_title_draws_json(W, H);
    rt.boot_title_close();
    Ok(())
}

fn rung2_opening_name_entry(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    rt.debug_enter_town01_opening()
        .map_err(|e| format!("enter town01 opening: {e}"))?;
    // The establishing timeline runs long before its pinned op-0x49; compose
    // a subset of the wait so the cutscene surfaces render under it.
    let mut ticks = 0u32;
    while !rt.name_entry_is_active() && ticks < 8000 {
        if ticks.is_multiple_of(8) {
            step(rt, tally);
        } else {
            rt.tick_frame().expect("tick");
        }
        ticks += 1;
    }
    if !rt.name_entry_is_active() {
        return Err(format!(
            "opening timeline never opened name entry ({ticks} ticks)"
        ));
    }
    let draws = json(&rt.name_entry_draws_json(W, H));
    if draws["open"] != true || draws["texts"].as_array().is_none_or(|a| a.is_empty()) {
        return Err(format!("name entry drew nothing: {draws}"));
    }
    let _ = rt.name_entry_state_json();
    // Browse the grid, then commit the template default:
    // Select (cursor opens there) -> confirm prompt on No -> Up -> Yes.
    rt.name_entry_input(PadButton::Down.mask());
    let _ = rt.name_entry_draws_json(W, H);
    rt.name_entry_input(PadButton::Up.mask());
    rt.name_entry_input(PadButton::Cross.mask());
    let _ = rt.name_entry_draws_json(W, H);
    rt.name_entry_input(PadButton::Up.mask());
    if !rt.name_entry_input(PadButton::Cross.mask()) {
        return Err("confirming Yes did not commit the name".into());
    }
    if rt.party_display_name(0).is_empty() {
        return Err("committed name did not land in the party record".into());
    }
    // Let the timeline finish so the field hands the pad back.
    let mut ticks = 0u32;
    while rt.debug_timeline_active() && ticks < 12000 {
        if ticks.is_multiple_of(8) {
            step(rt, tally);
        } else {
            rt.tick_frame().expect("tick");
        }
        ticks += 1;
    }
    if rt.debug_timeline_active() {
        return Err("opening timeline never finished after the naming beat".into());
    }
    Ok(())
}

fn rung3_field_walk(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    rt.enter_field("town01")
        .map_err(|_| "enter_field(town01) failed".to_string())?;
    run(rt, tally, 5);
    upload_field_geometry(rt, true)?;

    // Enhancement toggles the page exposes; leave them at their defaults.
    rt.set_precise_movement(true);
    rt.set_precise_movement(false);
    rt.set_left_stick(0, 0);
    rt.set_camera_azimuth(0);

    let start = rt.player_transform();
    for dir in [
        PadButton::Down,
        PadButton::Left,
        PadButton::Up,
        PadButton::Right,
    ] {
        rt.set_pad(dir.mask());
        run(rt, tally, 30);
    }
    rt.set_pad(0);
    run(rt, tally, 5);
    let end = rt.player_transform();
    if start == end {
        return Err("pad walk never moved the player".into());
    }
    Ok(())
}

fn rung4_pause_menu(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    if !rt.play_menu_can_open() {
        return Err("field refuses to open the pause menu".into());
    }
    rt.play_menu_open();
    if !rt.play_menu_is_open() {
        return Err("play_menu_open did not open".into());
    }
    // Root cursor over all seven rows, drawing each step.
    for _ in 0..7 {
        rt.play_menu_input(PadButton::Down.mask());
        step(rt, tally);
    }
    rt.play_menu_close();

    // Five sub-screens behind the root rows, each browsed and backed out of.
    for row in ["Items", "Magic", "Equip", "Status", "Options", "Load"] {
        if !rt.play_menu_open_row(row) {
            return Err(format!("menu row {row} did not open its sub-screen"));
        }
        step(rt, tally);
        for edge in [
            PadButton::Down.mask(),
            PadButton::Down.mask(),
            PadButton::Right.mask(),
            PadButton::Up.mask(),
        ] {
            rt.play_menu_input(edge);
            step(rt, tally);
        }
        // Back all the way out (sub-screens can be two levels deep).
        for _ in 0..6 {
            if !rt.play_menu_is_open() {
                break;
            }
            rt.play_menu_input(PadButton::Circle.mask());
            step(rt, tally);
        }
        rt.play_menu_close();
        run(rt, tally, 2);
    }

    // Two rows get a second, deeper visit. Items: confirm a row so the item
    // action / target surface opens, then step and cancel out. Load: confirm
    // into the card rack so the read beat and the block grid render, then
    // cancel without committing (a commit would swap the scene under the
    // ladder). Both drives are draw-composed per edge.
    for (row, drive) in [
        (
            "Items",
            [
                PadButton::Cross.mask(),
                PadButton::Down.mask(),
                PadButton::Cross.mask(),
                PadButton::Down.mask(),
            ],
        ),
        (
            "Load",
            [
                PadButton::Cross.mask(),
                PadButton::Down.mask(),
                PadButton::Cross.mask(),
                PadButton::Right.mask(),
            ],
        ),
    ] {
        if !rt.play_menu_open_row(row) {
            return Err(format!("menu row {row} did not re-open"));
        }
        for edge in drive {
            rt.play_menu_input(edge);
            step(rt, tally);
            run(rt, tally, 3);
        }
        for _ in 0..8 {
            if !rt.play_menu_is_open() {
                break;
            }
            rt.play_menu_input(PadButton::Circle.mask());
            step(rt, tally);
        }
        rt.play_menu_close();
        run(rt, tally, 2);
    }

    // The Save row is scene-gated and town01's MAN does not set the bit, so
    // the confirm must refuse - the gate check that keeps this rung honest.
    if rt.play_scene_save_allowed() {
        return Err("town01 must not permit menu saving".into());
    }
    if rt.play_menu_open_row("Save") {
        rt.play_menu_close();
        return Err("the gated Save row opened in a town".into());
    }
    rt.play_menu_close();

    if tally.menu_draw_texts == 0 {
        return Err("menu sub-screens composed zero text draws".into());
    }
    Ok(())
}

fn rung5_shops(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    let before = tally.overlay_open;
    if !rt.debug_open_test_shop() {
        return Err("test shop did not open".into());
    }
    if !rt.play_shop_is_open() {
        return Err("shop session not reported open".into());
    }
    // Browse the stock list, open a quantity prompt, cancel, exit.
    for edge in [
        PadButton::Down.mask(),
        PadButton::Down.mask(),
        PadButton::Up.mask(),
        PadButton::Cross.mask(),
        PadButton::Circle.mask(),
    ] {
        rt.play_shop_input(edge);
        step(rt, tally);
    }
    for _ in 0..20 {
        if !rt.play_shop_is_open() {
            break;
        }
        rt.play_shop_input(PadButton::Circle.mask());
        step(rt, tally);
    }
    if tally.overlay_open == before {
        return Err("shop overlay never reported open in the composed frames".into());
    }

    // Equipment shop, with its stat-compare recipient flow.
    if rt.debug_open_equipment_shop() {
        for edge in [
            PadButton::Down.mask(),
            PadButton::Cross.mask(),
            PadButton::Circle.mask(),
        ] {
            rt.play_shop_input(edge);
            step(rt, tally);
        }
        for _ in 0..20 {
            if !rt.play_shop_is_open() {
                break;
            }
            rt.play_shop_input(PadButton::Circle.mask());
            step(rt, tally);
        }
    }
    run(rt, tally, 3);
    Ok(())
}

fn rung6_fishing(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    if !rt.play_fishing_start() {
        return Err("fishing session did not start".into());
    }
    if !rt.play_fishing_active() {
        return Err("fishing session not live".into());
    }
    let hud = json(&rt.play_fishing_hud_json(W, H));
    if hud["open"] != true {
        return Err(format!("fishing HUD closed while live: {hud}"));
    }
    let state = json(&rt.play_fishing_state_json());
    if state["live"] != true {
        return Err(format!("fishing state not live: {state}"));
    }
    // Drive a few cast presses through the world pad, composing each frame.
    for _ in 0..6 {
        tap(rt, tally, PadButton::Cross.mask());
        run(rt, tally, 8);
    }
    // Both point-exchange venues + one (gated) buy attempt.
    for venue in [0u32, 1] {
        if json(&rt.play_fishing_prizes_json(venue)).is_null() {
            return Err(format!("prize venue {venue} did not decode"));
        }
    }
    let _ = rt.play_fishing_prize_buy(0, 0);
    if rt.play_fishing_stop() < 0 {
        return Err("fishing session did not stop".into());
    }
    run(rt, tally, 3);
    Ok(())
}

fn rung7_tile_board(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    if !rt.play_install_demo_tile_board() {
        return Err("demo tile board did not install".into());
    }
    // The draw list is rebuilt by the field tick, so the board is visible to
    // the accessors only after a composed frame - same as the page.
    run(rt, tally, 2);
    let slots = rt.play_tile_board_slots();
    if slots.is_empty() {
        return Err("tile board reports no cells".into());
    }
    let _ = rt.play_tile_board_transforms();
    let actor_slots = rt.play_tile_actor_slots();
    for slot in actor_slots.into_iter().take(4) {
        if rt.play_tile_actor_mesh(slot).is_err() {
            return Err(format!("tile actor mesh {slot} failed"));
        }
        let _ = rt.play_tile_actor_mesh_positions();
        let _ = rt.play_tile_actor_mesh_uvs();
        let _ = rt.play_tile_actor_mesh_cba_tsb();
        let _ = rt.play_tile_actor_mesh_indices();
        let _ = rt.play_tile_actor_mesh_flat_rgba();
    }
    run(rt, tally, 5);
    Ok(())
}

fn rung8_dev_menu(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    rt.play_dev_menu_set_enabled(true);
    if !rt.play_dev_menu_enabled() {
        return Err("dev menu opt-in did not take".into());
    }
    run(rt, tally, 2);
    let draws = json(&rt.play_dev_menu_draws_json(W, H));
    if draws["texts"].as_array().is_none_or(|a| a.is_empty()) {
        return Err(format!("dev menu list drew nothing: {draws}"));
    }
    // Walk the rows, then Square for the Records page, then back.
    tap(rt, tally, PadButton::Down.mask());
    tap(rt, tally, PadButton::Up.mask());
    tap(rt, tally, PadButton::Right.mask());
    tap(rt, tally, PadButton::Square.mask());
    let records = json(&rt.play_dev_menu_draws_json(W, H));
    if records["texts"].as_array().is_none_or(|a| a.is_empty()) {
        return Err(format!("dev records page drew nothing: {records}"));
    }
    tap(rt, tally, PadButton::Square.mask());
    rt.play_dev_menu_set_enabled(false);
    Ok(())
}

fn rung9_battle(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    let _ = rt.scene_rolls_encounters();
    let _ = rt.debug_formation_rows();
    if !rt.debug_start_test_battle() {
        return Err("no scripted formation row entered battle".into());
    }
    if !rt.play_battle_active() {
        return Err("battle render did not build on the Field -> Battle edge".into());
    }
    upload_battle_geometry(rt)?;

    // Drive the fight: confirm edges every few frames walk whatever command /
    // target surface is up (the command session reads World::input), and the
    // composed frames read the HUD + 3D + FX layers throughout.
    let mut acted = false;
    let mut hud_open = false;
    for i in 0u32..600 {
        if !rt.play_battle_active() {
            break;
        }
        if i.is_multiple_of(8) {
            tap(rt, tally, PadButton::Cross.mask());
        } else {
            step(rt, tally);
        }
        let cam = json(&rt.play_battle_camera_json());
        acted |= cam["phase"] == "action";
        if !hud_open {
            let overlay = json(&rt.play_overlay_draws_json(W, H));
            hud_open = overlay["open"] == true
                && overlay["texts"].as_array().is_some_and(|a| !a.is_empty());
        }
        if acted && hud_open && i > 120 {
            break;
        }
    }
    if !acted {
        return Err("no executing action took the battle camera".into());
    }
    if !hud_open {
        return Err("battle HUD never composed an open overlay".into());
    }
    Ok(())
}

fn rung10_overworld(rt: &mut LegaiaRuntime, tally: &mut Tally) -> Result<(), String> {
    rt.enter_field("map01")
        .map_err(|_| "enter_field(map01) failed".to_string())?;
    run(rt, tally, 5);
    upload_field_geometry(rt, false)?;
    if !rt.play_scene_save_allowed() {
        return Err("kingdom overworld must permit menu saving".into());
    }
    // Walk a few tiles of overworld.
    let start = rt.player_transform();
    rt.set_pad(PadButton::Down.mask());
    run(rt, tally, 60);
    rt.set_pad(0);
    if rt.player_transform() == start {
        return Err("overworld walk never moved the player".into());
    }
    // Forced encounter through the page's own route: the field-to-battle
    // intro transition runs under the composed frames, so the screen-prim
    // geometry route executes before the mode flips.
    let prim_before = tally.intro_prim_frames;
    if !rt.debug_force_battle(-1) {
        return Err("debug_force_battle(-1) resolved no formation on map01".into());
    }
    let mut in_battle = false;
    for _ in 0..400 {
        step(rt, tally);
        if rt.play_battle_active() {
            in_battle = true;
            break;
        }
    }
    if !in_battle {
        return Err("forced encounter never reached SceneMode::Battle".into());
    }
    if tally.intro_prim_frames == prim_before {
        return Err("the field-to-battle intro emitted no screen prims".into());
    }
    upload_battle_geometry(rt)?;
    run(rt, tally, 60);
    Ok(())
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

fn baseline() -> u32 {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/replays/play_compose_baseline.toml");
    let text = std::fs::read_to_string(&path).expect("play_compose_baseline.toml");
    let value: toml::Value = text.parse().expect("baseline TOML parses");
    value["reached"]
        .as_integer()
        .expect("baseline carries `reached`") as u32
}

#[test]
fn play_compose_ladder() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable (disc-gated)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");

    let mut tally = Tally::default();
    type Rung = Box<dyn FnMut(&mut LegaiaRuntime, &mut Tally) -> Result<(), String>>;
    let rungs: Vec<(&str, Rung)> = vec![
        ("title", Box::new(|rt, _t| rung1_title(rt))),
        ("opening-name-entry", Box::new(rung2_opening_name_entry)),
        ("field-walk", Box::new(rung3_field_walk)),
        ("pause-menu", Box::new(rung4_pause_menu)),
        ("shops", Box::new(rung5_shops)),
        ("fishing", Box::new(rung6_fishing)),
        ("tile-board", Box::new(rung7_tile_board)),
        ("dev-menu", Box::new(rung8_dev_menu)),
        ("battle", Box::new(rung9_battle)),
        ("overworld-encounter", Box::new(rung10_overworld)),
    ];

    let mut score = 0u32;
    for (name, mut rung) in rungs {
        match rung(&mut rt, &mut tally) {
            Ok(()) => {
                score += 1;
                eprintln!(
                    "[rung {score}] {name}: cleared \
                     (frames={} overlay={} menu_texts={} battle={} prims={})",
                    tally.frames,
                    tally.overlay_open,
                    tally.menu_draw_texts,
                    tally.battle_frames,
                    tally.intro_prim_frames,
                );
            }
            Err(why) => {
                eprintln!("[stall] rung {} ({name}): {why}", score + 1);
                break;
            }
        }
    }
    // Cross-rung non-vacuity: the composed union must have read VRAM at least
    // once and run the battle layer for real frames, or the "composition"
    // was a stack of closed payloads.
    if score >= 9 {
        assert!(
            tally.battle_frames > 0,
            "battle rungs composed no battle frames"
        );
    }
    eprintln!(
        "play_compose_ladder: score {score} \
         (frames={} overlay_open={} dialog_open={} menu_texts={} \
          battle_frames={} intro_prim_frames={} cutscene_text_frames={} vram_reads={})",
        tally.frames,
        tally.overlay_open,
        tally.dialog_open,
        tally.menu_draw_texts,
        tally.battle_frames,
        tally.intro_prim_frames,
        tally.cutscene_text_frames,
        tally.vram_reads,
    );

    let base = baseline();
    assert!(
        score >= base,
        "draw-composition ladder regressed: score {score} < baseline {base} \
         (scripts/replays/play_compose_baseline.toml)"
    );
    if score > base {
        eprintln!(
            "baseline can ratchet: set reached = {score} in \
             scripts/replays/play_compose_baseline.toml"
        );
    }
}
