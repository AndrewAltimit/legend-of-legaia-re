//! From-scratch **casino slot-machine** rules engine.
//!
//! A port of the confirmed numeric kernels of the slot-machine overlay (PROT
//! 0975, `data\OTHER4`) - the reel-strip permutation builder, the slot LCG,
//! the net-take-bracketed feature roll, the reel-landing search, and the
//! payline / payout / bonus-round evaluation - composed into an interactive
//! session. It consumes spin / stop input and produces reel outcomes and a
//! running coin balance the host commits back to the casino coin bank
//! (`_DAT_800845A4`) on cash-out.
//!
//! What is **Confirmed** (formula pinned in
//! [`docs/subsystems/minigame-slot-machine.md`](../../../docs/subsystems/minigame-slot-machine.md)):
//! - the slot LCG `x = x*5 + 1` with the 16-bit halves folded
//!   (`FUN_801d30cc`, [`SlotRng`]);
//! - the 20-slot reel-strip build: per slot draw RNG mod `0x14`, probe forward
//!   to the first unused position, and place the slot's value there
//!   (`FUN_801cf0d8` case 0, [`build_reel`]). Retail builds **two** strips per
//!   reel in one interleaved pass - the symbol strip `DAT_801d3e90` (`slot/2`,
//!   probe `+0xd`) and the bonus-numeral strip `DAT_801d3fd0`
//!   (`slot/2 + 0x10`, probe `+1`);
//! - the **display strip** `DAT_801d3d50` the win eval and the renderer read, and
//!   the one-row-per-frame copy that refills it from whichever source strip the
//!   feature mode selects - the mechanism by which a bonus round "swaps the reels
//!   to numbers" ([`SlotMachine::tick`], `FUN_801cf0d8` render tail);
//! - the per-reel **claimed value** `DAT_801d3d20` - the payline value + 1,
//!   latched the frame a reel locks and cleared at spin start (`FUN_801d0554`) -
//!   which is what the marquee tally prints ([`SlotMachine::tally`]);
//! - the per-spin feature roll: jitter `rand%5`, normal-mode target
//!   `rand%6 + 2`, one optional widen roll (`rand%100 + 200` when
//!   `DAT_801d3790` is set), and `rand % N == 0` feature-entry rolls whose
//!   denominators are bracketed on the **net-take counter** `DAT_801d3d40` -
//!   `< 1000` → `700`/`500`, `1001..=1999` → `350`/`250`, `> 2000` →
//!   `175`/`125`, plus a flat `widen+600` mode-3 roll (`FUN_801d258c`,
//!   [`feature_roll`]);
//! - the spin charge: a flat [`SPIN_COST_NORMAL`] = 3 coins (the overlay's
//!   "insert 3 coins" help text), [`SPIN_COST_FEATURE`] = 1 coin in feature
//!   modes 4..=6, accruing `+6` / `+1` into the net-take counter;
//! - the net-take counter itself: `+6`/`+1` per spin, **minus** each
//!   bonus-round payout, never reset during a session - the machine gets
//!   *more* generous as its net take rises;
//! - the entry init (`FUN_801cec94`): balance seeded by assignment from the
//!   casino coin bank (default [`ENTRY_DEFAULT_BALANCE`] = 70 when the
//!   battle-return flag `_DAT_8007B8B8` is clear - a dev-launch fallback),
//!   slot LCG seeded with the literal [`ENTRY_LCG_SEED`];
//! - the per-reel stop plan: depth `rand%3 + 2` targeting the normal-mode
//!   symbol in mode 0, depth `(rand&3) + 6` targeting the jackpot symbols
//!   `9` / `8` in the reach modes 1 / 2 (`FUN_801d2114`, [`stop_plan`]);
//! - the landing search: walk up to `depth` rows ahead for the target symbol,
//!   else stop on the next natural row (`FUN_801d2440`, [`land_row`]);
//! - the win evaluation: five paylines - three horizontal and two diagonal
//!   ([`legaia_minigame_formats::minigame_slot_scene::PAYLINE_ROW_OFFSETS`]) - checked
//!   all-equal on the display strip, highest-value line kept, normal payout =
//!   `payout_table[symbol]` ([`legaia_minigame_formats::slot_payout`]), jackpot symbols
//!   `9` / `8` kick off a bonus round of 3 / 1 free spins, and a **bonus round
//!   pays the product of the three payline `(value - 0xf)` factors** - no
//!   equality gate, and the winning line is forced to the centre
//!   (`FUN_801d13e8`, [`SlotMachine::evaluate_spin`]);
//! - the bonus round's own stop plan: depth `0`, target `-1` - i.e. the reel
//!   simply lands on the next row, so the three numbers are the player's timing
//!   and nothing else (`FUN_801d2114` case 6 -> `FUN_801d2440`);
//! - the coin economy: the playing balance is overlay-local, capped at
//!   `9_999_999` in the tally path, and *assigned* to the coin bank on
//!   cash-out (state `100`), not debited per spin.
//!
//! What is an **engine-side reconstruction** (marked at each site): the
//! spin-up velocity/timer magnitudes (visual pacing, not pinned), and the
//! BIOS-`rand` feature stream substituted with a second deterministic
//! [`BiosRand`] LCG so replays stay bit-identical.
//! Feature modes 3 (hot) and 5 (hold) are documented but folded to the
//! normal landing plan here - their bonus-strip value targeting is not modeled.
//!
//! Chain: retail `FUN_801cf0d8` (reel SM) -> `FUN_801d258c` (feature roll) ->
//! `FUN_801d2114` / `FUN_801d2440` (stop) -> `FUN_801d0554` (snap + claim) ->
//! `FUN_801d13e8` (win eval).

