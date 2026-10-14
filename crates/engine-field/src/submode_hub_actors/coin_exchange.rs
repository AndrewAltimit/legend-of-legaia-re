//! The coin-exchange handler: the digit counter, its pad cursor and the purchase arithmetic.

use super::*;

// ---------------------------------------------------------------------------
// coin exchange
// ---------------------------------------------------------------------------

/// The coin counter's own globals: the eight decimal cells at `DAT_801F35F0`,
/// the digit cursor `_DAT_8007BB9C`, the Yes/No row `_DAT_8007BB98` and the
/// frame-paced hold timer `_DAT_8007B458`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinCounter {
    /// `DAT_801F35F0..+7`, **least significant cell first**: the entered
    /// amount is `sum(digits[i] * 10^i)`. Cells are signed bytes because
    /// retail sign-extends them into the multiply.
    pub digits: [i8; COIN_ENTRY_DIGITS],
    /// `_DAT_8007BB9C` - which cell the up/down edits hit. Wraps over
    /// [`COIN_CURSOR_CELLS`], not over all eight cells.
    pub cursor: i32,
    /// `_DAT_8007BB98` - the confirm panel's Yes/No row, seeded to `1`.
    pub yes_no: i32,
    /// `_DAT_8007B458` - the post-commit hold timer.
    pub hold: i32,
    /// `_DAT_8007BB90` - the ceiling the head republishes every frame.
    pub ceiling: i32,
}

impl Default for CoinCounter {
    fn default() -> Self {
        Self {
            digits: [0; COIN_ENTRY_DIGITS],
            cursor: 0,
            yes_no: 0,
            hold: 0,
            ceiling: 0,
        }
    }
}

impl CoinCounter {
    /// The amount the eight cells spell.
    ///
    /// PORT: FUN_801f0adc (`0x801F0C20..0x801F0C4C`, and the two identical
    /// copies at `0x801F0E3C` / `0x801F102C`)
    pub fn entered(&self) -> i32 {
        let mut total: i32 = 0;
        let mut power: i32 = 1;
        for d in self.digits {
            total = total.wrapping_add(power.wrapping_mul(d as i32));
            power = power.wrapping_mul(10);
        }
        total
    }

    /// Rewrite the cells as the decimal expansion of `value`, most
    /// significant cell last.
    ///
    /// PORT: FUN_801f0adc (`0x801F0EDC..0x801F0F5C`)
    pub fn set_entered(&mut self, value: i32) {
        let mut rest = value;
        let mut place: i32 = 10_000_000;
        for cell in (0..COIN_ENTRY_DIGITS).rev() {
            let digit = (rest / place) as i8;
            self.digits[cell] = digit;
            rest = rest.wrapping_sub((digit as i32).wrapping_mul(place));
            place /= 10;
        }
    }

    /// Zero every cell (`0x801F0BA4` loop, repeated at `0x801F10A0`).
    pub fn clear_digits(&mut self) {
        self.digits = [0; COIN_ENTRY_DIGITS];
    }
}

/// Cells in the coin counter's decimal entry field.
pub const COIN_ENTRY_DIGITS: usize = 8;

/// Cells the left/right cursor actually visits - retail wraps at `5`, so the
/// top two cells are only ever written by the affordability clamp.
pub const COIN_CURSOR_CELLS: i32 = 6;

/// Idle panel descriptor the counter installs on entry and on a cancelled
/// confirm.
pub const PANEL_COIN_IDLE: u32 = 0x801F_3340;

/// Confirm ("buy this many?") panel descriptor.
pub const PANEL_COIN_CONFIRM: u32 = 0x801F_3360;

/// Cursor-move sting (`FUN_80035B50(0x21)`).
pub const CUE_CURSOR: u8 = 0x21;

/// Accept sting (`FUN_80035B50(0x36)`).
pub const CUE_ACCEPT: u8 = 0x36;

/// Back-out sting (`FUN_80035B50(0x37)`).
pub const CUE_BACK: u8 = 0x37;

/// Coin-jingle sting the commit plays (`FUN_80035B50(0x25)`).
pub const CUE_COINS: u8 = 0x25;

/// Refusal buzz (`FUN_80035BD0(0x23)`) - unaffordable or over the ceiling.
pub const CUE_REFUSE: u8 = 0x23;

/// The picker's own accept sting (`FUN_80035BD0(0)`).
pub const CUE_PICK: u8 = 0x00;

/// Hold frames the commit state waits before closing (`0x801F0FD0`).
pub const COIN_COMMIT_HOLD: i32 = 0x14;

/// Hold frames the post-purchase state waits (`0x801F10BC`).
pub const COIN_CLOSE_HOLD: i32 = 0x10;

