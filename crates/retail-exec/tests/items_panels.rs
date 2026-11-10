//! The Items screen's Throw Out confirm window `FUN_801D1B20` (PROT 0899,
//! window 9) against `engine-ui`'s `items_throw_confirm_draws_for` and
//! `items_throw_confirm_sprites_for`, on `menu_painters`' rig.
//!
//! The routine seats the count, and the first prompt line after it, at
//! `WX + 8 + name[0] * 12` (`0x801D1BB4..0x801D1BC8`): the item name's first
//! **byte**, read off the same pointer the string writer was just handed.
//! On this disc that byte is a letter, so the pen lands hundreds of pixels
//! right of the screen and neither draw is seen. A sprite is held to be seen
//! the way the GPU sees it: its x taken as a signed 11-bit coordinate,
//! inside the 320-px drawing area.

use crate::common::{Rng, disc};
use crate::menu_painters::{Ev, cstr, hands, rig};
use legaia_engine_menus::pause_screens::ThrowConfirmLabels;
use legaia_engine_ui::menu::window_painters::PainterRect;
use legaia_engine_ui::{
    ITEMS_THROW_CONFIRM_RECT, PauseThrowConfirmView, SaveMenuAtlasRects, TextDraw,
    items_throw_confirm_draws_for, items_throw_confirm_sprites_for,
};

const THROW_CONFIRM: u32 = 0x801D_1B20;
/// The staged bag slot, the bag's `[id, count]` pairs, the item table and
/// the confirm cursor.
const STAGED_SLOT: u32 = 0x8007_BB88;
const BAG: u32 = 0x8008_5958;
const ITEMS: u32 = 0x8007_4368;
const CONFIRM_CURSOR: u32 = 0x801E_46D0;
const INK: u32 = 0x8007_B454;

/// A draw's x as the GPU reads it, when it lands in the drawing area.
fn seen(d: &TextDraw) -> Option<TextDraw> {
    let x = (d.dst.0 + 1024).rem_euclid(2048) - 1024;
    (x > -(d.dst.2 as i32) && x < 320).then_some(TextDraw {
        dst: (x, d.dst.1, d.dst.2, d.dst.3),
        ..*d
    })
}

#[test]
fn the_throw_out_confirm_window_matches() {
    differential!(prot(899, THROW_CONFIRM));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let labels = ThrowConfirmLabels::from_menu_overlay(
        &disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    );
    assert!(labels.is_installed());
    // The items whose names are plain strings in the executable.
    let named: Vec<(u8, String)> = (1..=255u8)
        .filter_map(|id| {
            let p = rig.cpu.mem.rd32(ITEMS + u32::from(id) * 12 + 4);
            let s = (0x8001_0000..0x8008_0000)
                .contains(&p)
                .then(|| cstr(&mut rig.cpu.mem, p))?;
            (s.len() > 2 && s.iter().all(|b| (0x20..0x7F).contains(b)))
                .then(|| (id, String::from_utf8(s).unwrap()))
        })
        .collect();
    assert!(named.len() > 150, "{} named items", named.len());
    let chrome = SaveMenuAtlasRects {
        cursor: (152, 64, 16, 16),
        ..Default::default()
    };
    let on_grid = |d: &[TextDraw]| -> Vec<_> {
        d.iter()
            .map(|t| (t.dst, t.src, t.color.map(|c| (c * 31.0).round() as u32)))
            .collect()
    };
    let mut rng = Rng(0x5448_524F);
    let (mut unseen, mut two_digit) = (0, 0);
    for run in 0..4_000 {
        let (id, name) = &named[rng.below(named.len() as u32) as usize];
        let slot = rng.below(256);
        let count = match rng.below(3) {
            0 => 1 + rng.below(9),
            _ => 10 + rng.below(90),
        };
        rig.cpu.mem.wr32(STAGED_SLOT, slot);
        rig.cpu.mem.wr8(BAG + slot * 2, *id);
        rig.cpu.mem.wr8(BAG + slot * 2 + 1, count as u8);
        let cursor = rng.below(2);
        rig.cpu
            .mem
            .wr32(CONFIRM_CURSOR, cursor | (rng.below(2) << 12));
        // The window's own rect, and now and then another.
        let (wx, wy, w, h) = ITEMS_THROW_CONFIRM_RECT;
        let r = match rng.below(3) {
            0 => PainterRect::new(rng.below(100) as i32, rng.below(150) as i32, w, h),
            _ => PainterRect::new(wx, wy, w, h),
        };
        let log = rig.run(THROW_CONFIRM, r);
        let what = format!("run {run}: {name:?} x{count} cursor {cursor} at {r:?}");
        assert_eq!(
            rig.cpu.mem.rd32(INK),
            7,
            "{what}: the ink is handed back white"
        );

        let view = PauseThrowConfirmView {
            name,
            count: count as u16,
            cursor: cursor as u8,
            text_cursor: false,
            prompt: [labels.prompt[0].as_str(), labels.prompt[1].as_str()],
            choices: [labels.choices[0].as_str(), labels.choices[1].as_str()],
        };
        let port = items_throw_confirm_draws_for(&rig.font, &view, (r.x, r.y));
        let text: Vec<Ev> = log
            .iter()
            .filter(|e| matches!(e, Ev::Text(..) | Ev::Number(..)))
            .cloned()
            .collect();
        assert_eq!(text.len(), 6, "{what}: name, count, two lines, Yes, No");
        let all = rig.text_of(&text);
        let want: Vec<TextDraw> = all.iter().filter_map(seen).collect();
        unseen += u32::from(want.len() < all.len());
        two_digit += u32::from(count >= 10);
        assert_eq!(on_grid(&want), on_grid(&port), "{what}\n{text:?}");

        let hand = hands(&log);
        assert_eq!(hand.len(), 1, "{what}");
        let sprites = items_throw_confirm_sprites_for(&chrome, cursor as u8, (r.x, r.y), (0, 0), 1);
        assert_eq!(
            sprites
                .iter()
                .map(|s| (s.dst.0, s.dst.1))
                .collect::<Vec<_>>(),
            vec![(hand[0].x, hand[0].y)],
            "{what}: the hand"
        );
    }
    assert!(unseen > 3900 && two_digit > 2000, "{unseen} {two_digit}");
}
