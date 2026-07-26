//! `renderer_va` -> content-painter dispatch over the menu-overlay
//! **window-descriptor table** (`legaia_asset::menu_windows`).
//!
//! Retail never hard-codes "this screen draws that content". A window is
//! spawned from its descriptor, the descriptor's `+0xC` renderer VA is copied
//! into the live window struct at `+0x28`, and the per-frame window walker
//! calls it indirectly with the window struct as its only argument:
//!
//! ```text
//! 80031e30  lw   v0,0x28(s4)      ; the descriptor's +0xC renderer
//! 80031e38  beq  v0,zero,...      ; 0 = content-builder-driven list window
//! 80031e40  jalr v0
//! 80031e44  _move a0,s4           ; a0 = the live window (rect at +0xa)
//! ```
//!
//! This module is the port of that resolution step: given a parsed
//! descriptor, say which of this crate's painters draws its content. A host
//! then owns the two decisions retail also owns separately - *which windows
//! are open* (retail: the per-screen open script the window-script runner
//! `FUN_801D6628` interprets) and *what the content is* (retail: the live
//! globals each renderer reads).
//!
//! REF: FUN_80031D00 (`0x80031E30..0x80031E44` - the indirect renderer call)
//! REF: FUN_800326AC (create-time copy of descriptor `+0xC` into live `+0x28`)
//!
//! ## Why this returns a painter *kind* and not a draw list
//!
//! The painters take wildly different content - a `&str`, a `u64`, a
//! `&[(EquipTargetRow, &str)]`, two `[&str; 2]` arrays plus a flag word - and
//! each signature is that window's content contract. Folding them into one
//! `WindowContent` enum would make every host build every field's worth of
//! borrow plumbing to paint one window, and would put a second, drifting copy
//! of each painter's argument list in this file. So the dispatch answers
//! "which painter", and the host calls it with exactly that painter's
//! arguments.
//!
//! ## Painter map
//!
//! | Renderer | Windows | Painter |
//! |---|---|---|
//! | `FUN_801DCA0C` / `CA50` / `CA94` / `CAD8` / `CB1C` / `CFE4` | 0..=4, 43 | `title_tab_draws_for` |
//! | `FUN_801DCF14` | 33 | `record_title_tab_draws_for` |
//! | `FUN_801DCF84` / `FUN_801DD028` | 32 / 45 | `counter_panel_draws_for` |
//! | `FUN_801DCCB4` | 7 | `char_prompt_draws_for` |
//! | `FUN_801DCE20` | 31 | `amount_prompt_draws_for` |
//! | `FUN_801DCC20` | 24 | `count_panel_draws_for` |
//! | `FUN_801D603C` | 46 | `choice_panel_draws_for` |
//! | `FUN_801D61B0` | 5 | `two_line_choice_panel_draws_for` |
//! | `FUN_801D6360` | 6 | `label_list_draws_for` |
//! | `FUN_801D4A80` | 34 | `item_description_draws_for` |
//! | `FUN_801D56FC` | 36 | `equip_target_list_draws_for` |
//! | `FUN_801D5944` | 37 | `sell_quantity_draws_for` |
//! | `FUN_801D1290` | 25 | `equip_compare_panel_fields` -> `compare_panel_draws_for` |
//! | `FUN_801D4C28` | 41 | `party_compare_panel_fields` -> `compare_panel_draws_for` |
//!
//! The window ids in that table are where the retail disc's descriptors put
//! each renderer; the dispatch keys on the **renderer**, so a modded table
//! that moves a renderer to another id still resolves.

use legaia_asset::menu_windows::{MenuWindowDescriptor, MenuWindowTable};

use crate::ui_menu_window_painters::PainterRect;

// --- Renderer VAs -----------------------------------------------------------
//
// The five pause-menu title tabs and window 43's tab are six copies of one
// 17-instruction routine that differ only in the string pointer they load
// (`FUN_801DCA0C`: `addiu a0,a0,-0x1630`; `FUN_801DCFE4`: `-0x1394`), so they
// share one painter.