/// `FUN_801E9DC8` return meaning "the player accepted the row".
pub const PICK_ACCEPT: i32 = 1;

/// `FUN_801E9DC8` return meaning "the player backed out".
pub const PICK_CANCEL: i32 = 2;

/// PORT: FUN_801f0adc - the casino coin counter: buy coins with gold.
///
/// Handler slot `0x25` of `PTR_FUN_801F33B4`. The head runs every frame
/// regardless of sub-state: it converts party gold into buyable coins at
/// [`GOLD_PER_COIN`] gold each (a signed divide truncating toward zero,
/// spelled in retail as the `0x51EB851F` reciprocal multiply plus the sign
/// fixup), publishes it to `DAT_8007BB90`, and clamps it so the bank cannot
/// pass [`COIN_BANK_MAX`]. It then tail-jumps the sub-handler `[actor+0x54]`
/// out of the five-entry table at `0x801CF734`; the five arms share this
/// frame and fall into its epilogue, which is why the dumped body is
/// discontiguous.
///
/// The five arms, from `overlay_0897_801f0adc.txt` (264 instructions - the
/// minigame-named dumps of this VA stop at 46 and carry only the head, which
/// is why an earlier reading of this routine had no transaction in it at all):
///
/// | `+0x54` | What it does |
/// |---|---|
/// | `0` | Zero the cells + both cursors, install [`PANEL_COIN_IDLE`], advance. |
/// | `1` | Digit entry: cursor moves, digit edits, affordability clamp, accept / back. |
/// | `2` | Yes/No confirm over `FUN_801E9DC8`; accept arms the commit hold. |
/// | `3` | **Commit**: coins to `DAT_800845A4`, gold out of `DAT_8008459C`. |
/// | `4` | Count the hold timer down by the cadence scalar, then hand back. |
///
/// `bank` is read-only here: the commit is reported as
/// [`HubAction::BuyCoins`] so the host applies it to its own money model.
pub fn coin_exchange(
    actor: &mut HubActor,
    env: &HubEnv,
    counter: &mut CoinCounter,
    grid: &mut HubGrid,
) -> HubFrame {
    let mut out = HubFrame::default();
    counter.ceiling = coin_exchange_amount(env.gold, env.coin_bank);
    out.act(HubAction::SetCoinAmount(counter.ceiling));

    // `sltiu v1, 5` - an out-of-range `+0x54` skips straight to the pump.
    let Some(slot) = coin_exchange_slot(actor.sub) else {
        out.act(HubAction::DrawPump);
        return out;
    };

    // Every arm that finishes the screen falls into the shared hand-back
    // tail at `0x801F10F4`; the arms that stay open jump past it.
    let mut hand_back_now = false;
    match slot {
        0 => {
            counter.clear_digits();
            counter.cursor = 0;
            out.act(HubAction::ClearCursorRow);
            out.act(HubAction::InstallPanel(PANEL_COIN_IDLE));
            actor.sub = 1;
        }
        1 => {
            if env.input_blocked != 0 {
                out.act(HubAction::DrawPump);
                return out;
            }
            if env.pad_edge & env.accept_mask != 0 {
                let want = counter.entered();
                if !coin_purchase_affordable(want, env.gold, counter.ceiling) {
                    out.act(HubAction::ConfirmCue(CUE_REFUSE));
                } else if want != 0 {
                    out.act(HubAction::EntryCue(CUE_ACCEPT));
                    out.act(HubAction::InstallPanel(PANEL_COIN_CONFIRM));
                    counter.yes_no = 1;
                    actor.sub = 2;
                } else {
                    // Zero coins: the accept behaves as a back-out.
                    out.act(HubAction::EntryCue(CUE_BACK));
                    hand_back_now = true;
                }
            } else if env.pad_edge & env.back_mask != 0 {
                out.act(HubAction::EntryCue(CUE_BACK));
                hand_back_now = true;
            } else if env.pad_repeat & PAD_CURSOR_RIGHT != 0 {
                out.act(HubAction::EntryCue(CUE_CURSOR));
                counter.cursor = if counter.cursor == 0 {
                    COIN_CURSOR_CELLS - 1
                } else {
                    counter.cursor - 1
                };
            } else if env.pad_repeat & PAD_CURSOR_LEFT != 0 {
                out.act(HubAction::EntryCue(CUE_CURSOR));
                counter.cursor = if counter.cursor == COIN_CURSOR_CELLS - 1 {
                    0
                } else {
                    counter.cursor + 1
                };
            } else {
                if env.pad_repeat & PAD_CURSOR_UP != 0 {
                    out.act(HubAction::EntryCue(CUE_CURSOR));
                    bump_digit(counter, 1);
                }
                if env.pad_repeat & PAD_CURSOR_DOWN != 0 {
                    out.act(HubAction::EntryCue(CUE_CURSOR));
                    bump_digit(counter, -1);
                }
                // Whatever the cells now spell, an unaffordable total is
                // rewritten to the best the player can actually buy.
                let want = counter.entered();
                if !coin_purchase_affordable(want, env.gold, counter.ceiling) {
                    let afford = (env.gold / GOLD_PER_COIN).min(counter.ceiling);
                    counter.set_entered(afford);
                }
            }
        }
        2 => {
            if env.input_blocked != 0 {
                out.act(HubAction::DrawPump);
                return out;
            }
            match env.picker_result {
                PICK_ACCEPT => {
                    out.act(HubAction::ConfirmCue(CUE_PICK));
                    if counter.yes_no != 1 {
                        out.act(HubAction::EntryCue(CUE_COINS));
                        counter.hold = COIN_COMMIT_HOLD;
                        actor.sub = 3;
                    } else {
                        out.act(HubAction::EntryCue(CUE_BACK));
                        out.act(HubAction::InstallPanel(PANEL_COIN_IDLE));
                        actor.sub = 1;
                    }
                }
                PICK_CANCEL => {
                    out.act(HubAction::EntryCue(CUE_BACK));
                    out.act(HubAction::InstallPanel(PANEL_COIN_IDLE));
                    actor.sub = 1;
                }
                _ => {}
            }
        }
        3 => {
            let coins = counter.entered();
            out.act(HubAction::BuyCoins {
                coins,
                gold_cost: coins.wrapping_mul(GOLD_PER_COIN),
            });
            counter.clear_digits();
            out.act(HubAction::ClearCursorRow);
            counter.hold = COIN_CLOSE_HOLD;
            actor.sub = 4;
        }
        _ => {
            counter.hold = counter.hold.wrapping_sub(env.frame_delta);
            if counter.hold <= 0 {
                hand_back_now = true;
            }
        }
    }

    if hand_back_now {
        hand_back(actor, grid);
    }
    out.act(HubAction::DrawPump);
    out
}

