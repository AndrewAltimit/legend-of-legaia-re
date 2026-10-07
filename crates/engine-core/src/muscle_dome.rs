//! From-scratch **Muscle Dome** match rules engine.
//!
//! The dome is **not a card battle**. It is a ladder of ordinary Legaia
//! battles: three courses of 8 / 8 / 13 rounds, each round one real monster
//! staged into the ordinary battle formation cell
//! ([`parse_course_ladder`]). Each turn the fighter enters a directional
//! command string under an AP budget - the same input the normal battle
//! command screen takes - and the string plays out through the shared
//! battle-action path.
//!
//! It is **not** turn-limited, and the `"Turns Left: N  HP Left: P%"` strip
//! is not its HUD. That strip's draw sites gate on `*(u8*)0x8007BD0C ==
//! 0xB6`, and `0x8007BD0C` is the four-slot monster-id formation cell
//! ([`crate::encounter_record`],
//! [`FORMATION_CELL_ADDR`](crate::capture_observations::battle_init_overlay::FORMATION_CELL_ADDR)),
//! so the gate reads "the first enemy is monster `0xB6`" - Koru, the game's
//! one four-turn timed boss. The dome's own roster tops out at id `0xAA`, so
//! no dome round can satisfy it. See
//! [`docs/subsystems/minigame-muscle-dome.md`](../../../docs/subsystems/minigame-muscle-dome.md).
//!
//! **A leg ends on a KO and on nothing else.** The arena does not run a
//! battle loop of its own: `FUN_801D1510` stores the round's monster id into
//! formation slot 0, clears slots 1..3, and sets the global game-mode word
//! `_DAT_8007B83C = 0x14` ([`crate::mode::GameMode::BattleInit`]) - its only
//! game-mode write. From there the round is an ordinary battle, ended by the
//! state-`0x5A` end-of-action gate of `FUN_801E295C` when one side has no
//! standing combatant, and routed back to the arena (mode `0x18`) instead of
//! the field by the exit selector `FUN_80046A20`. The battle turn counter
//! `ctx+0x28a` is bumped in one place and never compared against a bound: its
//! readers are per-turn *scripted enemy behaviour* selectors plus Koru's own
//! countdown. So there is no timeout arm to port - see
//! [`crate::timed_fight`], which owns the strip and shows that even Koru's
//! limit is a script arm rather than a bound.
//!
//! This module is the *rules* layer: the four-direction deal, the
//! budget-gated commit into the fighter's action queue, the turn counter,
//! the HP-Left readout, and the win / lose / Seru-reward
//! bookkeeping - driven by the parsed tables ([`legaia_asset::muscle_dome`])
//! and per-command costs (the equipment sections' swing-record `+0x74`
//! bytes, [`legaia_asset::battle_char_assembly::SwingAnimation::cost`]). The
//! sprite presentation, dome camera and the full battle-action playback are
//! host concerns.
//!
//! What is pinned (see
//! [`docs/subsystems/minigame-muscle-dome.md`](../../../docs/subsystems/minigame-muscle-dome.md)):
//!
//! - The deal is four slots; each slot's id comes from the deck table
//!   `DAT_801f4b8c` (the direction-command ids `0xC..=0xF`) and its cost
//!   from the fighter's per-command record (`DAT_801c9360[char][cmd]+0x74`,
//!   the same byte the Arts gauge reads). `FUN_801d388c` case 9.
//! - The turn budget `ctx+0x6dc` seeds from the fighter record `+0x154`;
//!   commit (`case 0xb`) rejects an overspend, appends the direction's
//!   command id to the actor `+0x1df` queue (16 slots, zeroed on the turn's
//!   first commit), debits `ctx+0x6dc` and accrues `ctx+0x6d8`.
//! - The timed-fight strip is drawn by the phase-`0x14` arm of
//!   `FUN_801d0748` (the shared battle round driver), gated on formation
//!   slot 0 `== `[`TIMED_FIGHT_MONSTER_ID`]: `DAT_801f6958 = 4 -
//!   ctx[+0x28a]` (Turns Left, one digit) and `DAT_801f6959 = hp * 100 /
//!   max_hp` of the **enemy** record `DAT_801c937c` (HP Left, three digits).
//!   The format string is on the disc at PROT 0898 file offset `0x0` (VA
//!   `0x801CE818`). The strip is [`crate::timed_fight`]'s; no dome session
//!   reads it.
//! - `ctx+0x28a` is the battle turn counter the shared battle-action SM
//!   bumps at the end of a turn (case `0xff`, which also parks the match
//!   phase at `0x14`; see [`MuscleDomeSession::resolve_turn`]).
//!
//! **A dome leg pays nothing.** What a *contest* pays is casino coins, and
//! the arithmetic is [`DomeContest`] - the ladder layer above a leg. The
//! caption table `0x801F4DFC` that the session's [`reward_banner`] decodes is
//! the shared cast-caption composer's per-character label table, resident in
//! every battle-family overlay and reached whenever anyone casts; reading it
//! as a dome payout is what put an invented Seru capture on a dome win.
//!
//! [`reward_banner`]: MuscleDomeSession::reward_banner
//!
//! What is a documented host model: the opponent commits through the same
//! selection logic (retail has no dome-specific AI table) - here greedily in
//! deal order while its budget lasts, out of the player's own direction deck
//! rather than a monster action set; and per-command damage resolution goes
//! through a host-supplied function.
//!
//! Chain: retail `FUN_801d0748` (match SM, `ctx+6` phases) → `FUN_801d388c`
//! (deal / commit) → the battle-action path (queued-command playback).

// The leg-end chain the module docs above cite. None of it is ported here -
// the arena's handoff and the battle's own end scans live in the battle
// world, and this module only records that they, not a turn budget, decide
// a leg.
// REF: FUN_801d1510 (arena opponent installer: formation slot 0 + game mode 0x14)
// REF: FUN_801e295c (state 0x5A end-of-action KO scans set the battle-end signal)
// REF: FUN_80046a20 (battle-exit mode selector: mode 0x18 returns to the arena)

#[cfg(test)]
use legaia_asset::move_power;

mod loadout;
mod menu;
mod ring;
mod session;

pub use loadout::*;
pub use menu::*;
pub use ring::*;
pub use session::*;

// The World-free half of the dome - the course ladder and score tables, the
// contest ladder above a leg, the hub screen's envelopes and the shared
// constants - lives in `legaia-engine-minigames`; re-exported so every
// `legaia_engine_core::muscle_dome::<item>` path keeps resolving.
pub use legaia_engine_minigames::muscle_dome::*;

/// The `Turns Left / HP Left` strip is Koru's, not the dome's: its gate
/// (`TIMED_FIGHT_MONSTER_ID`), its bound and its readout live in
/// [`crate::timed_fight`], re-exported here because the dome's own ladder
/// tests prove no dome round can satisfy the gate.
pub use crate::timed_fight::{
    TIMED_FIGHT_MONSTER_ID, TIMED_FIGHT_TURN_LIMIT, timed_fight_turns_left,
};

#[cfg(test)]
mod tests;
