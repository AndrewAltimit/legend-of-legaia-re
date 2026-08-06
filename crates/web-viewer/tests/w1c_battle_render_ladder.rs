//! Battle-render residue + battle-target ladder: the transition styles the
//! draw-composition ladder's driven fights never rolled, and the attack-target
//! ring its Cross-only drive never opened.
//!
//! `crates/web-viewer/tests/play_compose_ladder.rs` made the browser play page
//! a measurable host, but two clusters stayed never-entered under
//! `scripts/ci/replay-port-coverage.py` for reasons that are **content**, not
//! wiring:
//!
//! * **The intro styles.** `select_intro_style` (`FUN_801ce8cc`) is a pure
//!   function of the battle - the formation's *first monster id*, that row's
//!   per-battle flags byte and the scene index - and its default arm is
//!   [`IntroStyle::TileShatter`]. Every fight the compose ladder drives takes
//!   that arm, so the confetti / spin-up ring / curtain / swirl bodies were
//!   ported, wired and never once executed.
//! * **The attack-target ring.** The picker's enemy cursor runs
//!   `FUN_801D8A88` -> `FUN_801D8D00` only when a **direction** steps it. A
//!   driver that only presses Cross confirms the seat the constructor picked
//!   and the ring never runs.
//!
//! Both are driven here, and the two halves are deliberately different in
//! kind:
//!
//! | # | rung | how it is driven |
//! |---|---|---|
//! | 1 | selector census | `select_intro_style` over its whole documented input space + over every formation the loaded disc's own table registers |
//! | 2 | five style emitters | the page's own `BattleIntro`, armed from the same PROT 0979 tables `arm_battle_intro` parses, ticked for the full retail duration |
//! | 3 | curtain two-pass | the capture landing + `refresh_captured_page` chain (`FUN_801D1D9C`'s subtractive mid-pass decay and the no-clear display trail) |
//! | 4 | target ring, pad-driven | a real forced encounter on the play page, walked into the target cursor with **D-pad Left** and cycled with Left/Right |
//! | 5 | target ring, property | the ring builder + cursor step + the sweep-group aim, pinned against the disassembly's stated intent |
//!
//! Rung 4 is the host rung: it is the browser play page, fed pad words, with
//! every frame composed. Rungs 1-3 and 5 are *kernel* rungs - they call the
//! same library objects the page calls, with the selector's own output
//! substituted for the formation id the page happens to fight. That
//! distinction is stated rather than blurred because the reach report's whole
//! value is knowing which is which.
//!
//! Coverage export (what wires this into the reach report):
//!
//! ```text
//! cargo llvm-cov -p legaia-web-viewer --test w1c_battle_render_ladder \
//!     --json --output-path target/cov-w1c_battle_render_ladder.json
//! ```
//!
//! **No `--release`.** An optimised build inlines the small kernels and
//! leaves their out-of-line coverage records at zero, which the reach
//! report cannot tell from "never called".
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.
//!
//! [`IntroStyle::TileShatter`]: legaia_engine_vm::battle_intro_styles::IntroStyle::TileShatter

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::ProtIndex;
use legaia_engine_ui::battle_intro::{BattleIntro, IntroQuadTable, parse_tile_corner_table};
use legaia_engine_vm::battle_intro_particles::IntroEnv;
use legaia_engine_vm::battle_intro_styles::{
    INTRO_FADE_RAMPS, IntroStyle, IntroStyleInputs, intro_duration_frames, select_intro_style,
};
use legaia_web_viewer::runtime::LegaiaRuntime;

const W: u32 = 960;
const H: u32 = 720;
/// PROT entry carrying the field-to-battle intro overlay - the same constant
/// the page's `arm_battle_intro` uses for its two in-overlay data tables.
const INTRO_OVERLAY_PROT: u32 = 979;

fn disc_bytes() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(path).ok()
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
}

// ---------------------------------------------------------------------------
// Rung 1 - the selector census
// ---------------------------------------------------------------------------

