use super::{BATTLE_HUD_PEN, battle_hud_popup_views, battle_hud_slot_views};
use legaia_engine_core::battle_hud::{BattleHud, DamagePopup, SlotSyncInfo};
use legaia_engine_render::{BattleHudDraws, BattleHudFrame, battle_hud_draws_for};

/// A recognisable 1x1 solid src for the filled-rect draws.
const SOLID: (u32, u32, u32, u32) = (7, 3, 1, 1);
/// 640x480 = an exact 2x of the 320x240 stage with a zero origin, so a
/// stage column `c` lands at surface `2 * c` and the pinned retail
/// columns are readable straight off `dst.0`.
const SURFACE: (u32, u32) = (640, 480);
const STAGE_SCALE: i32 = 2;

/// The numeral strip's seat in the baked atlas
/// (`save_menu_atlas::ATLAS_RECT_HUD_DIGITS`).
const BATTLE_MIRROR_DIGITS: (u32, u32, u32, u32) = (0, 244, 80, 12);
/// The minimum chrome set that puts the numerals on the sprite list, so
/// a cell's screen seat is readable rather than inferred from a glyph.
const BATTLE_MIRROR_RECTS: legaia_engine_render::SaveMenuAtlasRects =
    legaia_engine_render::SaveMenuAtlasRects {
        battle: Some(legaia_engine_render::BattleChromeRects {
            panel_bg: (0, 0, 102, 48),
            plate_cap_l: (208, 0, 8, 20),
            plate_body: (192, 0, 16, 20),
            plate_cap_r: (216, 0, 8, 20),
            separator: (96, 64, 8, 16),
            digits: Some(BATTLE_MIRROR_DIGITS),
            cross_out: None,
            rot_stamp: None,
            curse_plate: None,
        }),
        ..blank_rects()
    };

/// `SaveMenuAtlasRects::default()` is not `const`, so spell the zeroed
/// base out for [`BATTLE_MIRROR_RECTS`].
const fn blank_rects() -> legaia_engine_render::SaveMenuAtlasRects {
    const Z: (u32, u32, u32, u32) = (0, 0, 0, 0);
    legaia_engine_render::SaveMenuAtlasRects {
        panel_tl: Z,
        panel_tr: Z,
        panel_bl: Z,
        panel_br: Z,
        panel_top: Z,
        panel_bot: Z,
        panel_left: Z,
        panel_right: Z,
        slot1: Z,
        slot2: Z,
        cursor: Z,
        panel_interior: Z,
        panel_filigree: Z,
        label_lv: Z,
        label_hp: Z,
        label_mp: Z,
        icon_money: Z,
        label_time: Z,
        label_coin: Z,
        gauge_cap: Z,
        gauge_trough: Z,
        gauge_box: Z,
        gauge_tip: Z,
        gauge_digits: Z,
        gauge_100: Z,
        gauge_fill: Z,
        dialog_fill: Z,
        icon_weapon: Z,
        icon_helmet: Z,
        icon_armor: Z,
        icon_boot: Z,
        icon_goods: Z,
        pager_left: Z,
        pager_right: Z,
        tab_cap_l: Z,
        tab_body: Z,
        tab_cap_r: Z,
        atr_icons: [Z; 3],
        load_empty_frame: None,
        load_portrait_by_char: [None; 3],
        battle: None,
    }
}

fn hud_with_party_row(hp: u16, hp_max: u16, mp: u16, mp_max: u16) -> BattleHud {
    let mut hud = BattleHud::new();
    hud.sync_slot(
        0,
        SlotSyncInfo {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp,
            hp_max,
            mp,
            mp_max,
            ap: None,
        },
    );
    hud
}

