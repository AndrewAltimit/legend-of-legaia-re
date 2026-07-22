//! Party **target panel** (pause-menu window 14) draw builder - the
//! column that replaces the item list while the field Use flow picks a
//! target.
//!
//! Retail renderer: `FUN_801D0520` (menu overlay 0899), dispatched as
//! window descriptor 14 (content rect `(174, 28, 132, 176)`). The layout
//! here is instruction-traced from
//! `ghidra/scripts/funcs/overlay_menu_801d0520.txt`; see
//! `docs/subsystems/field-menu.md` for the full pen catalogue with
//! instruction addresses.
//!
//! Shape summary (all pens relative to the window content origin, one
//! block per roster member at the 0x3E pitch):
//!
//! - name (staged 7 white) at `X+0x14`, LV icon at `(X+0x58, Yb+2)` with
//!   the 2-digit level at `X+0x68`;
//! - **plain modes** (preview word `DAT_801E46CC` = 0, and the stat modes
//!   2/4/5): `HP cur / max` with the `FUN_800349EC` tier ink (cur at
//!   `X+0x2C`, slash glyph cell 6 at `X+0x4C`, max at `X+0x54`, row
//!   `Yb+0xF`) and the `FUN_80035EA8`-tiered MP row at `Yb+0x1C`;
//! - **mode 1** (Life / Magic Water preview): `eff_max ( base_max )` for
//!   HP and MP - effective maximum (record `+0x104`/`+0x108`) white at
//!   `X+0x38`, the teal paren group (glyph cells 7/8 staged 5) at
//!   `X+0x58`/`X+0x80` around the record-side base maximum
//!   (`+0x11C`/`+0x11E`) at `X+0x60`;
//! - **modes 2..5** (Power / Guardian / Swift / Wisdom Water previews):
//!   a stat row `LBL eff ( base )` at `Yb+0x29` - label white at
//!   `X+0x1C`, aggregator value (`FUN_801CF650`, clamped 999) 3-digit at
//!   `X+0x44`, paren group at `X+0x5C`/`X+0x7C` around the record base
//!   stat at `X+0x64`. Mode 3 (Guardian Water) draws **two** rows (UDF at
//!   `Yb+0x1C`, LDF at `Yb+0x29`) and skips the plain MP row.
//!
//! The hand cursor decodes the target cursor word `DAT_801E46C4`: bit
//! `0x4000` hides it, bit `0x2000` puts it on **every** member row (the
//! all-party items), otherwise the low 12 bits pick one row; bit `0x1000`
//! switches the sprite to its static variant while a confirm is staged.

use super::field_panels::{
    MENU_TEXT_GOLD, MENU_TEXT_TEAL, MENU_TEXT_WHITE, menu_hp_ink, menu_mp_ink, num_field_draws,
};
use crate::*;

/// Window-14 content rect from the descriptor table (`(174, 28, 132,
/// 176)` - id 14 of the `0x801E4738` table).
pub const TARGET_PANEL_RECT: (i32, i32, i32, i32) = (174, 28, 132, 176);

/// Per-member block pitch (`FUN_801D0520` advances every pen by `0x3E`
/// per roster row - `801d0ce2 addiu s7,s7,0x3e`).
pub const TARGET_PANEL_MEMBER_PITCH: i32 = 0x3e;

/// Preview shape of the target panel - the retail preview word
/// `DAT_801E46CC`, derived from the picked item by `FUN_801D6A54`
/// (`engine-core::pause_screens::target_panel_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetPanelMode {
    /// Mode 0: plain `HP cur/max` + `MP cur/max` rows (every
    /// non-stat-item pick).
    #[default]
    Plain,
    /// Mode 1: HP + MP `eff ( base )` maxima preview (Life Water /
    /// Magic Water).
    HpMp,
    /// Mode 2: ATK stat row (Power Water).
    Atk,
    /// Mode 3: UDF + LDF stat rows (Guardian Water; the plain MP row is
    /// skipped to make room).
    Def,
    /// Mode 4: SPD stat row (Swift Water).
    Spd,
    /// Mode 5: INT stat row (Wisdom Water).
    Int,
}

