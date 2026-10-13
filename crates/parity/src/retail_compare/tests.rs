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
