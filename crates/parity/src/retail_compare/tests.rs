//! Unit tests for the retail comparison corpus.

use super::*;
use legaia_engine_menus::field_menu::FieldMenuRow;
use legaia_engine_system::input::PadButton;

fn ram_with_subscreen(sub: u8) -> Vec<u8> {
    let mut ram = vec![0u8; 0x20_0000];
    ram[(MENU_SUBSCREEN & 0x1F_FFFF) as usize] = sub;
    ram
}

/// The capture-alignment seeds survive their env forms.
#[test]
fn morph_weights_rewind_their_own_ramp_by_the_display_lag() {
    // Rising lane (not peaked, not finishing): back by up * step * frames.
    let mut w = [0x0D26u16];
    rewind_morph_weights(&mut w, &[51], &[51], 0, 0x4415, 3, 2);
    assert_eq!(w, [0x0D26 - 51 * 6]);
    // Clamped at zero.
    let mut w = [100u16];
    rewind_morph_weights(&mut w, &[81], &[58], 0, 0x1015, 3, 2);
    assert_eq!(w, [0]);
    // Draining lane (peaked + finishing): back *up* by down * step * frames.
    let mut w = [0x0800u16];
    rewind_morph_weights(&mut w, &[81], &[58], 0x8000_0001, 0x1000, 3, 2);
    assert_eq!(w, [0x0800 + 58 * 6]);
    // HOLD keeps a peaked lane; FROZEN keeps everything.
    let mut w = [0x1000u16];
    rewind_morph_weights(&mut w, &[81], &[58], 0x8000_0001, 0x0400, 3, 2);
    assert_eq!(w, [0x1000]);
    let mut w = [0x0400u16];
    rewind_morph_weights(&mut w, &[81], &[58], 0, 0x8000, 3, 2);
    assert_eq!(w, [0x0400]);
    // A later lane waits for the one before it to peak.
    let mut w = [0x0400u16, 0x0200];
    rewind_morph_weights(&mut w, &[10, 10], &[0, 0], 0, 0, 1, 2);
    assert_eq!(w, [0x0400 - 20, 0x0200]);
}