impl TargetPanelMode {
    /// Map the retail preview word value (0..=5) onto the mode.
    pub fn from_preview_word(w: u32) -> Self {
        match w {
            1 => Self::HpMp,
            2 => Self::Atk,
            3 => Self::Def,
            4 => Self::Spd,
            5 => Self::Int,
            _ => Self::Plain,
        }
    }
}

/// Stat indices of [`TargetPanelMember::stat_eff`] / `stat_base`,
/// matching the retail label order (`"ATK" / "UDF" / "LDF" / "SPD" /
/// "INT"`, menu-overlay rodata `0x801CE9A0..`).
pub const TARGET_PANEL_STAT_LABELS: [&str; 5] = ["ATK", "UDF", "LDF", "SPD", "INT"];

/// One roster member of the target panel.
#[derive(Debug, Clone, Copy, Default)]
pub struct TargetPanelMember<'a> {
    pub name: &'a str,
    /// Record `+0x130` (2-digit field).
    pub level: u8,
    /// Current HP/MP (record `+0x106` / `+0x10A`).
    pub hp: u16,
    pub mp: u16,
    /// Effective maxima (record `+0x104` / `+0x108` - base + equips +
    /// passives, the pair the plain rows and the mode-1 left values
    /// draw).
    pub hp_max: u16,
    pub mp_max: u16,
    /// Record-side base maxima (`+0x11C` / `+0x11E` - the mode-1 teal
    /// paren values).
    pub base_hp_max: u16,
    pub base_mp_max: u16,
    /// Effective stats in retail label order (the `FUN_801CF650`
    /// aggregator words `DAT_801EF08C..9C`; modes 2..=5 clamp at 999).
    pub stat_eff: [u16; 5],
    /// Record-side base stats (`+0x124..+0x12C`), same order.
    pub stat_base: [u16; 5],
}

/// Hand-cursor state of the panel (the `DAT_801E46C4` bit decode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPanelCursor {
    /// Bit `0x4000` set - no hand.
    Hidden,
    /// Single-target pick: hand on `row` (the cursor word's low 12
    /// bits). `pressed` = bit `0x1000` (confirm staged; retail drops the
    /// hand to its static sprite variant).
    Single { row: u8, pressed: bool },
    /// Bit `0x2000` - all-party pick: hand on every member row.
    All { pressed: bool },
}

/// Plain-data view for [`target_panel_draws_for`].
pub struct TargetPanelView<'a> {
    pub members: &'a [TargetPanelMember<'a>],
    pub mode: TargetPanelMode,
    pub cursor: TargetPanelCursor,
    /// `true` when the caller draws the LV / HP / MP tags as UI-icon
    /// atlas sprites ([`target_panel_sprites_for`]) - suppresses the
    /// ASCII stand-ins here.
    pub label_icons: bool,
    /// Emit ASCII `>` cursors instead of the hand sprite.
    pub text_cursor: bool,
}

/// Row Y of member `i`'s block origin.
fn block_y(pen_y: i32, i: usize) -> i32 {
    pen_y + i as i32 * TARGET_PANEL_MEMBER_PITCH
}

