use super::*;
use crate::world_map_panel_actors::{
    HUD_SUPPRESS_PAD_MASK, HUD_Y_BOTTOM, SOFT_RESET_PAD_MASK, SOFT_RESET_SLIDE_REST,
    SOFT_RESET_SLIDE_START,
};

#[derive(Default)]
struct Flags(std::collections::HashSet<i32>);
impl PanelFlagStore for Flags {
    fn flag_test(&self, id: i32) -> bool {
        self.0.contains(&id)
    }
    fn flag_set(&mut self, id: i32) {
        self.0.insert(id);
    }
    fn flag_clear(&mut self, id: i32) {
        self.0.remove(&id);
    }
}

/// The engaged rearm term reaches the countdown a frame late, both on
/// the raise and on the release: retail's HUD runs in the player's tick
/// ahead of the object list whose runner raises the bit.
#[test]
fn the_engaged_rearm_term_lags_one_frame() {
    let mut hud = FieldPartyHud::new();
    assert!(!hud.rearm_term(false));
    assert!(!hud.rearm_term(true), "the raising frame still draws");
    assert!(hud.rearm_term(true));
    assert!(hud.rearm_term(false), "the release lags the same frame");
    assert!(!hud.rearm_term(false));
    // A scene-change rearm does not forget the term.
    hud.rearm_term(true);
    hud.rearm();
    assert!(hud.rearm_term(false));
}

/// A suppressed frame stores nothing but the cached decision: retail's
/// suppress arms return before the timer and position stores. So a host
/// that steps the kernel under a modal overlay and one that does not
/// step it at all leave the same state, and the next unsuppressed frame
/// decides the same way on both.
#[test]
fn a_suppressed_tick_changes_no_state() {
    let mut stepped = FieldPartyHud::new();
    // Enter, stand still a few frames (counting down), then open an
    // overlay.
    for _ in 0..5 {
        stepped.tick(false, 0, 0, Some((100, 200)), 1, None);
    }
    let mut skipped = stepped.clone();
    for _ in 0..30 {
        let d = stepped.tick(true, 0, 0, Some((100, 200)), 1, None);
        assert!(matches!(d, HudDecision::Suppressed));
    }
    assert_eq!(stepped.timer, skipped.timer);
    assert_eq!(stepped.cached, skipped.cached);
    // The overlay closes: both hosts' next frame decides identically.
    for _ in 0..60 {
        assert_eq!(
            stepped.tick(false, 0, 0, Some((100, 200)), 1, None),
            skipped.tick(false, 0, 0, Some((100, 200)), 1, None)
        );
    }
}

/// The phase hold lands the countdown on the retail value at the capture
/// tick: a card-load state three frames into the idle (`0x25` left) shows
/// no readout, a state whose countdown already expired (`0`) shows it.
#[test]
fn the_phase_hold_lands_retails_countdown_at_the_capture_tick() {
    use crate::world_map_panel_actors::HUD_IDLE_FRAMES_NEAR as IDLE;
    const CAPTURE: u64 = 120;
    for retail in [0i16, 1, 0x25, 0x26, IDLE] {
        let mut hud = FieldPartyHud::new();
        let mut last = None;
        for tick in 1..=CAPTURE {
            if hud_phase_hold(tick, CAPTURE, retail, IDLE) {
                hud.rearm();
            }
            last = Some(hud.tick(false, 0, 0, Some((100, 200)), 1, None));
        }
        let want = match retail {
            0 => HudDecision::Draw { y: HUD_Y_BOTTOM },
            t if t == IDLE => HudDecision::Rearmed { timer: IDLE },
            t => HudDecision::CountingDown { timer: t },
        };
        assert_eq!(last, Some(want), "retail countdown {retail}");
    }
    // The un-held run (no retail value to align to) draws long before.
    assert!(!hud_phase_hold(CAPTURE + 1, CAPTURE, 0, IDLE));
}