fn chans(kv: &[(&str, f64)]) -> BTreeMap<String, f64> {
    kv.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

/// A re-measured state takes the run's channel set (a retired channel
/// leaves the ratchet), keeps the image score a display-less run did not
/// take, and a state outside the run is untouched.
#[test]
fn a_bless_replaces_a_measured_states_channel_set() {
    let mut prior = Baseline::default();
    prior.states.insert(
        "arrival".into(),
        chans(&[("camera", 0.9), ("footing", 0.4), ("image", 0.7)]),
    );
    prior
        .states
        .insert("elsewhere".into(), chans(&[("camera", 0.5)]));
    let mut run = Baseline::default();
    run.states
        .insert("arrival".into(), chans(&[("camera", 0.8)]));
    let display_less = run.clone().merged_over(Some(prior.clone()), &["image"]);
    assert_eq!(
        display_less.states["arrival"],
        chans(&[("camera", 0.8), ("image", 0.7)])
    );
    assert_eq!(display_less.states["elsewhere"], chans(&[("camera", 0.5)]));
    let full = run.merged_over(Some(prior), &[]);
    assert_eq!(full.states["arrival"], chans(&[("camera", 0.8)]));
}

fn patched_report(label: &str, image: f64, missing: Option<&str>) -> StateReport {
    StateReport {
        label: label.into(),
        emulator: "pcsx-redux".into(),
        fingerprint: String::new(),
        scene: String::new(),
        game_mode: 0x15,
        class: StateClass::Battle,
        unseeded: String::new(),
        channels: chans(&[("image", image)]),
        detail: BTreeMap::new(),
        score: Some(image),
        image: None,
        resident_patch: Some("party swap".into()),
        resident_disc_missing: missing.map(str::to_string),
    }
}

/// A patched-disc state whose own image was missing ran the retail disc:
/// it is skipped by the ratchet with its reason, and a bless keeps the
/// score its own image measured.
#[test]
fn a_state_run_without_its_own_image_is_skipped_and_never_blessed() {
    let mut prior = Baseline::default();
    prior.states.insert("swap".into(), chans(&[("image", 0.9)]));
    let fallback = vec![patched_report("swap", 0.5, Some("image.bin not found"))];
    assert!(prior.patched_drift(&fallback, &[]).is_empty());
    let skips = prior.fallback_skips(&fallback);
    assert_eq!(skips.len(), 1);
    assert!(skips[0].contains("image.bin not found"));
    let blessed = Baseline::from_reports(&fallback).merged_over(Some(prior.clone()), &[]);
    assert_eq!(blessed.states["swap"], chans(&[("image", 0.9)]));
    // On its own image the same drop is reported.
    let own = vec![patched_report("swap", 0.5, None)];
    assert_eq!(prior.patched_drift(&own, &[]).len(), 1);
    assert!(prior.fallback_skips(&own).is_empty());
}

/// The sub-screen word names the screen: a root row's route, one of the
/// Equip row's three steps, the title family (clear) or a script-entered
/// screen no root row routes to.
#[test]
fn a_menu_capture_is_classified_by_its_subscreen() {
    let m = RetailMenu::from_ram(&ram_with_subscreen(0x15), "town01").unwrap();
    assert_eq!((m.row, m.equip_depth), (FieldMenuRow::Status, 0));
    let m = RetailMenu::from_ram(&ram_with_subscreen(0x13), "map01").unwrap();
    assert_eq!((m.row, m.equip_depth), (FieldMenuRow::Equip, 1));
    let m = RetailMenu::from_ram(&ram_with_subscreen(0x14), "map01").unwrap();
    assert_eq!((m.row, m.equip_depth), (FieldMenuRow::Equip, 2));
    assert!(RetailMenu::from_ram(&ram_with_subscreen(0), "opdeene").is_err());
    assert!(RetailMenu::from_ram(&ram_with_subscreen(0x20), "koin1").is_err());
}

fn field_run_ram(label: &str, loaded_define: u16) -> Vec<u8> {
    let mut ram = vec![0u8; 0x20_0000];
    let at = |va: u32| (va & 0x1F_FFFF) as usize;
    let l = at(game_anchors::SCENE_NAME_VA);
    ram[l..l + label.len()].copy_from_slice(label.as_bytes());
    ram[at(game_anchors::GAME_MODE_VA)] = 0x03;
    let p = at(game_anchors::PLAYER_PTR_VA);
    ram[p..p + 4].copy_from_slice(&0x8010_0000u32.to_le_bytes());
    let d = at(LOADED_SCENE_DEFINE);
    ram[d..d + 2].copy_from_slice(&loaded_define.to_le_bytes());
    ram
}

/// A walked crossing caught between the scene-change packet and the next
/// field init reads the incoming label over the outgoing scene: the state
/// is scored as the scene `0x80084540` names, and a settled state keeps
/// its label.
#[test]
fn a_pending_door_is_scored_as_the_loaded_scene() {
    let cdname: legaia_prot::cdname::IndexMap =
        [(391, "map03".to_string()), (399, "doman".to_string())].into();
    let mut obs = RetailObs::from_ram(&field_run_ram("doman", 391), None);
    assert_eq!(obs.class, StateClass::Field);
    obs.settle_on_loaded_scene(&cdname);
    assert_eq!(obs.scene, "map03");
    assert_eq!(obs.pending_scene.as_deref(), Some("doman"));
    assert_eq!(obs.class, StateClass::WorldMap);

    let mut settled = RetailObs::from_ram(&field_run_ram("doman", 399), None);
    settled.settle_on_loaded_scene(&cdname);
    assert_eq!(settled.scene, "doman");
    assert_eq!(settled.pending_scene, None);
}

/// The visible-tile window is the four signed scratchpad bytes at
/// `0x3E8`, read on a field state only.
#[test]
fn the_view_window_comes_off_the_scratchpad_of_a_field_state() {
    let mut pad = vec![0u8; 0x400];
    pad[0x3E8..0x3EC].copy_from_slice(&[0xF8, 0xFA, 6, 10]);
    let mut field = RetailObs::from_ram(&field_run_ram("town01", 0), None);
    assert_eq!(field.class, StateClass::Field);
    field.seat_view_window(Some(&pad));
    assert_eq!(field.view_window, Some([-8, -6, 6, 10]));
    field.seat_view_window(None);
    assert_eq!(field.view_window, None);
    let mut overworld = RetailObs::from_ram(&field_run_ram("map01", 0), None);
    assert_eq!(overworld.class, StateClass::WorldMap);
    overworld.seat_view_window(Some(&pad));
    assert_eq!(overworld.view_window, None);
}

/// The page mark's displayed frame: the RAM's frame taken back over the
/// two-frame display lag, and none while no page waits or an automatic
/// press counts down.
#[test]
fn the_page_mark_frame_is_taken_back_by_the_display_lag() {
    let at = |va: u32| (va & 0x1F_FFFF) as usize;
    let mut ram = field_run_ram("town01", 0);
    assert_eq!(retail_page_mark(&ram), None, "no page waits");
    ram[at(0x801F_2734)] = 0x19;
    ram[at(0x801C_6004)] = 1;
    ram[at(0x801C_6014)] = 9;
    assert_eq!(retail_page_mark(&ram), Some(1));
    // One vsync into the frame: the screen still shows the other one.
    ram[at(0x801C_6014)] = 1;
    assert_eq!(retail_page_mark(&ram), Some(0));
    ram[at(0x8007_3F00)] = 5;
    assert_eq!(retail_page_mark(&ram), None, "an automatic press hides it");
}

#[test]
fn the_drive_walks_the_root_cursor_onto_the_row() {
    let m = RetailMenu::from_ram(&ram_with_subscreen(0x17), "town01").unwrap();
    let presses = pause_menu_presses(&m);
    let buttons: Vec<PadButton> = presses.iter().map(|p| p.1).collect();
    let mut want = vec![PadButton::Start];
    want.extend(std::iter::repeat_n(PadButton::Down, 4));
    want.push(PadButton::Cross);
    assert_eq!(buttons, want);
    // The first press waits out the pause wipe; the rest are one gap
    // apart.
    assert_eq!(presses[1].0, MENU_OPEN_WIPE_TICKS + MENU_PRESS_GAP);
    assert!(
        presses[1..]
            .windows(2)
            .all(|w| w[1].0 - w[0].0 == MENU_PRESS_GAP)
    );
}

#[test]
fn cell_fx_and_fog_seeds_round_trip_through_their_env_forms() {
    let fx = vec![legaia_engine_field::clut_cell_fx::ClutCellFx {
        rect: (0, 502, 16, 1),
        h_add: 2880,
        s_add: 0,
        v_add: -98,
        mode: 1,
        white: 256,
    }];
    assert_eq!(cell_fx_from_env(&cell_fx_env(&fx)), fx);
    let fog = vec![legaia_engine_field::fog_particles::FogParticle {
        age: 0x480,
        rate: 12,
        slot: 77,
        alive: true,
        vx: -3,
        vz: 5,
        x: -(40 << 11),
        z: 90 << 11,
        y: -0x60,
        grey: 0x5A,
    }];
    assert_eq!(fog_from_env(&fog_env(&fog)), fog);
    // Period 2 on a step-3 frame fires every tick; a countdown short of
    // the period fired on an earlier tick.
    assert_eq!(scroll_fires_within(2, 2, 3, 2), 2);
    assert_eq!(scroll_fires_within(8, 4, 2, 2), 0);
    assert_eq!(scroll_fires_within(8, 6, 2, 2), 1);
    // Rotating right/down undoes the scroller's left/up rotation.
    let t: Vec<u16> = (0..6).collect(); // 3 wide, 2 high
    assert_eq!(unrotate_rect(&t, 3, 2, 1, 1), vec![5, 3, 4, 2, 0, 1]);
    let rects = vec![((0x280, 0, 2, 2), vec![1, 2, 3, 0x8004])];
    assert_eq!(vram_rects_from_file(&vram_rects_file(&rects)), rects);
    assert!(fog_from_env("1,2,3").is_empty());
}

/// A clip cursor is taken back by the display lag the way the anim tick
/// would have stood it: Noa's looping eight-frame clip at `48`, two game
/// frames of `8` back, shows `32`; a loop wraps by its span, a one-shot
/// clamps at the start, a held clip and a reversed one keep their rules.
#[test]
fn a_clip_cursor_rewinds_to_the_displayed_frame() {
    use legaia_engine_scene::field_env::{ANIM_CLAMP, ANIM_HOLD, ANIM_REVERSE};
    assert_eq!(rewind_clip_cursor(48, 0x15, 8, 16), 32);
    assert_eq!(rewind_clip_cursor(8, 0x15, 8, 16), 120);
    assert_eq!(rewind_clip_cursor(8, ANIM_CLAMP, 8, 16), 0);
    assert_eq!(rewind_clip_cursor(48, ANIM_HOLD, 8, 16), 48);
    assert_eq!(rewind_clip_cursor(48, ANIM_REVERSE, 8, 16), 64);
    assert_eq!(rewind_clip_cursor(48, 0x15, 8, 0), 48);
}

/// The lag is counted in game frames of `clip_step * frame_step` units:
/// Noa's gated clip (rate `8`, divisor `4` -> step `4`) on a step-`2` frame
/// moves `16` over the two-frame lag, which puts the RAM's `48` on `32`.
#[test]
fn a_clip_lag_scales_by_the_frame_step() {
    let step = legaia_engine_vm::actor_tick::clip_step(8, true, 4);
    assert_eq!(step, 4);
    assert_eq!(clip_display_lag_units(step, 2, 2), 16);
    assert_eq!(
        rewind_clip_cursor(48, 0x15, 8, clip_display_lag_units(step, 2, 2)),
        32
    );
    assert_eq!(clip_display_lag_units(8, 3, 2), 48);
}

/// A one-shot parked on its clamp limit with the end latch up stays there;
/// the same cursor without the latch, or a looping clip, is taken back.
#[test]
fn a_parked_one_shot_is_not_rewound() {
    use legaia_engine_scene::field_env::{ANIM_CLAMP, ANIM_END, ANIM_REVERSE};
    let parked = ANIM_CLAMP | ANIM_END;
    assert_eq!(rewind_clip_cursor(479, parked, 30, 64), 479);
    assert_eq!(rewind_clip_cursor(0, parked | ANIM_REVERSE, 30, 64), 0);
    assert_eq!(rewind_clip_cursor(479, ANIM_CLAMP, 30, 64), 415);
    assert_eq!(rewind_clip_cursor(470, parked, 30, 64), 406);
}

/// A walker is seated where the displayed frame drew it: `rikuroa`'s Caruban
/// stands at `Z = 9792` in the RAM, began the tick at `9760` (`+0x20`, the
/// tick iterator's pre-tick copy), and the frame two ticks back shows `9728`.
/// A still walker keeps its seat.
#[test]
fn a_walker_is_seated_at_the_displayed_frame() {
    assert_eq!(
        walker_displayed_seat((9152, 9792), (9152, 9760), DISPLAY_LAG_FRAMES),
        (9152, 9728)
    );
    assert_eq!(
        walker_displayed_seat((100, -40), (100, -40), DISPLAY_LAG_FRAMES),
        (100, -40)
    );
    assert_eq!(walker_displayed_seat((10, 20), (12, 18), 0), (10, 20));
}

/// A cycler's cell is read off the older frame's packet buffer (the one
/// `gp+0x434` does not name), first copy from its base, and not off the
/// buffer the newest frame built.
#[test]
fn older_buffer_cell_rects_come_from_the_other_packet_buffer() {
    let mut ram = vec![0u8; 0x20_0000];
    let put32 = |ram: &mut Vec<u8>, va: u32, v: u32| {
        let o = (va & 0x1F_FFFF) as usize;
        ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
    };
    let put16 = |ram: &mut Vec<u8>, va: u32, v: u16| {
        let o = (va & 0x1F_FFFF) as usize;
        ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
    };
    // Two buffers; the newest frame built buffer 1, the one before it 0.
    put32(&mut ram, 0x8007_B728, 0x8010_0000);
    put32(&mut ram, 0x8007_B72C, 0x8012_0000);
    put32(&mut ram, 0x8007_B9CC, 0x1_0000);
    put16(&mut ram, 0x8007_B74C, 1);
    // One live mode-3 cycler on the first actor list.
    let node = 0x8009_0000;
    put32(&mut ram, 0x8007_C34C, node);
    put32(&mut ram, node + 0x0C, 0x8002_1DF4);
    put16(&mut ram, node + 0x5A, 3);
    put16(&mut ram, node + 0x9C, 1000);
    for (i, v) in [16u16, 507, 2, 1].into_iter().enumerate() {
        put16(&mut ram, node + 0xA0 + 2 * i as u32, v);
    }
    let block = |ram: &mut Vec<u8>, at: u32, a: u16, b: u16| {
        for (i, v) in [16u16, 507, 2, 1, a, b].into_iter().enumerate() {
            put16(ram, at + 2 * i as u32, v);
        }
    };
    block(&mut ram, 0x8010_0020, 0x1111, 0x2222);
    block(&mut ram, 0x8010_8000, 0x7777, 0x7777);
    block(&mut ram, 0x8012_0020, 0x3333, 0x4444);
    assert_eq!(
        retail_older_buffer_cell_rects(&ram),
        vec![((16, 507, 2, 1), vec![0x1111, 0x2222])]
    );
    // Armed inside the lag: the older frame had not uploaded it.
    put16(&mut ram, node + 0x9C, 3);
    assert!(retail_older_buffer_cell_rects(&ram).is_empty());
}

/// A cycler parked on a `WAIT_SET` reads how many ticks its program has sat
/// out off the wait's drain, and one that sat out the display lag is taken
/// back along its velocities alone.
#[test]
fn an_idle_cycler_is_taken_back_along_its_velocities() {
    let mut ram = vec![0u8; 0x20_0000];
    let put16 = |ram: &mut Vec<u8>, va: u32, v: u16| {
        let o = (va & 0x1F_FFFF) as usize;
        ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
    };
    let node = 0x8009_0000;
    let buf: u32 = 0x800A_0000;
    let o = (node + 0x48 - 0x8000_0000) as usize;
    ram[o..o + 4].copy_from_slice(&buf.to_le_bytes());
    // `... 09 05 | <pc>`: parked behind `WAIT_SET 5` (`5 << 3 = 40`).
    put16(&mut ram, buf + 8, 0x09);
    put16(&mut ram, buf + 10, 5);
    put16(&mut ram, node + 0x70, 6);
    // One drain of `3 * 8` at frame step 3: the program ran one tick ago
    // (`koin1`'s spotlight cone).
    put16(&mut ram, node + 0x54, 16);
    assert_eq!(cell_fx_idle_ticks(&ram, node, 3), Some(1));
    put16(&mut ram, node + 0x54, 40);
    assert_eq!(cell_fx_idle_ticks(&ram, node, 3), Some(0));
    put16(&mut ram, node + 0x54, (-8i16) as u16);
    assert_eq!(cell_fx_idle_ticks(&ram, node, 3), Some(2));
    // Not parked on a wait.
    put16(&mut ram, buf + 8, 0x2E);
    assert_eq!(cell_fx_idle_ticks(&ram, node, 3), None);

    let fx = legaia_engine_field::clut_cell_fx::ClutCellFx {
        rect: (80, 505, 3, 1),
        h_add: -4160,
        s_add: 0,
        v_add: -12,
        mode: 0,
        white: 0x100,
    };
    // A hue velocity of `-128` moves `-48` a step-3 tick; `+16` on V, `+6`.
    let back = cell_fx_displayed(fx, [-128, 0, 16], 16, 3, 2);
    assert_eq!((back.h_add, back.s_add, back.v_add), (-4064, 0, -24));
    assert_eq!(back.white, 0x100);
    let ramping = cell_fx_displayed(
        legaia_engine_field::clut_cell_fx::ClutCellFx { white: 78, ..fx },
        [0; 3],
        16,
        2,
        2,
    );
    assert_eq!(ramping.white, 70);
}

/// The loop walker undoes the runs inside the lag: a velocity flip run one
/// tick ago lands a triangle wave back on the RAM's phase, and a strobe of
/// absolute sets steps back one set per run.
#[test]
fn the_cycler_walker_undoes_the_runs_inside_the_lag() {
    // `1A 4000 | 2E 0 0 -3 | 09 4 | 2E 0 0 3 | 09 4 | 1B`, parked on the
    // loop-back one tick after the `+3` run, at frame step 2.
    let wave: Vec<i16> = vec![
        0x4000, 0, 0x1A, 0x4000, 0x2E, 0, 0, -3, 0x09, 4, 0x2E, 0, 0, 3, 0x09, 4, 0x1B, 0x08,
    ];
    assert_eq!(
        cycler_displayed_adds(&wave, 16, 1, [0, 0, 0], [0, 0, 24], 2, 2),
        Some([0, 0, 0])
    );
    // Two ticks after the run, the same program has only integrated.
    assert_eq!(
        cycler_displayed_adds(&wave, 16, 2, [0, 0, 12], [0, 0, 24], 2, 2),
        Some([0, 0, 0])
    );
    // A strobe of three absolute sets, one run a tick: parked after the
    // `-20` set on the RAM's own tick, the frame on screen uploaded `-20`.
    let strobe: Vec<i16> = vec![
        0x4000, 0, 0x1A, 0x4000, 0x2B, 0, 0, -10, 0x09, 1, 0x2B, 0, 0, -20, 0x09, 1, 0x2B, 0, 0,
        -30, 0x09, 1, 0x1B, 0x08,
    ];
    assert_eq!(
        cycler_displayed_adds(&strobe, 16, 0, [0, 0, -20], [0; 3], 3, 2),
        Some([0, 0, -20])
    );
    // An op outside the cycler set, or a PC outside the loop, is not read.
    let mut ext = strobe.clone();
    ext[4] = 0x2F;
    assert_eq!(
        cycler_displayed_adds(&ext, 16, 0, [0, 0, -20], [0; 3], 3, 2),
        None
    );
    assert_eq!(
        cycler_displayed_adds(&strobe, 3, 0, [0, 0, -20], [0; 3], 3, 2),
        None
    );
}

/// A pair both sides park scores its exact coordinates: a park seat and the
/// one `41` step short of it are distinct placements (town01 `P1[36]`).
#[test]
fn a_parked_pair_scores_its_exact_coordinates() {
    let seat = (16320, 16320);
    let stepped = (16320, 16256);
    assert!(npc_parked(seat.0, seat.1) && npc_parked(stepped.0, stepped.1));
    assert_eq!(npc_pair_score(stepped, stepped), 1.0);
    assert_eq!(npc_pair_score(seat, seat), 1.0);
    assert_eq!(npc_pair_score(stepped, seat), 0.0);
    assert_eq!(npc_pair_score(seat, stepped), 0.0);
    // Parked on one side only, and the drawn falloff, are unchanged.
    assert_eq!(npc_pair_score(seat, (12608, 1984)), 0.0);
    assert_eq!(npc_pair_score((4160, 11968), (4160, 11964)), 1.0);
    assert_eq!(npc_pair_score((4160, 11968), (4160, 11840)), 0.0);
}
