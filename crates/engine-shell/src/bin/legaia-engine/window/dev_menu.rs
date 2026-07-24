//! Host surface for the retail developer menu (`LEGAIA_DEV_MENU=1`).
//!
//! Retail opens its dev tools from debug branches in the world-map and field
//! controllers - branches a retail player cannot reach. The engine's
//! equivalent is this opt-in: with `LEGAIA_DEV_MENU` set, `play-window`
//! drives [`DevMenuSession`] once a frame off the same packed pad words
//! retail's own dev code reads (`_DAT_8007BB84` newly-pressed,
//! `_DAT_8007B850` held, both published by the retail pad pump), and draws
//! its row list through the ported list-body renderer.
//!
//! Without the variable nothing here runs and no draw is produced, so the
//! default build is unchanged.

use super::*;
use legaia_engine_core::dev_menu_host::{DevMenuRow, DevMenuSession, WorldEquipHost};

/// Pen the dev-menu list draws from - clear of the field HUD's own rows.
const DEV_MENU_PEN: (i32, i32) = (16, 24);

impl PlayWindowApp {
    /// Whether the developer menu is enabled for this run.
    pub(super) fn dev_menu_enabled() -> bool {
        std::env::var_os("LEGAIA_DEV_MENU").is_some()
    }

    /// Advance the developer menu one frame and rebuild its draw list.
    ///
    /// The pad words come from [`legaia_engine_core::retail_pad`] - the same
    /// packed layout the retail dev code keys on, so the ported kernels see
    /// exactly the bits they were written against rather than a re-mapped
    /// approximation.
    pub(super) fn tick_dev_menu(&mut self) {
        if !Self::dev_menu_enabled() {
            return;
        }
        let session = self.dev_menu.get_or_insert_with(DevMenuSession::new);
        let world = &mut self.session.host.world;
        let (edge, held) = {
            let pad = world.input.retail_pad();
            (pad.pressed as u16, pad.held as u16)
        };

        {
            let mut records: Vec<&mut [u8]> = world
                .roster
                .members
                .iter_mut()
                .map(|m| m.raw.as_mut_slice())
                .collect();
            session.tick(edge, held, &mut records);
        }

        // The EQUIP row's confirm commits against the engine's own bag.
        if session.current_row() == DevMenuRow::Equip
            && edge & legaia_engine_core::dev_menu::PACK_CROSS != 0
        {
            let character = session.chars.character as usize;
            let weapon_slots: Vec<i16> = vec![2; world.roster.members.len().max(4)];
            if let Some(member) = world.roster.members.get_mut(character) {
                let mut raw = std::mem::take(&mut member.raw);
                let mut host = WorldEquipHost {
                    inventory: &mut world.inventory,
                    sfx: Vec::new(),
                };
                let committed = session.commit_equip_row(&mut host, &mut raw, &weapon_slots);
                let cues = std::mem::take(&mut host.sfx);
                world.roster.members[character].raw = raw;
                session.pending_sfx.extend(cues);
                match committed {
                    Some(c) => log::info!(
                        "dev-menu: equipped item {} into slot {} on character {character} \
                         (refunded {:?})",
                        session.equip_item,
                        c.slot,
                        c.refunded
                    ),
                    None => log::info!(
                        "dev-menu: item {} is not in the bag - nothing committed",
                        session.equip_item
                    ),
                }
            }
        }

        for cue in session.drain_sfx() {
            log::debug!("dev-menu: sfx cue {cue:#04x}");
        }

        self.dev_menu_draws = Self::build_dev_menu_draws(session, &self.font);
    }

    /// Build the row list's draws through the ported list-body renderer.
    ///
    /// The renderer owns the geometry - the `+8` label column, the 8-px row
    /// pitch, the `0x17` row clamp and the cursor column - so the only thing
    /// assembled here is each row's `(label, value)` pair.
    fn build_dev_menu_draws(
        session: &DevMenuSession,
        font: &legaia_font::Font,
    ) -> Vec<legaia_engine_render::TextDraw> {
        use legaia_engine_render::{
            DevMenuListRow, dev_menu_cursor_xy, dev_menu_list_draws_for, text_draws_for,
        };
        let values: Vec<String> = DevMenuRow::ALL
            .iter()
            .map(|r| session.row_value(*r).unwrap_or_default())
            .collect();
        let rows: Vec<DevMenuListRow<'_>> = DevMenuRow::ALL
            .iter()
            .zip(values.iter())
            .map(|(r, v)| DevMenuListRow {
                label: r.label(),
                value: Some((v.as_str(), 0x68)),
            })
            .collect();
        let last = (rows.len() - 1) as i32;
        let mut out = dev_menu_list_draws_for(font, &rows, 0, last, DEV_MENU_PEN);
        if let Some(xy) = dev_menu_cursor_xy(DEV_MENU_PEN, session.row as i32, 0, last) {
            out.extend(text_draws_for(
                &font.layout_ascii(">"),
                xy,
                legaia_engine_render::MENU_TEXT_WHITE,
            ));
        }
        out
    }
}
