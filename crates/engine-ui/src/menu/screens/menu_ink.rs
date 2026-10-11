//! The menu text inks and the HP / MP health-tier ink ladder.

/// Retail body-text white (menu ink 7): every CLUT-7 staged glyph reads
/// back as RGB `(206, 206, 206)` in the golden `menu_status_town`
/// capture. Stored as that byte value straight - `206/255` - because
/// nothing on the path converts colour spaces: [`TextDraw`] colours
/// multiply the whitewashed font atlas and land in a UNORM attachment,
/// so the presented pixel is the value written (see the renderer's
/// `choose_surface_format`). Same ink as [`OPTIONS_INK_WHITE`].
///
/// [`OPTIONS_INK_WHITE`]: crate::OPTIONS_INK_WHITE
pub const MENU_TEXT_WHITE: [f32; 4] = [0.807_843_1, 0.807_843_1, 0.807_843_1, 1.0];
/// Retail teal ink for the parenthesised base/growth values on the
/// status page (the HP/MP `( base)` group and the stat grid's
/// `( growth)` group, parens included): RGB `(66, 222, 222)` in the
/// golden capture - the CLUT row the separator-staging value 5 selects.
/// Byte values, like [`MENU_TEXT_WHITE`].
pub const MENU_TEXT_TEAL: [f32; 4] = [0.258_823_5, 0.870_588_2, 0.870_588_2, 1.0];
/// Retail gold/yellow warning ink - staging id 6. The string-CLUT rows
/// live at VRAM row 510, cell `x = 16*(6+id)`; the main ink is palette
/// entry 15, and id 6 decodes to RGB `(231, 173, 0)` (read from the
/// golden `menu_status_town` VRAM).
pub const MENU_TEXT_GOLD: [f32; 4] = [0.905_882_4, 0.678_431_4, 0.0, 1.0];
/// Retail orange "critical" ink - staging id 9, RGB `(222, 90, 0)`
/// (same VRAM CLUT-row decode as [`MENU_TEXT_GOLD`]).
pub const MENU_TEXT_ORANGE: [f32; 4] = [0.870_588_2, 0.352_941_2, 0.0, 1.0];
/// Retail red "downed" ink - staging id 2, RGB `(231, 33, 0)`
/// (same VRAM CLUT-row decode as [`MENU_TEXT_GOLD`]).
pub const MENU_TEXT_RED: [f32; 4] = [0.905_882_4, 0.129_411_8, 0.0, 1.0];

/// Menu HP health-tier ink - the retail per-character colour fn
/// `FUN_800349EC` the status / party panels stage before drawing the
/// HP current+max number fields:
///
/// - `hp == 0` -> staging 2 (red, [`MENU_TEXT_RED`])
/// - `hp <= max/4` -> staging 9 (orange, [`MENU_TEXT_ORANGE`])
/// - `hp <= max/2` -> staging 6 (gold, [`MENU_TEXT_GOLD`])
/// - else -> staging 7 (white, [`MENU_TEXT_WHITE`])
///
/// Retail also forces the gold tier when the char-record `+0x12E` status
/// halfword is non-zero - see [`menu_hp_ink_with_status`], which this
/// function is the `status == 0` case of.
///
/// PORT: FUN_800349EC - menu HP ink tier (record `+0x104`/`+0x106`
/// thresholds at max/4 and max/2).
pub fn menu_hp_ink(hp: u16, hp_max: u16) -> [f32; 4] {
    menu_hp_ink_with_status(hp, hp_max, 0)
}