/// The overworld idle (`0xA0`) outlasts a capture at tick 120, so the
/// hold alone leaves the countdown above any short retail value; the cap
/// brings it down so the capture still reads retail's countdown.
#[test]
fn the_countdown_cap_lands_a_short_retail_value_under_a_long_idle() {
    use crate::world_map_panel_actors::hud_idle_frames;
    const CAPTURE: u64 = 120;
    let idle = hud_idle_frames(1, false);
    assert!(i64::from(idle) > CAPTURE as i64);
    for retail in [0i16, 3, 0x30] {
        let mut hud = FieldPartyHud::new();
        let mut last = None;
        for tick in 1..=CAPTURE {
            if hud_phase_hold(tick, CAPTURE, retail, idle) {
                hud.rearm();
            }
            last = Some(hud.tick(false, 1, 0, Some((100, 200)), 1, None));
            if let Some(cap) = hud_countdown_cap(tick, CAPTURE, retail) {
                hud.cap_countdown(cap);
            }
        }
        let want = match retail {
            0 => HudDecision::Draw { y: HUD_Y_BOTTOM },
            t => HudDecision::CountingDown { timer: t },
        };
        assert_eq!(last, Some(want), "retail countdown {retail}");
    }
    assert_eq!(hud_countdown_cap(CAPTURE + 1, CAPTURE, 0), None);
}

/// The raw-to-packed conversion, on the two fixed points both layouts
/// document independently.
#[test]
fn packed_pad_swaps_the_byte_halves() {
    assert_eq!(
        packed_pad(legaia_engine_system::input::PadButton::Cross.mask()),
        0x0040
    );
    assert_eq!(
        packed_pad(legaia_engine_system::input::PadButton::Circle.mask()),
        0x0020
    );
    assert_eq!(
        packed_pad(legaia_engine_system::input::PadButton::Up.mask()),
        0x1000
    );
    assert_eq!(
        packed_pad(legaia_engine_system::input::PadButton::Down.mask()),
        0x4000
    );
    // And the round trip is an involution, so a host can convert back.
    assert_eq!(packed_pad(packed_pad(0x1234)), 0x1234);
}

/// The confirm/cancel masks the panel kernels test are the packed ones;
/// feeding the raw Cross bit must NOT read as a confirm.
#[test]
fn the_action_masks_are_packed_not_raw() {
    let raw_cross = u32::from(legaia_engine_system::input::PadButton::Cross.mask());
    assert_eq!(
        raw_cross & PANEL_ACTION_A,
        0,
        "raw Cross is not packed Cross"
    );
    assert_eq!(
        u32::from(packed_pad(
            legaia_engine_system::input::PadButton::Cross.mask()
        )) & PANEL_ACTION_A,
        PANEL_ACTION_A
    );
}

fn confirm() -> u16 {
    packed_pad(legaia_engine_system::input::PadButton::Cross.mask())
}
fn cancel() -> u16 {
    packed_pad(legaia_engine_system::input::PadButton::Circle.mask())
}

#[test]
fn the_sub_list_opens_a_window_and_sizes_the_party_panel() {
    let mut h = PanelActorHost::new();
    h.windows.party_members = 3;
    h.install(PanelActorKind::SubList, 0x11);
    let mut f = Flags::default();
    h.tick(0, 0, 1, &mut f);
    assert!(
        h.windows.is_open(SUBLIST_PANEL_INDEX),
        "the open script's op 1 spawned the window"
    );
    // `op 12` wrote the party panel descriptor: height = 3*56 - 7.
    let party = h
        .windows
        .descriptor(crate::world_map_panel::PARTY_PANEL_INDEX);
    assert_eq!(party.height, 3 * 56 - 7);
    assert_eq!(party.y, 202 - party.height);
}

#[test]
fn the_sub_list_cancel_closes_the_window_and_retires() {
    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::SubList, 0x11);
    let mut f = Flags::default();
    h.tick(0, 0, 1, &mut f); // phase 0 -> 1, window open
    assert!(h.windows.is_open(SUBLIST_PANEL_INDEX));
    h.tick(0, cancel(), 1, &mut f); // phase 1 -> 2
    let frame = h.tick(0, 0, 1, &mut f); // phase 2: close + exit to 0x1A
    assert!(!frame.retired, "the exit hands the actor to the draw tick");
    assert_eq!(h.kind, Some(PanelActorKind::DrawTick));
    assert_eq!(h.handler_id, crate::submode_hub_actors::slot::DRAW_TICK);
    // `FUN_801F20B0`: install its panel script, pump, clear the gate - the
    // dispatcher then retires the actor.
    let frame = h.tick(0, 0, 1, &mut f);
    assert!(frame.retired);
    assert!(!h.is_active());
    assert!(!h.windows.is_open(SUBLIST_PANEL_INDEX));
}