fn frame_draws(hud: &BattleHud, diag: bool) -> BattleHudDraws {
    battle_hud_draws_for(
        &legaia_font::synthetic_for_tests(),
        &BattleHudFrame {
            slots: &battle_hud_slot_views(hud),
            popups: &battle_hud_popup_views(hud),
            log: &[],
            solid_src: Some(SOLID),
            surface: SURFACE,
            diag,
            ..Default::default()
        },
        BATTLE_HUD_PEN,
    )
}

/// Solid rects of exactly `(w, h)` stage pixels, by stage `(x, y)`.
fn boxes_of(draws: &[legaia_engine_render::TextDraw], w: i32, h: i32) -> Vec<(i32, i32)> {
    draws
        .iter()
        .filter(|d| {
            d.src == SOLID
                && d.dst.2 == (w * STAGE_SCALE) as u32
                && d.dst.3 == (h * STAGE_SCALE) as u32
        })
        .map(|d| (d.dst.0 / STAGE_SCALE, d.dst.1 / STAGE_SCALE))
        .collect()
}

fn draws(hud: &BattleHud) -> Vec<legaia_engine_render::TextDraw> {
    frame_draws(hud, false).text
}

/// The party arm draws retail's resting surface: one 102x48 roster panel
/// per live member at `battle_chrome::panel_seats`, and **no gauge bar**.
///
/// The packet run carries no bar primitive in either readout, so a filled
/// HP or MP bar on a party row is the defect this pins shut.
#[test]
fn native_battle_party_is_retail_shaped_and_barless() {
    let hud = hud_with_party_row(250, 300, 12, 30);
    let out = draws(&hud);
    assert_eq!(
        boxes_of(&out, 102, 48),
        vec![(109, 164)],
        "the solo roster panel is not at its packet-pinned seat"
    );
    // Every solid rect on the retail surface is a plate body or one of
    // the 1-px rims the chrome-less fallback draws round it. Anything
    // with interior extents is a gauge bar.
    for d in out.iter().filter(|d| d.src == SOLID) {
        let w = d.dst.2 as i32 / STAGE_SCALE;
        let h = d.dst.3 as i32 / STAGE_SCALE;
        assert!(
            w == 1 || h == 1 || h == 20 || (w, h) == (102, 48),
            "a gauge-bar-shaped rect survives on the retail surface: {:?}",
            d.dst
        );
    }
    // Name glyph at the panel's pinned name pen (+5 inside the panel).
    assert!(
        out.iter().any(|d| d.src != SOLID
            && d.dst.0 == (109 + 5) * STAGE_SCALE
            && d.dst.1 == (164 + 4) * STAGE_SCALE),
        "no name glyph at the panel's pinned name pen"
    );
}

