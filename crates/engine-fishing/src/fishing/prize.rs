//! Point exchange (prize shop).
//! Split out of `fishing.rs`.

// --- Point exchange (prize shop) -------------------------------------------

/// One prize row of the point-exchange screen, decoded from the overlay's
/// per-venue table ([`legaia_asset::fishing_exchange`]) and optionally named
/// from the SCUS item table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrizeRow {
    /// Row index within the venue page (0..6).
    pub row: usize,
    /// Max obtainable count (1 = one-time prize, 99 = repeatable).
    pub limit: u32,
    /// Price in fishing points per unit.
    pub price: u32,
    /// Granted item id (SCUS item-name-table id space).
    pub item_id: u8,
    /// Display name (from the SCUS item table when available).
    pub name: Option<String>,
}

impl PrizeRow {
    /// Whether this is a one-time prize row (latched in the purchased mask).
    pub fn is_one_time(&self) -> bool {
        self.limit == 1
    }
}

/// A purchase committed by [`PrizeExchange::buy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrizePurchase {
    /// Granted item id.
    pub item_id: u8,
    /// Units granted.
    pub qty: u32,
    /// Points spent (`price * qty`).
    pub cost: u32,
    /// One-time bit latched into the purchased mask, if any
    /// (`row + venue * 8`).
    pub latched_bit: Option<u32>,
}

/// The fishing point-exchange session: a venue's 6 prize rows plus a cursor,
/// with the retail gating semantics of the exchange sub-screens
/// (`FUN_801d0c3c` list / `FUN_801d092c` quantity / `FUN_801d06c8` confirm /
/// `FUN_801d6f90` availability - see [`legaia_asset::fishing_exchange`]).
///
/// The kernel is pure over the caller's state: the point pool, the persistent
/// purchased bitmask, and the owned count come in per call (the engine keeps
/// them on `World`, mirroring retail's `_DAT_8008444C` / `_DAT_8008446C` /
/// inventory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrizeExchange {
    /// Venue page (0 = Buma, 1 = Vidna); selects the one-time bit block.
    pub venue: usize,
    /// The 6 prize rows.
    pub rows: Vec<PrizeRow>,
    /// List cursor (row index).
    pub cursor: usize,
}

impl PrizeExchange {
    /// Build from the parsed per-venue asset rows, naming each row from the
    /// SCUS item table when one is supplied.
    pub fn from_asset(
        venue: usize,
        rows: &[legaia_asset::fishing_exchange::ExchangeRow],
        names: Option<&legaia_asset::item_names::ItemNameTable>,
    ) -> Self {
        let rows = rows
            .iter()
            .map(|r| PrizeRow {
                row: r.row,
                limit: r.limit,
                price: r.price,
                item_id: r.item_id as u8,
                name: names
                    .and_then(|t| t.name(r.item_id as u8))
                    .map(str::to_owned),
            })
            .collect();
        Self {
            venue: venue.min(1),
            rows,
            cursor: 0,
        }
    }

    /// The one-time bit index for `row` (`row + venue * 8`).
    pub fn purchase_bit(&self, row: usize) -> u32 {
        legaia_asset::fishing_exchange::FishingExchange::purchase_bit(self.venue, row)
    }

    /// The first *visible* row for the current point total: row 0 is hidden
    /// until strictly affordable (`FUN_801d0c3c`'s `(price0 < points) ^ 1`
    /// cursor floor).
    pub fn first_visible(&self, points: i32) -> usize {
        // PORT: FUN_801d0c3c (prize-list cursor floor - row 0 hides until affordable)
        match self.rows.first() {
            Some(r0) if (r0.price as i64) < points as i64 => 0,
            Some(_) => 1,
            None => 0,
        }
    }

    /// Whether `row`'s **one-time bit is latched** in `purchased_mask` - i.e.
    /// the prize has already been taken.
    ///
    /// Not the same question as [`Self::is_available`], which folds three
    /// independent refusals together (price, owned cap, latch). Reading
    /// availability as the latch labels every unaffordable one-time prize on
    /// a fresh save "sold". The three hosts each answered this differently -
    /// one re-tested availability with the other two gates forced open, one
    /// shifted the mask by hand, and one never asked at all - so it lives
    /// here, once.
    pub fn is_latched(&self, row: usize, purchased_mask: u32) -> bool {
        (purchased_mask >> self.purchase_bit(row)) & 1 != 0
    }

    /// Row availability (drawn white vs grey; `FUN_801d6f90`): affordable,
    /// the owned count is not at [`legaia_asset::fishing_exchange::OWNED_CAP`],
    /// and a one-time row is not already latched in `purchased_mask`.
    pub fn is_available(&self, row: usize, points: i32, owned: u32, purchased_mask: u32) -> bool {
        // PORT: FUN_801d6f90 (row availability: afford + owned-cap + one-time latch)
        let Some(r) = self.rows.get(row) else {
            return false;
        };
        (r.price as i64) <= points as i64
            && owned != legaia_asset::fishing_exchange::OWNED_CAP
            && !self.is_latched(row, purchased_mask)
    }

    /// Max purchasable quantity for `row` (`FUN_801d092c`):
    /// `min(points / price, limit - owned)`, where a not-yet-latched one-time
    /// row treats `owned` as 0. Zero when unaffordable or at the limit.
    pub fn max_qty(&self, row: usize, points: i32, owned: u32, purchased_mask: u32) -> u32 {
        // PORT: FUN_801d092c (quantity picker cap: min(points/price, limit - owned))
        let Some(r) = self.rows.get(row) else {
            return 0;
        };
        if r.price == 0 {
            return 0;
        }
        let owned = if (purchased_mask >> self.purchase_bit(row)) & 1 == 0 && r.limit == 1 {
            0
        } else {
            owned
        };
        let by_points = (points.max(0) as u32) / r.price;
        by_points.min(r.limit.saturating_sub(owned))
    }

    /// Commit a purchase of `qty` units of `row` (`FUN_801d06c8`'s Yes arm):
    /// returns the grant + cost + the one-time bit to latch, or `None` when
    /// the row is unavailable or `qty` exceeds [`Self::max_qty`]. The caller
    /// applies the returned deltas (deduct points, OR the latched bit, grant
    /// the item).
    pub fn buy(
        &self,
        row: usize,
        qty: u32,
        points: i32,
        owned: u32,
        purchased_mask: u32,
    ) -> Option<PrizePurchase> {
        // PORT: FUN_801d06c8 (confirm Yes arm: grant + deduct + one-time latch)
        if qty == 0 || !self.is_available(row, points, owned, purchased_mask) {
            return None;
        }
        if qty > self.max_qty(row, points, owned, purchased_mask) {
            return None;
        }
        let r = &self.rows[row];
        Some(PrizePurchase {
            item_id: r.item_id,
            qty,
            cost: r.price * qty,
            latched_bit: r.is_one_time().then(|| self.purchase_bit(row)),
        })
    }
}