/// A locked input keeps the draw tick's completion gate up, as the close
/// tick's does, so the actor stays until the lock drops.
#[test]
fn the_draw_tick_waits_out_the_input_lock() {
    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::SubList, 0x11);
    let mut f = Flags::default();
    h.tick(0, 0, 1, &mut f);
    h.tick(0, cancel(), 1, &mut f);
    h.tick(0, 0, 1, &mut f);
    assert_eq!(h.kind, Some(PanelActorKind::DrawTick));
    h.input_locked = true;
    assert!(!h.tick(0, 0, 1, &mut f).retired);
    assert!(h.is_active());
    h.input_locked = false;
    assert!(h.tick(0, 0, 1, &mut f).retired);
}

#[test]
fn the_sub_list_row_one_confirm_raises_the_hand_off() {
    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::SubList, 0x11);
    let mut f = Flags::default();
    h.tick(0, 0, 1, &mut f);
    // Down then confirm: cursor 1 -> phase 3 (the hand-off).
    h.tick(
        packed_pad(legaia_engine_system::input::PadButton::Down.mask()),
        0,
        1,
        &mut f,
    );
    assert_eq!(h.cursor, 1);
    h.tick(0, confirm(), 1, &mut f);
    assert_eq!(h.phase, 3);
    let frame = h.tick(0, 0, 1, &mut f);
    assert!(frame.hand_off);
}

#[test]
fn the_flag_window_sizes_its_panel_and_commits_a_flag() {
    let mut h = PanelActorHost::new();
    h.flag_desc = FlagWindowDescriptor {
        count: 8,
        first_visible: 0,
        rows: 4,
        base_flag: 0x100,
    };
    h.install(PanelActorKind::FlagWindow, 0x11);
    let mut f = Flags::default();
    f.flag_set(0x103);
    h.tick(0, 0, 1, &mut f);
    // Phase 0 sized descriptor 14 from the row count and opened it.
    let d = h
        .windows
        .descriptor(crate::world_map_panel_actors::FLAG_WINDOW_PANEL_INDEX as i16);
    assert_eq!(d.height, 4 * 16);
    assert_eq!(d.y, (8 - 4) * 16 + 0x48);
    assert!(
        h.windows
            .is_open(crate::world_map_panel_actors::FLAG_WINDOW_PANEL_INDEX as i16)
    );
    // The scan found flag 3 set, remembered it, and the range clear wiped
    // the whole span.
    assert_eq!(h.remembered_row, 3);
    assert!(!f.flag_test(0x103), "phase 0 clears the range it covers");
    // The list draws bottom-up, so the remembered selection 3 sits on
    // screen row 0 and only Down can leave it. Confirming on the
    // remembered row is retail's cancel, so the pick has to move first.
    h.tick(
        packed_pad(legaia_engine_system::input::PadButton::Down.mask()),
        0,
        1,
        &mut f,
    );
    assert_ne!(h.cursor, h.remembered_row);
    let frame = h.tick(0, confirm(), 1, &mut f);
    assert_eq!(frame.flags_set, vec![0x100 + h.cursor]);
    assert!(f.flag_test(frame.flags_set[0]));
}

/// `FUN_801EE90C` entered at phase 0 never reaches its prompt: it jumps to
/// the fill-fade block and parks at phase 14, which has no exit arm. The
/// host's entry phase has to be the prompt, or the actor wedges the screen
/// - which is exactly what a live session showed before this was fixed.
#[test]
fn the_text_box_entry_phase_is_the_prompt_not_the_fade_block() {
    assert_eq!(PanelActorKind::TextBox.entry_phase(), 1);
    assert_eq!(PanelActorKind::SubList.entry_phase(), 0);

    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::TextBox, 0x11);
    h.phase = 0; // retail's arrival path, seeded by hand
    let mut f = Flags::default();
    for _ in 0..256 {
        h.tick(0, 0, 4, &mut f);
    }
    assert_eq!(h.phase, 14, "the fade chain parks with no terminal arm");
    assert!(h.is_active(), "and nothing retires it");
    assert!(h.dismiss(), "so the host's escape hatch has to");
    assert!(!h.is_active());
}