/// Build [`TextDraw`]s for the window-14 party target panel.
///
/// `pen` is the window's content origin (`(174, 28)` from the descriptor
/// table). Pens per the `FUN_801D0520` trace - see the module docs.
///
/// PORT: FUN_801D0520 (window-14 party target panel renderer)
pub fn target_panel_draws_for(
    font: &legaia_font::Font,
    view: &TargetPanelView<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let (wx, wy) = pen;
    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };
    for (i, m) in view.members.iter().enumerate() {
        let yb = block_y(wy, i);
        // Name + level header (staged 7 white).
        str_at(&mut out, m.name, wx + 0x14, yb, MENU_TEXT_WHITE);
        if !view.label_icons {
            str_at(&mut out, "LV", wx + 0x58, yb + 2, MENU_TEXT_WHITE);
        }
        out.extend(num_field_draws(
            font,
            m.level as u64,
            wx + 0x68,
            yb,
            2,
            MENU_TEXT_WHITE,
        ));

        let hp_y = yb + 0x0f;
        let mp_y = yb + 0x1c;
        if view.mode == TargetPanelMode::HpMp {
            // Mode 1: eff_max ( base_max ) for HP then MP. Left value
            // staged 7 white, paren group + base staged 5 teal.
            for (tag_y, num_y, eff, base, tag) in [
                (yb + 0x11, hp_y, m.hp_max, m.base_hp_max, "HP"),
                (yb + 0x1e, mp_y, m.mp_max, m.base_mp_max, "MP"),
            ] {
                if !view.label_icons {
                    str_at(&mut out, tag, wx + 0x14, tag_y, MENU_TEXT_WHITE);
                }
                out.extend(num_field_draws(
                    font,
                    eff as u64,
                    wx + 0x38,
                    num_y,
                    4,
                    MENU_TEXT_WHITE,
                ));
                str_at(&mut out, "(", wx + 0x58, num_y, MENU_TEXT_TEAL);
                out.extend(num_field_draws(
                    font,
                    base as u64,
                    wx + 0x60,
                    num_y,
                    4,
                    MENU_TEXT_TEAL,
                ));
                str_at(&mut out, ")", wx + 0x80, num_y, MENU_TEXT_TEAL);
            }
        } else {
            // Plain HP row: cur / max with the health-tier ink; the
            // slash (glyph cell 6) stays staged-7 white.
            if !view.label_icons {
                str_at(&mut out, "HP", wx + 0x1c, yb + 0x11, MENU_TEXT_WHITE);
            }
            let hp_ink = menu_hp_ink(m.hp, m.hp_max);
            out.extend(num_field_draws(
                font,
                m.hp as u64,
                wx + 0x2c,
                hp_y,
                4,
                hp_ink,
            ));
            str_at(&mut out, "/", wx + 0x4c, hp_y, MENU_TEXT_WHITE);
            out.extend(num_field_draws(
                font,
                m.hp_max as u64,
                wx + 0x54,
                hp_y,
                4,
                hp_ink,
            ));
            // Plain MP row - skipped in mode 3 (the two Guardian Water
            // stat rows take its place).
            if view.mode != TargetPanelMode::Def {
                if !view.label_icons {
                    str_at(&mut out, "MP", wx + 0x1c, yb + 0x1e, MENU_TEXT_WHITE);
                }
                let mp_ink = menu_mp_ink(m.mp, m.mp_max);
                out.extend(num_field_draws(
                    font,
                    m.mp as u64,
                    wx + 0x2c,
                    mp_y,
                    4,
                    mp_ink,
                ));
                str_at(&mut out, "/", wx + 0x4c, mp_y, MENU_TEXT_WHITE);
                out.extend(num_field_draws(
                    font,
                    m.mp_max as u64,
                    wx + 0x54,
                    mp_y,
                    4,
                    mp_ink,
                ));
            }
        }

        // Stat preview rows: (stat index, row y) per mode.
        let stat_rows: &[(usize, i32)] = match view.mode {
            TargetPanelMode::Atk => &[(0, 0x29)],
            TargetPanelMode::Def => &[(1, 0x1c), (2, 0x29)],
            TargetPanelMode::Spd => &[(3, 0x29)],
            TargetPanelMode::Int => &[(4, 0x29)],
            _ => &[],
        };
        for &(stat, dy) in stat_rows {
            let y = yb + dy;
            str_at(
                &mut out,
                TARGET_PANEL_STAT_LABELS[stat],
                wx + 0x1c,
                y,
                MENU_TEXT_WHITE,
            );
            out.extend(num_field_draws(
                font,
                m.stat_eff[stat].min(999) as u64,
                wx + 0x44,
                y,
                3,
                MENU_TEXT_WHITE,
            ));
            str_at(&mut out, "(", wx + 0x5c, y, MENU_TEXT_TEAL);
            out.extend(num_field_draws(
                font,
                m.stat_base[stat] as u64,
                wx + 0x64,
                y,
                3,
                MENU_TEXT_TEAL,
            ));
            str_at(&mut out, ")", wx + 0x7c, y, MENU_TEXT_TEAL);
        }

        // ASCII hand stand-in (retail hand pens are in the sprites fn).
        if view.text_cursor && cursor_on_row(view.cursor, i) {
            str_at(&mut out, ">", wx, yb, MENU_TEXT_GOLD);
        }
    }
    out
}