/// `engine-ui`'s `party_panel_stage_x` reads the packet-pinned
/// `engine-vm` kernels on its production path
/// (`battle_party_panel::panel_anchors`, falling back to
/// `battle_chrome::panel_seats` + the text inset); only the panel
/// *backgrounds* still carry a local seat mirror. This test holds the
/// drawn HUD to `battle_chrome`'s seats end to end - a drift here is a
/// HUD drawn at coordinates nothing pinned.
#[test]
fn engine_ui_seats_mirror_the_packet_pinned_battle_chrome() {
    use legaia_engine_render::battle_chrome as bc;
    let font = legaia_font::synthetic_for_tests();
    let hud = hud_with_party_row(250, 300, 12, 30);
    // The two party surfaces are mutually exclusive, so each is measured
    // in the frame that owns it: the panels at rest, the bar acting.
    let frame = |active: Option<u8>| -> Vec<legaia_engine_render::TextDraw> {
        battle_hud_draws_for(
            &font,
            &BattleHudFrame {
                slots: &battle_hud_slot_views(&hud),
                solid_src: Some(SOLID),
                surface: SURFACE,
                active_slot: active,
                plaque: Some("Vahn"),
                ..Default::default()
            },
            BATTLE_HUD_PEN,
        )
        .text
    };
    let resting = frame(None);
    let acting = frame(Some(0));

    // Panel seats + row.
    let seats = bc::panel_seats(1);
    assert_eq!(
        boxes_of(&resting, bc::PANEL_BG.2 as i32, bc::PANEL_BG.3 as i32),
        vec![(seats[0] as i32, bc::PANEL_Y as i32)],
        "the mirrored panel seat drifted from battle_chrome"
    );
    assert!(
        boxes_of(&acting, bc::PANEL_BG.2 as i32, bc::PANEL_BG.3 as i32).is_empty(),
        "the roster cluster drew under the active-actor bar"
    );
    // Active-actor bar: plate footprint and name pen.
    let bar_w = (bc::BAR_INTERIOR_W + 2 * bc::PLATE_CAP_W) as i32;
    assert_eq!(
        boxes_of(&acting, bar_w, bc::PLATE_H as i32),
        vec![(bc::BAR_X as i32, bc::BAR_Y as i32)],
        "the mirrored active-actor bar drifted from battle_chrome"
    );
    assert!(
        acting.iter().any(|d| d.src != SOLID
            && d.dst.0 == bc::BAR_NAME.0 as i32 * STAGE_SCALE
            && d.dst.1 == bc::BAR_NAME.1 as i32 * STAGE_SCALE),
        "the mirrored bar name pen drifted from battle_chrome"
    );
    // Plaque: plate sized to the measured name at the pinned seat.
    let plaque = bc::name_plaque(font.layout_ascii("Vahn").advance_x as u16, false);
    assert!(
        boxes_of(
            &acting,
            bc::plate_width(plaque.interior_w) as i32,
            bc::PLATE_H as i32
        )
        .contains(&(bc::PLAQUE_X as i32, bc::PLAQUE_Y as i32)),
        "the mirrored plaque drifted from battle_chrome::name_plaque"
    );
    assert!(
        acting.iter().any(|d| d.src != SOLID
            && d.dst.0 == plaque.text.0 as i32 * STAGE_SCALE
            && d.dst.1 == plaque.text.1 as i32 * STAGE_SCALE),
        "the mirrored plaque text seat drifted from battle_chrome"
    );
}

