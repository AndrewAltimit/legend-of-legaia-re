//! The trading cards' monster portraits: rendered once per pack from the
//! player's own disc, the first time the card screen opens, and kept on the
//! world (`ExpansionState::cards.portraits`) for both hosts' card screens.
//!
//! Each card whose art is a retail monster gets one small RGBA image of that
//! monster ([`legaia_actor_formats::monster_portrait`]). The images live in
//! memory only; a pack carries the monster's id, never its picture.

use std::collections::BTreeSet;

use legaia_engine_expansion::cards::{Portrait, PortraitCache};
use legaia_expansion_format::refs::DiscRef;

use super::SceneHost;

/// The portrait size: the binder's art box. Larger boxes draw it scaled.
pub const PORTRAIT_W: u16 = 64;
pub const PORTRAIT_H: u16 = 48;

/// Render the portrait of every monster `ids` names out of `archive`.
pub fn render_portraits(archive: &[u8], ids: &BTreeSet<u16>) -> PortraitCache {
    let opts = legaia_actor_formats::monster_portrait::portrait_options(
        usize::from(PORTRAIT_W),
        usize::from(PORTRAIT_H),
    );
    let mut cache = PortraitCache::default();
    for &id in ids {
        if let Some(rgba) = legaia_actor_formats::monster_portrait::render(archive, id, &opts) {
            cache.by_monster.insert(
                id,
                Portrait {
                    w: PORTRAIT_W,
                    h: PORTRAIT_H,
                    rgba,
                },
            );
        }
    }
    cache
}

impl SceneHost {
    /// Build the active pack's card portraits while the card screen or a
    /// Grid Duel is open and the cache belongs to another pack (or none). One build per pack;
    /// a disc without the monster archive leaves the cards on their element
    /// frames.
    pub(crate) fn ensure_card_portraits(&mut self) {
        let Some(pack) = self.world.expansion.pack.clone() else {
            return;
        };
        let duel = self.world.duel_page_hands().is_some();
        if (self.world.expansion.cards.seat.is_none() && !duel)
            || self.world.expansion.cards.portraits.pack_id == pack.manifest.id
        {
            return;
        }
        let ids: BTreeSet<u16> = pack
            .cards
            .cards
            .iter()
            .filter_map(|c| match c.art {
                Some(DiscRef::Monster { id }) => Some(id),
                _ => None,
            })
            .collect();
        let mut cache = match self.monster_archive_bytes() {
            Some(archive) => render_portraits(&archive, &ids),
            None => PortraitCache::default(),
        };
        log::info!(
            "cards: {} of {} monster portraits rendered for pack `{}`",
            cache.by_monster.len(),
            ids.len(),
            pack.manifest.id
        );
        cache.pack_id = pack.manifest.id.clone();
        self.world.expansion.cards.portraits = cache;
    }
}
