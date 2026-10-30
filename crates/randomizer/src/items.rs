//! The item populations a randomizer draws from and the ones it never moves.
//!
//! Price `0` is the game's own marker for an item no shop trades: the door
//! keys, the garden-quest tools, the eggs, talismans and books, the letters,
//! the fishing rods, the casino cards. Moving one out of the place the story
//! hands it over can strand a run, so those are *kept*: a site that gives one
//! keeps it, and none enters a pool. Everything a shop prices is the
//! *sellable* pool - what a randomized drop, chest or shop slot can hold.
//! This is the patcher's `sellable_pool` / quest-item rule; it reads the
//! prices through a closure so it needs no disc access of its own.

/// The two populations, by item id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemPools {
    /// Ids with a shop price above zero, ascending.
    pub sellable: Vec<u8>,
    /// Named ids priced zero: never moved, never drawn.
    pub keep: Vec<u8>,
}

impl ItemPools {
    /// Sort every id `1..=255` by `item(id) -> Some((named, price))`; `None`
    /// is an id with no record.
    pub fn from_items(item: impl Fn(u8) -> Option<(bool, u32)>) -> Self {
        let mut p = Self::default();
        for id in 1..=u8::MAX {
            match item(id) {
                Some((_, price)) if price > 0 => p.sellable.push(id),
                Some((true, 0)) => p.keep.push(id),
                _ => {}
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priced_items_pool_and_named_free_ones_are_kept() {
        let p = ItemPools::from_items(|id| match id {
            1 => Some((true, 100)),
            2 => Some((true, 0)),
            3 => Some((false, 0)),
            _ => None,
        });
        assert_eq!(p.sellable, vec![1]);
        assert_eq!(p.keep, vec![2]);
    }
}