/// Third face of the same mirror: the **command-chip clusters**. Both
/// pinned clusters and every seat on them have to agree with
/// `battle_chrome`, or the menu draws chips at coordinates nothing
/// pinned. This window is again the only crate that can see both sides.
#[test]
fn engine_ui_command_chips_mirror_the_packet_pinned_battle_chrome() {
    use legaia_engine_render::battle_chrome as bc;
    use legaia_engine_render::battle_command_ui as bcu;

    let pairs = [
        (bcu::CLUSTER_COMMAND, bc::CLUSTER_COMMAND),
        (bcu::CLUSTER_TOP_LEVEL, bc::CLUSTER_TOP_LEVEL),
        (bcu::CLUSTER_COMMIT_CONFIRM, bc::CLUSTER_COMMIT_CONFIRM),
    ];
    for (ui, vm) in pairs {
        assert_eq!(ui.centre, (vm.centre.0 as i32, vm.centre.1 as i32));
        assert_eq!(ui.dx, vm.dx as i32);
        assert_eq!(ui.dy, vm.dy as i32);
        assert_eq!(ui.interior_w, vm.interior_w as i32);
        assert_eq!(
            ui.plate_width(),
            bc::plate_width(vm.interior_w) as i32,
            "the mirrored plate width drifted from battle_chrome"
        );
        let seats = [
            (bcu::ChipSeat::Up, bc::ChipSeat::Up),
            (bcu::ChipSeat::Left, bc::ChipSeat::Left),
            (bcu::ChipSeat::Right, bc::ChipSeat::Right),
            (bcu::ChipSeat::Down, bc::ChipSeat::Down),
        ];
        for (us, vs) in seats {
            let (px, py) = vm.plate_origin(vs);
            assert_eq!(
                ui.plate_origin(us),
                (px as i32, py as i32),
                "the mirrored chip plate seat drifted from battle_chrome"
            );
            let (lx, ly) = vm.label_seat(vs);
            assert_eq!(
                ui.label_seat(us),
                (lx as i32, ly as i32),
                "the mirrored chip label pen drifted from battle_chrome"
            );
        }
        let (dx, dy, dw, dh) = vm.dpad_rect();
        assert_eq!(ui.dpad_rect(), (dx as i32, dy as i32, dw as u32, dh as u32));
    }
    // The plate 3-slice and the D-pad cell the chips sample are the
    // same rects `battle_chrome` names.
    let a = bcu::CommandChipAtlas::SHEET;
    assert_eq!(a.plate_cap_l.0 as u16, bc::PLATE_CAP_L_U);
    assert_eq!(a.plate_body.0 as u16, bc::PLATE_BODY_U);
    assert_eq!(a.plate_cap_r.0 as u16, bc::PLATE_CAP_R_U);
    for r in [a.plate_cap_l, a.plate_body, a.plate_cap_r] {
        assert_eq!(r.1 as u16, bc::PLATE_BLUE.v);
        assert_eq!(r.3 as u16, bc::PLATE_H);
    }
    assert_eq!(
        a.dpad,
        (
            bc::DPAD_GLYPH.0 as u32,
            bc::DPAD_GLYPH.1 as u32,
            bc::DPAD_GLYPH.2 as u32,
            bc::DPAD_GLYPH.3 as u32
        ),
        "the command cluster stopped sampling battle_chrome's D-pad cell"
    );
    assert_eq!(bcu::DPAD_DRAW, bc::DPAD_DRAW_W as u32);
    // One chip per ring entry, and every one of them is a pinned diamond
    // arm - there is no invented seating left on this screen.
    assert_eq!(
        bcu::MENU_SEATS.len(),
        legaia_engine_core::battle_input::BattleCommand::MENU.len(),
        "the seating table and the command ring disagree on entry count"
    );
    assert_eq!(
        bcu::MENU_SEATS
            .iter()
            .filter(|s| matches!(s, bcu::CommandSeat::Diamond(_)))
            .count(),
        4,
        "the pinned diamond has four arms and they must all be used"
    );
    // The other two phases seat on pinned arms too: the round prompt on
    // the top-level pair, the attack-mode prompt on the diamond's own
    // left / right.
    assert_eq!(
        bcu::ROUND_PROMPT_SEATS.len(),
        legaia_engine_core::battle_input::RoundChoice::PROMPT.len()
    );
    assert!(
        bcu::ROUND_PROMPT_SEATS
            .iter()
            .all(|s| matches!(s, bcu::CommandSeat::TopLevel(_)))
    );
    assert_eq!(
        bcu::ATTACK_MODE_SEATS.len(),
        legaia_engine_core::battle_input::AttackMode::PROMPT.len()
    );
    assert_eq!(bcu::ATTACK_MODE_SEATS[0], bcu::MENU_SEATS[1]);
    assert_eq!(bcu::ATTACK_MODE_SEATS[1], bcu::MENU_SEATS[2]);
    // The commit confirm seats its two chips on its own pinned pair, in
    // the engine's `Begin`, `Reselect` order.
    assert_eq!(
        bcu::COMMIT_CONFIRM_SEATS.len(),
        legaia_engine_core::battle_input::CommitChoice::PROMPT.len()
    );
    assert_eq!(
        bcu::COMMIT_CONFIRM_SEATS,
        [
            bcu::CommandSeat::Commit(bcu::ChipSeat::Left),
            bcu::CommandSeat::Commit(bcu::ChipSeat::Right),
        ]
    );
}

