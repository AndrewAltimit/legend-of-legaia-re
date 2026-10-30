//! The expansion game modes' screen, projected off the world's mode seat
//! once for both play hosts.
//!
//! The prim half joins the frame through
//! [`crate::screen_layers::compose_screen_prims`], which hands the whole
//! under-text list to it while the seat owns the frame; the text half is
//! [`pack_mode_stage_texts`], which each host's text layer appends and
//! scales with the rest of its stage group. Both come from
//! `legaia_engine_ui::expansion_modes`, so the two hosts draw one screen.
//!
//! A golf round or race played on its pack map draws in the world: the
//! field renders under the seat, the world pieces come from
//! `legaia_engine_world_views::pack_mode_world` (the host's
//! `HostScreenPrims::pack_mode_world`), and the screen here is only the HUD
//! (`ModeScreen::InWorld`).
//!
//! The trading-card screen shares the seat's frame: while the card seat is
//! open the same two calls draw it instead (`card_screen_prims` /
//! `card_screen_texts`), with the portraits the scene host cached on the
//! world.

use legaia_engine_core::world::World;
use legaia_engine_expansion::modes::{ModeSource, ModeStage};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::cards::CardBook;
use legaia_engine_ui::TextDraw;
use legaia_engine_ui::expansion_modes::{
    CardScreenView, ItemCompareView, ItemMemberView, ItemRowView, ItemScreenView, ModeScreen,
    PortraitRef, card_screen_prims, card_screen_texts, card_toast_texts, item_screen_prims,
    item_screen_texts, mode_screen_prims, mode_screen_texts,
};
use legaia_render_kernels::screen_prim::ScreenPrim;

/// The seat's screen this frame, `None` unless the seat owns the frame.
pub fn pack_mode_screen(world: &World) -> Option<ModeScreen<'_>> {
    if world.mode != SceneMode::Expansion {
        return None;
    }
    let seat = world.expansion.mode_seat.as_deref()?;
    Some(match &seat.stage {
        ModeStage::Picker {
            entries,
            cursor,
            error,
        } => ModeScreen::Picker {
            heading: match seat.source {
                ModeSource::Pack => "Pack modes",
                ModeSource::Builtin => "Practice modes",
            },
            entries,
            cursor: *cursor,
            error: error.as_deref(),
        },
        // Played on its pack map: the field draws the game, the screen only
        // its HUD.
        ModeStage::Playing { mode, quit_prompt } if seat.stage_frame.is_some() => {
            ModeScreen::InWorld {
                mode,
                log: &seat.log,
                quit_prompt: *quit_prompt,
                banner: seat.stage_frame.as_ref().and_then(|f| f.banner.as_deref()),
            }
        }
        ModeStage::Playing { mode, quit_prompt } => ModeScreen::Playing {
            mode,
            log: &seat.log,
            quit_prompt: *quit_prompt,
        },
        ModeStage::Results {
            name,
            kind,
            rows,
            granted,
            completed,
            local_seat,
        } => ModeScreen::Results {
            name,
            kind: *kind,
            rows,
            granted,
            completed: *completed,
            local_seat: *local_seat,
        },
    })
}

/// Run `f` on the card screen's view, when the card seat owns the frame.
pub fn with_card_screen<T>(world: &World, f: impl FnOnce(&CardScreenView<'_>) -> T) -> Option<T> {
    if !world.card_screen_open() {
        return None;
    }
    let seat = world.expansion.cards.seat.as_deref()?;
    let book = CardBook::new(world.expansion.pack.as_deref()?.cards.clone());
    let portraits = &world.expansion.cards.portraits;
    let portrait = |id: u16| {
        portraits.get(id).map(|p| PortraitRef {
            w: p.w,
            h: p.h,
            rgba: &p.rgba,
        })
    };
    let view = CardScreenView {
        screen: &seat.screen,
        book: &book,
        progress: &world.expansion.quests.progress.cards,
        gold: world.party.money,
        portrait: &portrait,
    };
    Some(f(&view))
}

/// The pack item screen's view, when it owns the frame.
pub fn item_screen_view(world: &World) -> Option<ItemScreenView> {
    let m = world.item_screen_view()?;
    Some(ItemScreenView {
        title: m.title,
        gold: m.gold,
        tabs: m.tabs.iter().map(|t| t.to_string()).collect(),
        tab: m.tab,
        rows: m
            .rows
            .into_iter()
            .map(|r| ItemRowView {
                name: r.name,
                right: r.right,
                sub: r.sub,
                dim: r.dim,
            })
            .collect(),
        cursor: m.cursor,
        empty: m.empty.to_string(),
        detail: m.detail,
        quantity: m.quantity,
        members: m.members.map(|(rows, cursor)| {
            (
                rows.into_iter()
                    .map(|r| ItemMemberView {
                        name: r.name,
                        line: r.line,
                        dim: r.dim,
                    })
                    .collect(),
                cursor,
            )
        }),
        compare: m.compare.map(|c| ItemCompareView {
            heading: c.heading,
            rows: c
                .rows
                .into_iter()
                .map(|(l, a, b)| (l.to_string(), a, b))
                .collect(),
        }),
        message: m.message,
        footer: m.footer.to_string(),
    })
}

/// The seat's flat prims in display space; empty unless it owns the frame.
pub fn pack_mode_prims(world: &World) -> Vec<ScreenPrim> {
    if let Some(v) = item_screen_view(world) {
        return item_screen_prims(&v);
    }
    if let Some(p) = with_card_screen(world, card_screen_prims) {
        return p;
    }
    pack_mode_screen(world)
        .map(|s| mode_screen_prims(&s))
        .unwrap_or_default()
}

/// The seat's labels in **stage** pixels (the host scales them with its
/// stage group), then the card toasts ("Got card: X") - those in any mode
/// but the card screen's own, so a battle drop shows over the results. Empty while neither is up.
pub fn pack_mode_stage_texts(world: &World, font: &legaia_font::Font) -> Vec<TextDraw> {
    let mut out = item_screen_view(world)
        .map(|v| item_screen_texts(font, &v))
        .or_else(|| with_card_screen(world, |v| card_screen_texts(font, v)))
        .or_else(|| pack_mode_screen(world).map(|s| mode_screen_texts(font, &s)))
        .unwrap_or_default();
    // The card screen marks new cards itself; its toasts wait out unseen.
    let toasts = world.expansion.cards.toast_lines();
    if !toasts.is_empty() && !world.card_screen_open() {
        out.extend(card_toast_texts(
            font,
            &toasts,
            world.mode != SceneMode::Expansion,
        ));
    }
    out
}