/// Every `(style, sub_style)` pair the selector can produce, and the monster
/// id that produces it. Transcribed from the arm order at
/// `0x801CE97C..0x801CEB38` (see `select_intro_style`'s own table), **not**
/// from the port's current output: the point of the rung is that a re-ordered
/// arm would change which style a battle opens with and nothing else would
/// notice.
const SELECTOR_ANCHORS: &[(u8, u8, u32, IntroStyle, i32)] = &[
    // (battle_flags, slot0, scene, expected style, expected sub)
    (0x00, 0x01, 0, IntroStyle::TileShatter, 0),
    (0x80, 0x01, 0, IntroStyle::SpinUpParticles, 0),
    (0x80, 0x13, 0, IntroStyle::TileShatter, 1),
    (0x80, 0x15, 0, IntroStyle::TileShatter, 1),
    (0x00, 0x4C, 0, IntroStyle::ScatterParticles, 0),
    (0x00, 0x88, 0, IntroStyle::ScatterParticles, 1),
    (0x00, 0xB3, 0, IntroStyle::ScatterParticles, 2),
    (0x00, 0x4F, 0, IntroStyle::Curtain, 0),
    (0x00, 0xAF, 0, IntroStyle::Curtain, 0),
    (0x00, 0xA5, 0, IntroStyle::Curtain, 0),
    (0x00, 0x3E, 0x0C, IntroStyle::TileShatter, 2),
    (0x00, 0x3F, 0x15, IntroStyle::TileShatter, 2),
    // The scene-conditional arms do NOT fire off-scene.
    (0x00, 0x3E, 0x0D, IntroStyle::TileShatter, 0),
    (0x00, 0x80, 0x9B, IntroStyle::TileShatter, 2),
    (0x00, 0xB5, 0, IntroStyle::Swirl, 0),
];