/// A direction press must commit the chip **drawn on that side of the
/// screen**. `engine-core` cannot see the seating table (it does not
/// link `engine-ui`), so it carries the direction → seat map as its own
/// `match`; this is where that map is held equal to the drawn geometry:
/// from every starting arm, Up commits the topmost plate, Down the
/// bottommost, Left the leftmost and Right the rightmost. The committed
/// arm is read back out of the phase the one press leaves the session
/// in (retail's direct-commit dispatch - there is no highlight step to
/// inspect).
#[test]
fn direction_presses_land_on_the_chip_drawn_on_that_side() {
    use legaia_engine_core::battle_input::{
        BattleCommand, BattleCommandInput, BattleCommandSession, CommandPhase, Resolution,
    };
    use legaia_engine_core::target_picker::SlotState;
    use legaia_engine_render::battle_command_ui as bcu;

    let party = [SlotState::alive(true, true); 3];
    let monsters = [
        SlotState::alive(true, true),
        SlotState::default(),
        SlotState::default(),
        SlotState::default(),
        SlotState::default(),
    ];
    type Dir = fn(&mut BattleCommandInput);
    type Axis = fn(&bcu::CommandSeat) -> i32;
    let dirs: [(Dir, Axis, bool); 4] = [
        (|e| e.up = true, |s| s.plate_origin().1, false),
        (|e| e.down = true, |s| s.plate_origin().1, true),
        (|e| e.left = true, |s| s.plate_origin().0, false),
        (|e| e.right = true, |s| s.plate_origin().0, true),
    ];
    for from in 0..BattleCommand::MENU.len() {
        for (dir, axis, want_max) in dirs {
            let mut s = BattleCommandSession::new(0, 0);
            s.phase = CommandPhase::Menu { cursor: from as u8 };
            let mut ev = BattleCommandInput::default();
            dir(&mut ev);
            s.input(ev, party, monsters);
            let committed = if s.attack_mode().is_some() {
                BattleCommand::Attack
            } else {
                match s.resolved() {
                    Some(Resolution::OpenItemMenu) => BattleCommand::Item,
                    Some(Resolution::OpenSpellMenu) => BattleCommand::Magic,
                    Some(Resolution::SpiritGuard) => BattleCommand::Spirit,
                    other => panic!("the press committed no ring arm: {other:?}"),
                }
            };
            let to = BattleCommand::MENU
                .iter()
                .position(|c| *c == committed)
                .expect("the committed arm is a ring arm");
            let landed = axis(&bcu::MENU_SEATS[to]);
            let extreme = bcu::MENU_SEATS
                .iter()
                .map(axis)
                .reduce(|a, b| if want_max { a.max(b) } else { a.min(b) })
                .unwrap();
            assert_eq!(
                landed, extreme,
                "from arm {from}, the press did not land on the outermost \
                 drawn chip along its axis (want_max={want_max})"
            );
        }
    }
}

