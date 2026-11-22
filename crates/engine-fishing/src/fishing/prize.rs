//! Point exchange (prize shop).

// --- Point exchange (prize shop) -------------------------------------------

/// One prize row of the point-exchange screen, decoded from the overlay's
/// per-venue table ([`legaia_minigame_formats::fishing_exchange`]) and optionally named
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
/// `FUN_801d6f90` availability - see [`legaia_minigame_formats::fishing_exchange`]).
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
        rows: &[legaia_minigame_formats::fishing_exchange::ExchangeRow],
        names: Option<&legaia_game_tables::item_names::ItemNameTable>,
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
        legaia_minigame_formats::fishing_exchange::FishingExchange::purchase_bit(self.venue, row)
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

    /// One frame of the prize list's cursor (`FUN_801d0c3c`): up and down
    /// each move it one row (both in one frame cancel), and it **wraps**: past
    /// the first visible row ([`Self::first_visible`]) it lands on the last
    /// row, past the last row on the first visible one. Retail holds the list
    /// at six rows (`li v0,0x5` / `slti v0,v0,0x6`); a page with fewer rows
    /// wraps at its own last row.
    ///
    /// A cursor already under the floor - left on row 0 by a purchase that
    /// took the points below its price - goes to the last row before the
    /// press moves it (`0x801D0C88..0x801D0C9C`, ahead of the edges), so
    /// an up then lands one row above it.
    pub fn step_cursor(&self, cursor: usize, points: i32, up: bool, down: bool) -> usize {
        // PORT: FUN_801d0c3c (the list cursor: the entry clamp, up / down, wrapping at the floor and the last row)
        let floor = self.first_visible(points) as i32;
        let last = self.rows.len().max(1) as i32 - 1;
        let start = if (cursor as i32) < floor {
            last
        } else {
            cursor as i32
        };
        let mut c = start - i32::from(up) + i32::from(down);
        if c < floor {
            c = last;
        }
        if c > last {
            c = floor;
        }
        c as usize
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
    /// the owned count is not at [`legaia_minigame_formats::fishing_exchange::OWNED_CAP`],
    /// and a one-time row is not already latched in `purchased_mask`.
    pub fn is_available(&self, row: usize, points: i32, owned: u32, purchased_mask: u32) -> bool {
        // PORT: FUN_801d6f90 (row availability: afford + owned-cap + one-time latch)
        let Some(r) = self.rows.get(row) else {
            return false;
        };
        (r.price as i64) <= points as i64
            && owned != legaia_minigame_formats::fishing_exchange::OWNED_CAP
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

// --- The exchange's three screens: list, quantity picker, confirm ----------

/// Which of the exchange's screens takes the pad: the session states `0x78`
/// (the list, `FUN_801D0C3C(1)`), `0x79` (the quantity picker,
/// `FUN_801D092C(1)` over the list) and `0x7A` (the confirm, `FUN_801D06C8(1)`
/// over both). The session draws every screen under the live one with its
/// input off (`0x801D0028..0x801D0080`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrizeScreen {
    #[default]
    List,
    Quantity,
    Confirm,
}

/// The pickers' words: the screen, the picked row `0x801D90D4`, the quantity
/// `0x801D90B0` and the confirm's row `0x801D90B4` (`0` Yes, `1` No).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrizeDialog {
    pub screen: PrizeScreen,
    pub row: usize,
    pub qty: i32,
    pub confirm_row: u32,
}

/// One exchange pad step's outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrizeStep {
    /// The cue the step left in `0x8007B6D8` (the last store wins).
    pub sfx: Option<u16>,
    /// The list's cancel: back to the hub menu (state `0x64`).
    pub closed: bool,
    /// The confirm's Yes: what the caller grants and spends.
    pub purchase: Option<PrizePurchase>,
}

/// The pad masks the exchange screens read: the cursor keys off the screen's
/// pad word `0x801D90D8`, confirm / cancel off the edge word `0x8007B874`.
pub const PRIZE_UP: u32 = 0x1000;
pub const PRIZE_RIGHT: u32 = 0x2000;
pub const PRIZE_DOWN: u32 = 0x4000;
pub const PRIZE_LEFT: u32 = 0x8000;
pub const PRIZE_CONFIRM: u32 = 0x44;
pub const PRIZE_CANCEL: u32 = 0x21;

/// The cue the confirm's Yes stores (`li v0,0x206` at `0x801D089C`).
pub const PRIZE_BOUGHT_SFX: u16 = 0x206;

