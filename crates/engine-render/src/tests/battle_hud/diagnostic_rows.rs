//! Log lines, popups, monster rows, and the level / ailment cell on a member's panel.

use super::*;
use legaia_engine_ui::HudLogView;
use legaia_engine_ui::HudPopupView;
use legaia_engine_ui::HudSlotView;
use legaia_engine_ui::battle_hud_chrome;

#[test]
fn battle_hud_draws_for_includes_log_lines_below_slots() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let log = [HudLogView {
        text: "Vahn attacks.",
        color: [1.0, 1.0, 1.0, 1.0],
    }];
    let n_no_log = hud_draws(&font, &[slot], &[], &[]).len();
    let draws_with_log = hud_draws(&font, &[slot], &[], &log);
    assert!(draws_with_log.len() > n_no_log);
}

/// The HUD's text popup is the **diagnostic** readout of a hit, not the retail
/// one: retail throws its numeral off the effect atlas
/// ([`legaia_engine_battle_vm::battle_value_readout`]) over the actor that was struck,
/// so nothing about a landed hit reaches the default surface here. The anchor
/// this asserts is therefore the diagnostic column's, and the retail seat is
/// covered by the readout module's own tests.
#[test]
fn battle_hud_popup_is_a_diagnostic_row_riding_its_panel_anchor() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let popup = HudPopupView {
        slot: 0,
        amount: 250,
        is_heal: false,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    assert_eq!(
        hud_draws(&font, &[slot], &[popup], &[]).len(),
        hud_draws(&font, &[slot], &[], &[]).len(),
        "a popup drew on the default retail surface"
    );

    let n_no_popup = hud_frame(&font, &[slot], &[], &[], true).text.len();
    let draws = hud_frame(&font, &[slot], &[popup], &[], true).text;
    assert!(draws.len() > n_no_popup, "popup produced no glyphs");
    // Damage popups draw in the cyan tint, above the member's panel (a
    // stage-space anchor, unlike the diagnostic rows' pen anchor).
    let cyan = [0.5, 0.85, 1.0, 1.0];
    assert!(
        draws.iter().any(|d| d.src != SOLID && d.color == cyan),
        "no cyan popup glyph"
    );
}

/// Monster rows are the **diagnostic** surface now: retail's HUD draws no
/// monster gauge at all, so nothing about a monster reaches the default list.
#[test]
fn monster_rows_only_draw_under_the_diagnostic_toggle() {
    let font = legaia_font::synthetic_for_tests();
    let mut slot = slot_view("Gimard", false, true, 100, 100, 0, 0);
    // Sprite 0x19 = Toxic, the element retail's ladder picks for `0x0002`.
    slot.status_sprite = 0x19;
    assert!(
        hud_draws(&font, &[slot], &[], &[]).is_empty(),
        "a monster row drew on the default retail surface"
    );

    let diag = hud_frame(&font, &[slot], &[], &[], true).text;
    // The element renders past the column origin at pen.x + 190.
    assert!(
        diag.iter().filter(|d| d.dst.0 >= PEN.0 + 190).count() > 0,
        "expected a status element in the diagnostic monster column"
    );

    // And it is exactly ONE element regardless of how many ailments the slot
    // carries - the whole point of the retail ladder.
    let clean = slot_view("Gimard", false, true, 100, 100, 0, 0);
    let none = hud_frame(&font, &[clean], &[], &[], true).text;
    assert_eq!(
        none.iter().filter(|d| d.dst.0 >= PEN.0 + 190).count(),
        0,
        "an unafflicted monster row draws no status element"
    );
}

/// `FUN_8002C2E4`'s no-ailment arm is the level, and retail draws it as a
/// **panel row** - LV label plus digits on the panel's top cell - not as the
/// floating marker the port used to put over the party.
///
/// The digits are right-aligned to `+96` like every other number on the
/// screen, so a one-digit level occupies `+88` and a two-digit one `+80` and
/// `+88`. Both cases are packet-pinned: a solo capture draws level `1` at
/// panel `+88`, a three-party frame draws level `99` at `+80` / `+88`.
#[test]
fn level_draws_on_the_panels_top_cell() {
    let font = legaia_font::synthetic_for_tests();
    let mut lv = slot_view("Vahn", true, true, 100, 100, 30, 30);
    lv.level = 27;
    let mut no_lv = slot_view("Vahn", true, true, 100, 100, 30, 30);
    no_lv.level = 0;

    let with = hud_draws(&font, &[lv], &[], &[]);
    assert!(
        with.len() > hud_draws(&font, &[no_lv], &[], &[]).len(),
        "the level readout never drew"
    );
    let cells = numeral_cells(&hud_frame_with_chrome(&font, &[lv], None));
    assert_eq!(
        numerals_on_row(&cells, PANEL_Y + 4),
        vec![(SOLO_PANEL_X + 80, "27".into())],
        "a two-digit level is not right-aligned to the panel's +96"
    );
    let mut single = lv;
    single.level = 1;
    assert_eq!(
        numerals_on_row(
            &numeral_cells(&hud_frame_with_chrome(&font, &[single], None)),
            PANEL_Y + 4
        ),
        vec![(SOLO_PANEL_X + 88, "1".into())],
        "a one-digit level is not at the solo capture's +88"
    );
}