/// The sibling half of the mirror check: the numeral fields. Every one is
/// a right edge the field grows leftward from in 8-px cells, and the
/// `engine-ui` literals have to name the same edges `battle_chrome` pins
/// - a drift here is a four-digit HP drawn off the end of its panel.
#[test]
fn engine_ui_numeral_edges_mirror_the_packet_pinned_battle_chrome() {
    use legaia_engine_render::battle_chrome as bc;
    let font = legaia_font::synthetic_for_tests();
    // Widest values every field is laid out against.
    let hud = hud_with_party_row(9999, 9999, 999, 999);
    let cells = |active: Option<u8>| -> Vec<(i32, i32)> {
        battle_hud_draws_for(
            &font,
            &BattleHudFrame {
                slots: &battle_hud_slot_views(&hud),
                solid_src: Some(SOLID),
                surface: SURFACE,
                chrome: Some(&BATTLE_MIRROR_RECTS),
                active_slot: active,
                ..Default::default()
            },
            BATTLE_HUD_PEN,
        )
        .sprites
        .iter()
        .filter(|s| s.src.1 == BATTLE_MIRROR_DIGITS.1 && s.src.2 == bc::DIGIT_W as u32)
        .map(|s| (s.dst.0 / STAGE_SCALE, s.dst.1 / STAGE_SCALE))
        .collect()
    };

    // Panel: the HP row's two four-cell runs, at the pinned right edges.
    let px = bc::panel_seats(1)[0] as i32;
    let panel = cells(None);
    for (right, digits, y) in [
        (bc::panel::CUR_RIGHT, 4, bc::panel::HP_DIGIT_Y),
        (bc::panel::MAX_RIGHT, 4, bc::panel::HP_DIGIT_Y),
        (bc::panel::CUR_RIGHT, 3, bc::panel::MP_DIGIT_Y),
        (bc::panel::MAX_RIGHT, 3, bc::panel::MP_DIGIT_Y),
    ] {
        let left = px + bc::digits_left_of(right, digits) as i32;
        let row = bc::PANEL_Y as i32 + y as i32;
        assert!(
            panel.contains(&(left, row)),
            "no numeral cell at the mirrored panel edge {right} ({digits} digits): {panel:?}"
        );
        assert!(
            panel
                .iter()
                .all(|(x, _)| x + bc::DIGIT_W as i32 <= px + bc::PANEL_BG.2 as i32),
            "a panel numeral runs past the 102-px plate: {panel:?}"
        );
    }

    // Bar: four cells per HP field, three per MP field.
    let bar = cells(Some(0));
    let y = bc::BAR_DIGIT_Y as i32;
    for (right, digits) in [
        (bc::BAR_HP_CUR_RIGHT, 4),
        (bc::BAR_HP_MAX_RIGHT, 4),
        (bc::BAR_MP_CUR_RIGHT, 3),
        (bc::BAR_MP_MAX_RIGHT, 3),
    ] {
        let left = bc::digits_left_of(right, digits) as i32;
        assert!(
            bar.contains(&(left, y)),
            "no numeral cell at the mirrored bar edge {right} ({digits} digits): {bar:?}"
        );
    }
}

/// Retail draws **no monster gauge at all**
/// (`docs/subsystems/battle-action.md`), so a monster contributes nothing
/// to the default surface - and everything it used to contribute has to
/// still be reachable under `LEGAIA_DIAG_HUD`.
#[test]
fn monster_rows_are_diagnostic_only() {
    let mut hud = hud_with_party_row(100, 100, 0, 0);
    hud.sync_slot(
        3,
        SlotSyncInfo {
            name: "Goblin",
            is_party: false,
            alive: true,
            hp: 40,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap: None,
        },
    );
    let monster_row_y = BATTLE_HUD_PEN.1 + 3 * 14;
    assert!(
        !frame_draws(&hud, false)
            .text
            .iter()
            .any(|d| d.dst.1 == monster_row_y),
        "a monster row drew on the default surface"
    );
    assert!(
        frame_draws(&hud, true)
            .text
            .iter()
            .any(|d| d.dst.1 == monster_row_y),
        "the diagnostic surface lost the monster row"
    );
}