impl PrizeExchange {
    /// One frame of the live exchange screen. `keys` is the screen's cursor
    /// word, `pressed` the edge word, `owned_of` the bag's count of an item
    /// (`FUN_80042F4C`), read for the row the screen tests.
    ///
    /// - The list (`0x801D0CB0..0x801D0DB0`): the cursor step
    ///   ([`Self::step_cursor`], cue `0x21` per key); `& 0x44` opens the
    ///   picker on an available row (row latched, quantity `1`, cue `0x20`) or
    ///   refuses (cue `0x22`); `& 0x21` closes to the hub (cue `0x37`).
    /// - The picker (`0x801D0A3C..0x801D0B14`): Left / Right step the quantity
    ///   (cue `0x21`), held at `1` from below and at [`Self::max_qty`] from
    ///   above in that order; `& 0x44` opens the confirm on No (cue `0x20`),
    ///   `& 0x21` backs out to the list (cue `0x37`).
    /// - The confirm (`0x801D06E0..0x801D0910`): Up / Down step its row (cue
    ///   `0x21`), kept to its low bit; `& 0x21`, or `& 0x44` on No, backs out
    ///   to the picker (cue `0x22`); `& 0x44` on Yes buys (cue `0x206`) and
    ///   returns to the list.
    ///
    /// PORT: FUN_801d0c3c (the confirm and cancel edges, 0x801D0D28..0x801D0DB0)
    /// PORT: FUN_801d092c (the quantity step and its edges, 0x801D0A3C..0x801D0B14)
    /// PORT: FUN_801d06c8 (the row step and its edges, 0x801D06E0..0x801D0910)
    #[allow(clippy::too_many_arguments)]
    pub fn pad_step(
        &mut self,
        dlg: &mut PrizeDialog,
        keys: u32,
        pressed: u32,
        points: i32,
        owned_of: impl Fn(u8) -> u32,
        purchased_mask: u32,
    ) -> PrizeStep {
        let owned = |row: usize| self.rows.get(row).map_or(0, |r| owned_of(r.item_id));
        let mut out = PrizeStep::default();
        match dlg.screen {
            PrizeScreen::List => {
                let (up, down) = (keys & PRIZE_UP != 0, keys & PRIZE_DOWN != 0);
                if up || down {
                    out.sfx = Some(0x21);
                }
                let cursor = self.step_cursor(self.cursor, points, up, down);
                if pressed & PRIZE_CONFIRM != 0 {
                    if self.is_available(cursor, points, owned(cursor), purchased_mask) {
                        out.sfx = Some(0x20);
                        *dlg = PrizeDialog {
                            screen: PrizeScreen::Quantity,
                            row: cursor,
                            qty: 1,
                            confirm_row: dlg.confirm_row,
                        };
                    } else {
                        out.sfx = Some(0x22);
                    }
                }
                if pressed & PRIZE_CANCEL != 0 {
                    out.sfx = Some(0x37);
                    dlg.screen = PrizeScreen::List;
                    out.closed = true;
                }
                self.cursor = cursor;
            }
            PrizeScreen::Quantity => {
                let max = self.picker_max(dlg.row, points, owned(dlg.row), purchased_mask);
                if keys & PRIZE_LEFT != 0 {
                    out.sfx = Some(0x21);
                    dlg.qty -= 1;
                }
                if keys & PRIZE_RIGHT != 0 {
                    out.sfx = Some(0x21);
                    dlg.qty += 1;
                }
                if dlg.qty <= 0 {
                    dlg.qty = 1;
                }
                if max < i64::from(dlg.qty) {
                    dlg.qty = max as i32;
                }
                if pressed & PRIZE_CONFIRM != 0 {
                    out.sfx = Some(0x20);
                    dlg.confirm_row = 1;
                    dlg.screen = PrizeScreen::Confirm;
                }
                if pressed & PRIZE_CANCEL != 0 {
                    out.sfx = Some(0x37);
                    dlg.screen = PrizeScreen::List;
                }
            }
            PrizeScreen::Confirm => {
                if keys & PRIZE_UP != 0 {
                    out.sfx = Some(0x21);
                    dlg.confirm_row = dlg.confirm_row.wrapping_sub(1);
                }
                if keys & PRIZE_DOWN != 0 {
                    out.sfx = Some(0x21);
                    dlg.confirm_row = dlg.confirm_row.wrapping_add(1);
                }
                dlg.confirm_row &= 1;
                let confirm = pressed & PRIZE_CONFIRM != 0;
                if pressed & PRIZE_CANCEL != 0 || (confirm && dlg.confirm_row != 0) {
                    out.sfx = Some(0x22);
                    dlg.screen = PrizeScreen::Quantity;
                }
                if confirm && dlg.confirm_row == 0 {
                    // The arm commits whatever the picker left; the list
                    // and the picker bound it to a buy `buy` allows.
                    out.purchase = u32::try_from(dlg.qty)
                        .ok()
                        .and_then(|q| self.buy(dlg.row, q, points, owned(dlg.row), purchased_mask));
                    out.sfx = Some(PRIZE_BOUGHT_SFX);
                    dlg.screen = PrizeScreen::List;
                }
            }
        }
        out
    }