fn cursor_on_row(cursor: TargetPanelCursor, row: usize) -> bool {
    match cursor {
        TargetPanelCursor::Hidden => false,
        TargetPanelCursor::All { .. } => true,
        TargetPanelCursor::Single { row: r, .. } => r as usize == row,
    }
}

/// Build the panel's [`SpriteDraw`]s: the LV / HP / MP label tags plus
/// the hand cursor(s). Retail draws the hand via `FUN_8002B994(0,
/// variant, WX, Yb)` - at the raw window X on the focused row (every row
/// in all-party mode).
///
/// PORT: FUN_801D0520 (hand + label sprite placement). REF: FUN_8002B994.
pub fn target_panel_sprites_for(
    rects: &SaveMenuAtlasRects,
    view: &TargetPanelView<'_>,
    pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (wx, wy) = pen;
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::new();
    let mut push = |src: (u32, u32, u32, u32), pos: (i32, i32)| {
        let (_, _, w, h) = src;
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + pos.0 * scale,
                stage_origin.1 + pos.1 * scale,
                w * stage_scale,
                h * stage_scale,
            ),
            src,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    };
    for (i, _) in view.members.iter().enumerate() {
        let yb = block_y(wy, i);
        push(rects.label_lv, (wx + 0x58, yb + 2));
        if view.mode == TargetPanelMode::HpMp {
            push(rects.label_hp, (wx + 0x14, yb + 0x11));
            push(rects.label_mp, (wx + 0x14, yb + 0x1e));
        } else {
            push(rects.label_hp, (wx + 0x1c, yb + 0x11));
            if view.mode != TargetPanelMode::Def {
                push(rects.label_mp, (wx + 0x1c, yb + 0x1e));
            }
        }
        if cursor_on_row(view.cursor, i) {
            push(rects.cursor, (wx, yb));
        }
    }
    out
}

#[cfg(test)]
mod target_panel_tests {
    use super::*;