/// The retail readout-tint law has to reach the **surface**, not just
/// exist in engine-ui: normal / caution / danger numerals must each take
/// their own tier's colour.
///
/// Expectations come from `gauge_fill_color`, retail's own law, rather
/// than from literals. This test used to carry `[1.0, 0.95, 0.4, 1.0]`
/// ("builder's yellow") and `[1.0, 0.4, 0.4, 1.0]` ("builder's red") -
/// the port's pre-VRAM approximations - so once the colours were pinned
/// off a retail frame it failed while asserting nothing retail does.
/// What it always meant to protect is that the law reaches this host and
/// separates the tiers; both survive, and neither is spelled here.
#[test]
fn native_battle_hud_hp_tints_span_the_retail_tiers() {
    let glyph_colors = |hp: u16| -> Vec<[f32; 4]> {
        let hud = hud_with_party_row(hp, 100, 0, 0);
        draws(&hud)
            .iter()
            .filter(|d| d.src != SOLID)
            .map(|d| d.color)
            .collect()
    };
    // Retail's tier ids: 7 normal, 6 caution, 9 danger.
    let caution = legaia_engine_render::gauge_fill_color(6);
    let danger = legaia_engine_render::gauge_fill_color(9);
    // A law whose tiers collapsed to one colour would satisfy every
    // "contains" below while drawing a single flat readout.
    assert!(
        caution != danger
            && caution != legaia_engine_render::READOUT_NORMAL
            && danger != legaia_engine_render::READOUT_NORMAL,
        "the three tiers must be visually distinct"
    );
    assert!(
        !glyph_colors(90)
            .iter()
            .any(|c| *c == caution || *c == danger),
        "normal tier numerals took a warning tint"
    );
    assert!(
        glyph_colors(40).contains(&caution),
        "caution tier numerals do not take the tier-6 colour"
    );
    assert!(
        glyph_colors(20).contains(&danger),
        "danger tier numerals do not take the tier-9 colour"
    );
}

/// The production path: `engine-ui`'s `party_panel_stage_x` (re-exported
/// by `engine-render`, called by `battle_hud_draws_for`'s roster loop)
/// reads the canonical `engine-vm` port of retail's `FUN_801D84C0`
/// anchor table - `panel_anchors` - rather than mirroring it as
/// literals. Assert the production function returns the kernel's values
/// for every party size retail writes an anchor for, and that the seats
/// the table leaves unwritten fall back to the packet-pinned panel seat
/// plus the +5 name inset.
#[test]
fn panel_stage_x_production_path_returns_the_kernel_anchors() {
    use legaia_engine_vm::battle_party_panel::panel_anchors;
    for size in 1usize..=3 {
        let (primary, secondary) =
            panel_anchors(size as u8).expect("party sizes 1..=3 take a build arm");
        assert_eq!(
            legaia_engine_render::party_panel_stage_x(size, 0),
            i32::from(primary),
            "primary anchor for a party of {size}"
        );
        if let Some(sec) = secondary {
            assert_eq!(
                legaia_engine_render::party_panel_stage_x(size, 1),
                i32::from(sec),
                "secondary anchor for a party of {size}"
            );
        }
    }
    // The seat retail writes no anchor for (a full party's third
    // panel): its packet-pinned seat plus the +5 name inset.
    assert_eq!(
        legaia_engine_render::party_panel_stage_x(3, 2),
        i32::from(legaia_engine_render::battle_chrome::panel_seats(3)[2])
            + i32::from(legaia_engine_render::battle_chrome::PANEL_TEXT_INSET),
        "unwritten third seat is not seat + inset"
    );
}

/// The end-to-end wiring: a live `World` battle state must reach the
/// shared builder's draw list, MP included.
///
/// This is the assertion that fails if `sync_battle_hud_rows` is dropped
/// from the tick - the HUD model's slots stay `active == false`, the
/// builder skips every empty-name row, and `draws` comes back empty.
#[test]
fn live_world_battle_state_reaches_the_shared_builder() {
    use legaia_engine_core::world::World;

    let mut world = World::new();
    world.party.party_count = 1;
    world.actors[0].active = true;
    world.actors[0].battle.liveness = 1;
    world.actors[0].battle.hp = 250;
    world.actors[0].battle.max_hp = 300;
    world.actors[0].battle.mp = 12;
    world.set_character_max_mp(0, 30);

    let mut hud = legaia_engine_core::battle_hud::BattleHud::new();
    super::super::battle::sync_battle_hud_rows(&mut hud, &world);
    assert!(hud.slots[0].active, "party slot 0 did not sync");
    assert_eq!(
        hud.slots[0].mp_max, 30,
        "MP ceiling did not reach the model"
    );

    let out = draws(&hud);
    assert!(!out.is_empty(), "synced battle state produced no draws");
    // The MP field only draws for a slot carrying a ceiling, so the live
    // world's MP has to reach the panel's pinned MP row.
    assert!(
        out.iter()
            .any(|d| d.src != SOLID && d.dst.1 == (164 + 34) * STAGE_SCALE),
        "live world state produced no MP field on the panel's MP row"
    );
}