#[test]
fn the_text_box_confirm_asks_for_the_party_restore() {
    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::TextBox, 0x11);
    assert_eq!(h.phase, 1, "the prompt");
    let mut f = Flags::default();
    let frame = h.tick(0, confirm(), 1, &mut f);
    assert!(frame.restore_party);
    assert_eq!(h.phase, 2);
    // Dismissing the confirmation hands the actor to the draw tick, which
    // retires it on its first frame.
    let frame = h.tick(confirm(), confirm(), 1, &mut f);
    assert!(!frame.retired);
    assert_eq!(h.kind, Some(PanelActorKind::DrawTick));
    assert!(h.tick(0, 0, 1, &mut f).retired);
}

#[test]
fn the_fill_fade_closes_every_window_and_runs_to_its_exit() {
    let mut h = PanelActorHost::new();
    h.windows
        .run_script(crate::world_map_panel_actors::SUBLIST_OPEN_SCRIPT);
    assert!(h.windows.is_open(SUBLIST_PANEL_INDEX));
    h.install(PanelActorKind::FillFade, 0x11);
    let mut f = Flags::default();
    // Phase 0 falls into 1 and runs the fill-fade script, which closes all.
    h.tick(0, 0, 1, &mut f);
    assert!(!h.windows.is_open(SUBLIST_PANEL_INDEX));
    assert_eq!(h.windows.open_count(), 0);
    let mut retired = false;
    let mut saw_scene_bit = false;
    for _ in 0..256 {
        let fr = h.tick(0, 0, 1, &mut f);
        saw_scene_bit |= h.scene_obj_flags & 0x0008_0000 != 0;
        if fr.retired {
            retired = true;
            break;
        }
    }
    assert!(saw_scene_bit, "phase 3 sets the scene object's flag bit");
    assert!(retired, "phase 4 exits");
}

#[test]
fn the_fill_fade_exit_runs_the_close_tick_before_retiring() {
    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::FillFade, 0x11);
    let mut f = Flags::default();
    let mut exited = false;
    for _ in 0..256 {
        let fr = h.tick(0, 0, 1, &mut f);
        if !fr.exits.is_empty() {
            exited = true;
            assert!(!fr.retired, "handler 0 is followed, not dropped");
            break;
        }
    }
    assert!(exited, "phase 4 exits");
    assert_eq!(h.kind, Some(PanelActorKind::CloseTick));
    assert_eq!(h.scene_field_40, 0x11, "the old id is parked");
    // A locked input keeps the completion gate up, so the actor stays.
    h.windows
        .run_script(crate::world_map_panel_actors::SUBLIST_OPEN_SCRIPT);
    h.input_locked = true;
    let ticks = h.text_actor_ticks;
    assert!(!h.tick(0, 0, 1, &mut f).retired);
    assert_eq!(h.windows.open_count(), 0, "state 0 closes every panel");
    assert_eq!(
        h.text_actor_ticks,
        ticks + 1,
        "every state ticks the text actors"
    );
    assert!(h.is_active());
    h.input_locked = false;
    assert!(h.tick(0, 0, 1, &mut f).retired);
    assert!(!h.is_active());
}

#[test]
fn the_fade_flash_parks_until_the_host_releases_it() {
    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::FadeFlash, 0x11);
    let mut f = Flags::default();
    for _ in 0..200 {
        h.tick(0, 0, 4, &mut f);
        if h.phase == 3 {
            break;
        }
    }
    assert_eq!(h.phase, 3, "the ramp parks waiting on the flash counter");
    // Parked: 50 more frames change nothing.
    for _ in 0..50 {
        h.tick(0, 0, 4, &mut f);
    }
    assert_eq!(h.phase, 3);
    h.release_flash();
    for _ in 0..200 {
        h.tick(0, 0, 8, &mut f);
        if matches!(h.kind, Some(PanelActorKind::TravelArt(_))) {
            break;
        }
    }
    // The released ramp-down reaches its terminal arm, whose exit names the
    // Riremito travel art (`PTR_FUN_801F33B4[0x29]`); the host runs it next.
    assert_eq!(h.kind, Some(PanelActorKind::TravelArt(TravelArt::Riremito)));
    assert_eq!(h.handler_id, 0x29);
}