/// The whole of `FUN_800349EC`, ailment arm included.
///
/// Retail's five tests run in this order (`0x800349EC..0x80034A68`), and
/// the ailment arm sits **between** the two HP thresholds rather than
/// ahead of them - so a character below a quarter HP stays orange even
/// while poisoned, and only the `max/4 < hp` band can be re-inked gold:
///
/// 1. `hp == 0` -> staging 2 (red, [`MENU_TEXT_RED`]);
/// 2. `hp <= max/4` -> staging 9 (orange, [`MENU_TEXT_ORANGE`]);
/// 3. `status != 0` -> staging 6 (gold, [`MENU_TEXT_GOLD`]);
/// 4. `hp <= max/2` -> staging 6 (gold);
/// 5. else -> staging 7 (white, [`MENU_TEXT_WHITE`]).
///
/// `status` is the char-record `+0x12E` battle-status halfword (read as a
/// signed halfword; any non-zero value re-inks). `hp` / `hp_max` are the
/// **live** pair `+0x106` / `+0x104`, not the record copies.
///
/// The ailment arm has a live input on both hosts: [`FieldMenuPartyView`]
/// and [`StatusPanelView`] each carry the `+0x12E` word, filled from
/// `legaia_engine_core::status_screen::StatusSnapshot::status_flags` -
/// which `field_menu_dispatch::status_snapshots` reads off the live
/// `World::battle.status_effects` tracker (`display_flags`, the `+0x16E` mirror
/// retail's `FUN_80047430` copies into the record). The pause-menu party
/// panel and the Status page both call this function directly, so test
/// three fires from a real session whenever a party member carries an
/// ailment out of a battle. [`menu_hp_ink`] remains the `status == 0`
/// convenience for surfaces with no roster behind them.
///
/// PORT: FUN_800349EC
pub fn menu_hp_ink_with_status(hp: u16, hp_max: u16, status: i16) -> [f32; 4] {
    if hp == 0 {
        MENU_TEXT_RED
    } else if hp <= hp_max / 4 {
        MENU_TEXT_ORANGE
    } else if status != 0 || hp <= hp_max / 2 {
        MENU_TEXT_GOLD
    } else {
        MENU_TEXT_WHITE
    }
}

/// Menu MP tier ink - retail `FUN_80035EA8`: `mp <= max/4` -> orange
/// (staging 9), `mp <= max/2` -> gold (staging 6), else white
/// (staging 7). Unlike HP there is no zero special-case.
///
/// PORT: FUN_80035EA8 - menu MP ink tier (record `+0x108`/`+0x10A`).
pub fn menu_mp_ink(mp: u16, mp_max: u16) -> [f32; 4] {
    if mp > mp_max / 4 {
        if mp <= mp_max / 2 {
            MENU_TEXT_GOLD
        } else {
            MENU_TEXT_WHITE
        }
    } else {
        MENU_TEXT_ORANGE
    }
}

#[cfg(test)]
mod health_tier_ink_tests {
    use super::*;
    use crate::FieldMenuPartyView;
    use crate::StatusPanelView;
    use crate::field_menu_info_draws_for;
    use crate::status_screen_draws_for;

    /// The ailment arm reaches a real draw list, not just the kernel.
    ///
    /// Both panels ink their HP number through `FUN_800349EC` with the
    /// member's `+0x12E` word, so a party member at a healthy HP fraction
    /// draws **gold** while poisoned and **white** while clean. This is the
    /// output end of the wire that carries `StatusSnapshot::status_flags`
    /// onto the two view structs; the kernel's own thresholds are covered
    /// above.
    #[test]
    fn a_non_zero_status_word_re_inks_the_party_panel_hp_number() {
        let font = legaia_font::Font::placeholder();
        let member = |status: u16| FieldMenuPartyView {
            name: "Vahn",
            level: 5,
            hp: 180,
            hp_max: 180,
            mp: 10,
            mp_max: 12,
            ap: 40,
            status,
        };
        let inks = |status: u16| -> Vec<[f32; 4]> {
            field_menu_info_draws_for(&font, &[member(status)], (0, 0))
                .into_iter()
                .map(|d| d.color)
                .collect()
        };
        let clean = inks(0);
        let ailing = inks(1);
        assert!(
            clean.contains(&MENU_TEXT_WHITE),
            "a full-HP member should ink some glyph white"
        );
        assert!(
            !clean.contains(&MENU_TEXT_GOLD),
            "nothing on a clean full-HP member should be gold"
        );
        assert!(
            ailing.contains(&MENU_TEXT_GOLD),
            "a non-zero +0x12E word must re-ink the HP number gold"
        );
        assert_eq!(clean.len(), ailing.len(), "only the ink may differ");
    }