    fn member<'a>(name: &'a str) -> TargetPanelMember<'a> {
        TargetPanelMember {
            name,
            level: 12,
            hp: 180,
            mp: 40,
            hp_max: 200,
            mp_max: 80,
            base_hp_max: 190,
            base_mp_max: 72,
            stat_eff: [111, 92, 93, 74, 65],
            stat_base: [101, 82, 83, 64, 55],
        }
    }

    fn view<'a>(
        members: &'a [TargetPanelMember<'a>],
        mode: TargetPanelMode,
        cursor: TargetPanelCursor,
    ) -> TargetPanelView<'a> {
        TargetPanelView {
            members,
            mode,
            cursor,
            label_icons: false,
            text_cursor: false,
        }
    }

    fn draw_at(draws: &[TextDraw], x: i32, y: i32) -> bool {
        draws.iter().any(|d| d.dst.0 == x && d.dst.1 == y)
    }

    const PEN: (i32, i32) = (174, 28);

    /// Plain mode: name/LV header + HP cur/max + MP cur/max at the
    /// traced pens, one block per member at the 0x3E pitch.
    #[test]
    fn plain_mode_rows_at_traced_pens() {
        let members = [member("Vahn"), member("Noa")];
        let draws = target_panel_draws_for(
            &legaia_font::synthetic_for_tests(),
            &view(&members, TargetPanelMode::Plain, TargetPanelCursor::Hidden),
            PEN,
        );
        for i in 0..2 {
            let yb = 28 + i * 0x3e;
            assert!(draw_at(&draws, 174 + 0x14, yb)); // name
            // HP cur 180 right-aligns into cells 1..3 of the 4-digit
            // field at +0x2C.
            assert!(draw_at(&draws, 174 + 0x2c + 8, yb + 0x0f));
            assert!(draw_at(&draws, 174 + 0x4c, yb + 0x0f)); // slash
            assert!(draw_at(&draws, 174 + 0x54 + 8, yb + 0x0f)); // HP max 200
            assert!(draw_at(&draws, 174 + 0x4c, yb + 0x1c)); // MP slash
            assert!(draw_at(&draws, 174 + 0x1c, yb + 0x1e)); // "MP" tag
        }
    }

    /// Plain HP numbers take the FUN_800349EC tier ink (a quarter-tank
    /// member draws orange), the slash stays white.
    #[test]
    fn plain_mode_hp_tier_ink() {
        let mut m = member("Noa");
        m.hp = 40; // 40 <= 200/4 -> orange tier
        let members = [m];
        let draws = target_panel_draws_for(
            &legaia_font::synthetic_for_tests(),
            &view(&members, TargetPanelMode::Plain, TargetPanelCursor::Hidden),
            PEN,
        );
        let ink = |x: i32| {
            draws
                .iter()
                .find(|d| d.dst.0 >= 174 + x && d.dst.0 < 174 + x + 0x20 && d.dst.1 == 28 + 0x0f)
                .map(|d| d.color)
                .unwrap()
        };
        assert_eq!(ink(0x2c), crate::MENU_TEXT_ORANGE);
        let slash = draws
            .iter()
            .find(|d| d.dst.0 == 174 + 0x4c && d.dst.1 == 28 + 0x0f)
            .unwrap();
        assert_eq!(slash.color, MENU_TEXT_WHITE);
    }

    /// Mode 1 (Life/Magic Water): eff max white at +0x38, teal paren
    /// group at +0x58/+0x80 around the base max at +0x60, HP and MP
    /// rows both preview - no slash rows.
    #[test]
    fn hpmp_preview_rows_at_traced_pens() {
        let members = [member("Vahn")];
        let draws = target_panel_draws_for(
            &legaia_font::synthetic_for_tests(),
            &view(&members, TargetPanelMode::HpMp, TargetPanelCursor::Hidden),
            PEN,
        );
        assert!(draw_at(&draws, 174 + 0x58, 28 + 0x0f)); // "("
        assert!(draw_at(&draws, 174 + 0x80, 28 + 0x0f)); // ")"
        assert!(draw_at(&draws, 174 + 0x58, 28 + 0x1c)); // MP "("
        assert!(!draw_at(&draws, 174 + 0x4c, 28 + 0x0f)); // no slash
        let paren = draws
            .iter()
            .find(|d| d.dst.0 == 174 + 0x58 && d.dst.1 == 28 + 0x0f)
            .unwrap();
        assert_eq!(paren.color, MENU_TEXT_TEAL);
    }

    /// Mode 3 (Guardian Water): UDF row at Yb+0x1C and LDF at Yb+0x29,
    /// plain MP row skipped; eff value clamps at 999.
    #[test]
    fn def_preview_draws_two_stat_rows_and_skips_mp() {
        let mut m = member("Gala");
        m.stat_eff[1] = 1500; // clamps to 999
        let members = [m];
        let draws = target_panel_draws_for(
            &legaia_font::synthetic_for_tests(),
            &view(&members, TargetPanelMode::Def, TargetPanelCursor::Hidden),
            PEN,
        );
        assert!(draw_at(&draws, 174 + 0x1c, 28 + 0x1c)); // "UDF"
        assert!(draw_at(&draws, 174 + 0x1c, 28 + 0x29)); // "LDF"
        assert!(draw_at(&draws, 174 + 0x44, 28 + 0x1c)); // eff 999 fills cell 0
        // The plain MP row is skipped: no "MP" tag at Yb+0x1E.
        assert!(!draw_at(&draws, 174 + 0x1c, 28 + 0x1e));
        // HP plain row still draws.
        assert!(draw_at(&draws, 174 + 0x4c, 28 + 0x0f));
    }

    /// Single-mode stat previews sit at Yb+0x29 with the traced label.
    #[test]
    fn single_stat_preview_rows() {
        let members = [member("Vahn")];
        for (mode, label_x) in [
            (TargetPanelMode::Atk, 0x1c),
            (TargetPanelMode::Spd, 0x1c),
            (TargetPanelMode::Int, 0x1c),
        ] {
            let draws = target_panel_draws_for(
                &legaia_font::synthetic_for_tests(),
                &view(&members, mode, TargetPanelCursor::Hidden),
                PEN,
            );
            assert!(draw_at(&draws, 174 + label_x, 28 + 0x29));
            assert!(draw_at(&draws, 174 + 0x5c, 28 + 0x29)); // "("
            assert!(draw_at(&draws, 174 + 0x7c, 28 + 0x29)); // ")"
        }
    }

    /// Cursor decode: Single puts the hand sprite on one row, All on
    /// every row, Hidden on none.
    #[test]
    fn cursor_modes_place_hand_sprites() {
        let members = [member("Vahn"), member("Noa"), member("Gala")];
        let z = (0, 0, 0, 0);
        let rects = SaveMenuAtlasRects {
            panel_tl: z,
            panel_tr: z,
            panel_bl: z,
            panel_br: z,
            panel_top: z,
            panel_bot: z,
            panel_left: z,
            panel_right: z,
            slot1: z,
            slot2: z,
            cursor: (152, 64, 16, 16),
            panel_interior: z,
            panel_filigree: z,
            label_lv: (40, 232, 16, 10),
            label_hp: (60, 232, 16, 10),
            label_mp: (80, 232, 16, 10),
            icon_money: z,
            label_time: z,
            label_coin: z,
            gauge_cap: z,
            gauge_trough: z,
            gauge_box: z,
            gauge_tip: z,
            gauge_digits: z,
            gauge_100: z,
            gauge_fill: z,
            dialog_fill: z,
            icon_weapon: z,
            icon_helmet: z,
            icon_armor: z,
            icon_boot: z,
            icon_goods: z,
            pager_left: z,
            pager_right: z,
            tab_cap_l: z,
            tab_body: z,
            tab_cap_r: z,
            atr_icons: [z; 3],
            load_empty_frame: None,
            load_portrait_by_char: [None; 3],
        };
        let hands = |cursor: TargetPanelCursor| {
            target_panel_sprites_for(
                &rects,
                &view(&members, TargetPanelMode::Plain, cursor),
                PEN,
                (0, 0),
                1,
            )
            .into_iter()
            .filter(|d| d.src == rects.cursor && d.dst.0 == 174)
            .count()
        };
        assert_eq!(hands(TargetPanelCursor::Hidden), 0);
        assert_eq!(
            hands(TargetPanelCursor::Single {
                row: 1,
                pressed: false
            }),
            1
        );
        assert_eq!(hands(TargetPanelCursor::All { pressed: false }), 3);
    }

    /// Preview-word mapping mirrors the retail value space 0..=5.
    #[test]
    fn preview_word_maps_modes() {
        assert_eq!(
            TargetPanelMode::from_preview_word(0),
            TargetPanelMode::Plain
        );
        assert_eq!(TargetPanelMode::from_preview_word(1), TargetPanelMode::HpMp);
        assert_eq!(TargetPanelMode::from_preview_word(3), TargetPanelMode::Def);
        assert_eq!(TargetPanelMode::from_preview_word(5), TargetPanelMode::Int);
        assert_eq!(
            TargetPanelMode::from_preview_word(9),
            TargetPanelMode::Plain
        );
    }
}