/// Items tab (descriptor id 0).
pub const RENDERER_TAB_ITEMS: u32 = 0x801D_CA0C;
/// Magic tab (id 1).
pub const RENDERER_TAB_MAGIC: u32 = 0x801D_CA50;
/// Equip tab (id 2).
pub const RENDERER_TAB_EQUIP: u32 = 0x801D_CA94;
/// Status tab (id 3).
pub const RENDERER_TAB_STATUS: u32 = 0x801D_CAD8;
/// Options tab (id 4).
pub const RENDERER_TAB_OPTIONS: u32 = 0x801D_CB1C;
/// The prize-exchange (ticket-counter) screen's tab (id 43) - the same
/// routine with its own string pointer; see `docs/subsystems/field-menu.md`.
pub const RENDERER_TAB_EXCHANGE: u32 = 0x801D_CFE4;
/// Record-sourced title tab (id 33).
pub const RENDERER_RECORD_TAB: u32 = 0x801D_CF14;
/// Party-gold counter (id 32).
pub const RENDERER_COUNTER_GOLD: u32 = 0x801D_CF84;
/// Casino-coin counter (id 45).
pub const RENDERER_COUNTER_COINS: u32 = 0x801D_D028;
/// One-line prompt with a substituted record character (id 7).
pub const RENDERER_CHAR_PROMPT: u32 = 0x801D_CCB4;
/// Heading + wide number + unit label (id 31).
pub const RENDERER_AMOUNT_PROMPT: u32 = 0x801D_CE20;
/// Two-digit count over a reserved sub-rect (id 24).
pub const RENDERER_COUNT_PANEL: u32 = 0x801D_CC20;
/// One heading over a two-row choice group (id 46).
pub const RENDERER_CHOICE_PANEL: u32 = 0x801D_603C;
/// Two headings over the same choice group (id 5).
pub const RENDERER_TWO_LINE_CHOICE_PANEL: u32 = 0x801D_61B0;
/// Six stacked labels with an extent-anchored cursor (id 6).
pub const RENDERER_LABEL_LIST: u32 = 0x801D_6360;
/// Item name / owned count / description (id 34).
pub const RENDERER_ITEM_DESCRIPTION: u32 = 0x801D_4A80;
/// Equip-target list gated on the character mask (id 36).
pub const RENDERER_EQUIP_TARGET_LIST: u32 = 0x801D_56FC;
/// Sell quantity + halved gold total (id 37).
pub const RENDERER_SELL_QUANTITY: u32 = 0x801D_5944;
/// Active-character stat compare (id 25).
pub const RENDERER_ACTIVE_STAT_COMPARE: u32 = 0x801D_1290;
/// Party-wide stat compare (id 41).
pub const RENDERER_PARTY_STAT_COMPARE: u32 = 0x801D_4C28;

/// Pictogram id the party-gold counter draws (`li a2,0x62`).
pub const COUNTER_PICTOGRAM_GOLD: u8 = 0x62;
/// Pictogram id the casino-coin counter draws (`li a2,0x66`).
pub const COUNTER_PICTOGRAM_COINS: u8 = 0x66;

/// Which live total a counter window prints.
///
/// The two counter renderers are the same 24-instruction routine with two
/// literals changed - the pictogram id and the global they load - so the
/// value is part of the dispatch rather than of the painter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterSource {
    /// `_DAT_8008459C` - party gold (`FUN_801DCF84`, `lw a0,0x459c(v0)`).
    /// Engine side: `World::money`.
    PartyGold,
    /// `_DAT_800845A4` - the casino coin bank (`FUN_801DD028`,
    /// `lw a0,0x45a4(v0)`). Engine side: `World::casino_coins`.
    CasinoCoins,
}

/// A painter in this crate, named by the descriptor's `renderer_va`.
///
/// The variants carry only what the *dispatch* resolves - never layout,
/// which stays in the painter, and never content, which stays with the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuWindowPainter {
    /// One label at the content origin.
    TitleTab,
    /// The same label, sourced from the live `_DAT_8007B450` record behind a
    /// variable-length prefix (`title_record_text_offset`).
    RecordTitleTab,
    /// Pictogram + right-aligned 8-digit counter.
    Counter {
        /// UI-icon-atlas id the window's pictogram draws.
        pictogram: u8,
        /// The live total the digits print.
        source: CounterSource,
    },
    /// One prompt line with a substituted character + the corner cursor.
    CharPrompt,
    /// Heading, wide number field, trailing unit label, corner cursor. The
    /// number is the Point Card counter `_DAT_800845B4`.
    AmountPrompt,
    /// Two-digit count over a reserved sub-rect.
    CountPanel,
    /// One heading over a two-row choice group.
    ChoicePanel,
    /// Two headings over a two-row choice group.
    TwoLineChoicePanel,
    /// Six stacked labels + an extent-anchored cursor.
    LabelList,
    /// Item name, owned count, description line.
    ItemDescription,
    /// Header + one row per party member, greyed by the character mask.
    EquipTargetList,
    /// Sell quantity, held count, halved gold total.
    SellQuantity,
    /// The active character's stat compare (`equip_compare_panel_fields`).
    ActiveStatCompare,
    /// The party-wide stat compare (`party_compare_panel_fields`).
    PartyStatCompare,
}

