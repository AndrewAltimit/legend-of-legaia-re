//! Host surface for the retail developer menu (`LEGAIA_DEV_MENU=1`).
//!
//! Retail opens its dev tools from debug branches in the world-map and field
//! controllers - branches a retail player cannot reach. The engine's
//! equivalent is this opt-in: with `LEGAIA_DEV_MENU` set, `play-window`
//! drives [`DevMenuSession`] once a frame off the same **packed** pad words
//! retail's own dev code reads (`_DAT_8007BB84` newly-pressed,
//! `_DAT_8007B850` held), and draws its row list through the ported list-body
//! renderer. The packed words are built here by [`retail_packed`] rather than
//! taken from `InputState::retail_pad()`, because the native host feeds
//! `World::set_pad` a raw PSX pad word and the pump republishes it unchanged -
//! see that function for the two layouts and why they are not the same word.
//!
//! Without the variable nothing here runs and no draw is produced, so the
//! default build is unchanged.
//!
//! # The Records page
//!
//! Retail's battle-records readout (`FUN_801ED710`) is a page of that same
//! world-map dev menu, so this host is where it belongs. Square swaps the row
//! list for it while the list has the pad. The six per-character counters come
//! straight out of the live `0x414`-byte character records through
//! [`legaia_engine_ui::record_counters`], which rebases the retail
//! save-block displacements onto a bare record; the clamping and the H:MM:SS
//! decomposition are the ported model
//! (`legaia_engine_dev_menu::dev_menu_leaves::records_screen`).
//!
//! The lifetime battle / escape counters and the treasure census are state the
//! engine does not keep, so those read zero and the treasure line stays hidden
//! - the same page retail draws off a save that never incremented them.

use super::*;
use legaia_engine_menu_runtime::dev_menu_host::DevMenuWorldExt;
use legaia_engine_menus::dev_menu_host::{DevMenuSession, DevPage};
use legaia_engine_screens::dev_menu::{
    DEV_RECORDS_PEN, RECORDS_TOGGLE, dev_menu_list_draws, records_draws_for_world,
};
use legaia_engine_system::pad::retail_packed;

impl PlayWindowApp {
    /// Whether the developer menu is enabled for this run.
    pub(super) fn dev_menu_enabled() -> bool {
        std::env::var_os("LEGAIA_DEV_MENU").is_some()
    }

    /// Advance the developer menu one frame and rebuild its draw list.
    ///
    /// The pad words are in the retail packed layout the dev kernels key on,
    /// so they see exactly the bits they were written against rather than a
    /// re-mapped approximation - [`retail_packed`] does the conversion and
    /// records why the host has to.
    pub(super) fn tick_dev_menu(&mut self) {
        if !Self::dev_menu_enabled() {
            return;
        }
        let host = &self.session.host;
        let session = self
            .dev_menu
            .get_or_insert_with(|| DevMenuSession::with_flag_tags(host.dev_flag_list_tags()));
        let world = &mut self.session.host.world;
        let (edge, held) = {
            let (now, prev) = (
                retail_packed(world.input.pad()),
                retail_packed(world.input.pad_prev()),
            );
            (now & !prev, now)
        };

        // The shared tick: party records, the CAMERA row's follow switch on
        // this host's camera, and the EQUIP row's confirm against the
        // engine's own bag.
        match session.tick_host(world, &mut self.session.camera, edge, held) {
            Some(Some(c)) => log::info!(
                "dev-menu: equipped item {} into slot {} on character {} (refunded {:?})",
                session.equip_item,
                c.slot,
                session.chars.character,
                c.refunded
            ),
            Some(None) => log::info!(
                "dev-menu: item {} is not in the bag - nothing committed",
                session.equip_item
            ),
            None => {}
        }

        // The screen's cues ride the world's SFX ring (retail
        // `FUN_80035B50`), which `route_field_sfx` replays.
        for cue in session.route_sfx(world) {
            log::debug!("dev-menu: sfx cue {cue:#04x}");
        }

        // Square swaps the row list for the Records page while the list has
        // the pad; the sub-editors keep their own key map.
        let on_list = session.page == DevPage::List;
        if on_list && edge & RECORDS_TOGGLE != 0 {
            self.dev_menu_records = !self.dev_menu_records;
        }
        self.dev_menu_draws = if on_list && self.dev_menu_records {
            self.build_dev_records_draws()
        } else {
            dev_menu_list_draws(self.dev_menu.as_ref().expect("just inserted"), &self.font)
        };
    }

    /// Build the battle-records page for the live world.
    fn build_dev_records_draws(&self) -> Vec<legaia_engine_ui::TextDraw> {
        self.records_draws_at(DEV_RECORDS_PEN)
    }

    /// The records screen (`FUN_801ED710`) at `pen`, retail stage pixels -
    /// the dev Records page and the end-of-game soft reset - through the
    /// builder the page calls too (`records_draws_for_world`).
    pub(super) fn records_draws_at(&self, pen: (i32, i32)) -> Vec<legaia_engine_ui::TextDraw> {
        records_draws_for_world(&self.session.host.world, &self.font, pen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The raw-to-packed pad conversion, checked on every button whose two
    /// encodings are documented independently (`retail_pad`'s module table
    /// and the `dev_menu` `PACK_*` constants).
    #[test]
    fn pad_repack_matches_both_documented_layouts() {
        use legaia_engine_system::input::PadButton as B;
        use legaia_engine_system::pad::{
            PACK_CIRCLE, PACK_CROSS, PACK_DOWN, PACK_LEFT, PACK_RIGHT, PACK_SQUARE, PACK_TRIANGLE,
            PACK_UP,
        };
        let cases = [
            (B::Up, PACK_UP),
            (B::Down, PACK_DOWN),
            (B::Left, PACK_LEFT),
            (B::Right, PACK_RIGHT),
            (B::Triangle, PACK_TRIANGLE),
            (B::Circle, PACK_CIRCLE),
            (B::Cross, PACK_CROSS),
            (B::Square, PACK_SQUARE),
        ];
        for (btn, packed) in cases {
            assert_eq!(
                retail_packed(btn.mask()),
                packed,
                "{} repacks wrong",
                btn.name()
            );
        }
        // Start's packed bit is pinned by `retail_pad`'s own decode test.
        assert_eq!(retail_packed(B::Start.mask()), 0x0800);
        // The conversion is an involution, so no caller can double-apply it
        // silently - a second pass returns the raw word.
        assert_eq!(retail_packed(retail_packed(B::Square.mask())), 0x8000);
    }
}