/// D-pad bits of the auto-repeating word `_DAT_8007BB84`, in the **packed**
/// Legaia layout (`legaia_engine_system::retail_pad`-shaped, the same bits
/// `legaia_engine_system::pad::PACK_*` names).
///
/// The cell index counts **right to left** - cell `0` is the units digit, the
/// rightmost on screen - so pressing right walks the index down and wraps it
/// to [`COIN_CURSOR_CELLS`]` - 1`, which is why the two arms look inverted
/// against their bit names.
pub const PAD_CURSOR_RIGHT: u32 = 0x2000;

/// See [`PAD_CURSOR_RIGHT`].
pub const PAD_CURSOR_LEFT: u32 = 0x8000;

/// Digit `+1` on the selected cell.
pub const PAD_CURSOR_UP: u32 = 0x1000;

/// Digit `-1` on the selected cell.
pub const PAD_CURSOR_DOWN: u32 = 0x4000;

/// One up/down edit of the selected cell, wrapping `0..=9` (`0x801F0D80` /
/// `0x801F0DE8`).
fn bump_digit(counter: &mut CoinCounter, delta: i8) {
    let Some(cell) = counter.digits.get_mut(counter.cursor.max(0) as usize) else {
        return;
    };
    let next = cell.wrapping_add(delta);
    *cell = if next == 10 {
        0
    } else if next < 0 {
        9
    } else {
        next
    };
}

/// The purchase gate: retail refuses when the gold cost exceeds party gold
/// **or** the amount exceeds the published ceiling (`0x801F0C64..0x801F0C88`).
pub fn coin_purchase_affordable(coins: i32, gold: i32, ceiling: i32) -> bool {
    gold >= coins.wrapping_mul(GOLD_PER_COIN) && ceiling >= coins
}

/// The clamped coin amount `FUN_801F0ADC` publishes to `DAT_8007BB90`.
///
/// PORT: FUN_801f0adc (`0x801F0AE8..0x801F0B44`)
pub fn coin_exchange_amount(gold: i32, coin_bank: i32) -> i32 {
    let coins = gold / GOLD_PER_COIN;
    if COIN_BANK_MAX < coin_bank.wrapping_add(coins) {
        COIN_BANK_MAX - coin_bank
    } else {
        coins
    }
}

/// Sub-handler index, or `None` when `+0x54` is outside the five-slot table
/// (retail's `sltiu v1, 5` guard, which is unsigned - a negative `+0x54`
/// therefore also falls through).
pub fn coin_exchange_slot(sub: i16) -> Option<usize> {
    let s = sub as u16 as u32;
    (s < 5).then_some(s as usize)
}