#[test]
fn the_soft_reset_slides_then_reloads_on_a_face_press() {
    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::SoftReset, 0x11);
    let mut f = Flags::default();
    let frame = h.tick(0, 0, 1, &mut f);
    assert!(frame.records_y.is_none(), "phase 0 only arms");
    assert_eq!(h.slide, SOFT_RESET_SLIDE_START);
    for _ in 0..(SOFT_RESET_SLIDE_START - SOFT_RESET_SLIDE_REST) {
        h.tick(0, 0, 1, &mut f);
    }
    assert_eq!(h.slide, SOFT_RESET_SLIDE_REST);
    // The pad is only sampled at rest.
    let frame = h.tick(0, SOFT_RESET_PAD_MASK as u16, 1, &mut f);
    assert!(frame.white_fade.is_some());
    let mut reloaded = false;
    for _ in 0..200 {
        if h.tick(0, 0, 1, &mut f).reload_executable {
            reloaded = true;
            break;
        }
    }
    assert!(reloaded);
}

#[test]
fn the_travel_art_returns_the_party_to_the_stored_tile() {
    let mut h = PanelActorHost::new();
    h.note_visit(7, 96, 25);
    h.install(PanelActorKind::TravelArt(TravelArt::Riremito), 0x11);
    let mut f = Flags::default();
    let mut warp = None;
    for _ in 0..400 {
        let frame = h.tick(0, 0, 4, &mut f);
        if let Some(d) = frame.warp {
            warp = Some(d);
            break;
        }
    }
    let d = warp.expect("the scan hits the recorded map");
    assert_eq!((d.x, d.y, d.z), ((96 << 7) + 0x40, 0, (25 << 7) + 0x40));
    assert!(!h.is_active(), "the resolve retires the actor");
}

#[test]
fn a_travel_art_with_no_recorded_map_parks_unfound() {
    let mut h = PanelActorHost::new();
    h.install(PanelActorKind::TravelArt(TravelArt::Rula), 0x11);
    let mut f = Flags::default();
    let mut unfound = false;
    for _ in 0..400 {
        if h.tick(0, 0, 4, &mut f).travel_unfound {
            unfound = true;
            break;
        }
    }
    assert!(unfound);
}

#[test]
fn the_party_hud_suppresses_on_the_packed_dpad_and_counts_down_when_idle() {
    let mut h = PanelActorHost::new();
    // First call rearms (no cached position yet).
    let d = h.tick_party_hud(false, 0, 0, Some((10, 20)), 1, None);
    assert!(matches!(d, HudDecision::Rearmed { .. }));
    // A held packed d-pad direction suppresses outright.
    let d = h.tick_party_hud(
        false,
        0,
        HUD_SUPPRESS_PAD_MASK as u16,
        Some((10, 20)),
        1,
        None,
    );
    assert_eq!(d, HudDecision::Suppressed);
    // Stationary + no pad: the countdown runs and then draws.
    let mut drew = false;
    for _ in 0..0x100 {
        if let HudDecision::Draw { y } = h.tick_party_hud(false, 0, 0, Some((10, 20)), 1, None) {
            assert_eq!(y, HUD_Y_BOTTOM, "no projection forces the low panel");
            drew = true;
            break;
        }
    }
    assert!(drew);
}

#[test]
fn an_idle_host_does_nothing() {
    let mut h = PanelActorHost::new();
    let mut f = Flags::default();
    assert_eq!(
        h.tick(confirm(), confirm(), 4, &mut f),
        PanelFrame::default()
    );
}

#[test]
fn note_visit_updates_the_stored_tile_in_place() {
    let mut h = PanelActorHost::new();
    h.note_visit(3, 1, 1);
    h.note_visit(4, 2, 2);
    h.note_visit(3, 9, 9);
    assert_eq!(h.visited.len(), 2);
    assert_eq!(h.visited[0].tile_x, 9);
}