    /// The picker's cap as it reads it (`0x801D09DC..0x801D0A30`): signed,
    /// `limit - owned` below `points / price` when it is smaller.
    fn picker_max(&self, row: usize, points: i32, owned: u32, purchased_mask: u32) -> i64 {
        let Some(r) = self.rows.get(row) else {
            return 0;
        };
        if r.price == 0 {
            return 0;
        }
        let owned = if !self.is_latched(row, purchased_mask) && r.limit == 1 {
            0
        } else {
            owned
        };
        let by_points = i64::from((points as u32) / r.price);
        by_points.min(i64::from(r.limit) - i64::from(owned))
    }

    /// What the prize list draws (`FUN_801D0C3C`'s draw half,
    /// `0x801D0DB4..0x801D0F28`), in retail's 320x240 space. Each row from
    /// the floor ([`Self::first_visible`]) to row `5`: its item name at
    /// `(0x52, 0x48 + 16 * row)` through the `0xC2` item-name escape, and its
    /// price as a six-place field at `x = 0xD4` (`FUN_80034B78`), both in ink
    /// `7` when [`Self::is_available`] holds for it and ink `0` when not (the
    /// ink word `_DAT_8007B454` the row stages). Then the overlay's points
    /// label at `(0x52, 0xB8)` and the point pool, held at `999999`, at
    /// `(0xD4, 0xB8)`, both in ink `7`; the cursor icon `0x4E` at
    /// `(0x42, 0x48 + 16 * cursor)` only while the list takes the pad
    /// (`live`); and the panel `(0xA6, 0x40 + 16 * floor, 0xD0,
    /// 16 * (8 - floor))`, so a hidden row 0 shortens it from the top.
    ///
    /// The cursor drawn is the one the routine's entry clamp leaves
    /// ([`Self::step_cursor`] with no key), which it runs ahead of the draw
    /// whether or not the list is live.
    ///
    /// PORT: FUN_801d0c3c (the list's draw, 0x801D0DB4..0x801D0F28)
    // REF: FUN_80034b78 (the decimal field), FUN_8002c488 (the cursor icon)
    pub fn list_draw(
        &self,
        live: bool,
        points: i32,
        available: impl Fn(usize) -> bool,
    ) -> PrizeListDraw {
        let floor = self.first_visible(points);
        let rows = (floor..self.rows.len().min(6))
            .map(|row| {
                let y = 0x48 + 16 * row as i16;
                PrizeListRow {
                    row,
                    name_at: (0x52, y),
                    price: (i64::from(self.rows[row].price), 6, 0xD4, y),
                    available: available(row),
                }
            })
            .collect();
        let cursor = self.step_cursor(self.cursor, points, false, false);
        PrizeListDraw {
            rows,
            points_label_at: (0x52, 0xB8),
            points: (i64::from(points).min(999_999), 6, 0xD4, 0xB8),
            cursor: live.then_some((0x42, 0x48 + 16 * cursor as i16)),
            panel: (
                0xA6,
                0x40 + 16 * floor as i16,
                0xD0,
                16 * (8 - floor as i16),
            ),
        }
    }

    /// What the picker and the confirm draw over the list, in retail's
    /// 320x240 space, or nothing on the list alone. The picker
    /// (`0x801D0B18..0x801D0C14`): the item's name at `(0x38, 0x58)` and its
    /// label at `(0xA0, 0x58)`, then on `y = 0x68` the quantity (`4` places
    /// at `0x5F`), glyph `6` (`/`) at `0x84`, the cap (`4` at `0x81`) and the
    /// cost (`6` at `0xD4`), the cursor icon at `0x4F` while it is live, and
    /// the panel `(0xA6, 0x50, 0x11E, 0x20)`. The confirm
    /// (`0x801D072C..0x801D07C4`): its question at `(0x6C, 0x88)`, Yes and No
    /// at `(0x93, 0x98)` / `(0x93, 0xA8)`, the cursor icon at
    /// `(0x83, 0x98 + 16 * row)` and the panel `(0xA6, 0x80, 0x75, 0x30)`.
    ///
    /// PORT: FUN_801d092c (the picker's draw, 0x801D0B18..0x801D0C14)
    /// PORT: FUN_801d06c8 (the confirm's draw, 0x801D072C..0x801D07C4)
    pub fn dialog_draw(
        &self,
        dlg: &PrizeDialog,
        points: i32,
        owned: u32,
        purchased_mask: u32,
    ) -> PrizeDialogDraw {
        let mut d = PrizeDialogDraw::default();
        for l in self.dialog_layers(dlg, points, owned, purchased_mask) {
            d.texts.extend(l.texts);
            d.numbers.extend(l.numbers);
            d.slash = d.slash.or(l.slash);
            d.cursors.extend(l.cursors);
            d.panels.extend(l.panels);
        }
        d
    }