/// Resolve a descriptor's content renderer to the painter that draws it.
///
/// `None` means either a renderer this crate has no painter for or the `0`
/// of a content-builder-driven list window (`MenuWindowDescriptor::content_id`
/// selects the SCUS builder there, and the list content is the host's).
pub fn painter_for_renderer_va(renderer_va: u32) -> Option<MenuWindowPainter> {
    use MenuWindowPainter as P;
    Some(match renderer_va {
        RENDERER_TAB_ITEMS
        | RENDERER_TAB_MAGIC
        | RENDERER_TAB_EQUIP
        | RENDERER_TAB_STATUS
        | RENDERER_TAB_OPTIONS
        | RENDERER_TAB_EXCHANGE => P::TitleTab,
        RENDERER_RECORD_TAB => P::RecordTitleTab,
        RENDERER_COUNTER_GOLD => P::Counter {
            pictogram: COUNTER_PICTOGRAM_GOLD,
            source: CounterSource::PartyGold,
        },
        RENDERER_COUNTER_COINS => P::Counter {
            pictogram: COUNTER_PICTOGRAM_COINS,
            source: CounterSource::CasinoCoins,
        },
        RENDERER_CHAR_PROMPT => P::CharPrompt,
        RENDERER_AMOUNT_PROMPT => P::AmountPrompt,
        RENDERER_COUNT_PANEL => P::CountPanel,
        RENDERER_CHOICE_PANEL => P::ChoicePanel,
        RENDERER_TWO_LINE_CHOICE_PANEL => P::TwoLineChoicePanel,
        RENDERER_LABEL_LIST => P::LabelList,
        RENDERER_ITEM_DESCRIPTION => P::ItemDescription,
        RENDERER_EQUIP_TARGET_LIST => P::EquipTargetList,
        RENDERER_SELL_QUANTITY => P::SellQuantity,
        RENDERER_ACTIVE_STAT_COMPARE => P::ActiveStatCompare,
        RENDERER_PARTY_STAT_COMPARE => P::PartyStatCompare,
        _ => return None,
    })
}

/// [`painter_for_renderer_va`] on a parsed descriptor.
pub fn painter_for(descriptor: &MenuWindowDescriptor) -> Option<MenuWindowPainter> {
    painter_for_renderer_va(descriptor.renderer_va)
}

/// A descriptor's content rect in the painters' own shape.
///
/// Both are `(x, y, w, h)` off the descriptor's `+0xA..+0x10` - the rect the
/// retail renderer reads out of the live window struct.
pub fn painter_rect(descriptor: &MenuWindowDescriptor) -> PainterRect {
    let (x, y, w, h) = descriptor.rect();
    PainterRect::new(x, y, w, h)
}

/// Every window in a parsed table this crate can paint, as
/// `(descriptor id, painter)` in id order.
///
/// This is the disc-driven half: a host asks the table what it holds instead
/// of carrying a list of screens. Ids absent from the result are either
/// renderer-less list containers or renderers with no painter here yet.
pub fn menu_window_painters(table: &MenuWindowTable) -> Vec<(usize, MenuWindowPainter)> {
    table
        .windows
        .iter()
        .enumerate()
        .filter_map(|(id, d)| painter_for(d).map(|p| (id, p)))
        .collect()
}

