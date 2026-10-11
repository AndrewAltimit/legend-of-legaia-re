//! The coin exchange counter: the digit-entry field, the gold / stock quote and its panel draw list.

// --- Coin exchange counter --------------------------------------------------

/// Gold price of one casino coin at the exchange counter (`Total Cost` is the
/// requested coin count times this).
pub const COIN_PRICE_GOLD: i32 = 100;

/// Digit slots in the counter's "Coins to Buy" entry field.
pub const COIN_ENTRY_DIGITS: usize = 8;

/// A quote from the casino's coin-exchange counter: what the entered coin
/// count costs and whether the purchase is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinQuote {
    /// Coin count decoded from the entry field.
    pub coins: i32,
    /// `coins * COIN_PRICE_GOLD`.
    pub cost: i32,
    /// The party can pay: `gold >= cost`.
    pub affordable: bool,
    /// The counter can serve it: `stock >= coins`.
    pub in_stock: bool,
}

impl CoinQuote {
    /// Whether the counter will accept this purchase - retail draws the total
    /// in the normal ink only when both gates pass, and in the alert ink when
    /// either fails.
    pub fn is_valid(&self) -> bool {
        self.affordable && self.in_stock
    }
}

/// Decode the counter's per-digit entry field into a coin count.
///
/// The field is [`COIN_ENTRY_DIGITS`] single-digit cells stored
/// **least-significant first** (the accumulator starts at 1 and multiplies by
/// ten each cell), so `digits[0]` is the units place. The cells are signed
/// bytes: retail loads them with `lb` (`0x801E6FBC`).
// PORT: FUN_801e6f70 (`0x801E6FB8..0x801E6FE4`, the entry field's accumulator)
pub fn coin_entry_value(digits: &[i8]) -> i32 {
    let mut place = 1i32;
    let mut total = 0i32;
    for &d in digits.iter().take(COIN_ENTRY_DIGITS) {
        total = total.wrapping_add(place.wrapping_mul(i32::from(d)));
        place = place.wrapping_mul(10);
    }
    total
}

/// Quote the coin-exchange counter for the entered `digits`, against the
/// party's `gold` and the counter's buyable ceiling `stock`.
///
/// Coins cost a flat [`COIN_PRICE_GOLD`] each. Retail gates the sale twice -
/// on the party's gold (`_DAT_8008459C`) against the total, and on the
/// ceiling the counter's state machine publishes every frame
/// (`_DAT_8007BB90`, `min(gold / 100, bank headroom)`) against the coin
/// count - and recolours the total to the alert ink when *either* fails. The
/// bank word this feeds (`_DAT_800845A4`) is the same one
/// [`SlotMachine::cash_out`] assigns back.
///
/// This is the quote half of the counter's entry panel
/// ([`coin_entry_panel`]); the sale itself commits in the state machine
/// (`legaia_engine_field::submode_hub_actors::coin_exchange`, state 3).
// PORT: FUN_801e6f70 (`0x801E7138..0x801E7174`, the cost and its two gates)
pub fn coin_exchange_quote(digits: &[i8], gold: i32, stock: i32) -> CoinQuote {
    let coins = coin_entry_value(digits);
    let cost = coins.wrapping_mul(COIN_PRICE_GOLD);
    CoinQuote {
        coins,
        cost,
        affordable: gold >= cost,
        in_stock: stock >= coins,
    }
}

/// Record of the field overlay's panel-window table (`0x801F2B98`) whose
/// painter is [`coin_entry_panel`]: the coin counter's idle descriptor
/// (`0x801F3340`) opens it.
pub const COIN_ENTRY_WINDOW: usize = 10;

/// Rodata VAs of the entry panel's four labels, in draw order (bank, entry,
/// gold, total). The bytes are the user's disc's; hosts read them off the
/// field overlay (PROT 0897).
pub const COIN_ENTRY_LABEL_VAS: [u32; 4] = [0x801C_F0D4, 0x801C_F0E0, 0x801C_F0F0, 0x801C_F0FC];

/// System-UI sprite cell the panel draws under the edited digit
/// (`FUN_8002C488(x, y, 0x67)`).
pub const COIN_ENTRY_CARET_CELL: i32 = 0x67;

/// Text pens (`_DAT_8007B454`) the panel stages: the default white, the
/// accent the entry row takes, the normal total and the refused total.
pub const COIN_PEN_DEFAULT: u8 = 7;
pub const COIN_PEN_ENTRY: u8 = 6;
pub const COIN_PEN_TOTAL: u8 = 5;
pub const COIN_PEN_REFUSED: u8 = 9;

/// One draw of the coin counter's entry panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoinPanelDraw {
    /// `FUN_8003CD00(label, x, y)` in pen `pen`.
    Label { va: u32, x: i16, y: i16, pen: u8 },
    /// `FUN_80034B78(value, digits, x, y)` - a blank-padded, right-aligned
    /// decimal field of 8-pixel cells, in pen `pen`.
    Number {
        value: i32,
        digits: u8,
        x: i16,
        y: i16,
        pen: u8,
    },
    /// `FUN_8002C488(x, y, cell)` - the caret under the edited digit.
    Cell { x: i16, y: i16, cell: i32 },
}