/// An ailment **replaces** the level on the member's panel - `FUN_8002C2E4`
/// draws exactly one element per slot. The selection is the ported part; the
/// tag's art is an approximation of an unresolved sprite sheet.
#[test]
fn ailment_replaces_the_level_on_the_members_panel() {
    let font = legaia_font::synthetic_for_tests();
    let mut clean = slot_view("Vahn", true, true, 100, 100, 30, 30);
    clean.level = 27;
    let mut sick = slot_view("Vahn", true, true, 100, 100, 30, 30);
    sick.level = 27;
    sick.status_sprite = 0x1F;
    let with = hud_draws(&font, &[sick], &[], &[]);
    // `FUN_8002C2E4`'s ladder is exclusive: the ailment REPLACES the level
    // element rather than joining it, so the LV label sprite stops drawing.
    //
    // The tag stands in for retail's 48x16 badge **cell**, whose seat is the
    // ladder arm's `pen + (0x33, -4)` = panel `+(56, 0)` - not the `+(64, 6)`
    // `LV` label seat this test used to pin it to. The cell is capture-pinned:
    // a retail battle frame with a downed member queues widget record `0x20`
    // as a 48x16 `SPRT` at `(panel.x + 56, panel.y)`.
    let cell = (
        SOLO_PANEL_X + battle_hud_chrome::STATUS_BADGE_PANEL_SEAT.0,
        PANEL_Y + battle_hud_chrome::STATUS_BADGE_PANEL_SEAT.1,
    );
    let tag: Vec<_> = with
        .iter()
        .filter(|d| {
            d.src != SOLID
                && d.dst.0 >= cell.0 * STAGE_SCALE
                && d.dst.0 < (cell.0 + battle_hud_chrome::STATUS_BADGE_SIZE.0) * STAGE_SCALE
                && d.dst.1 >= cell.1 * STAGE_SCALE
                && d.dst.1 < (cell.1 + battle_hud_chrome::STATUS_BADGE_SIZE.1) * STAGE_SCALE
        })
        .collect();
    assert!(
        !tag.is_empty(),
        "the ailment tag is not inside the retail badge cell at panel +(56, 0)"
    );
    // Non-vacuous: it must be the *tag*, in the ailment's own colour, and it
    // must sit clear of the LV label seat the level would have taken.
    assert!(
        tag.iter()
            .all(|d| d.color == legaia_engine_ui::status_element_color(0x1F)),
        "a non-tag glyph landed in the badge cell"
    );
    let lv_row = |slot: HudSlotView<'_>| -> Vec<(i32, String)> {
        numerals_on_row(
            &numeral_cells(&hud_frame_with_chrome(&font, &[slot], None)),
            PANEL_Y + 4,
        )
    };
    assert_eq!(
        lv_row(clean),
        vec![(SOLO_PANEL_X + 80, "27".into())],
        "the unafflicted slot lost its level readout"
    );
    assert!(
        lv_row(sick).is_empty(),
        "the level drew alongside an ailment - the retail ladder is exclusive"
    );
}

#[test]
fn battle_hud_draws_for_popup_for_invalid_slot_is_dropped() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let popup = HudPopupView {
        slot: 99,
        amount: 50,
        is_heal: false,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    // Measured on the diagnostic frame: popups no longer reach the default
    // surface at all, so comparing there would pass for the wrong reason - the
    // out-of-range guard would be untested rather than proven.
    let with_popup = hud_frame(&font, &[slot], &[popup], &[], true).text;
    let no_popup = hud_frame(&font, &[slot], &[], &[], true).text;
    assert_eq!(with_popup.len(), no_popup.len());
    // Non-vacuity: an in-range popup on the same frame does add glyphs.
    let valid = HudPopupView { slot: 0, ..popup };
    assert!(hud_frame(&font, &[slot], &[valid], &[], true).text.len() > no_popup.len());
}

#[test]
fn battle_hud_draws_for_heal_popup_uses_green_tint() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let popup = HudPopupView {
        slot: 0,
        amount: 60,
        is_heal: true,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    let draws = hud_frame(&font, &[slot], &[popup], &[], true).text;
    // Heal color is green: [0.5, 1.0, 0.5, 1.0]; any glyph with that profile.
    let any_green = draws
        .iter()
        .any(|d| d.src != SOLID && d.color[1] >= 0.95 && d.color[0] < d.color[1]);
    assert!(any_green);
}