/// Resolve one id through a table, rejecting a descriptor whose renderer is
/// not the painter the caller expects.
///
/// A host that wants "window 32, which had better still be a counter" gets
/// `None` rather than a mis-drawn panel when a modded table moved the
/// renderer - the dispatch is on the renderer, so the id alone is never the
/// authority.
pub fn painter_at(
    table: &MenuWindowTable,
    id: usize,
    expect: MenuWindowPainter,
) -> Option<(&MenuWindowDescriptor, MenuWindowPainter)> {
    let d = table.window(id)?;
    let p = painter_for(d)?;
    (p == expect).then_some((d, p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::menu_windows::MENU_WINDOW_COUNT;

    /// A table carrying the retail renderer VAs at the retail ids, built
    /// without a disc: only the fields the dispatch reads are set.
    fn table_with(renderers: &[(usize, u32)]) -> MenuWindowTable {
        let mut windows = vec![
            MenuWindowDescriptor {
                content_id: 0,
                park_edge: 0,
                kind: 3,
                x: 16,
                y: 12,
                w: 60,
                h: 12,
                renderer_va: 0,
            };
            MENU_WINDOW_COUNT
        ];
        for &(id, va) in renderers {
            windows[id].renderer_va = va;
        }
        MenuWindowTable { windows }
    }

    #[test]
    fn every_tab_renderer_shares_one_painter() {
        for va in [
            RENDERER_TAB_ITEMS,
            RENDERER_TAB_MAGIC,
            RENDERER_TAB_EQUIP,
            RENDERER_TAB_STATUS,
            RENDERER_TAB_OPTIONS,
            RENDERER_TAB_EXCHANGE,
        ] {
            assert_eq!(
                painter_for_renderer_va(va),
                Some(MenuWindowPainter::TitleTab),
                "{va:#010x}"
            );
        }
    }

    #[test]
    fn the_two_counters_differ_only_in_pictogram_and_source() {
        assert_eq!(
            painter_for_renderer_va(RENDERER_COUNTER_GOLD),
            Some(MenuWindowPainter::Counter {
                pictogram: COUNTER_PICTOGRAM_GOLD,
                source: CounterSource::PartyGold,
            })
        );
        assert_eq!(
            painter_for_renderer_va(RENDERER_COUNTER_COINS),
            Some(MenuWindowPainter::Counter {
                pictogram: COUNTER_PICTOGRAM_COINS,
                source: CounterSource::CasinoCoins,
            })
        );
    }

    /// A renderer-less list window (`renderer_va == 0`) and a renderer with
    /// no painter here both resolve to nothing - the dispatch never guesses.
    #[test]
    fn zero_and_unknown_renderers_resolve_to_nothing() {
        assert_eq!(painter_for_renderer_va(0), None);
        // FUN_801D33D8 - the status main panel, painted by its own builder.
        assert_eq!(painter_for_renderer_va(0x801D_33D8), None);
    }

    #[test]
    fn the_walk_reports_ids_in_order_and_skips_the_rest() {
        let table = table_with(&[
            (32, RENDERER_COUNTER_GOLD),
            (3, RENDERER_TAB_STATUS),
            (34, RENDERER_ITEM_DESCRIPTION),
            (28, 0x801D_33D8),
        ]);
        let found = menu_window_painters(&table);
        assert_eq!(
            found.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![3, 32, 34]
        );
        assert!(matches!(found[1].1, MenuWindowPainter::Counter { .. }));
    }

    /// The id is a lookup key, not the authority: a table that moved a
    /// renderer refuses the mismatch instead of painting the wrong content.
    #[test]
    fn painter_at_refuses_a_renderer_that_moved() {
        let table = table_with(&[(32, RENDERER_ITEM_DESCRIPTION)]);
        assert!(painter_at(&table, 32, MenuWindowPainter::ItemDescription).is_some());
        assert!(
            painter_at(
                &table,
                32,
                MenuWindowPainter::Counter {
                    pictogram: COUNTER_PICTOGRAM_GOLD,
                    source: CounterSource::PartyGold,
                }
            )
            .is_none()
        );
        assert!(painter_at(&table, MENU_WINDOW_COUNT, MenuWindowPainter::TitleTab).is_none());
    }

    #[test]
    fn the_painter_rect_is_the_descriptor_rect() {
        let table = table_with(&[(43, RENDERER_TAB_EXCHANGE)]);
        let d = table.window(43).unwrap();
        let r = painter_rect(d);
        assert_eq!((r.x, r.y, r.w, r.h), d.rect());
    }
}