/// What the entry panel reads: its window's origin and the counter's live
/// globals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinPanelInput<'a> {
    /// The window actor's `+0x0A` / `+0x0C` - where the descriptor's open
    /// placed it ([`COIN_ENTRY_ORIGIN`] for the retail record).
    pub origin: (i16, i16),
    /// `DAT_801F35F0..+7`, least significant first.
    pub digits: &'a [i8],
    /// `_DAT_8007BB9C` - the edited cell.
    pub cursor: i32,
    /// `_DAT_800845A4` - the coin bank.
    pub bank: i32,
    /// `_DAT_8008459C` - party gold.
    pub gold: i32,
    /// `_DAT_8007BB90` - the buyable ceiling.
    pub stock: i32,
    /// `_DAT_80084570` - the play clock; bits `0xC` blink the caret.
    pub clock: u32,
}

/// Where record [`COIN_ENTRY_WINDOW`]'s open places its window: the record's
/// geometry words at `+8` / `+0xA` (`(0x40, 0x26)`), which the installer's
/// open sub-op hands to `FUN_800357FC` as the window actor's target.
pub const COIN_ENTRY_ORIGIN: (i16, i16) = (0x40, 0x26);

/// The coin counter's **entry panel**: the painter of panel-window record
/// [`COIN_ENTRY_WINDOW`], drawn every frame the counter's idle descriptor
/// has the window open - the digit entry and the confirm both.
///
/// Four rows from the window origin `(x, y)`:
///
/// | Row | Label | Value |
/// |---|---|---|
/// | `y + 2` | label 0, pen 7 | the coin bank, 8 digits at `x + 0x78` |
/// | `y + 0x12` | label 1, pen 6 | cells `5..=0`, one digit each from `x + 0x88`, 8 px apart |
/// | `y + 0x30` | label 2, pen 7 | party gold, 8 digits at `x + 0x78` |
/// | `y + 0x40` | label 3, pen 7 | the total cost, 8 digits at `x + 0x78`, pen 5 or 9 |
///
/// The caret cell sits at `(x + 0xB0 - 8 * cursor, y + 0x1D)` - under the
/// edited digit, the units cell rightmost - and draws only while the cursor
/// is below ten and the play clock's `0xC` bits are non-zero, a blink three
/// frames in four. The total takes the refused pen whenever
/// [`coin_exchange_quote`] fails either gate. Only cells `0..=5` are drawn;
/// the two top cells are summed into the total but never shown.
///
/// Read from the field overlay's bytes (PROT 0897, base `0x801CE818`,
/// `see ghidra/scripts/funcs/801e6f70.txt`). The epilogue restores pen 7.
// PORT: FUN_801e6f70
pub fn coin_entry_panel(input: &CoinPanelInput<'_>) -> Vec<CoinPanelDraw> {
    let (x, y) = input.origin;
    let at = |dx: i32, dy: i32| ((i32::from(x) + dx) as i16, (i32::from(y) + dy) as i16);
    let quote = coin_exchange_quote(input.digits, input.gold, input.stock);
    let mut out = Vec::new();
    let label = |va: u32, (x, y): (i16, i16), pen: u8| CoinPanelDraw::Label { va, x, y, pen };
    let number = |value: i32, digits: u8, (x, y): (i16, i16), pen: u8| CoinPanelDraw::Number {
        value,
        digits,
        x,
        y,
        pen,
    };

    out.push(label(COIN_ENTRY_LABEL_VAS[0], at(0, 2), COIN_PEN_DEFAULT));
    out.push(number(input.bank, 8, at(0x78, 2), COIN_PEN_DEFAULT));

    out.push(label(COIN_ENTRY_LABEL_VAS[1], at(0, 0x12), COIN_PEN_ENTRY));
    for (col, cell) in (0..=5usize).rev().enumerate() {
        let d = input.digits.get(cell).copied().unwrap_or(0);
        out.push(number(
            i32::from(d),
            1,
            at(0x88 + 8 * col as i32, 0x12),
            COIN_PEN_ENTRY,
        ));
    }
    // `slti v0,a0,0xa` is a signed compare, and the blink tests the clock.
    if input.cursor < 10 && input.clock & 0xC != 0 {
        let (cx, cy) = at(0xB0 - 8 * input.cursor, 0x1D);
        out.push(CoinPanelDraw::Cell {
            x: cx,
            y: cy,
            cell: COIN_ENTRY_CARET_CELL,
        });
    }

    out.push(label(
        COIN_ENTRY_LABEL_VAS[2],
        at(0, 0x30),
        COIN_PEN_DEFAULT,
    ));
    out.push(number(input.gold, 8, at(0x78, 0x30), COIN_PEN_DEFAULT));

    out.push(label(
        COIN_ENTRY_LABEL_VAS[3],
        at(0, 0x40),
        COIN_PEN_DEFAULT,
    ));
    let pen = if quote.is_valid() {
        COIN_PEN_TOTAL
    } else {
        COIN_PEN_REFUSED
    };
    out.push(number(quote.cost, 8, at(0x78, 0x40), pen));
    out
}
