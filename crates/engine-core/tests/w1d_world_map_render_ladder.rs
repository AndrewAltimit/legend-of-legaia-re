//! Pad-driven ladder for the **world-map render pass** and the world-map
//! **panel-actor screen** - the two clusters
//! `docs/tooling/reach-triage.md` keeps apart under the `world-map` and
//! `world-map-panel` reach labels, neither of which any existing ladder
//! entered.
//!
//! ## Why a ladder and not more unit tests
//!
//! Both clusters were already unit-tested at the kernel. What no run
//! exercised was the *chain*: `World::tick` -> `tick_world_map` ->
//! `WorldMapController::tick` -> the kernels. The reach report
//! (`scripts/ci/replay-port-coverage.py`) joins coverage from integration
//! ladders, and a `#[cfg(test)]` unit test in `src/` is not linked into one -
//! so every one of these addresses read *live but never entered* while its
//! own unit tests were green. This file drives the chain the way a pad does.
//!
//! Coverage export (what wires this into the reach report):
//!
//! ```text
//! cargo llvm-cov --release -p legaia-engine-core \
//!     --test w1d_world_map_render_ladder \
//!     --json --output-path target/cov-w1d_world_map_render_ladder.json
//! ```
//!
//! ## The pad layout this file is about
//!
//! `FUN_801E76D4`'s literals are **packed** pad bits (`_DAT_8007B850` held,
//! `_DAT_8007B874` newly pressed - the byte-swapped word `FUN_8001822C`
//! builds), not [`PadButton`]'s raw BIOS layout. Every chord below is
//! therefore written as `raw(<packed literal>)`, and the retail literal stays
//! visible next to it. Driving the same words at the raw layout is what the
//! controller used to do, and it is why the whole top-view band was
//! unreachable: the camera scroll landed on the four face buttons and the
//! top-view toggle became `Down + Start + L3`.
//!
//! ## Disc
//!
//! None. Everything here is `World`-level simulation, so the ladder runs in
//! CI without `LEGAIA_DISC_BIN` - the world-map render pass and its panel
//! screen are disc-free kernels over controller state.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::World;
use legaia_engine_core::world_map_panel_host::PanelActorKind;

// ---------------------------------------------------------------------------
// Pad
// ---------------------------------------------------------------------------

/// The raw pad word that reaches the controller as the retail packed word
/// `packed`.
fn raw(packed: u16) -> u16 {
    packed.swap_bytes()
}

/// Packed literals from `FUN_801E76D4` / `FUN_801D01B0`, named once.
mod packed {
    /// `0x801E7710` - the whole held word the top-view toggle compares to.
    pub const TOGGLE_HELD: u16 = 0x4A;
    /// `0x801E7724` - the whole newly-pressed word it compares to.
    pub const TOGGLE_EDGE: u16 = 0x40;
    /// The toggle's two shoulders, so Cross can be the only new press.
    pub const TOGGLE_SHOULDERS: u16 = 0x0A;
    /// `0x801E7830` - the modifier that opens the camera bank.
    pub const CAM_MOD: u16 = 0x0004;
    /// `0x801E7838` / `0x801E7860` - camera X.
    pub const CAM_X_DEC: u16 = 0x1000;
    pub const CAM_X_INC: u16 = 0x4000;
    /// `0x801E7888` / `0x801E78A4` - camera Z.
    pub const CAM_Z_DEC: u16 = 0x2000;
    pub const CAM_Z_INC: u16 = 0x8000;
    /// `0x801E78C0` / `0x801E78E8` - azimuth.
    pub const AZ_INC: u16 = 0x0020;
    pub const AZ_DEC: u16 = 0x0080;
    /// `0x801E7910` / `0x801E792C` - zoom.
    pub const ZOOM_DEC: u16 = 0x0008;
    pub const ZOOM_INC: u16 = 0x0002;
    /// `0x801E77F0` / `0x801E780C` - the anim-flag toggles.
    pub const ANIM_A: u16 = 0x0020;
    pub const ANIM_B: u16 = 0x0080;
    /// `0x801D0214` - the map-display fade arm.
    pub const MAP_DISPLAY_ARM: u16 = 0x0004;
}

/// One frame with `pad` held.
fn frame(w: &mut World, pad: u16) {
    w.set_pad(pad);
    let _ = w.tick();
}

/// A press: one frame with the bit down, one released, so the engine's
/// `pad & !pad_prev` edge fires exactly once.
fn tap(w: &mut World, pad: u16) {
    frame(w, pad);
    frame(w, 0);
}