    /// [`Self::dialog_draw`] one screen at a time, back to front: the
    /// picker's layer, then (on the confirm) the confirm's, each with its
    /// own panel. The session calls the confirm first and the picker after
    /// it, so the ordering table draws the picker under the confirm.
    pub fn dialog_layers(
        &self,
        dlg: &PrizeDialog,
        points: i32,
        owned: u32,
        purchased_mask: u32,
    ) -> Vec<PrizeDialogDraw> {
        let mut out = Vec::new();
        if dlg.screen == PrizeScreen::List {
            return out;
        }
        let Some(r) = self.rows.get(dlg.row) else {
            return out;
        };
        let max = self.picker_max(dlg.row, points, owned, purchased_mask);
        let mut d = PrizeDialogDraw::default();
        d.texts.push((PrizeText::ItemName, 0x38, 0x58));
        d.texts.push((PrizeText::QuantityLabel, 0xA0, 0x58));
        d.numbers.push((i64::from(dlg.qty), 4, 0x5F, 0x68));
        d.slash = Some((0x84, 0x68));
        d.numbers.push((max, 4, 0x81, 0x68));
        d.numbers
            .push((i64::from(r.price) * i64::from(dlg.qty), 6, 0xD4, 0x68));
        if dlg.screen == PrizeScreen::Quantity {
            d.cursors.push((0x4F, 0x68));
        }
        d.panels.push((0xA6, 0x50, 0x11E, 0x20));
        out.push(d);
        if dlg.screen == PrizeScreen::Confirm {
            let mut d = PrizeDialogDraw::default();
            d.texts.push((PrizeText::Question, 0x6C, 0x88));
            d.texts.push((PrizeText::Yes, 0x93, 0x98));
            d.texts.push((PrizeText::No, 0x93, 0xA8));
            d.cursors
                .push((0x83, 0x98 + 16 * (dlg.confirm_row & 1) as i16));
            d.panels.push((0xA6, 0x80, 0x75, 0x30));
            out.push(d);
        }
        out
    }
}

/// One prize-list row as [`PrizeExchange::list_draw`] lays it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrizeListRow {
    /// The row of the venue page.
    pub row: usize,
    /// The item name's pen.
    pub name_at: (i16, i16),
    /// The price field `(value, places, x, y)`.
    pub price: (i64, i32, i16, i16),
    /// Ink `7` (available) or ink `0` (not), for both the name and the price.
    pub available: bool,
}

/// What [`PrizeExchange::list_draw`] lays out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrizeListDraw {
    pub rows: Vec<PrizeListRow>,
    /// The overlay's points label (`0x801CEF9C`).
    pub points_label_at: (i16, i16),
    /// The point pool field `(value, places, x, y)`, held at `999999`.
    pub points: (i64, i32, i16, i16),
    /// The cursor icon, while the list takes the pad.
    pub cursor: Option<(i16, i16)>,
    /// The panel `(cx, y, w, h)` ([`crate::fishing_chrome::centred_panel`]).
    pub panel: (i16, i16, i16, i16),
}

/// A string the exchange's dialogs draw: the picked item's name, or one of
/// the overlay's own lines ([`crate::fishing_hub::FishingHubText`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrizeText {
    ItemName,
    QuantityLabel,
    Question,
    Yes,
    No,
}

/// What [`PrizeExchange::dialog_draw`] lays out: text at its pen, decimal
/// fields `(value, places, x, y)` (`FUN_80034B78`), the `/` glyph, the
/// cursor icon `0x4E` (`FUN_8002C488`) and the panels `(cx, y, w, h)`
/// ([`crate::fishing_chrome::centred_panel`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrizeDialogDraw {
    pub texts: Vec<(PrizeText, i16, i16)>,
    pub numbers: Vec<(i64, i32, i16, i16)>,
    pub slash: Option<(i16, i16)>,
    pub cursors: Vec<(i16, i16)>,
    pub panels: Vec<(i16, i16, i16, i16)>,
}