use legaia_minigame_formats::slot_payout;

mod cash_out;
mod coin_exchange;
mod frame;
mod machine;
mod paylines;
mod reels;
mod spin_cycle;

pub use cash_out::*;
pub use coin_exchange::*;
pub use frame::*;
pub use machine::*;
pub use paylines::*;
pub use reels::*;

/// Reels on the machine.
pub const REEL_COUNT: usize = 3;
/// Symbols per reel strip (`0x14`).
pub const STRIP_LEN: usize = 20;
/// Distinct symbol ids (`slot/2` over the 20-slot strip → `0..=9`).
pub const SYMBOL_COUNT: usize = 10;
/// Fixed-point reel wrap (`STRIP_LEN << 8`; positions wrap mod `0x1400`).
pub const REEL_WRAP: i32 = (STRIP_LEN as i32) << 8;
/// Balance cap applied in the payout tally path (`9999999`).
pub const BALANCE_CAP: i32 = 9_999_999;
/// The state-1 "not enough coins" gate: a spin needs at least 3 coins banked
/// (`DAT_801d4114 < 3` routes to the state-`0x5a` prompt) - applied in every
/// feature mode, even though a feature spin only costs 1.
pub const MIN_SPIN_BALANCE: i32 = 3;
/// Flat coin cost of a normal spin (feature modes 0..=3): `DAT_801d4114 -= 3`
/// in state `1`. All five paylines always play - there is no bet-line
/// selection ("insert 3 coins" is the whole bet).
pub const SPIN_COST_NORMAL: i32 = 3;
/// Coin cost of a spin during feature modes 4..=6 (`DAT_801d4114 -= 1`).
pub const SPIN_COST_FEATURE: i32 = 1;
/// Net-take accrual per normal spin (`DAT_801d3d40 += 6` - twice the coins
/// charged).
pub const NET_TAKE_NORMAL_SPIN: i32 = 6;
/// Net-take accrual per feature-mode spin (`DAT_801d3d40 += 1`).
pub const NET_TAKE_FEATURE_SPIN: i32 = 1;
/// Entry-init balance fallback (`FUN_801cec94`): when the battle-return flag
/// `_DAT_8007B8B8` is clear (the overlay launched outside the casino door
/// path), the balance defaults to `0x46` = 70 coins instead of the coin-bank
/// copy.
pub const ENTRY_DEFAULT_BALANCE: i32 = 70;
/// The literal seed `FUN_801cec94` writes into the slot LCG (`DAT_801d3c80`)
/// on every machine entry.
pub const ENTRY_LCG_SEED: u32 = 0x6C0A_2AF0;
/// Bonus rounds granted when the jackpot symbol `9` (the red "punch") matches
/// (`FUN_801d13e8`).
pub const BONUS_SPINS_JACKPOT: i32 = slot_payout::PUNCH_BONUS_ROUNDS as i32;
/// Bonus rounds granted when the bonus symbol `8` (the blue "kick") matches.
pub const BONUS_SPINS_BONUS: i32 = slot_payout::KICK_BONUS_ROUNDS as i32;
/// The probe step used for the **symbol** strip array `DAT_801d3e90`
/// (`(pos + 0xd) % 0x14`).
pub const STRIP_PROBE_PRIMARY: usize = 0xd;
/// The probe step used for the **bonus** strip array `DAT_801d3fd0`
/// (`(pos + 1) % 0x14`).
pub const STRIP_PROBE_SECONDARY: usize = 1;
/// The feature mode a bonus round runs in (`DAT_801d3cac == 6`).
pub const FEATURE_MODE_BONUS: u8 = 6;
/// How far **ahead of the payline row** the display strip is refilled, in strip
/// rows.
///
/// Retail refills exactly one row of the display strip per reel per frame, at
/// row `(pos >> 8) + 0x19`, while the payline it pays on is row
/// `(pos >> 8) + 0x10` - so the row being rewritten is always this many rows
/// ahead of the payline (`0x19 - 0x10`, both mod `0x14`). That gap is the whole
/// bonus-round strip swap: rows are converted to the other strip *before* they
/// reach the payline, and the mode-6 spin timer ([`BONUS_SPIN_UP_FRAMES`]) is
/// sized to guarantee the reel travels far enough for the conversion to arrive.
pub const DISPLAY_REFRESH_LEAD: usize = 9;

use legaia_minigame_formats::minigame_slot_scene::SlotScreen;

#[cfg(test)]
mod tests;