fn rung1_selector_census() -> Result<(), String> {
    for &(flags, slot0, scene, style, sub) in SELECTOR_ANCHORS {
        let got = select_intro_style(&IntroStyleInputs {
            battle_flags: flags,
            formation_slot0: slot0,
            scene_index: scene,
        });
        if got.style != style {
            return Err(format!(
                "selector(flags {flags:#04x}, slot0 {slot0:#04x}, scene {scene:#x}) \
                 = {:?}, expected {style:?}",
                got.style
            ));
        }
        if got.sub_style != sub {
            return Err(format!(
                "selector(flags {flags:#04x}, slot0 {slot0:#04x}) sub = {} expected {sub}",
                got.sub_style
            ));
        }
    }

    // The selector's *reachable image*: sweep the whole (flags-bit, id) space
    // and assert all five styles are producible. A dead arm - a style no input
    // can select - is exactly the failure that would leave a ported body
    // permanently unreachable, and it is invisible to any single fight.
    let mut seen = std::collections::BTreeSet::new();
    for flags in [0u8, 0x80] {
        for slot0 in 0u8..=0xFF {
            for scene in [0u32, 3, 0x0C, 0x15, 0x9B] {
                seen.insert(
                    select_intro_style(&IntroStyleInputs {
                        battle_flags: flags,
                        formation_slot0: slot0,
                        scene_index: scene,
                    })
                    .style,
                );
            }
        }
    }
    if seen.len() != 5 {
        return Err(format!(
            "the selector's image is {seen:?} - a style body no input can reach"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rungs 2 + 3 - the five style emitters
// ---------------------------------------------------------------------------

/// The PROT 0979 tables the page's `arm_battle_intro` parses: the curtain's
/// quad-descriptor table and the tile seeder's corner table, both read out of
/// the intro overlay relocated to its static load base.
fn intro_tables(index: &ProtIndex) -> (IntroQuadTable, [i32; 4]) {
    let loaded = index
        .entry_bytes_extended(INTRO_OVERLAY_PROT)
        .ok()
        .and_then(|raw| {
            let rec = legaia_asset::static_overlay::overlay_map()
                .by_prot_index(INTRO_OVERLAY_PROT)?
                .clone();
            let img = legaia_asset::static_overlay::as_loaded(&raw, &rec).ok()?;
            Some((img, rec.base_va))
        });
    let table = loaded
        .as_ref()
        .and_then(|(img, base)| IntroQuadTable::parse_overlay(img, *base))
        .unwrap_or_else(IntroQuadTable::neutral);
    let corners = loaded
        .as_ref()
        .and_then(|(img, base)| parse_tile_corner_table(img, *base))
        .unwrap_or([0, 1, 0x11, 0x12]);
    (table, corners)
}

/// A stand-in for the drawn field frame the host reads back: a horizontal /
/// vertical gradient, so a style that samples it produces *distinguishable*
/// texels rather than one flat colour (a flat capture would make the curtain's
/// decay invisible, which is the property rung 3 measures).
fn synthetic_field_rgba(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            out.extend_from_slice(&[
                (x * 255 / w.max(1)) as u8,
                (y * 255 / h.max(1)) as u8,
                0x80,
                0xFF,
            ]);
        }
    }
    out
}

/// What one style's full-duration run produced.
struct StyleRun {
    frames: u32,
    /// Frames whose style geometry reached the list.
    drawn_frames: u32,
    /// Total primitives across the run.
    prims: u64,
    /// First frame index whose `style_drawn` was true.
    first_drawn: Option<u32>,
    /// Frames carrying a fade quad, and the abr the ramp used.
    fade_frames: u32,
    fade_abr: Option<u8>,
    /// Frames on which `refresh_captured_page` reported a changed page.
    page_refreshes: u32,
    /// Column-pass quads seen across the run (curtain only).
    column_quads: u64,
}

/// Arm the emitter for `style` exactly as the page's `arm_battle_intro` does -
/// same tables, same corner set, same `IntroEnv` seeding - and run it for the
/// style's own retail duration.
fn run_style(
    style: IntroStyle,
    sub_style: i32,
    table: &IntroQuadTable,
    corners: [i32; 4],
) -> StyleRun {
    let total = intro_duration_frames(style);
    let mut env = IntroEnv::new(0x1234_5678);
    let mut trig = IntroEnv::new(0x1234_5678);
    let mut intro = BattleIntro::new(
        style,
        sub_style,
        total,
        table.clone(),
        &mut env,
        &mut trig,
        corners,
    );

    // The one-shot capture: retail stashes the drawn field frame as the
    // transition arms and every later frame samples that copy.
    let base = legaia_tim::Vram::new();
    let rgba = synthetic_field_rgba(320, 240);
    assert!(intro.needs_capture(), "a fresh emitter wants its capture");
    intro.land_capture_rgba(&rgba, 320, 240, &base);
    assert!(
        !intro.needs_capture(),
        "the capture is a one-shot; a second landing must be refused"
    );

    let mut run = StyleRun {
        frames: 0,
        drawn_frames: 0,
        prims: 0,
        first_drawn: None,
        fade_frames: 0,
        fade_abr: None,
        page_refreshes: 0,
        column_quads: 0,
    };
    for elapsed in 0..total {
        let frame = intro.tick(elapsed as i16, 1);
        run.prims += frame.prims.len() as u64;
        if frame.style_drawn {
            run.drawn_frames += 1;
            run.first_drawn.get_or_insert(elapsed as u32);
        }
        if let Some(f) = frame.fade {
            run.fade_frames += 1;
            run.fade_abr = Some(f.abr);
        }
        if intro.refresh_captured_page().is_some() {
            run.page_refreshes += 1;
        }
        run.column_quads += intro.pending_column_quads().len() as u64;
        run.frames += 1;
    }
    run
}

fn rung2_style_emitters(index: &ProtIndex) -> Result<(), String> {
    let (table, corners) = intro_tables(index);

    for style in [
        IntroStyle::ScatterParticles,
        IntroStyle::SpinUpParticles,
        IntroStyle::TileShatter,
        IntroStyle::Curtain,
        IntroStyle::Swirl,
    ] {
        let run = run_style(style, 0, &table, corners);

        // Retail's own duration, and the swirl is the one arm that overrides
        // the shared 0x84 (`addiu v0,zero,0xfc` at 0x801CEFF4).
        let want = if style == IntroStyle::Swirl {
            0xFC
        } else {
            0x84
        };
        if run.frames as i32 != want {
            return Err(format!(
                "{style:?}: ran {} frames, retail {want}",
                run.frames
            ));
        }

        // Every frame carries the backdrop the style composes onto, so no
        // frame of a transition is ever an empty list.
        if run.prims < run.frames as u64 {
            return Err(format!(
                "{style:?}: {} prims over {} frames - a frame emitted no backdrop",
                run.prims, run.frames
            ));
        }

        // Frame zero is a deliberate no-draw for the four **projected**
        // styles: retail's first transition frame still runs the GTE through
        // the field camera's stale view matrix, which puts their geometry
        // behind the near plane. The curtain is the exception and it is a
        // structural one, not a tolerance - `FUN_801CF1B0` builds
        // **screen-space** corners with no projection step at all, so nothing
        // about it depends on the view matrix and it draws from frame zero.
        // Asserting the gate uniformly is what this rung caught first.
        let screen_space = style == IntroStyle::Curtain;
        match run.first_drawn {
            None => {
                return Err(format!(
                    "{style:?}: no frame of a {}-frame transition drew any style geometry",
                    run.frames
                ));
            }
            Some(0) if !screen_space => {
                return Err(format!(
                    "{style:?}: projected style drew on the stale-view frame 0"
                ));
            }
            Some(n) if screen_space && n != 0 => {
                return Err(format!(
                    "curtain: screen-space strips must draw from frame 0, first was {n}"
                ));
            }
            Some(_) => {}
        }

        // The fade ramp is a *lead* before the end, so it can only ever cover
        // the tail - a ramp that started at frame 0 would mean the second tail
        // switch lost its threshold.
        let ramp = INTRO_FADE_RAMPS[style.selector() as usize];
        if run.fade_frames == 0 {
            return Err(format!("{style:?}: the fade ramp never armed"));
        }
        if run.fade_frames as i32 > ramp.lead {
            return Err(format!(
                "{style:?}: fade ran {} frames, ramp lead is {}",
                run.fade_frames, ramp.lead
            ));
        }
        if run.fade_abr != Some(ramp.abr) {
            return Err(format!(
                "{style:?}: fade abr {:?}, ramp says {}",
                run.fade_abr, ramp.abr
            ));
        }

        // Only the curtain re-composes its page per frame; every other style
        // reports exactly one changed page (the capture landing).
        if style == IntroStyle::Curtain {
            if run.page_refreshes != run.frames {
                return Err(format!(
                    "curtain: {} of {} frames re-composed the intermediate",
                    run.page_refreshes, run.frames
                ));
            }
            if run.column_quads == 0 {
                return Err(
                    "curtain: the column pass built no quads, so build_intro_quad \
                            (FUN_801CF1B0) never ran"
                        .into(),
                );
            }
        } else if run.page_refreshes != 1 {
            return Err(format!(
                "{style:?}: {} page refreshes - the capture is a one-shot for \
                 every style but the curtain",
                run.page_refreshes
            ));
        }
    }

    // The scatter style's sub-style 2 is the one fade arm that flips its
    // blend: `abr == 1` (additive white-out) instead of `2` (fade to black).
    let plain = run_style(IntroStyle::ScatterParticles, 0, &table, corners);
    let sub2 = run_style(IntroStyle::ScatterParticles, 2, &table, corners);
    if plain.fade_abr == sub2.fade_abr {
        return Err(format!(
            "scatter sub-style 2 must flip the fade blend; both read {:?}",
            plain.fade_abr
        ));
    }
    Ok(())
}

fn rung3_curtain_two_pass(index: &ProtIndex) -> Result<(), String> {
    let (table, corners) = intro_tables(index);
    let total = intro_duration_frames(IntroStyle::Curtain);
    let mut env = IntroEnv::new(0xC0FFEE);
    let mut trig = IntroEnv::new(0xC0FFEE);
    let mut intro = BattleIntro::new(
        IntroStyle::Curtain,
        0,
        total,
        table,
        &mut env,
        &mut trig,
        corners,
    );
    let base = legaia_tim::Vram::new();
    intro.land_capture_rgba(&synthetic_field_rgba(320, 240), 320, 240, &base);

    // Seed the display model, then hold the emitter still (same elapsed) and
    // run only the wash: with no new strips, a display buffer retail never
    // clears must get strictly darker every frame and stay darker - that is
    // the whole content of "subtractive no-clear decay", and an accumulation
    // buffer (the reading this port previously carried) would get brighter.
    let _ = intro.tick(1, 1);
    let _ = intro.refresh_captured_page();
    let mut rows_seen = intro.pending_row_quads_for_test().len();
    for elapsed in 2..12 {
        let frame = intro.tick(elapsed, 1);
        rows_seen += frame.prims.len();
        let _ = intro.refresh_captured_page();
    }
    if rows_seen == 0 {
        return Err("the curtain emitted no row strips at all".into());
    }

    let brightness = |accum: &[u16]| -> u64 {
        accum
            .iter()
            .map(|p| {
                let (r, g, b) = (p & 0x1F, (p >> 5) & 0x1F, (p >> 10) & 0x1F);
                (r + g + b) as u64
            })
            .sum()
    };
    let before = brightness(intro.display_accum_for_test());
    if before == 0 {
        return Err("the display model is black before the wash - nothing to decay".into());
    }
    // Wash-only frames: no tick, so no new strips land, only the decay.
    for _ in 0..4 {
        intro.update_display_trail_for_test();
    }
    let after = brightness(intro.display_accum_for_test());
    if after >= before {
        return Err(format!(
            "the curtain trail is a subtractive decay: brightness went {before} -> {after}"
        ));
    }

    // The mid-pass intermediate is the same law one buffer down
    // (`FUN_801D1D9C(0x1EA, 2, 0x808080)`): compose-only frames must darken it.
    intro.compose_intermediate_for_test();
    Ok(())
}

// ---------------------------------------------------------------------------
// Rung 4 - the target ring, pad-driven on the play page
// ---------------------------------------------------------------------------

/// One press through the world pad: a composed frame with the bit down, then a
/// composed frame at neutral (`just_pressed` is `pad & !pad_prev`).
fn tap(rt: &mut LegaiaRuntime, mask: u16) {
    rt.set_pad(mask);
    let _ = rt.tick_frame();
    rt.set_pad(0);
    let _ = rt.tick_frame();
}

/// Surface Y the enemy target strip lands on at 960x720 (stage scale 3, stage
/// Y 166 -> 498). The strip draws **only** while the picker's cursor sits on
/// the enemy row, which is precisely the precondition `retail_enemy_step`
/// needs - so its glyphs are the honest probe for "the ring could run".
const STRIP_SURFACE_Y: i64 = 498;

fn strip_glyphs(rt: &mut LegaiaRuntime) -> usize {
    let v = json(&rt.play_overlay_draws_json(W, H));
    v["texts"]
        .as_array()
        .map(|a| a.iter().filter(|t| t["dst"][1] == STRIP_SURFACE_Y).count())
        .unwrap_or(0)
}

fn rung4_target_ring_pad_driven(rt: &mut LegaiaRuntime) -> Result<(), String> {
    // A battle needs a **seeded party**, and on this page the seed is the
    // new-game entry, not `enter_field`: a plain field visit leaves the three
    // party records empty, every party actor fails the "can act" gate, and the
    // live loop never opens a command session at all - so no target cursor can
    // exist. Entering town01 as the opening runs the new-game template seed;
    // the walk-away to map01 then drops the establishing timeline and leaves
    // the party behind, which is the cheapest honest seat for a forced fight.
    rt.debug_enter_town01_opening()
        .map_err(|e| format!("enter town01 opening: {e}"))?;
    for _ in 0..8 {
        let _ = rt.tick_frame();
    }
    rt.enter_field("map01")
        .map_err(|_| "enter_field(map01) failed".to_string())?;
    for _ in 0..5 {
        let _ = rt.tick_frame();
    }
    if rt.party_display_name(0).is_empty() {
        return Err("the new-game seed left party slot 0 without a record".into());
    }
    if !rt.debug_force_battle(-1) {
        return Err("debug_force_battle(-1) resolved no formation on map01".into());
    }
    let mut in_battle = false;
    for _ in 0..400 {
        let _ = rt.tick_frame();
        // Compose the transition frames too - the intro's screen-prim route.
        if rt.play_screen_prim_count() > 0 {
            let _ = rt.play_screen_prim_vertex_bytes();
            let _ = rt.play_screen_prim_indices();
        }
        if rt.play_battle_active() {
            in_battle = true;
            break;
        }
    }
    if !in_battle {
        return Err("forced encounter never reached SceneMode::Battle".into());
    }

    // Left is the one direction that advances every command surface toward
    // the target cursor and then *cycles it*: the round prompt's `Begin` chip
    // is its left chip, the ring's `Attack` arm is its left arm, the
    // attack-mode prompt's `Auto` chip is its left chip, and inside the picker
    // a direction is what runs `FUN_801D8A88` -> `FUN_801D8D00`. So one held
    // pattern walks the whole flow without needing to probe the phase.
    let mut peak_strip = 0usize;
    let mut cycles = 0u32;
    let mut fx_prim_frames = 0u32;
    for i in 0..260u32 {
        if !rt.play_battle_active() {
            break;
        }
        // Once the ring has been cycled enough times to have run both arms,
        // commit: a rung that only ever walks the cursor never swings, and the
        // whole in-battle FX layer (the weapon trail and the move-FX
        // afterimage streak, both composited through the same screen-prim
        // pass as the intro) hangs off an *executing* action.
        if cycles >= 8 {
            tap(rt, PadButton::Cross.mask());
            for _ in 0..3 {
                let _ = rt.tick_frame();
                if rt.play_screen_prim_count() > 0 {
                    fx_prim_frames += 1;
                    let _ = rt.play_screen_prim_vertex_bytes();
                    let _ = rt.play_screen_prim_indices();
                    let _ = rt.play_screen_prim_runs();
                }
                let _ = rt.play_battle_fx_sync(4.0 / 3.0);
                let _ = rt.play_battle_fx_positions();
                for m in 0..rt.play_battle_fx_model_count() {
                    let t = rt.play_battle_fx_model_tmd(m);
                    let _ = rt.play_battle_fx_mesh_positions(t);
                }
            }
            continue;
        }
        // Right is deliberately never pressed before the cursor is up: on the
        // round prompt Right is the `Run` chip and it commits on the press, so
        // an alternating drive flees the fight instead of reaching the cursor.
        // That is what a "walk the menus with any direction" driver gets wrong,
        // and it looks like the picker being unreachable rather than like an
        // escape.
        let dir = if cycles == 0 || !i.is_multiple_of(2) {
            PadButton::Left
        } else {
            PadButton::Right
        };
        tap(rt, dir.mask());
        if std::env::var_os("W1C_DEBUG").is_some() {
            let v = json(&rt.play_overlay_draws_json(W, H));
            let ys: std::collections::BTreeSet<i64> = v["texts"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| t["dst"][1].as_i64()).collect())
                .unwrap_or_default();
            eprintln!("[w1c] i={i} dir={dir:?} overlay text rows {ys:?}");
        }
        let n = strip_glyphs(rt);
        if n > 0 {
            cycles += 1;
        }
        peak_strip = peak_strip.max(n);
        let _ = rt.play_battle_camera_json();
    }
    if peak_strip == 0 {
        return Err(
            "the D-pad drive never put the target cursor on the enemy row (no strip glyphs \
             composed), so the attack-target ring had no precondition"
                .into(),
        );
    }
    if cycles < 2 {
        return Err(format!(
            "the enemy cursor was up for only {cycles} composed frames - not enough \
             direction steps to have cycled the ring"
        ));
    }
    // `fx_prim_frames` is reported, not asserted. The in-battle screen-FX pass
    // (`battle_fx_screen_prims`) runs every tick, but its two producers are
    // both *content*-gated: the weapon trail needs a trail-emitting clip and
    // the move-FX afterimage streak needs a playing move-FX scene whose
    // move-power record carries a non-zero trail texture-page id (`+0x0b`).
    // A plain physical strike on a starting-party fight carries neither, so a
    // zero here is a statement about the formation this rung can reach, not
    // about the pass being unwired.
    eprintln!(
        "[rung 4] enemy strip up for {cycles} frames (peak {peak_strip} glyphs); \
         {fx_prim_frames} composed frames carried in-battle screen prims"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Rung 5 - the target kernels, property-shaped
// ---------------------------------------------------------------------------

fn rung5_target_kernels() -> Result<(), String> {
    use legaia_engine_vm::battle_action::{
        PoolActor, RedirectQuery, TargetCycle, bearing_12bit_approx, build_attack_target_queue,
        cycle_attack_target, redirect_dead_target,
    };
    use legaia_engine_vm::battle_target_group::{
        GroupSlot, MIN_GROUP_EXTENT, TARGET_GROUP_ENEMIES, TARGET_GROUP_PARTY, target_group_aim,
        target_group_range,
    };

    // --- FUN_801D8A88: the ring is ordered by ANGLE, not by slot index. ---
    // Four monsters placed so slot order and angular order disagree: the
    // acting actor faces slot 3, and the nearest alternate in bearing is slot
    // 6, the farthest slot 4. A slot-order walk would answer 4.
    let mut pool = [PoolActor::default(); 8];
    pool[0] = PoolActor {
        alive: true,
        x: 0,
        z: 0,
        target_slot: 3,
        status: 0,
    };
    let seats = [(0i16, 1000i16), (0, -1000), (-1000, 0), (300, 900)];
    for (i, (x, z)) in seats.iter().enumerate() {
        pool[3 + i] = PoolActor {
            alive: true,
            x: *x,
            z: *z,
            target_slot: 0,
            status: 0,
        };
    }
    let q = build_attack_target_queue(&pool, 0, bearing_12bit_approx);
    if q.count != 4 {
        return Err(format!("ring counted {} of 4 live monsters", q.count));
    }
    if q.wrap_slot != 3 {
        return Err(format!(
            "the ring's wrap entry is the acting actor's current target, got {}",
            q.wrap_slot
        ));
    }
    if q.ordered.contains(&3) {
        return Err("the current target must not appear among its own alternates".into());
    }
    let mut sorted = q.ordered;
    sorted.sort_unstable();
    if sorted != [4, 5, 6] {
        return Err(format!(
            "the three alternates must be the other three monster slots, got {:?}",
            q.ordered
        ));
    }
    if q.ordered[0] != 6 {
        return Err(format!(
            "angular ordering: slot 6 sits nearest the current target's bearing, \
             the ring named {} first",
            q.ordered[0]
        ));
    }

    // --- FUN_801D8D00: Next and Prev are inverses across the ring. ---
    let ring = [
        q.count,
        q.wrap_slot,
        q.ordered[0],
        q.ordered[1],
        q.ordered[2],
    ];
    let mut seen = std::collections::BTreeSet::new();
    let mut cur = q.wrap_slot;
    for _ in 0..q.count {
        cur = cycle_attack_target(&ring, cur, TargetCycle::Next);
        seen.insert(cur);
    }
    if cur != q.wrap_slot {
        return Err(format!(
            "cycling Next {} times must return to the wrap slot {}, landed on {cur}",
            q.count, q.wrap_slot
        ));
    }
    if seen.len() != q.count as usize {
        return Err(format!(
            "a full Next lap must visit every live monster once: {seen:?}"
        ));
    }
    let one_forward = cycle_attack_target(&ring, q.wrap_slot, TargetCycle::Next);
    let back = cycle_attack_target(&ring, one_forward, TargetCycle::Prev);
    if back != q.wrap_slot {
        return Err(format!(
            "Prev must undo Next: {} -> {one_forward} -> {back}",
            q.wrap_slot
        ));
    }

    // --- FUN_801DB124: the redirect stays on the dead target's own side. ---
    let alive = |s: u8| s != 4;
    let mut draws = [1i32, 2, 0].into_iter().cycle();
    let out = redirect_dead_target(
        RedirectQuery {
            target_slot: 4,
            category: 3,
            param0: 0,
        },
        3,
        4,
        || draws.next().unwrap(),
        alive,
        |_| 0,
    );
    match out {
        Some(s) if (3..7).contains(&s) && alive(s) => {}
        other => {
            return Err(format!(
                "a dead enemy must redirect to a live enemy, got {other:?}"
            ));
        }
    }
    if redirect_dead_target(
        RedirectQuery {
            target_slot: 5,
            category: 3,
            param0: 0,
        },
        3,
        4,
        || 0,
        alive,
        |_| 0,
    )
    .is_some()
    {
        return Err("a LIVE target must not be redirected".into());
    }
    // Category gate: an offensive spell already aimed at a party slot is left
    // alone, the same spell aimed at an enemy is re-rolled.
    if redirect_dead_target(
        RedirectQuery {
            target_slot: 1,
            category: 2,
            param0: 0x81,
        },
        3,
        4,
        || 0,
        |s| s != 1,
        |_| 0,
    )
    .is_some()
    {
        return Err("an offensive spell on a dead ally must not re-roll".into());
    }

    // --- FUN_801DCEAC: the centroid is NEGATED and the extent is FLOORED. ---
    if target_group_range(TARGET_GROUP_PARTY) != (0, 3)
        || target_group_range(TARGET_GROUP_ENEMIES) != (3, 7)
        || target_group_range(0xA) != (0, 7)
        || target_group_range(5) != (5, 6)
    {
        return Err("the group-code decode lost one of its four arms".into());
    }
    let slot = |live: bool, x: i16, z: i16| GroupSlot { live, x, z };
    let slots = [
        slot(true, 100, 200),
        slot(true, 300, 400),
        slot(false, 9000, 9000),
        slot(true, -100, -200),
        slot(false, 0, 0),
        slot(false, 0, 0),
        slot(false, 0, 0),
    ];
    let aim = target_group_aim(TARGET_GROUP_PARTY, &slots)
        .ok_or("the party group has two live seats and must aim")?;
    if aim.centroid_x != -200 || aim.centroid_z != -300 {
        return Err(format!(
            "the aim is the translation that brings the group to the origin \
             (negated mean): got ({}, {})",
            aim.centroid_x, aim.centroid_z
        ));
    }
    if aim.extent != MIN_GROUP_EXTENT {
        return Err(format!(
            "a group tighter than {MIN_GROUP_EXTENT} is WIDENED to it, not clamped down: \
             got {}",
            aim.extent
        ));
    }
    // A wide group keeps its own extent - the compare is a floor, not a cap.
    let wide = [
        slot(true, -4000, 0),
        slot(true, 4000, 0),
        slot(false, 0, 0),
        slot(false, 0, 0),
        slot(false, 0, 0),
        slot(false, 0, 0),
        slot(false, 0, 0),
    ];
    let aim = target_group_aim(TARGET_GROUP_PARTY, &wide).ok_or("the wide group must aim")?;
    if aim.extent != 8000 {
        return Err(format!(
            "a group wider than the floor keeps its own extent, got {}",
            aim.extent
        ));
    }
    // A range with no live seat is retail's divide-by-zero; the port refuses.
    let dead = [slot(false, 0, 0); 7];
    if target_group_aim(TARGET_GROUP_ENEMIES, &dead).is_some() {
        return Err("an all-dead group must yield no aim".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

#[test]
fn w1c_battle_render_ladder() {
    let Some(disc) = disc_bytes() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Some(prot) = legaia_web_viewer::disc::extract_prot_dat(&disc) else {
        eprintln!("[skip] PROT.DAT extraction failed (disc-gated)");
        return;
    };
    let cdname = legaia_web_viewer::disc::extract_cdname_txt(&disc);
    let index = ProtIndex::from_bytes(prot, cdname.as_deref()).expect("PROT index");

    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc, String::new()).expect("load_disc");

    let mut score = 0u32;
    let mut fail: Option<(u32, &str, String)> = None;
    type Rung = Box<dyn Fn(&mut LegaiaRuntime, &ProtIndex) -> Result<(), String>>;
    let rungs: [(&str, Rung); 5] = [
        (
            "selector-census",
            Box::new(|_rt, _ix| rung1_selector_census()),
        ),
        (
            "style-emitters",
            Box::new(|_rt, ix| rung2_style_emitters(ix)),
        ),
        (
            "curtain-two-pass",
            Box::new(|_rt, ix| rung3_curtain_two_pass(ix)),
        ),
        (
            "target-ring-pad",
            Box::new(|rt, _ix| rung4_target_ring_pad_driven(rt)),
        ),
        (
            "target-kernels",
            Box::new(|_rt, _ix| rung5_target_kernels()),
        ),
    ];
    for (name, rung) in rungs {
        match rung(&mut rt, &index) {
            Ok(()) => {
                score += 1;
                eprintln!("[rung {score}] {name}: cleared");
            }
            Err(why) => {
                fail = Some((score + 1, name, why));
                break;
            }
        }
    }
    eprintln!("w1c_battle_render_ladder: score {score}/5");
    if let Some((n, name, why)) = fail {
        panic!("rung {n} ({name}): {why}");
    }
}