/// Retail's top-view toggle: shoulders down for a frame, then Cross added.
fn press_top_view_chord(w: &mut World) {
    frame(w, raw(packed::TOGGLE_SHOULDERS));
    frame(w, raw(packed::TOGGLE_HELD));
    frame(w, 0);
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// An overworld with a seated player, entered the way
/// `SceneHost::enter_world_map_scene` leaves it.
fn overworld() -> World {
    let mut w = World::default();
    w.enter_world_map();
    w.spawn_actor(0).active = true;
    w.player_actor_slot = Some(0);
    w.seat_player_at_tile(20, 30);
    frame(&mut w, 0);
    w
}

fn ctrl(w: &World) -> &legaia_engine_core::world_map::WorldMapController {
    w.world_map_ctrl.as_ref().expect("world-map controller")
}

fn ctrl_mut(w: &mut World) -> &mut legaia_engine_core::world_map::WorldMapController {
    w.world_map_ctrl.as_mut().expect("world-map controller")
}

/// The developer opt-in the native window takes from its `--world-map` arm
/// (`window/run.rs` sets `debug_enabled = true`). Everything after it is pad.
fn set_debug_band(w: &mut World, on: bool) {
    ctrl_mut(w).debug_enabled = on;
}

// ---------------------------------------------------------------------------
// Rungs - the render pass
// ---------------------------------------------------------------------------

/// Rung 1. An overworld frame with no chords is *quiet*: no dim, no horizon
/// batch, no panel actor. Every later rung's assertion is against this floor,
/// so a kernel that fires unconditionally fails here rather than passing as a
/// success everywhere else.
fn rung1_quiet_overworld(w: &mut World) -> Result<(), String> {
    for _ in 0..8 {
        frame(w, 0);
    }
    let c = ctrl(w);
    if c.screen_dim.is_some() {
        return Err("a walk-mode frame emitted the top-view dim".into());
    }
    if c.horizon.is_some() {
        return Err("an unarmed frame emitted a horizon batch".into());
    }
    if c.panels.is_active() {
        return Err("a chordless frame installed a panel actor".into());
    }
    if c.entry_fade_draw.is_some() || c.map_display_requested {
        return Err("an idle frame ran the map-display fade".into());
    }
    Ok(())
}

/// Rung 2. `FUN_801D01B0`'s arm -> `FUN_801D1344`'s head -> `FUN_800196A4`:
/// an L1 edge on the overworld cues SFX `0x20`, seeds the fade ramp, and the
/// ramp runs monotonically to a single mode-12 (MAPDSIP) hand-off.
///
/// Debug off: the port's own panel chords also bind L1 and shadow this arm
/// while the developer band is enabled.
fn rung2_map_display_fade(w: &mut World) -> Result<(), String> {
    set_debug_band(w, false);
    ctrl_mut(w).scene_base = 0xF4; // Sebucus - kingdom index 1
    tap(w, raw(packed::MAP_DISPLAY_ARM));

    let sfx = ctrl_mut(w).drain_sfx();
    if !sfx.contains(&legaia_engine_core::world_map::MAP_DISPLAY_SFX) {
        return Err(format!("the arm raised no SFX cue: {sfx:?}"));
    }

    // The arm frame already ran one fade tick, so the ramp is live and the
    // first quad is on the board.
    let mut greys: Vec<u8> = ctrl(w).entry_fade_draw.iter().map(|q| q.grey).collect();
    let mut reports = 0;
    for _ in 0..64 {
        frame(w, 0);
        let c = ctrl(w);
        if let Some(q) = c.entry_fade_draw {
            greys.push(q.grey);
        }
        if c.map_display_requested {
            reports += 1;
        }
    }
    if reports != 1 {
        return Err(format!("expected one mode-12 hand-off, got {reports}"));
    }
    if greys.len() < 4 {
        return Err(format!("the ramp produced {} quads", greys.len()));
    }
    if !greys.windows(2).all(|p| p[0] < p[1]) {
        return Err(format!("the fade-up ramp is not monotone: {greys:?}"));
    }
    if greys.last() != Some(&0xFF) {
        return Err(format!("the ramp did not reach white: {greys:?}"));
    }
    if ctrl(w).entry_fade.kingdom_index != Some(1) {
        return Err("the fade tick did not re-derive the kingdom index".into());
    }
    if ctrl(w).entry_fade.ramp != 0 {
        return Err("the completed ramp did not self-clear".into());
    }
    Ok(())
}

/// Rung 3. The top-view toggle. Retail compares the two pad *words* for
/// equality, so the chord is exact: R1+R2 held with Cross the only new press.
fn rung3_top_view_toggle(w: &mut World) -> Result<(), String> {
    set_debug_band(w, true);
    if raw(packed::TOGGLE_HELD) & !raw(packed::TOGGLE_SHOULDERS) != raw(packed::TOGGLE_EDGE) {
        return Err("the chord's new press is not Cross alone".into());
    }

    // A near-miss must not toggle: an extra held direction breaks the word
    // equality (`bne v1,v0` at 0x801E7714).
    frame(w, raw(packed::TOGGLE_SHOULDERS));
    frame(w, raw(packed::TOGGLE_HELD) | PadButton::Up.mask());
    frame(w, 0);
    if ctrl(w).is_top_view() {
        return Err("a stray held button still toggled the view".into());
    }

    press_top_view_chord(w);
    if !ctrl(w).is_top_view() {
        return Err("the retail chord did not enter top view".into());
    }
    Ok(())
}

/// Rung 4. `FUN_801E75DC` - the top-view screen dim. The gate is the anim-A
/// bit, and the only thing that sets it is a Circle edge in top view with no
/// shoulder held (`0x801E77D8`), so this rung is the whole reason the dim
/// pass had no runtime reach.
fn rung4_screen_dim(w: &mut World) -> Result<(), String> {
    if ctrl(w).screen_dim.is_some() {
        return Err("the dim fired before the anim bit was set".into());
    }
    // A shoulder held must shadow the toggle.
    frame(w, raw(packed::CAM_MOD));
    frame(w, raw(packed::CAM_MOD | packed::ANIM_A));
    frame(w, 0);
    if ctrl(w).anim_flags & 1 != 0 {
        return Err("the anim toggle fired with a shoulder held".into());
    }

    tap(w, raw(packed::ANIM_A));
    if ctrl(w).anim_flags & 1 == 0 {
        return Err("a clean Circle edge did not set anim bit 0".into());
    }
    let dim = ctrl(w)
        .screen_dim
        .ok_or("anim bit 0 set but no dim pass emitted")?;
    // The retail packet: a black semi-transparent flat quad over the whole
    // 320x224 draw area, dither bracketed off then on.
    if dim.quad.cmd != 0x2A || dim.quad.color != (0, 0, 0) {
        return Err(format!("dim quad is not the retail packet: {:?}", dim.quad));
    }
    if dim.quad.verts != [(0, -4), (320, -4), (0, 224), (320, 224)] {
        return Err(format!("dim quad geometry: {:?}", dim.quad.verts));
    }
    if dim.mode_before.dither || !dim.mode_after.dither {
        return Err("the dim is not dither-bracketed".into());
    }

    // Anim-B is a second, independent bit, and clearing anim-A must stop the
    // pass on the very next frame rather than leaving a stale one.
    tap(w, raw(packed::ANIM_B));
    if ctrl(w).anim_flags != 3 {
        return Err(format!("anim flags after B: {:#x}", ctrl(w).anim_flags));
    }
    tap(w, raw(packed::ANIM_A));
    if ctrl(w).screen_dim.is_some() {
        return Err("clearing anim bit 0 left a stale dim pass".into());
    }
    Ok(())
}

/// Rung 5. The top-view camera bank, which retail keeps behind the L1
/// modifier. Both halves matter: the steps, and the fact that the same
/// directions do nothing without the modifier (that is what stops the anim
/// toggles and the camera from fighting over Circle/Square).
fn rung5_top_view_camera(w: &mut World) -> Result<(), String> {
    let before = {
        let c = ctrl(w);
        (c.camera_x, c.camera_z, c.azimuth, c.zoom)
    };
    for _ in 0..3 {
        frame(w, raw(packed::CAM_X_DEC | packed::CAM_Z_INC));
    }
    let after = {
        let c = ctrl(w);
        (c.camera_x, c.camera_z, c.azimuth, c.zoom)
    };
    if before != after {
        return Err("the camera moved without the L1 modifier held".into());
    }

    frame(
        w,
        raw(packed::CAM_MOD | packed::CAM_X_DEC | packed::CAM_Z_INC),
    );
    frame(w, raw(packed::CAM_MOD | packed::AZ_INC | packed::ZOOM_INC));
    frame(
        w,
        raw(packed::CAM_MOD | packed::CAM_X_INC | packed::CAM_Z_DEC),
    );
    frame(w, raw(packed::CAM_MOD | packed::AZ_DEC | packed::ZOOM_DEC));
    let c = ctrl(w);
    // Every pair is a step and its inverse, so the retail step sizes are the
    // thing under test rather than a running total.
    if (c.camera_x, c.camera_z, c.azimuth, c.zoom) != before {
        return Err(format!(
            "the camera steps are not symmetric: {:?} vs {before:?}",
            (c.camera_x, c.camera_z, c.azimuth, c.zoom)
        ));
    }
    // ... and one unmatched step lands on the retail magnitudes.
    frame(w, raw(packed::CAM_MOD | packed::CAM_X_DEC));
    frame(w, raw(packed::CAM_MOD | packed::AZ_INC));
    frame(w, raw(packed::CAM_MOD | packed::ZOOM_INC));
    let c = ctrl(w);
    if c.camera_x != before.0 - 8 || c.azimuth != before.2 + 0x14 || c.zoom != before.3 + 4 {
        return Err(format!(
            "camera step sizes: x {} az {} zoom {}",
            c.camera_x - before.0,
            c.azimuth - before.2,
            c.zoom - before.3
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rungs - the panel-actor screen
// ---------------------------------------------------------------------------

/// Rung 6. Back to walk mode, where the panel chords live.
fn rung6_leave_top_view(w: &mut World) -> Result<(), String> {
    press_top_view_chord(w);
    if ctrl(w).is_top_view() {
        return Err("the chord did not return to walk mode".into());
    }
    Ok(())
}

/// Rung 7. `FUN_801ED590` - the sub-list picker, opened by the Square chord,
/// walked to row 1 and confirmed into retail's state-3 hand-off, which the
/// port binds to the Riremito travel art. The art then warps the party back
/// to the tile the screen froze on entry.
fn rung7_sub_list_handoff(w: &mut World) -> Result<(), String> {
    let frozen = ctrl(w)
        .panels
        .visited
        .last()
        .copied()
        .ok_or("the idle overworld tick recorded no visited tile")?;

    frame(w, PadButton::Square.mask());
    {
        let c = ctrl(w);
        if c.panels.kind != Some(PanelActorKind::SubList) {
            return Err(format!("Square installed {:?}", c.panels.kind));
        }
        if !c
            .panels
            .windows
            .is_open(legaia_engine_core::world_map_panel_host::SUBLIST_PANEL_INDEX)
        {
            return Err("the sub-list open script spawned no window".into());
        }
    }
    frame(w, 0);
    // Down moves the cursor to row 1, Cross takes the hand-off.
    frame(w, PadButton::Down.mask());
    frame(w, 0);
    if ctrl(w).panels.cursor != 1 {
        return Err(format!("sub-list cursor is {}", ctrl(w).panels.cursor));
    }
    tap(w, PadButton::Cross.mask());

    // The player is moved off the frozen tile so the warp is observable.
    w.seat_player_at_tile(120, 5);
    let mut handed_off = false;
    for _ in 0..600 {
        frame(w, 0);
        if matches!(ctrl(w).panels.kind, Some(PanelActorKind::TravelArt(_))) {
            handed_off = true;
        }
        if !ctrl(w).panels.is_active() && handed_off {
            break;
        }
    }
    if !handed_off {
        return Err("the row-1 confirm never handed off to the travel art".into());
    }
    let slot = w.player_actor_slot.ok_or("no player actor")? as usize;
    let a = &w.actors[slot];
    let want = (
        ((frozen.tile_x << 7) + 0x40) as i16,
        ((frozen.tile_z << 7) + 0x40) as i16,
    );
    if (a.move_state.world_x, a.move_state.world_z) != want {
        return Err(format!(
            "travel art landed at {:?}, wanted the frozen tile {want:?}",
            (a.move_state.world_x, a.move_state.world_z)
        ));
    }
    Ok(())
}

/// Rung 8. `FUN_801EF014` - the story-flag window. Its phase 0 clears the
/// range it covers out of the world's own system-flag bank, and its confirm
/// commits the picked row back into that same bank.
fn rung8_flag_window(w: &mut World) -> Result<(), String> {
    use legaia_engine_vm::world_map_panel_actors::FlagWindowDescriptor;
    // Own the whole span the picker scans, so the row it remembers is this
    // rung's and not whatever an earlier rung left in the shared bank. The
    // list draws bottom-up and does not wrap, so remembering row 3 (screen
    // row 0) is the one seed a single Down can leave.
    const BASE: u16 = 0x100;
    for i in 0..8u16 {
        w.system_flag_clear(BASE + i);
    }
    w.system_flag_set(BASE + 3);
    ctrl_mut(w).panels.flag_desc = FlagWindowDescriptor {
        count: 8,
        first_visible: 0,
        rows: 4,
        base_flag: i32::from(BASE),
    };
    frame(w, PadButton::R1.mask());
    if ctrl(w).panels.kind != Some(PanelActorKind::FlagWindow) {
        return Err(format!("R1 installed {:?}", ctrl(w).panels.kind));
    }
    if w.system_flag_test(BASE + 3) {
        return Err("phase 0's range clear never reached the world bank".into());
    }
    if ctrl(w).panels.remembered_row != 3 {
        return Err(format!(
            "the scan remembered row {}",
            ctrl(w).panels.remembered_row
        ));
    }
    frame(w, 0);
    frame(w, PadButton::Down.mask());
    frame(w, 0);
    if ctrl(w).panels.cursor == ctrl(w).panels.remembered_row {
        return Err("Down did not move the pick off the remembered row".into());
    }
    tap(w, PadButton::Cross.mask());
    let picked = BASE + ctrl(w).panels.cursor as u16;
    if !w.system_flag_test(picked) {
        return Err(format!("the confirm did not set flag {picked:#x}"));
    }
    ctrl_mut(w).panels.dismiss();
    frame(w, 0);
    Ok(())
}

/// Rung 9. `FUN_801EE90C` - the yes/no text box. Installed at its prompt
/// phase (phase 0 is the arrival path and parks with no exit), its confirm
/// arm restores the party's HP and MP from the live records.
fn rung9_text_box(w: &mut World) -> Result<(), String> {
    w.roster = legaia_save::Party::zeroed(3);
    for m in w.roster.members.iter_mut() {
        m.raw[0x104..0x106].copy_from_slice(&300u16.to_le_bytes()); // hp max
        m.raw[0x106..0x108].copy_from_slice(&1u16.to_le_bytes()); // hp cur
        m.raw[0x108..0x10A].copy_from_slice(&80u16.to_le_bytes()); // mp max
        m.raw[0x10A..0x10C].copy_from_slice(&0u16.to_le_bytes()); // mp cur
    }
    frame(w, PadButton::R2.mask());
    {
        let c = ctrl(w);
        if c.panels.kind != Some(PanelActorKind::TextBox) {
            return Err(format!("R2 installed {:?}", c.panels.kind));
        }
        if c.panels.phase != 1 {
            return Err(format!(
                "the text box seated phase {} - phase 0 wedges",
                c.panels.phase
            ));
        }
    }
    frame(w, 0);
    tap(w, PadButton::Cross.mask());
    for m in w.roster.members.iter() {
        if u16::from_le_bytes([m.raw[0x106], m.raw[0x107]]) != 300 {
            return Err("the confirm arm did not restore HP".into());
        }
        if u16::from_le_bytes([m.raw[0x10A], m.raw[0x10B]]) != 80 {
            return Err("the confirm arm did not restore MP".into());
        }
    }
    // Dismissing the confirmation retires the actor.
    for _ in 0..16 {
        tap(w, PadButton::Cross.mask());
        if !ctrl(w).panels.is_active() {
            break;
        }
    }
    ctrl_mut(w).panels.dismiss();
    frame(w, 0);
    Ok(())
}

/// Rung 10. `FUN_801EE5D4` - the screen-fill fade. It closes every open
/// window on its first frame, raises the scene object's `0x80000` bit in its
/// ramp phase, and exits on its own without any further pad.
fn rung10_fill_fade(w: &mut World) -> Result<(), String> {
    // Leave a window open so "closes every window" is observable.
    frame(w, PadButton::Square.mask());
    frame(w, 0);
    let open_before = ctrl(w).panels.windows.open_count();
    ctrl_mut(w).panels.dismiss();
    frame(w, 0);
    if open_before == 0 {
        return Err("the sub-list left no window for the fill fade to close".into());
    }

    frame(w, PadButton::L2.mask());
    if ctrl(w).panels.kind != Some(PanelActorKind::FillFade) {
        return Err(format!("L2 installed {:?}", ctrl(w).panels.kind));
    }
    frame(w, 0);
    if ctrl(w).panels.windows.open_count() != 0 {
        return Err("the fill fade did not close the window stack".into());
    }
    let mut saw_scene_bit = false;
    let mut retired = false;
    for _ in 0..512 {
        frame(w, 0);
        let c = ctrl(w);
        saw_scene_bit |= c.panels.scene_obj_flags & 0x0008_0000 != 0;
        if !c.panels.is_active() {
            retired = true;
            break;
        }
    }
    if !saw_scene_bit {
        return Err("the fill fade never set the scene object's flag bit".into());
    }
    if !retired {
        return Err("the fill fade never reached its exit arm".into());
    }
    Ok(())
}

/// Rung 11. `FUN_801EDF00` - the return-to-title soft reset. It slides the
/// records screen in, samples the pad only once it is at rest, and then runs
/// its white fade to the executable reload the engine records but declines.
fn rung11_soft_reset(w: &mut World) -> Result<(), String> {
    frame(w, PadButton::Start.mask());
    if ctrl(w).panels.kind != Some(PanelActorKind::SoftReset) {
        return Err(format!("Start installed {:?}", ctrl(w).panels.kind));
    }
    frame(w, 0);
    let mut at_rest = false;
    for _ in 0..512 {
        frame(w, 0);
        if ctrl(w).panels.slide == legaia_engine_vm::world_map_panel_actors::SOFT_RESET_SLIDE_REST {
            at_rest = true;
            break;
        }
    }
    if !at_rest {
        return Err("the records screen never finished sliding in".into());
    }
    // The pad is only sampled at rest, so a press before this point does
    // nothing - which is what makes the slide a real gate.
    tap(w, PadButton::Cross.mask());
    if ctrl(w).panels.phase != 2 {
        return Err(format!(
            "the face press did not start the white fade (phase {})",
            ctrl(w).panels.phase
        ));
    }
    // Phase 2 has no terminal arm: retail reloads the executable from here
    // and never returns, so the port's observable end state is the fade
    // counter crossing `SOFT_RESET_RELOAD_AT` with the actor still up.
    let mut reached = false;
    for _ in 0..512 {
        frame(w, 0);
        if ctrl(w).panels.slide >= legaia_engine_vm::world_map_panel_actors::SOFT_RESET_RELOAD_AT {
            reached = true;
            break;
        }
    }
    if !reached {
        return Err("the white fade never reached the reload point".into());
    }
    if !ctrl(w).panels.is_active() {
        return Err("the soft reset retired itself - retail never returns here".into());
    }
    // The screen has no exit of its own, so the host's escape hatch has to.
    if !ctrl_mut(w).panels.dismiss() {
        return Err("the escape hatch did not dismiss the soft reset".into());
    }
    frame(w, 0);
    Ok(())
}

/// Rung 12. `FUN_801ED308` - the brightness fade / flash. It ramps, then
/// **parks** waiting on the flash counter; only the host's release lets it
/// ramp back down and retire. The park is the point: a run that never
/// releases it must stay parked.
fn rung12_fade_flash(w: &mut World) -> Result<(), String> {
    frame(w, PadButton::L1.mask());
    if ctrl(w).panels.kind != Some(PanelActorKind::FadeFlash) {
        return Err(format!("L1 installed {:?}", ctrl(w).panels.kind));
    }
    frame(w, 0);
    let mut parked = false;
    for _ in 0..512 {
        frame(w, 0);
        if ctrl(w).panels.phase == 3 {
            parked = true;
            break;
        }
    }
    if !parked {
        return Err("the brightness ramp never reached its park".into());
    }
    for _ in 0..64 {
        frame(w, 0);
    }
    if ctrl(w).panels.phase != 3 {
        return Err("the park did not hold without a release".into());
    }
    // A second L1 press is the port's release chord.
    tap(w, PadButton::L1.mask());
    let mut retired = false;
    for _ in 0..512 {
        frame(w, 0);
        if !ctrl(w).panels.is_active() {
            retired = true;
            break;
        }
    }
    if !retired {
        return Err("the released ramp-down never retired the actor".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

#[test]
fn w1d_world_map_render_ladder() {
    let mut w = overworld();
    type Rung = (&'static str, fn(&mut World) -> Result<(), String>);
    let rungs: [Rung; 12] = [
        ("quiet-overworld", rung1_quiet_overworld),
        ("map-display-fade", rung2_map_display_fade),
        ("top-view-toggle", rung3_top_view_toggle),
        ("screen-dim", rung4_screen_dim),
        ("top-view-camera", rung5_top_view_camera),
        ("leave-top-view", rung6_leave_top_view),
        ("sub-list-handoff", rung7_sub_list_handoff),
        ("flag-window", rung8_flag_window),
        ("text-box", rung9_text_box),
        ("fill-fade", rung10_fill_fade),
        ("soft-reset", rung11_soft_reset),
        ("fade-flash", rung12_fade_flash),
    ];

    let mut score = 0usize;
    let mut stall = None;
    for (name, rung) in rungs {
        match rung(&mut w) {
            Ok(()) => {
                score += 1;
                eprintln!("[rung {score}] {name}: cleared");
            }
            Err(why) => {
                stall = Some(format!("rung {} ({name}): {why}", score + 1));
                break;
            }
        }
    }
    eprintln!("w1d_world_map_render_ladder: score {score}/12");
    assert!(stall.is_none(), "{}", stall.unwrap());
}

// ---------------------------------------------------------------------------
// The world-map band's developer screen
// ---------------------------------------------------------------------------

/// `FUN_801EA9B0` - the dev-menu row-action dispatcher, which retail runs on
/// the list's **cancel** leg rather than on its confirm. Both hosts reach it
/// through the same `DevMenuSession::tick` (`window/dev_menu.rs` and
/// `play_dev_menu.rs`); this drives that shared tick with the packed pad word
/// the hosts convert to, because the browser surface exposes neither the
/// picker phase nor the draw gate the dispatcher's return feeds.
#[test]
fn w1d_dev_menu_cancel_runs_the_row_action_dispatcher() {
    use legaia_engine_core::dev_menu::{PACK_CIRCLE, PACK_DOWN};
    use legaia_engine_core::dev_menu_host::{DevMenuSession, DevPage};
    use legaia_engine_vm::world_map_overlay::ListPickerPhase;

    let mut s = DevMenuSession::new();
    let mut none: [&mut [u8]; 0] = [];
    s.tick(0, 0, &mut none);
    assert_eq!(s.page, DevPage::List);
    assert_eq!(
        s.list_phase,
        ListPickerPhase::Active,
        "the list holds the pad, so it runs retail's Active leg"
    );

    // The dispatcher is only consulted on cancel, and every in-range row
    // answers "no park" - the phase becomes the unwind, not a parked one.
    s.tick(PACK_CIRCLE, 0, &mut none);
    assert_eq!(s.list_phase, ListPickerPhase::CancelUnwind);
    let after_row0 = s.list_gate;

    // The gate factor is the dispatcher's own return and must not depend on
    // which row the cursor sits over, for every row the engine's list has.
    for _ in 0..DevMenuRowCount::ALL {
        s.tick(PACK_DOWN, 0, &mut none);
        s.tick(PACK_CIRCLE, 0, &mut none);
        assert_eq!(
            s.list_gate, after_row0,
            "row {} changed the dispatcher's gate factor",
            s.row
        );
        assert_eq!(s.list_phase, ListPickerPhase::CancelUnwind);
    }
}

/// The engine's dev-menu list length, named so the loop above reads as a
/// sweep over the rows rather than a magic count.
struct DevMenuRowCount;
impl DevMenuRowCount {
    const ALL: usize = legaia_engine_core::dev_menu_host::DevMenuRow::ALL.len();
}

/// `FUN_801E5A08` - the dev-menu EQUIP commit, and `FUN_801E5B4C`'s slot
/// resolution under it. Both hosts call `DevMenuSession::commit_equip_row`
/// from their dev-menu tick (`window/dev_menu.rs:115`,
/// `play_dev_menu.rs:119`); this drives that wrapper, because the bag and the
/// record it moves an item between are not on either host's read surface.
///
/// The two properties are the disassembly's, not the port's current shape: a
/// commit takes the item **out** of the bag and gives back whatever occupied
/// the destination slot, and a bag miss must leave both untouched.
#[test]
fn w1d_dev_menu_equip_commit_swaps_bag_and_slot() {
    use legaia_engine_core::dev_menu_host::{DevMenuSession, WorldEquipHost};
    use std::collections::HashMap;

    // Slot bits `0x00` -> destination slot 0 (`(bits & 0x60) >> 5 == 0`).
    let mut s = DevMenuSession::new();
    s.equip_item = 0x20;
    s.equip_slot_bits = 0x00;

    let mut record = vec![0u8; 0x414];
    let equip_base = {
        // Seat a different item in the destination slot so the give-back is
        // observable rather than a no-op zero.
        let mut probe = record.clone();
        let mut bag: HashMap<u8, u8> = HashMap::from([(0x20, 1)]);
        let mut host = WorldEquipHost {
            inventory: &mut bag,
            sfx: Vec::new(),
        };
        let out = s
            .commit_equip_row(&mut host, &mut probe, &[2, 2, 2, 2])
            .expect("an owned item must commit");
        // Find where the commit wrote, so the assertions below key on the
        // port's own record layout rather than a duplicated constant.
        let at = probe
            .iter()
            .zip(record.iter())
            .position(|(a, b)| a != b)
            .expect("the commit changed no record byte");
        assert_eq!(out.equipped, 0x20);
        at
    };

    let mut bag: HashMap<u8, u8> = HashMap::from([(0x20, 1)]);
    record[equip_base] = 0x11; // the outgoing item
    let mut host = WorldEquipHost {
        inventory: &mut bag,
        sfx: Vec::new(),
    };
    let out = s
        .commit_equip_row(&mut host, &mut record, &[2, 2, 2, 2])
        .expect("an owned item must commit");
    assert_eq!(out.equipped, 0x20);
    assert_eq!(record[equip_base], 0x20, "the slot took the staged item");
    assert_eq!(bag.get(&0x20), None, "the item left the bag");
    assert_eq!(
        bag.get(&0x11).copied(),
        Some(1),
        "the outgoing item came back to the bag"
    );

    // A bag miss must change nothing at all.
    let mut empty: HashMap<u8, u8> = HashMap::new();
    let before = record.clone();
    let mut host = WorldEquipHost {
        inventory: &mut empty,
        sfx: Vec::new(),
    };
    s.equip_item = 0x77;
    let missed = s.commit_equip_row(&mut host, &mut record, &[2, 2, 2, 2]);
    assert!(missed.is_none(), "an unowned item must not commit");
    assert_eq!(record, before, "a bag miss must not touch the record");
    assert!(empty.is_empty(), "a bag miss must not touch the bag");
}

/// `FUN_801E5B4C`'s aggregation half, on the path a running frame takes:
/// `World::tick` -> `tick_submode_screen` -> `HubPainter::EntryList` -> the
/// per-entry equipment sub-draw. Calling the painter directly would prove the
/// kernel computes, not that a session asks it to.
#[test]
fn w1d_submode_frame_loop_reaches_the_equipment_sub_panel() {
    use legaia_engine_core::actor_handler::ActorHandler;
    use legaia_engine_vm::baka_hub_actors::{HubDraw, HubPainter, slot};

    let mut w = World::new();
    w.man_load_actor_reset();
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some(),
        "every MAN load spawns the op-0x49 submode driver"
    );
    const ENTRY_LIST_WINDOW: usize = 3;
    assert_eq!(
        HubPainter::for_window(ENTRY_LIST_WINDOW),
        Some(HubPainter::EntryList)
    );
    w.active_party = vec![0, 1];
    w.open_field_submode_screen(slot::DRAW_TICK, Some(ENTRY_LIST_WINDOW));

    let mut painted = 0usize;
    for _ in 0..16 {
        w.tick();
        painted = w
            .submode_screen
            .draws()
            .iter()
            .filter(|d| matches!(d, HubDraw::EntrySubPanel(_)))
            .count();
        if painted > 0 {
            break;
        }
    }
    // Three stat rows per entry, label + current value each.
    assert_eq!(
        painted,
        2 * 3 * 2,
        "the frame loop never reached the entry list's sub-draw"
    );
}

/// The horizon emitter (`FUN_801D7EA0`) is the one world-map render kernel
/// no *pad* reaches, and the reason is data rather than plumbing.
///
/// `FUN_801D1344` arms the gate from three scene globals
/// (`_DAT_8007BCD0/_D4/_D8`) and skips the arm entirely while the first two
/// are zero. Nothing in the engine writes them: their retail source is the
/// field VM, which stores each from a script operand (`sw v0,-0x4330(v1)` at
/// `0x801E1638` and its two siblings, plus a ramp arm each through the
/// `0x801E205C` epilogue). Until those register ids are routed, a world map
/// the engine loads leaves the gate shut every frame.
///
/// **Read the entry this test produces carefully.** The params below are the
/// test's, not the disc's, so the coverage it contributes for `FUN_801D8258`
/// and `FUN_801D7EA0` proves the *chain* - `World::tick` -> `tick_world_map`
/// -> the arm -> `run_horizon_emitter` -> the emitter body - and says nothing
/// about a playthrough reaching it. Those two addresses stay on the reach
/// worklist as gated on a scene whose script arms them; what is closed is the
/// older reading that the port had no arming call site at all.
#[test]
fn w1d_horizon_gate_is_data_blocked_not_plumbing_blocked() {
    let mut w = overworld();
    for _ in 0..8 {
        frame(&mut w, 0);
    }
    assert!(
        ctrl(&w).horizon.is_none(),
        "the world-map tick must not emit horizon bands from a zero gate"
    );
    assert!(!ctrl(&w).emitter_gate.armed);

    // The plumbing above the data: with the globals set, the same chain -
    // World::tick -> tick_world_map -> arm -> run_horizon_emitter -> the
    // emitter body - runs on the very next frame.
    ctrl_mut(&mut w).horizon_params = (0x500, 0x10, 4);
    frame(&mut w, 0);
    let batch = ctrl(&w)
        .horizon
        .as_ref()
        .expect("armed params must reach the emitter through the world tick");
    assert_eq!(batch.bands.len(), 224, "retail's 224-scanline band loop");
    assert_eq!(batch.ot_layer, 4, "the staged OT layer carries through");
    assert_eq!(batch.prim_count(), 224 * 4);
}