    /// Same arm on the Status page, which inks through the same kernel.
    #[test]
    fn a_non_zero_status_word_re_inks_the_status_panel_hp_number() {
        let font = legaia_font::Font::placeholder();
        let panel = |status: u16| StatusPanelView {
            name: "Vahn",
            level: 5,
            xp: 100,
            xp_to_next: 200,
            hp: 180,
            hp_max: 180,
            mp: 10,
            mp_max: 12,
            ap: 40,
            ap_max: 100,
            stat_rows: &[],
            equip_rows: &[],
            status,
        };
        let golds = |status: u16| -> usize {
            status_screen_draws_for(&font, &panel(status), None, (0, 0), true)
                .into_iter()
                .filter(|d| d.color == MENU_TEXT_GOLD)
                .count()
        };
        assert_eq!(golds(0), 0, "a clean full-HP character wears no gold ink");
        assert!(golds(1) > 0, "a non-zero +0x12E word must ink the HP gold");
    }

    /// The retail HP ink thresholds (`FUN_800349EC`): red at 0, orange
    /// at `<= max/4`, gold at `<= max/2`, white above. Boundaries are
    /// inclusive on the low side (the decompile tests `max>>2 < hp`).
    #[test]
    fn hp_tiers_match_the_retail_thresholds() {
        let max = 180;
        assert_eq!(menu_hp_ink(0, max), MENU_TEXT_RED);
        assert_eq!(menu_hp_ink(1, max), MENU_TEXT_ORANGE);
        assert_eq!(menu_hp_ink(45, max), MENU_TEXT_ORANGE); // == max/4
        assert_eq!(menu_hp_ink(46, max), MENU_TEXT_GOLD);
        assert_eq!(menu_hp_ink(90, max), MENU_TEXT_GOLD); // == max/2
        assert_eq!(menu_hp_ink(91, max), MENU_TEXT_WHITE);
        assert_eq!(menu_hp_ink(180, max), MENU_TEXT_WHITE);
    }

    /// MP tiers (`FUN_80035EA8`): same quarter/half thresholds but no
    /// zero special-case - 0 MP sits in the orange tier.
    #[test]
    fn mp_tiers_match_the_retail_thresholds() {
        let max = 20;
        assert_eq!(menu_mp_ink(0, max), MENU_TEXT_ORANGE);
        assert_eq!(menu_mp_ink(5, max), MENU_TEXT_ORANGE); // == max/4
        assert_eq!(menu_mp_ink(6, max), MENU_TEXT_GOLD);
        assert_eq!(menu_mp_ink(10, max), MENU_TEXT_GOLD); // == max/2
        assert_eq!(menu_mp_ink(11, max), MENU_TEXT_WHITE);
        assert_eq!(menu_mp_ink(20, max), MENU_TEXT_WHITE);
    }

    /// The ailment arm sits between the two HP thresholds, so it re-inks
    /// only the band above `max/4` - a dying character stays orange and a
    /// downed one stays red.
    #[test]
    fn hp_ink_ailment_arm_only_reaches_the_upper_band() {
        let max = 40u16;
        assert_eq!(menu_hp_ink_with_status(0, max, 1), MENU_TEXT_RED);
        assert_eq!(menu_hp_ink_with_status(10, max, 1), MENU_TEXT_ORANGE);
        assert_eq!(menu_hp_ink_with_status(11, max, 1), MENU_TEXT_GOLD);
        assert_eq!(menu_hp_ink_with_status(40, max, 1), MENU_TEXT_GOLD);
        // Healthy and clear stays white; the arm is the only difference.
        assert_eq!(menu_hp_ink_with_status(40, max, 0), MENU_TEXT_WHITE);
        assert_eq!(menu_hp_ink_with_status(40, max, -1), MENU_TEXT_GOLD);
        // `menu_hp_ink` is exactly the `status == 0` case.
        for hp in [0u16, 1, 10, 11, 20, 21, 40] {
            assert_eq!(menu_hp_ink(hp, max), menu_hp_ink_with_status(hp, max, 0));
        }
    }
}