/// Popups carry an absolute actor slot. The **diagnostic** readout
/// anchors them by slice index, so the projection must keep inactive
/// slots in place - a compacted list would put a monster's damage number
/// on a party row. The default surface no longer draws them at all:
/// retail's landed-hit numeral is seated over the struck actor, which
/// only a host holding the camera can place, so it is the window's own
/// `battle_value_readout_prims` (see `engine-ui::battle_numerals`).
#[test]
fn popup_anchors_track_absolute_actor_slot() {
    let mut hud = hud_with_party_row(100, 100, 0, 0);
    // Slots 1 and 2 stay empty; the monster occupies slot 3.
    hud.sync_slot(
        3,
        SlotSyncInfo {
            name: "Goblin",
            is_party: false,
            alive: true,
            hp: 40,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap: None,
        },
    );
    hud.push_popup(DamagePopup::damage(3, 25));
    let out = frame_draws(&hud, true).text;
    // Row stride is 14; monster slot 3's row sits at pen.y + 42, popups
    // 16 above (monster popups keep the index-anchored surface layout).
    let want_y = BATTLE_HUD_PEN.1 + 3 * 14 - 16;
    let popup_x = BATTLE_HUD_PEN.0 + 80;
    assert!(
        out.iter().any(|d| d.dst.1 == want_y && d.dst.0 >= popup_x),
        "no popup glyph at slot 3's anchor (y={want_y})"
    );
}

/// `engine-render`'s HUD tests repeat the badge block's atlas layout as
/// literals, because that crate sits below `engine-core` and cannot
/// import the bake. This is the seam that keeps the copy honest - the
/// same job `engine_ui_command_chips_mirror_the_packet_pinned_battle_chrome`
/// does for the chip cluster.
#[test]
fn badge_atlas_seats_match_the_bake() {
    use legaia_engine_core::save_menu_atlas as sma;
    for i in 0..sma::STATUS_BADGE_COUNT {
        assert_eq!(
            sma::status_badge_atlas_rect(i),
            (
                48 * (i as u32 % 4),
                128 + 16 * (i as u32 / 4),
                sma::STATUS_BADGE_W,
                sma::STATUS_BADGE_H
            ),
            "status badge {i} atlas seat drifted from the mirrored layout"
        );
    }
    for i in 0..sma::ELEMENT_BADGE_COUNT {
        assert_eq!(
            sma::element_badge_atlas_rect(i),
            (
                20 * i as u32,
                176,
                sma::ELEMENT_BADGE_W,
                sma::ELEMENT_BADGE_H
            ),
            "element badge {i} atlas seat drifted from the mirrored layout"
        );
    }
    // The badge block must not land on anything the atlas already
    // carries; these are the neighbours it was seated between.
    let (bx, by) = sma::ATLAS_RECT_STATUS_BADGES_ORIGIN;
    assert_eq!((bx, by), (0, 128));
    assert!(
        by >= 128 && by + 3 * sma::STATUS_BADGE_H <= sma::ATLAS_RECT_ELEMENT_BADGES_ORIGIN.1,
        "the status block overruns the element strip"
    );
    const {
        assert!(
            4 * sma::STATUS_BADGE_W <= 200,
            "the status block reaches the arts chip triple at x=200"
        )
    };
    assert!(
        sma::ATLAS_RECT_ELEMENT_BADGES_ORIGIN.1 + sma::ELEMENT_BADGE_H
            <= sma::ATLAS_RECT_FILIGREE.1,
        "the element strip overruns the filigree tile"
    );
}
