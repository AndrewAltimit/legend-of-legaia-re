//! The Baka Fighter **cabinet shell**: the overlay's top-level mode state
//! machine, the HUD renderer it calls every frame, and the developer
//! action-table dump one of its states reaches.
//!
//! [`crate::baka_fighter`] holds the *fight* rules - the exchange resolver,
//! the damage kernel, the CPU picker. This module holds the layer above them:
//! the attract card, the player-select screen, the per-round bracket, the
//! win / lose / all-clear sequences, the "NEXT GAME / PAY OUT" tally menu, the
//! in-duel pause menu and the developer menu. Retail runs all of it out of one
//! 2973-instruction dispatcher, `FUN_801CF388`, switching on a single
//! **cabinet-state** word (`DAT_801DBF44`).
//!
//! ## The state word is not a boolean
//!
//! `DAT_801DBF44` is documented elsewhere as "the match-active gate (`== 100`
//! while a round runs)". That is true but narrow: `100` (`0x64`) is one of
//! **37** values the dispatcher branches on, and the rest are the whole cabinet
//! sequence. `FUN_801D4FC8`'s "editor band" gate (`DAT_801DBF44 - 400 < 100`)
//! is the same word - states `0x190` / `0x191` are the developer keyframe
//! editor, which is why that band exists at all.
//!
//! Every unlisted value falls through to the shared epilogue, which draws the
//! arena and (for the in-duel states) the HUD, then decays the screen shake.
//!
//! ## Pad masks
//!
//! The cabinet reads the **packed** Legaia pad word (`_DAT_8007B874` edges,
//! `_DAT_8007B850` held), not the raw PSX layout - see
//! [`crate::retail_pad`] and the [`PACK_*`](crate::dev_menu::PACK_LEFT)
//! constants. That is what makes the cursor bits `0x8000` / `0x2000` the
//! **left / right** d-pad on the horizontal screens and `0x1000` / `0x4000`
//! **up / down** on the vertical ones, while `0xF0` is "any face button".
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_baka_fighter_801cf388.txt`,
//! `overlay_baka_fighter_801d2afc.txt`,
//! `overlay_baka_fighter_801d553c.txt` - disassembly, not the decompiled C
//! (the dispatcher's C renders most of its exit paths as fake
//! `FUN_801xxxxx()` label-calls).

use legaia_asset::baka_opponents::BakaActionSet;

// --------------------------------------------------------------- pad masks

/// Confirm edge the cabinet's menus accept (`0x44` - Cross plus L1).
pub const CABINET_CONFIRM: u16 = 0x0044;
/// Cancel edge the cabinet's menus accept (`0x21` - Circle plus L2).
pub const CABINET_CANCEL: u16 = 0x0021;
/// Attract-screen "press to start" edge (`0x844` - Start, Cross or L1).
pub const CABINET_START: u16 = 0x0844;
/// Any of the four face buttons (`0xF0`) - the instructions screen's dismiss.
pub const CABINET_ANY_FACE: u16 = 0x00F0;
/// In-duel pause-menu edge (`0x110` - Select or Triangle). The HUD's top strip
/// reads "PRESS SELECT TO MENU"; Triangle opens it too.
pub const CABINET_DUEL_MENU: u16 = 0x0110;
/// Cursor-left edge on the horizontal screens (`0x8000`).
pub const CABINET_LEFT: u16 = 0x8000;
/// Cursor-right edge on the horizontal screens (`0x2000`).
pub const CABINET_RIGHT: u16 = 0x2000;
/// Cursor-up edge on the vertical menus (`0x1000`).
pub const CABINET_UP: u16 = 0x1000;
/// Cursor-down edge on the vertical menus (`0x4000`).
pub const CABINET_DOWN: u16 = 0x4000;
/// Developer-menu "dump the action table" edge (`0x4` - L1), state
/// [`ST_DEV_EDITOR_RUN`].
pub const CABINET_DEV_DUMP: u16 = 0x0004;
/// Developer-menu "leave the editor" edge (`0x100` - Select).
pub const CABINET_DEV_EXIT: u16 = 0x0100;
/// Held-pad bit that spawns the editor actor from the round-setup state
/// (`0x10` - Triangle).
pub const CABINET_DEV_SPAWN_HELD: u16 = 0x0010;

// ------------------------------------------------------------- state ids

/// Cold entry: zero the counters, arm the attract BGM.
pub const ST_BOOT: u32 = 0x00;
/// Attract / title card (`FUN_801D59D4` animates it).
pub const ST_ATTRACT: u32 = 0x01;
/// Attract fade-out into the player-select screen.
pub const ST_ATTRACT_OUT: u32 = 0x02;
/// Player-select setup: spawn the three party fighters, seed the RNG.
pub const ST_SELECT_SETUP: u32 = 0x0A;
/// Player select: a **horizontal** 3-way cursor over the party fighters.
pub const ST_SELECT: u32 = 0x0B;
/// Player chosen: start the wipe into the opponent reveal.
pub const ST_SELECT_DONE: u32 = 0x0C;
/// Wipe hold (`0x1F` frame-steps).
pub const ST_SELECT_WIPE: u32 = 0x0D;
/// Spawn the chosen fighter's preview actor.
pub const ST_SELECT_POSE: u32 = 0x0E;
/// Opponent install: resolve the rung's roster id + mesh, load them, arm the
/// duel BGM. This is where the ladder's rung actually becomes an opponent.
pub const ST_OPPONENT_INSTALL: u32 = 0x1E;
/// Round setup: seat both fighters, reset HP to [`FULL_HP`].
pub const ST_ROUND_SETUP: u32 = 0x32;
/// Round banner, first beat.
pub const ST_ROUND_BANNER_A: u32 = 0x33;
/// Round banner, second beat (secret-opponent BGM ramp runs here).
pub const ST_ROUND_BANNER_B: u32 = 0x34;
/// Round banner, camera pull-in.
pub const ST_ROUND_BANNER_C: u32 = 0x35;
/// Round banner hold (`0x79` frame-steps).
pub const ST_ROUND_BANNER_D: u32 = 0x36;
/// Wait for the scene-ready scratchpad flag.
pub const ST_ROUND_READY: u32 = 0x37;
/// "FIGHT!" flash, then hand off to the duel.
pub const ST_ROUND_GO: u32 = 0x38;
/// **The duel.** The fight rules run under this state; it also owns the
/// round bracket, the win / lose tests and the pause-menu edge.
pub const ST_DUEL: u32 = 0x64;
/// Perfect-round flourish (opponent took no rounds).
pub const ST_PERFECT: u32 = 0x65;
/// Score tally screen (`FUN_801D239C` drains the counters).
pub const ST_TALLY: u32 = 0x66;
/// Tally wind-down.
pub const ST_TALLY_OUT: u32 = 0x67;
/// "NEXT GAME / PAY OUT" menu - a **horizontal** 2-way cursor.
pub const ST_CHOICE: u32 = 0x68;
/// The secret opponent's own tally variant.
pub const ST_CHOICE_SECRET: u32 = 0x6D;
/// Teardown + reload for the next rung.
pub const ST_NEXT_RUNG: u32 = 0x6E;
/// Match lost: wipe in.
pub const ST_LOSE: u32 = 0x96;
/// "GAME OVER" - **this is where the accumulated pot is forfeited**.
pub const ST_GAME_OVER: u32 = 0x97;
/// In-duel pause menu, 2 options.
pub const ST_PAUSE_2: u32 = 0xBE;
/// In-duel pause menu, 3 options.
pub const ST_PAUSE_3: u32 = 0xBF;
/// "How to Play" instructions screen (`FUN_801D6CBC` draws it).
pub const ST_HOWTO: u32 = 0xC0;
/// Developer menu (5 rows), reachable only while `_DAT_8007B868` is set.
pub const ST_DEV_MENU: u32 = 0xC8;
/// All-stage-clear sequence, beat 1.
pub const ST_CLEAR_A: u32 = 0xFA;
/// All-stage-clear sequence, beat 2.
pub const ST_CLEAR_B: u32 = 0xFB;
/// All-stage-clear sequence, beat 3.
pub const ST_CLEAR_C: u32 = 0xFC;
/// All-stage-clear sequence, beat 4.
pub const ST_CLEAR_D: u32 = 0xFD;
/// All-stage-clear sequence, beat 5.
pub const ST_CLEAR_E: u32 = 0xFE;
/// Developer keyframe-editor entry (`FUN_801D4FC8`'s band opens here).
pub const ST_DEV_EDITOR: u32 = 0x190;
/// Developer keyframe editor, running. L1 dumps the action table.
pub const ST_DEV_EDITOR_RUN: u32 = 0x191;
/// Exit: pay the accumulated pot into the casino coin bank and leave.
pub const ST_EXIT: u32 = 0x1F4;

/// Every state the dispatcher has a branch for, in ascending order. Anything
/// else falls through to the shared epilogue.
pub const CABINET_STATES: [u32; 37] = [
    ST_BOOT,
    ST_ATTRACT,
    ST_ATTRACT_OUT,
    ST_SELECT_SETUP,
    ST_SELECT,
    ST_SELECT_DONE,
    ST_SELECT_WIPE,
    ST_SELECT_POSE,
    ST_OPPONENT_INSTALL,
    ST_ROUND_SETUP,
    ST_ROUND_BANNER_A,
    ST_ROUND_BANNER_B,
    ST_ROUND_BANNER_C,
    ST_ROUND_BANNER_D,
    ST_ROUND_READY,
    ST_ROUND_GO,
    ST_DUEL,
    ST_PERFECT,
    ST_TALLY,
    ST_TALLY_OUT,
    ST_CHOICE,
    ST_CHOICE_SECRET,
    ST_NEXT_RUNG,
    ST_LOSE,
    ST_GAME_OVER,
    ST_PAUSE_2,
    ST_PAUSE_3,
    ST_HOWTO,
    ST_DEV_MENU,
    ST_CLEAR_A,
    ST_CLEAR_B,
    ST_CLEAR_C,
    ST_CLEAR_D,
    ST_CLEAR_E,
    ST_DEV_EDITOR,
    ST_DEV_EDITOR_RUN,
    ST_EXIT,
];

/// `true` when the dispatcher has a case for `state`.
pub fn is_cabinet_state(state: u32) -> bool {
    CABINET_STATES.contains(&state)
}

/// PORT: FUN_801cf388 - the epilogue's arena pass gate (`s4`).
///
/// `s4` enters the dispatcher at `0`; a state raises it by writing `1` and the
/// epilogue draws the four arena walls only if it is set. Which states raise it
/// is not "all of them" - the attract pair, the score tally, three of the five
/// all-clear beats and the exit state leave it clear, and two of those
/// (`0xFD` / `0xFE`) clear it explicitly. Read off the `li s4, 1` /
/// `clear s4` sites, delay slots included.
pub fn draws_arena(state: u32) -> bool {
    !matches!(
        state,
        ST_BOOT | ST_ATTRACT | ST_ATTRACT_OUT | ST_TALLY | ST_CLEAR_B | ST_CLEAR_D | ST_CLEAR_E
    ) && state != ST_EXIT
        && is_cabinet_state(state)
}

/// PORT: FUN_801cf388 - the epilogue's HUD pass gate (`s7`).
///
/// Far narrower than `s4`: only eight states raise it, and they are exactly the
/// round-hold / duel / result band. Notably the score tally (`0x66`) does
/// **not** - the tally screen is its own presentation, not the duel HUD - and
/// nor does the "NEXT GAME / PAY OUT" menu.
pub fn draws_hud(state: u32) -> bool {
    matches!(
        state,
        ST_ROUND_BANNER_D
            | ST_ROUND_READY
            | ST_ROUND_GO
            | ST_DUEL
            | ST_PERFECT
            | ST_NEXT_RUNG
            | ST_LOSE
            | ST_GAME_OVER
    )
}

// ---------------------------------------------------------------- constants

/// Per-fighter starting HP the round setup writes (`0xC80`).
pub const FULL_HP: i32 = 0xC80;
/// Stage counter value at which the ladder wraps and the all-clear flag is
/// raised (`DAT_801DC10C >= 0xE`).
pub const STAGE_WRAP: i32 = 0x0E;
/// `roster_id = stage + ROSTER_FROM_STAGE`.
pub const ROSTER_FROM_STAGE: i32 = 3;
/// `mesh_index = stage + MESH_FROM_STAGE`.
pub const MESH_FROM_STAGE: i32 = 5;
/// Stage the **first** secret opponent is offered on.
pub const SECRET_A_STAGE: i32 = 5;
/// High score above which that offer fires (`0x3D090`).
pub const SECRET_A_SCORE: i32 = 250_000;
/// Stage the **second** secret opponent is offered on.
pub const SECRET_B_STAGE: i32 = 0x0D;
/// High score above which that offer fires (`0xAAE60`).
pub const SECRET_B_SCORE: i32 = 700_000;
/// Tally bonus row seeded when the opponent took **no** round (a perfect).
pub const PERFECT_BONUS: i32 = 0x7530;
/// Tally bonus row seeded otherwise.
pub const MATCH_BONUS: i32 = 0x4E20;
/// Player-select cursor span (the three party fighters).
pub const SELECT_OPTIONS: i32 = 3;
/// Rows the developer menu offers.
pub const DEV_MENU_ROWS: i32 = 5;

/// The secret-opponent override (`DAT_801DBF06`): which of the two bonus
/// rungs the cabinet is about to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretOpponent {
    /// None pending - the rung comes from the stage counter.
    None,
    /// The stage-5 rung (roster id `3`).
    First,
    /// The stage-13 rung (roster id `4`).
    Second,
}

impl SecretOpponent {
    /// Raw override value the overlay stores.
    pub fn raw(self) -> i16 {
        match self {
            Self::None => 0,
            Self::First => 1,
            Self::Second => 2,
        }
    }

    /// Decode the raw override value the overlay stores (anything outside
    /// `1..=2` is "none", which is how retail's own `!= 0` / `== 2` tests read
    /// it).
    pub fn from_raw(v: i16) -> Self {
        match v {
            1 => Self::First,
            2 => Self::Second,
            _ => Self::None,
        }
    }
}

/// PORT: FUN_801cf388 (`0x801D06FC`..`0x801D0798`) - the **secret-opponent
/// score gate**.
///
/// After a match win, before the stage counter advances, the cabinet tests the
/// rung it just cleared against the running high score. Stage
/// [`SECRET_A_STAGE`] past [`SECRET_A_SCORE`] arms [`SecretOpponent::First`];
/// stage [`SECRET_B_STAGE`] past [`SECRET_B_SCORE`] arms
/// [`SecretOpponent::Second`]. An armed override *suppresses* the stage
/// advance, so the bonus rung is inserted into the ladder rather than
/// replacing one - which is the route roster ids `3` and `4` are reached by
/// without a full lap of the counter.
///
/// The comparisons are strict (`slt`): a score exactly on the threshold does
/// not qualify.
pub fn secret_opponent_gate(stage: i32, high_score: i32) -> SecretOpponent {
    if stage == SECRET_A_STAGE && high_score > SECRET_A_SCORE {
        SecretOpponent::First
    } else if stage == SECRET_B_STAGE && high_score > SECRET_B_SCORE {
        SecretOpponent::Second
    } else {
        SecretOpponent::None
    }
}

/// Outcome of advancing the ladder's stage counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageAdvance {
    /// Stage counter after the step.
    pub stage: i32,
    /// `true` when the counter reached [`STAGE_WRAP`] and wrapped to `0`,
    /// which is the ladder's all-clear.
    pub all_clear: bool,
}

/// PORT: FUN_801cf388 (`0x801D0808`..`0x801D083C`) - the stage advance.
///
/// The counter only steps when no secret rung is pending. Reaching
/// [`STAGE_WRAP`] raises the all-clear flag and wraps the counter to `0`, so
/// a second lap starts at roster id [`ROSTER_FROM_STAGE`].
pub fn advance_stage(stage: i32, secret: SecretOpponent) -> StageAdvance {
    let stage = if secret == SecretOpponent::None {
        stage + 1
    } else {
        stage
    };
    // Retail tests `(u32)stage < 0xE`, so a negative stage folds to the wrap
    // arm as well.
    if (stage as u32) < STAGE_WRAP as u32 {
        StageAdvance {
            stage,
            all_clear: false,
        }
    } else {
        StageAdvance {
            stage: 0,
            all_clear: true,
        }
    }
}

/// PORT: FUN_801cf388 (`0x801CFD14`..`0x801CFD4C`) - the rung fold.
///
/// Which roster record and which mesh index the opponent-install state loads.
/// With no secret rung pending the stage counter supplies both
/// (`stage + 3` / `stage + 5`); an armed override replaces them with
/// `override + 2` / `override + 4`, which is how roster ids `3` and `4` pair
/// with mesh indices `5` and `6`.
pub fn rung_fold(stage: i32, secret: SecretOpponent) -> (i32, i32) {
    match secret {
        SecretOpponent::None => (stage + ROSTER_FROM_STAGE, stage + MESH_FROM_STAGE),
        other => {
            let sel = other.raw() as i32;
            (sel + 2, sel + 4)
        }
    }
}

/// PORT: FUN_801cf388 (`0x801D0844`..`0x801D085C`) - the match bonus row.
///
/// The tally's first score row is seeded [`PERFECT_BONUS`] when the opponent
/// won no round at all, else [`MATCH_BONUS`].
pub fn match_bonus(opponent_round_wins: u32) -> i32 {
    if opponent_round_wins == 0 {
        PERFECT_BONUS
    } else {
        MATCH_BONUS
    }
}

/// PORT: FUN_801cf388 (`0x801CFA64`..`0x801CFAF8`) - the **player-select**
/// cursor.
///
/// Left steps down, right steps up, and the fold is **two** independent clamps
/// in retail's order: `>= span` goes to `0` first, then `< 0` goes to
/// `span - 1`. So this cursor wraps both ways. Returns the new cursor plus
/// whether a cursor blip fires.
///
/// The two two-option menus (the tally choice `0x68` and the pause menu `0xBE`)
/// fold with `& 1` instead, which is the same thing for a span of `2` -
/// `-1 & 1 == 1` and `2 & 1 == 0` - so they use this too.
pub fn cursor_step(cursor: i32, span: i32, pad_edge: u16, left: u16, right: u16) -> (i32, bool) {
    let (mut c, moved) = cursor_raw(cursor, pad_edge, left, right);
    if c >= span {
        c = 0;
    }
    if c < 0 {
        c = span - 1;
    }
    (c, moved)
}

/// PORT: FUN_801cf388 (`0x801D14B4`..`0x801D1508`, `0x801D1748`..`0x801D17B0`) -
/// the **modulo** cursor fold, used by the 3-option pause menu (`0xBF`) and the
/// 5-row developer menu (`0xC8`).
///
/// These two do not wrap the way the player-select cursor does. Retail folds
/// them with an **unsigned** `% span` (the `0xAAAAAAAB` / `0xCCCCCCCD`
/// reciprocal multiplies), and `-1` as a `u32` is `0xFFFFFFFF`, which both `3`
/// and `5` divide exactly - so stepping *up* off the top row lands on row `0`
/// again rather than on the bottom row. The cursor is sticky at the top and
/// wraps only at the bottom.
pub fn cursor_step_mod(
    cursor: i32,
    span: i32,
    pad_edge: u16,
    left: u16,
    right: u16,
) -> (i32, bool) {
    let (c, moved) = cursor_raw(cursor, pad_edge, left, right);
    if span <= 0 {
        return (c, moved);
    }
    ((c as u32 % span as u32) as i32, moved)
}

/// The step both folds share: left decrements, right increments, and either
/// edge counts as a move (both in one frame cancel out but still blip).
fn cursor_raw(cursor: i32, pad_edge: u16, left: u16, right: u16) -> (i32, bool) {
    let mut c = cursor;
    let mut moved = false;
    if pad_edge & left != 0 {
        c -= 1;
        moved = true;
    }
    if pad_edge & right != 0 {
        c += 1;
        moved = true;
    }
    (c, moved)
}

/// PORT: FUN_801cf388 (`0x801D1FE8`..`0x801D2070`) - the epilogue's screen
/// shake decay.
///
/// The shake amplitude (`DAT_801DBEC0`) and the paired audio-pan word both
/// step toward zero by `|amplitude| * frame_step / 6 + 1` per frame, and both
/// are hard-clamped at zero the moment the step overshoots - so a shake always
/// terminates rather than ringing about zero.
pub fn shake_decay(amplitude: i32, frame_step: i32) -> i32 {
    let step = (amplitude.abs() * frame_step) / 6 + 1;
    if amplitude > 0 {
        let next = amplitude - step;
        if next < 0 { 0 } else { next }
    } else if amplitude < 0 {
        let next = amplitude + step;
        if next > 0 { 0 } else { next }
    } else {
        0
    }
}

// ------------------------------------------------------------- HUD renderer

/// One VITAL fill bar, as the HUD renderer builds it: a raw gouraud quad, one
/// pixel of width per 32 HP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VitalBar {
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
    /// Colour at the bar's far (empty) end.
    pub rgb_far: [u8; 3],
    /// Colour at the bar's anchored end.
    pub rgb_anchor: [u8; 3],
}

/// Top scanline of the VITAL bars (`0x26`).
pub const VITAL_Y0: i16 = 0x26;
/// Bottom scanline of the VITAL bars (`0x2B`).
pub const VITAL_Y1: i16 = 0x2B;
/// The player's bar is right-anchored here and fills leftward.
pub const VITAL_ANCHOR_LEFT: i16 = 0x89;
/// The opponent's bar is left-anchored here and fills rightward.
pub const VITAL_ANCHOR_RIGHT: i16 = 0xB8;
/// Red channel both bars hold flat (`0xBC`).
pub const VITAL_RED: u8 = 0xBC;

/// Arithmetic-shift-right rounding toward zero, the `bgez` + bias idiom the
/// renderer applies to every HP division.
fn sra_round_to_zero(v: i32, shift: u32) -> i32 {
    (if v < 0 { v + ((1 << shift) - 1) } else { v }) >> shift
}

/// PORT: FUN_801d2afc (`0x801D2B44`..`0x801D2C38`) - the VITAL fill bar.
///
/// Width is `hp >> 5` (rounded toward zero), so one pixel per 32 HP: at
/// [`FULL_HP`] the bar is `0x64` pixels. The green channel *is* that width, so
/// the quad's far end runs `(0xBC, hp >> 5, 0)` and reddens toward the anchor
/// as HP drops. Slot `0` anchors right at [`VITAL_ANCHOR_LEFT`] and fills
/// leftward; slot `1` anchors left at [`VITAL_ANCHOR_RIGHT`] and fills
/// rightward.
pub fn vital_bar(slot: usize, hp: i32) -> VitalBar {
    let w = sra_round_to_zero(hp, 5);
    let green = w.clamp(0, 0xFF) as u8;
    let (x0, x1) = if slot == 0 {
        (VITAL_ANCHOR_LEFT - w as i16, VITAL_ANCHOR_LEFT)
    } else {
        (VITAL_ANCHOR_RIGHT, VITAL_ANCHOR_RIGHT + w as i16)
    };
    VitalBar {
        x0,
        y0: VITAL_Y0,
        x1,
        y1: VITAL_Y1,
        rgb_far: [VITAL_RED, green, 0],
        rgb_anchor: [VITAL_RED, 0, 0],
    }
}

/// Texture-U of a **filled** round-win pip (`0x30`).
pub const PIP_U_FILLED: u8 = 0x30;
/// Texture-U of an **empty** round-win pip (`0x40`).
pub const PIP_U_EMPTY: u8 = 0x40;
/// Scanline the pip row sits on (`0x30`).
pub const PIP_Y: i16 = 0x30;

/// PORT: FUN_801d2afc (`0x801D2FBC`..`0x801D30C4`) - the round-win pip row.
///
/// One 16x16 cell per pip up to the win target ([`crate::baka_fighter`]'s
/// best-of-3 `DAT_801DBED0`), filled while `i < wins`. The player's row steps
/// right from `0x70`, the opponent's steps **left** from `0xC0`. Returns
/// `(x, y, u)` per pip in draw order.
pub fn round_win_pips(slot: usize, wins: u32, target: u32) -> Vec<(i16, i16, u8)> {
    (0..target as i16)
        .map(|i| {
            let x = if slot == 0 {
                0x70 + i * 16
            } else {
                0xC0 - i * 16
            };
            let u = if (i as u32) < wins {
                PIP_U_FILLED
            } else {
                PIP_U_EMPTY
            };
            (x, PIP_Y, u)
        })
        .collect()
}

/// PORT: FUN_801d2afc (`0x801D3150`..`0x801D31A0`) - the combo counter's
/// brightness.
///
/// Not a monotonic ramp. The count is first clamped to `12`, then: `<= 3`
/// draws at full `0xF0`; `4..=6` *dims* to `(10 - combo) * 16 + 0x80`
/// (`0xE0` / `0xD0` / `0xC0`); and from `7` up the second `< 4` test fires and
/// it snaps back to `0xF0`. The dip is the flash - a long streak is drawn at
/// full brightness, not brighter.
pub fn combo_counter_level(combo: i32) -> u8 {
    let combo = if combo >= 13 { 12 } else { combo };
    if combo < 4 {
        return 0xF0;
    }
    let k = 10 - combo;
    if k < 4 { 0xF0 } else { (k * 16 + 0x80) as u8 }
}

/// PORT: FUN_801d2afc (`0x801D3270`..`0x801D32FC`) - the stage digits.
///
/// The drawn value is `DAT_801DC110 + 1` (the stage counter is 0-based, the
/// cabinet numbers its rungs from 1). Below ten only the units digit is drawn,
/// at `x = 0x48`; from ten the tens digit goes at `x = 0x40` first. Returns
/// `(x, y, digit)` in draw order.
pub fn stage_digits(stage_display: i32) -> Vec<(i16, i16, u8)> {
    let v = stage_display;
    let mut out = Vec::with_capacity(2);
    if v >= 10 {
        out.push((0x40, 0x1E, ((v / 10) % 10) as u8));
    }
    out.push((0x48, 0x1E, (v % 10) as u8));
    out
}

/// PORT: FUN_801d2afc (`0x801D3300`..`0x801D335C`) - the high-score readout's
/// right-aligned origin.
///
/// Retail counts the score's decimal places by testing it against
/// `10^k - 1` for `k = 0..=7`, starting from eight slots and decrementing on
/// each miss. The `- 1` makes the test off by one at every power of ten, so a
/// score of `9` counts as **two** places, not one - the readout sits one cell
/// further left than a true digit count would put it. That quirk is retail's;
/// the port keeps it.
pub fn high_score_places(score: i32) -> i32 {
    let mut places = 8;
    let mut p: i32 = 1;
    for _ in 0..8 {
        if score < p - 1 {
            places -= 1;
        }
        p = p.saturating_mul(10);
    }
    places
}

/// Screen x the high-score number is drawn from (`0x28 - places * 8`); the
/// readout is skipped entirely at a score of zero.
pub fn high_score_x(score: i32) -> Option<i16> {
    if score == 0 {
        return None;
    }
    Some(0x28 - (high_score_places(score) * 8) as i16)
}

/// PORT: FUN_801d2afc (`0x801D2DC8`..`0x801D2DF0`) - the running-max latch.
///
/// The HUD renderer is where the score's combo term is latched, and it feeds
/// **two** globals from the same source (`DAT_801DC094`, slot 1's
/// consecutive-hits-taken counter): `DAT_801DBF58` and `DAT_801DBEC8`. Both
/// are plain running maxima, so a streak that ends does not lower them.
pub fn max_streak_latch(latch: i32, streak: i32) -> i32 {
    if latch < streak { streak } else { latch }
}

/// Everything the HUD renderer emits for one frame, renderer-agnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudFrame {
    /// Per-slot VITAL fill bars.
    pub bars: [VitalBar; 2],
    /// Per-slot round-win pips, `(x, y, u)`.
    pub pips: [Vec<(i16, i16, u8)>; 2],
    /// Per-slot combo-counter brightness. The sides are **crossed**: index `0`
    /// carries slot 1's hits-taken counter (the streak the player is landing).
    pub combo_level: [u8; 2],
    /// Stage digits, `(x, y, digit)`.
    pub stage: Vec<(i16, i16, u8)>,
    /// High-score readout origin, absent at a score of zero.
    pub high_score_x: Option<i16>,
    /// The running max-streak latch after this frame.
    pub max_streak: i32,
}

/// PORT: FUN_801d2afc - the duel HUD renderer.
///
/// Retail calls this from [`FUN_801CF388`'s epilogue](BakaCabinet::tick) once
/// per frame, for the in-duel states only, and only while the
/// suppress-scene flag is clear. Every piece it draws is one of the kernels
/// above; the textured cells themselves go through the widget-quad emitter
/// (`crate::baka_fighter::hud_widget_quad`), which stays a host job.
///
/// `combo_taken` is the per-slot consecutive-hits-taken counter; the renderer
/// crosses the two so each side shows the streak it is *landing*.
pub fn hud_frame(
    hp: [i32; 2],
    round_wins: [u32; 2],
    win_target: u32,
    combo_taken: [i32; 2],
    stage_display: i32,
    high_score: i32,
    latch: i32,
) -> HudFrame {
    HudFrame {
        bars: [vital_bar(0, hp[0]), vital_bar(1, hp[1])],
        pips: [
            round_win_pips(0, round_wins[0], win_target),
            round_win_pips(1, round_wins[1], win_target),
        ],
        combo_level: [
            combo_counter_level(combo_taken[1]),
            combo_counter_level(combo_taken[0]),
        ],
        stage: stage_digits(stage_display),
        high_score_x: high_score_x(high_score),
        max_streak: max_streak_latch(latch, combo_taken[1]),
    }
}

// ------------------------------------------------- developer action dump

/// Actions per fighter the dump walks (`9`).
pub const DUMP_ACTIONS_PER_FIGHTER: usize = 9;
/// Fighter action tables the dump walks (`0x11`).
pub const DUMP_FIGHTERS: usize = 0x11;

/// PORT: FUN_801d553c - the developer action-table dump.
///
/// Retail allocates a `0x64000`-byte scratch buffer, walks all `0x11` fighter
/// action tables (`PTR_DAT_801DB8B8[i]`) and all `9` `0x60`-byte action
/// records inside each, appends one line per action and one per sub-keyframe,
/// and writes the whole thing out as `ot5stat.txt` (`ot5` = `other5`, the
/// battle pack the fighters come from) before freeing the buffer. Its return
/// value is the byte count written.
///
/// The port builds the same text and hands it back instead of touching a
/// filesystem. Two columns are narrower than retail's: retail prints all eight
/// leading words of the action record and the four halfwords of every
/// sub-keyframe, while `legaia_asset::baka_opponents::parse_actions` decodes
/// only the record's power (`+0x18`) and sub-keyframe count (`+0x1C`) - so the
/// per-keyframe TRS rows are absent until that parser widens. Retail's own
/// format strings are overlay rodata and are not reproduced; the labels here
/// are the port's.
pub fn action_table_dump(tables: &[BakaActionSet]) -> String {
    let mut out = String::new();
    for (i, set) in tables.iter().take(DUMP_FIGHTERS).enumerate() {
        out.push_str(&format!("ACT_TBL {i}\n"));
        for action in 0..DUMP_ACTIONS_PER_FIGHTER {
            let power = set.power.get(action).copied().unwrap_or(0);
            let keyframes = set.keyframes.get(action).copied().unwrap_or(0);
            out.push_str(&format!(
                "  act {action} power {power} keyframes {keyframes}\n"
            ));
        }
        out.push('\n');
    }
    out
}

// ------------------------------------------------------- the state machine

/// What the cabinet did this frame, for the host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CabinetFrame {
    /// Cabinet state after the tick.
    pub state: u32,
    /// The epilogue's arena pass ran (`s4` in the disassembly).
    pub draw_arena: bool,
    /// The epilogue called the HUD renderer (`s7`).
    pub draw_hud: bool,
    /// The HUD frame, when it drew one.
    pub hud: Option<HudFrame>,
    /// Cue-ring writes this frame, in fire order.
    pub cues: Vec<u8>,
    /// Roster id + mesh index the opponent-install state asked for.
    pub install_opponent: Option<(i32, i32)>,
    /// Coins banked into the casino coin bank by the exit state.
    pub payout: Option<u32>,
    /// Coins the game-over state forfeited out of the pot.
    pub forfeit: Option<u32>,
    /// The developer dump the editor state produced.
    pub action_dump: Option<String>,
}

/// Per-frame inputs the cabinet reads out of shared globals.
#[derive(Debug, Clone, Copy, Default)]
pub struct CabinetInput {
    /// `DAT_1F800393` - the frame-rate step every timer advances by.
    pub frame_step: i32,
    /// `_DAT_8007B874` - the newly-pressed packed pad word.
    pub pad_edge: u16,
    /// `_DAT_8007B938` - the second edge word the select screen folds in.
    pub pad_edge_alt: u16,
    /// `_DAT_8007B850` - the held packed pad word.
    pub pad_held: u16,
    /// `_DAT_8007B868` - non-zero routes the pause menu to the developer menu.
    pub dev_menu_enabled: bool,
    /// Player round wins this match (`DAT_801DBFF0`).
    pub player_round_wins: u32,
    /// Opponent round wins this match (`DAT_801DC098`).
    pub opponent_round_wins: u32,
    /// Round-win target (`DAT_801DBED0`, `2` in retail).
    pub win_target: u32,
    /// Prize gold of the rung just cleared (roster record `+0x20`).
    pub rung_prize: u32,
    /// Per-slot HP, for the HUD pass.
    pub hp: [i32; 2],
    /// Per-slot consecutive-hits-taken counters, for the HUD pass.
    pub combo_taken: [i32; 2],
}

/// PORT: FUN_801cf388 - the Baka Fighter cabinet's top-level state machine.
///
/// One tick per frame. The state word drives a 37-way fold; every branch
/// advances its own timer by the frame step and hands off to the next state on
/// a threshold, an input edge or a match result. The shared epilogue then
/// decides whether the arena and the HUD draw and decays the screen shake.
///
/// The port keeps the state ids, the transitions, the thresholds, the cue-ring
/// writes and the score / stage / pot bookkeeping. The per-state presentation
/// (actor spawns, widget quads, XA stings, GPU packet templates) stays a host
/// job, surfaced through [`CabinetFrame`]'s draw flags rather than reproduced
/// here.
#[derive(Debug, Clone)]
pub struct BakaCabinet {
    state: u32,
    /// `DAT_801DC128` - the general per-state frame timer.
    scene_timer: i32,
    /// `DAT_801DBF88` - the second per-state timer (round bracket, tally).
    state_timer: i32,
    /// `DAT_801DBE94` - the attract card's own clock.
    intro_timer: i32,
    /// `DAT_801DBE90` - the game-over hold.
    lose_timer: i32,
    /// `DAT_801DBE9C` - the exit fade.
    exit_timer: i32,
    /// `DAT_801DC12C` / `DAT_801DC130` - the attract blink phase and the
    /// cabinet's own frame counter.
    blink: i32,
    frames: i32,
    /// `DAT_801DBEB0` / `DAT_801DBEB4` - the all-clear sequence's two fades.
    clear_fade_in: i32,
    clear_fade_out: i32,
    /// `DAT_801DBF70` - the player-select cursor.
    select_cursor: i32,
    /// `DAT_801DBF90` - the menu cursor (tally choice, pause, developer menu).
    menu_cursor: i32,
    /// `DAT_801DC10C` - the ladder's stage counter (retail seeds it `2`).
    stage: i32,
    /// `DAT_801DC110` - the displayed rung number.
    stage_display: i32,
    /// `DAT_801DBF06` - the secret-opponent override.
    secret: SecretOpponent,
    /// `DAT_801DBEE4` - the running high score.
    high_score: i32,
    /// `DAT_801DBEE8` - the rung prize the tally pays.
    prize: i32,
    /// `DAT_801DBEE0` - the tally's match-bonus row.
    bonus: i32,
    /// `DAT_801DBEB8` - the all-clear flag.
    all_clear: bool,
    /// `DAT_801DBF8C` - the round index, clamped to `9`.
    round: i32,
    /// `DAT_801DBF78` - the match phase the fight rules gate on.
    match_phase: i32,
    /// `DAT_801DBED4` - suppresses the arena + HUD passes.
    suppress_scene: bool,
    /// `DAT_801DBEC0` - the screen-shake amplitude.
    shake: i32,
    /// `_DAT_80084440` - the casino prize accumulator the run banks into.
    pot: u32,
    /// `DAT_801DBF58` / `DAT_801DBEC8` - the HUD's max-streak latch.
    max_streak: i32,
    /// `DAT_801DBF28` - matches won that were *not* shutouts.
    rounds_dropped: i32,
    /// The parsed action tables, when a host installed them: the only thing
    /// the developer dump state has to dump.
    action_tables: Vec<BakaActionSet>,
}

impl Default for BakaCabinet {
    fn default() -> Self {
        Self::new()
    }
}

impl BakaCabinet {
    /// A freshly booted cabinet: state [`ST_BOOT`], stage counter seeded `2`
    /// exactly as `FUN_801CF00C` seeds it.
    pub fn new() -> Self {
        Self {
            state: ST_BOOT,
            scene_timer: 0,
            state_timer: 0,
            intro_timer: 0,
            lose_timer: 0,
            exit_timer: 0,
            blink: 0,
            frames: 0,
            clear_fade_in: 0,
            clear_fade_out: 0,
            select_cursor: 0,
            menu_cursor: 0,
            stage: 2,
            stage_display: 0,
            secret: SecretOpponent::None,
            high_score: 0,
            prize: 0,
            bonus: 0,
            all_clear: false,
            round: 0,
            match_phase: 0,
            suppress_scene: false,
            shake: 0,
            pot: 0,
            max_streak: 0,
            rounds_dropped: 0,
            action_tables: Vec::new(),
        }
    }

    /// Install the parsed action tables so the developer editor state has
    /// something to dump.
    pub fn with_action_tables(mut self, tables: Vec<BakaActionSet>) -> Self {
        self.action_tables = tables;
        self
    }

    /// Drop the cabinet straight into the duel, the way a host that owns the
    /// fight itself enters: state [`ST_DUEL`], match phase active.
    pub fn enter_duel(&mut self) {
        self.state = ST_DUEL;
        self.match_phase = 2;
        self.state_timer = 0;
    }

    pub fn state(&self) -> u32 {
        self.state
    }
    /// `DAT_801DBF78` - `0` teardown, `1` paused, `2` active.
    pub fn match_phase(&self) -> i32 {
        self.match_phase
    }
    pub fn stage(&self) -> i32 {
        self.stage
    }
    pub fn secret_opponent(&self) -> SecretOpponent {
        self.secret
    }
    pub fn all_clear(&self) -> bool {
        self.all_clear
    }
    pub fn high_score(&self) -> i32 {
        self.high_score
    }
    /// The casino prize accumulator (`_DAT_80084440`).
    pub fn pot(&self) -> u32 {
        self.pot
    }
    pub fn round(&self) -> i32 {
        self.round
    }
    pub fn shake(&self) -> i32 {
        self.shake
    }
    pub fn max_streak(&self) -> i32 {
        self.max_streak
    }
    /// `DAT_801DBF28` - matches won that were not shutouts.
    pub fn rounds_dropped(&self) -> i32 {
        self.rounds_dropped
    }

    /// Publish the running high score the secret-opponent gate reads.
    pub fn set_high_score(&mut self, score: i32) {
        self.high_score = score;
    }

    /// Kick the screen shake (`DAT_801DBEC0`); the epilogue decays it.
    pub fn shake_screen(&mut self, amplitude: i32) {
        self.shake = amplitude;
    }

    /// One frame of the cabinet.
    pub fn tick(&mut self, input: &CabinetInput) -> CabinetFrame {
        let step = input.frame_step;
        let edge = input.pad_edge;
        let mut f = CabinetFrame::default();

        // Prologue: two accumulators run regardless of state.
        self.blink += step;
        self.frames += 1;

        f.draw_arena = draws_arena(self.state);
        f.draw_hud = draws_hud(self.state);

        match self.state {
            ST_BOOT => {
                self.high_score = 0;
                self.scene_timer = 0;
                self.state = ST_ATTRACT;
            }
            ST_ATTRACT => {
                self.intro_timer += step;
                self.scene_timer += step;
                if edge & CABINET_START != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    self.scene_timer = 0;
                    self.state = ST_ATTRACT_OUT;
                }
            }
            ST_ATTRACT_OUT => {
                self.intro_timer += step;
                self.scene_timer += step;
                if self.scene_timer >= 0x3D {
                    self.state = ST_SELECT_SETUP;
                }
            }
            ST_SELECT_SETUP => {
                self.high_score = 0;
                self.match_phase = 0;
                self.round = 0;
                self.state = ST_SELECT;
            }
            ST_SELECT => {
                let mask = edge | input.pad_edge_alt;
                let (c, moved) = cursor_step(
                    self.select_cursor,
                    SELECT_OPTIONS,
                    mask,
                    CABINET_LEFT,
                    CABINET_RIGHT,
                );
                self.select_cursor = c;
                if moved {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
                }
                if edge & CABINET_CONFIRM != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    self.state = ST_SELECT_DONE;
                }
                // The tail clears both unconditionally, whether or not the
                // player confirmed this frame.
                self.max_streak = 0;
                self.secret = SecretOpponent::None;
            }
            ST_SELECT_DONE => {
                self.match_phase = 1;
                self.state_timer = 0;
                self.state = ST_SELECT_WIPE;
            }
            ST_SELECT_WIPE => {
                if self.state_timer >= 0x1F {
                    self.state_timer = 0;
                    self.state = ST_SELECT_POSE;
                }
                self.state_timer += step;
            }
            ST_SELECT_POSE => {
                self.state = ST_OPPONENT_INSTALL;
            }
            ST_OPPONENT_INSTALL => {
                let (roster, mesh) = rung_fold(self.stage, self.secret);
                f.install_opponent = Some((roster, mesh));
                self.match_phase = 2;
                self.state = ST_ROUND_SETUP;
            }
            ST_ROUND_SETUP => {
                self.state_timer = 0;
                self.state = ST_ROUND_BANNER_A;
            }
            ST_ROUND_BANNER_A => {
                self.scene_timer += step;
                self.state = ST_ROUND_BANNER_B;
            }
            ST_ROUND_BANNER_B => {
                self.scene_timer += step;
                self.state = ST_ROUND_BANNER_C;
            }
            ST_ROUND_BANNER_C => {
                self.scene_timer += step;
                self.state = ST_ROUND_BANNER_D;
            }
            ST_ROUND_BANNER_D => {
                self.scene_timer += step;
                if self.scene_timer >= 0x79 {
                    self.scene_timer = 0;
                    self.state = ST_ROUND_READY;
                }
            }
            ST_ROUND_READY => {
                self.state = ST_ROUND_GO;
            }
            ST_ROUND_GO => {
                self.scene_timer += step;
                if self.scene_timer >= 0x31 {
                    self.state = ST_DUEL;
                    self.state_timer = 0;
                }
            }
            ST_DUEL => self.tick_duel(input, &mut f),
            ST_PERFECT => {
                self.state_timer += step * 3;
                if self.state_timer >= 0xF1 {
                    self.state_timer = 0xF0;
                    self.state = ST_TALLY;
                }
            }
            ST_TALLY => {
                self.state_timer += step;
                if self.state_timer >= 0x79 {
                    self.state_timer = 0xF0;
                    self.state = ST_TALLY_OUT;
                    if self.secret != SecretOpponent::None {
                        self.state_timer = 0;
                        self.state = ST_CHOICE_SECRET;
                        self.suppress_scene = true;
                        self.stage_display += 1;
                    }
                }
            }
            ST_TALLY_OUT => {
                self.suppress_scene = false;
                self.state_timer -= step * 3;
                if self.state_timer < 0 {
                    self.state_timer = 0;
                    self.stage_display += 1;
                    self.state = ST_CHOICE;
                    if self.all_clear {
                        self.clear_fade_in = 0;
                        self.clear_fade_out = 0;
                        self.state = ST_CLEAR_A;
                    }
                }
            }
            ST_CHOICE => self.tick_choice(input, &mut f),
            ST_CHOICE_SECRET => {
                // Cleared at the top of the arm, every frame - the tally's
                // secret branch raises it and this state is where it comes
                // back down, one frame later.
                self.suppress_scene = false;
                self.state_timer += step;
                if self.state_timer >= 0xB5 {
                    self.match_phase = 1;
                    self.state = ST_NEXT_RUNG;
                }
            }
            ST_NEXT_RUNG => {
                self.match_phase = 1;
                self.state = ST_OPPONENT_INSTALL;
            }
            ST_LOSE => {
                self.state_timer += step * 2;
                if self.state_timer >= 0xF1 {
                    self.match_phase = 0;
                    self.suppress_scene = true;
                    self.state_timer = 0;
                    self.lose_timer = 0;
                    self.state = ST_GAME_OVER;
                }
            }
            ST_GAME_OVER => {
                if self.state_timer == 0 {
                    self.state_timer = 1;
                    // The pot is zeroed here, not on the way out: a mid-run
                    // defeat forfeits every coin the run had accumulated.
                    if self.pot != 0 {
                        f.forfeit = Some(self.pot);
                    }
                    self.pot = 0;
                }
                self.lose_timer += step;
                if self.lose_timer >= 0x79 {
                    self.match_phase = 0;
                    self.exit_timer = 0;
                    self.state = ST_EXIT;
                }
            }
            ST_PAUSE_2 => {
                let (c, moved) = cursor_step(self.menu_cursor, 2, edge, CABINET_UP, CABINET_DOWN);
                self.menu_cursor = c;
                if moved {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
                }
                if edge & CABINET_CANCEL != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CANCEL);
                    self.state = ST_DUEL;
                } else if edge & CABINET_CONFIRM != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    if self.menu_cursor == 0 {
                        self.state = ST_DUEL;
                    } else {
                        self.match_phase = 0;
                        self.state = ST_BOOT;
                    }
                }
            }
            ST_PAUSE_3 => {
                let (c, moved) =
                    cursor_step_mod(self.menu_cursor, 3, edge, CABINET_UP, CABINET_DOWN);
                self.menu_cursor = c;
                if moved {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
                }
                if edge & CABINET_CANCEL != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CANCEL);
                    self.state = ST_DUEL;
                } else if edge & CABINET_CONFIRM != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    match self.menu_cursor {
                        2 => self.state = ST_HOWTO,
                        1 => self.state = ST_EXIT,
                        _ => self.state = ST_DUEL,
                    }
                }
            }
            ST_HOWTO => {
                if edge & CABINET_ANY_FACE != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    self.state = ST_PAUSE_3;
                }
            }
            ST_DEV_MENU => self.tick_dev_menu(input, &mut f),
            ST_CLEAR_A => {
                self.clear_fade_in += step * 4;
                if self.clear_fade_in >= 0x101 {
                    self.clear_fade_in = 0x100;
                    self.state_timer = 0;
                    self.state = ST_CLEAR_B;
                }
            }
            ST_CLEAR_B => {
                self.state_timer += step * 4;
                if self.state_timer >= 0x100 {
                    self.state_timer = 0xFF;
                    self.state = ST_CLEAR_C;
                }
            }
            ST_CLEAR_C => {
                self.clear_fade_in = (self.clear_fade_in - step * 2).max(0);
                self.clear_fade_out += step * 2;
                if self.clear_fade_out >= 0x101 {
                    self.clear_fade_out = 0x100;
                    self.state = ST_CLEAR_D;
                }
            }
            ST_CLEAR_D => {
                self.state_timer -= step;
                if self.state_timer < 0x50 {
                    self.state_timer = 0x50;
                    self.state = ST_CLEAR_E;
                }
            }
            ST_CLEAR_E => {
                self.clear_fade_out = (self.clear_fade_out - step * 2).max(0);
                self.state_timer += step;
                if self.state_timer >= 0x100 {
                    self.state_timer = 0xFF;
                    self.match_phase = 0;
                    self.exit_timer = 0;
                    self.state = ST_EXIT;
                }
            }
            ST_DEV_EDITOR => {
                self.state = ST_DEV_EDITOR_RUN;
            }
            ST_DEV_EDITOR_RUN => {
                if edge & CABINET_DEV_DUMP != 0 {
                    f.action_dump = Some(action_table_dump(&self.action_tables));
                }
                if edge & CABINET_DEV_EXIT != 0 {
                    self.match_phase = 1;
                    self.state = ST_SELECT_POSE;
                }
            }
            ST_EXIT => {
                self.exit_timer += step;
                if self.exit_timer >= 0x3D && self.pot != 0 {
                    // `FUN_80026018` - the mode-24 return warp, which is what
                    // moves the accumulator into the casino coin bank.
                    f.payout = Some(self.pot);
                    self.pot = 0;
                }
            }
            _ => {}
        }

        // Epilogue. Retail re-reads the state word here, *after* the body, and
        // the exit state short-circuits the whole epilogue - so the frame a
        // state transitions into `0x1F4` draws nothing either, whatever draw
        // flags its body raised.
        if self.state == ST_EXIT {
            f.draw_arena = false;
            f.draw_hud = false;
            f.state = self.state;
            return f;
        }
        self.shake = shake_decay(self.shake, step);
        if f.draw_hud && !self.suppress_scene {
            let hud = hud_frame(
                input.hp,
                [input.player_round_wins, input.opponent_round_wins],
                input.win_target,
                input.combo_taken,
                self.stage_display + 1,
                self.high_score,
                self.max_streak,
            );
            self.max_streak = hud.max_streak;
            f.hud = Some(hud);
        }
        if self.suppress_scene {
            f.draw_arena = false;
            f.draw_hud = false;
        }
        f.state = self.state;
        f
    }

    /// The duel state's own body: the round bracket plus the pause-menu edge.
    ///
    /// One port-side departure, and it is the timer. Retail's duel state only
    /// *reads* the round timer `DAT_801DBF88` against `0xB5` - it never
    /// advances it, because the fight resolution SM (`FUN_801D3468`) and the
    /// actor tick own that counter. The port's cabinet has no such sibling
    /// driving it, so it advances the timer itself; the threshold, the
    /// bookkeeping it gates and the exits are retail's.
    fn tick_duel(&mut self, input: &CabinetInput, f: &mut CabinetFrame) {
        let step = input.frame_step;
        // The menu cursor is re-zeroed every duel frame, not on entry to the
        // menu - so a re-opened pause menu always starts on its first row.
        self.menu_cursor = 0;
        if self.state_timer < 0xB5 {
            self.state_timer += step;
        } else {
            // Round over.
            self.state_timer = 0;
            self.round = (self.round + 1).min(9);
            self.state = ST_ROUND_SETUP;

            if input.player_round_wins == input.win_target {
                self.prize = input.rung_prize as i32;
                self.pot = self.pot.saturating_add(input.rung_prize);
                if self.secret == SecretOpponent::None {
                    self.secret = secret_opponent_gate(self.stage, self.high_score);
                }
                let adv = advance_stage(self.stage, self.secret);
                self.stage = adv.stage;
                if adv.all_clear {
                    self.all_clear = true;
                }
                self.bonus = match_bonus(input.opponent_round_wins);
                // `state = 0x65` sits in the **delay slot** of the
                // "did the opponent take a round" branch, so it runs either
                // way - a match win always reaches the flourish. The branch
                // only skips the dropped-round tally `DAT_801DBF28`.
                self.state = ST_PERFECT;
                if input.opponent_round_wins != 0 {
                    self.rounds_dropped += 1;
                }
            } else if input.opponent_round_wins == input.win_target {
                self.state = ST_LOSE;
                self.state_timer = 0;
            }
        }

        if input.pad_edge & CABINET_DUEL_MENU != 0 {
            f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
            self.menu_cursor = 0;
            self.state = if input.dev_menu_enabled {
                ST_DEV_MENU
            } else {
                self.state_timer = 0;
                ST_PAUSE_3
            };
        }
    }

    /// The "NEXT GAME / PAY OUT" menu.
    fn tick_choice(&mut self, input: &CabinetInput, f: &mut CabinetFrame) {
        let step = input.frame_step;
        let edge = input.pad_edge;
        if self.state_timer == 0 {
            let (c, moved) = cursor_step(self.menu_cursor, 2, edge, CABINET_LEFT, CABINET_RIGHT);
            self.menu_cursor = c;
            if moved {
                f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
            }
            if edge & CABINET_CONFIRM != 0 {
                f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                if self.menu_cursor == 0 {
                    // NEXT GAME: risk the pot on the next rung.
                    self.state_timer = 1;
                } else {
                    // PAY OUT: bank it and leave.
                    self.match_phase = 0;
                    self.exit_timer = 0;
                    self.state = ST_EXIT;
                }
            }
        } else {
            self.state_timer += step;
            if self.state_timer >= 0xD3 {
                self.match_phase = 1;
                self.state = ST_NEXT_RUNG;
            }
        }
    }

    /// The developer menu: five rows, with left/right editing the debug-mode
    /// and stage fields on rows `2` and `3`.
    fn tick_dev_menu(&mut self, input: &CabinetInput, f: &mut CabinetFrame) {
        let edge = input.pad_edge;
        let (c, moved) = cursor_step_mod(
            self.menu_cursor,
            DEV_MENU_ROWS,
            edge,
            CABINET_UP,
            CABINET_DOWN,
        );
        self.menu_cursor = c;
        if moved {
            f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
        }
        if self.menu_cursor == 3 {
            if edge & CABINET_LEFT != 0 {
                self.stage -= 1;
            }
            if edge & CABINET_RIGHT != 0 {
                self.stage += 1;
            }
        }
        if edge & CABINET_CONFIRM != 0 {
            f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
            match self.menu_cursor {
                0 => self.state = ST_DUEL,
                1 => {
                    self.match_phase = 0;
                    self.state = ST_BOOT;
                }
                4 => {
                    self.match_phase = 0;
                    self.state = ST_DEV_EDITOR;
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dispatcher_case_is_a_distinct_ascending_state() {
        for pair in CABINET_STATES.windows(2) {
            assert!(pair[0] < pair[1], "{pair:?} out of order");
        }
        assert!(is_cabinet_state(ST_DUEL));
        assert!(is_cabinet_state(ST_EXIT));
        // The editor band `FUN_801D4FC8` gates on is exactly two states.
        assert!(is_cabinet_state(0x190) && is_cabinet_state(0x191));
        assert!(!is_cabinet_state(0x192));
        assert!(!is_cabinet_state(3));
    }

    #[test]
    fn secret_gate_is_stage_and_score_keyed_and_strict() {
        assert_eq!(secret_opponent_gate(5, 250_001), SecretOpponent::First);
        // Strict: exactly on the threshold does not qualify.
        assert_eq!(secret_opponent_gate(5, 250_000), SecretOpponent::None);
        assert_eq!(secret_opponent_gate(4, 999_999), SecretOpponent::None);
        assert_eq!(secret_opponent_gate(13, 700_001), SecretOpponent::Second);
        assert_eq!(secret_opponent_gate(13, 700_000), SecretOpponent::None);
    }

    #[test]
    fn a_pending_secret_rung_suppresses_the_stage_advance() {
        assert_eq!(
            advance_stage(5, SecretOpponent::None),
            StageAdvance {
                stage: 6,
                all_clear: false
            }
        );
        assert_eq!(
            advance_stage(5, SecretOpponent::First),
            StageAdvance {
                stage: 5,
                all_clear: false
            }
        );
        // Reaching the wrap raises the all-clear and restarts at 0, so the
        // second lap serves roster ids 3 and 4 from stages 0 and 1.
        assert_eq!(
            advance_stage(13, SecretOpponent::None),
            StageAdvance {
                stage: 0,
                all_clear: true
            }
        );
        assert_eq!(rung_fold(0, SecretOpponent::None), (3, 5));
        assert_eq!(rung_fold(2, SecretOpponent::None), (5, 7));
        assert_eq!(rung_fold(9, SecretOpponent::First), (3, 5));
        assert_eq!(rung_fold(9, SecretOpponent::Second), (4, 6));
    }

    #[test]
    fn the_bonus_row_pays_more_for_a_shutout() {
        assert_eq!(match_bonus(0), PERFECT_BONUS);
        assert_eq!(match_bonus(1), MATCH_BONUS);
        const { assert!(PERFECT_BONUS > MATCH_BONUS) };
    }

    #[test]
    fn cursor_wraps_in_retails_clamp_order() {
        assert_eq!(
            cursor_step(0, 3, CABINET_LEFT, CABINET_LEFT, CABINET_RIGHT).0,
            2
        );
        assert_eq!(
            cursor_step(2, 3, CABINET_RIGHT, CABINET_LEFT, CABINET_RIGHT).0,
            0
        );
        assert_eq!(
            cursor_step(1, 3, 0, CABINET_LEFT, CABINET_RIGHT),
            (1, false)
        );
        // Both edges in one frame cancel out and still count as a move.
        assert_eq!(
            cursor_step(
                1,
                3,
                CABINET_LEFT | CABINET_RIGHT,
                CABINET_LEFT,
                CABINET_RIGHT
            ),
            (1, true)
        );
        // A span of 2 is what retail's `& 1` fold does.
        assert_eq!(
            cursor_step(0, 2, CABINET_LEFT, CABINET_LEFT, CABINET_RIGHT).0,
            1
        );
        assert_eq!(
            cursor_step(1, 2, CABINET_RIGHT, CABINET_LEFT, CABINET_RIGHT).0,
            0
        );
    }

    #[test]
    fn the_modulo_cursors_are_sticky_at_the_top() {
        // Down wraps at the bottom, the same as the clamped fold.
        assert_eq!(
            cursor_step_mod(2, 3, CABINET_DOWN, CABINET_UP, CABINET_DOWN).0,
            0
        );
        assert_eq!(
            cursor_step_mod(4, 5, CABINET_DOWN, CABINET_UP, CABINET_DOWN).0,
            0
        );
        // Up off row 0 does *not* wrap: `-1` as a u32 is divisible by both 3
        // and 5, so it folds back to 0.
        assert_eq!(
            cursor_step_mod(0, 3, CABINET_UP, CABINET_UP, CABINET_DOWN).0,
            0
        );
        assert_eq!(
            cursor_step_mod(0, 5, CABINET_UP, CABINET_UP, CABINET_DOWN).0,
            0
        );
        // Which is exactly where it differs from the player-select fold.
        assert_eq!(cursor_step(0, 3, CABINET_UP, CABINET_UP, CABINET_DOWN).0, 2);
        // Ordinary steps behave.
        assert_eq!(
            cursor_step_mod(1, 5, CABINET_UP, CABINET_UP, CABINET_DOWN).0,
            0
        );
        assert_eq!(
            cursor_step_mod(1, 5, CABINET_DOWN, CABINET_UP, CABINET_DOWN).0,
            2
        );
    }

    #[test]
    fn shake_always_terminates() {
        let mut a = 0x400;
        for _ in 0..200 {
            a = shake_decay(a, 1);
        }
        assert_eq!(a, 0);
        let mut b = -0x400;
        for _ in 0..200 {
            b = shake_decay(b, 1);
        }
        assert_eq!(b, 0);
        // A single step never crosses zero.
        assert_eq!(shake_decay(1, 1), 0);
        assert_eq!(shake_decay(-1, 1), 0);
    }

    #[test]
    fn vital_bar_is_one_pixel_per_32_hp() {
        let full = vital_bar(0, FULL_HP);
        assert_eq!(full.x1 - full.x0, 0x64);
        assert_eq!(full.x1, VITAL_ANCHOR_LEFT);
        assert_eq!(full.rgb_far, [VITAL_RED, 0x64, 0]);
        let empty = vital_bar(0, 0);
        assert_eq!(empty.x0, empty.x1);
        // The opponent fills the other way from its own anchor.
        let opp = vital_bar(1, FULL_HP);
        assert_eq!(opp.x0, VITAL_ANCHOR_RIGHT);
        assert_eq!(opp.x1, VITAL_ANCHOR_RIGHT + 0x64);
    }

    #[test]
    fn pip_rows_grow_toward_each_other() {
        let p = round_win_pips(0, 1, 2);
        assert_eq!(
            p,
            vec![(0x70, PIP_Y, PIP_U_FILLED), (0x80, PIP_Y, PIP_U_EMPTY)]
        );
        let o = round_win_pips(1, 0, 2);
        assert_eq!(
            o,
            vec![(0xC0, PIP_Y, PIP_U_EMPTY), (0xB0, PIP_Y, PIP_U_EMPTY)]
        );
    }

    #[test]
    fn combo_level_dips_then_snaps_back() {
        assert_eq!(combo_counter_level(0), 0xF0);
        assert_eq!(combo_counter_level(3), 0xF0);
        assert_eq!(combo_counter_level(4), 0xE0);
        assert_eq!(combo_counter_level(5), 0xD0);
        assert_eq!(combo_counter_level(6), 0xC0);
        // From 7 the `10 - combo < 4` test fires and it is full again.
        assert_eq!(combo_counter_level(7), 0xF0);
        assert_eq!(combo_counter_level(50), 0xF0);
    }

    #[test]
    fn stage_digits_split_at_ten() {
        assert_eq!(stage_digits(1), vec![(0x48, 0x1E, 1)]);
        assert_eq!(stage_digits(9), vec![(0x48, 0x1E, 9)]);
        assert_eq!(stage_digits(14), vec![(0x40, 0x1E, 1), (0x48, 0x1E, 4)]);
    }

    #[test]
    fn high_score_place_count_carries_retails_off_by_one() {
        assert_eq!(high_score_x(0), None);
        // 9 measures as two places because the test is against `10^k - 1`.
        assert_eq!(high_score_places(9), 2);
        assert_eq!(high_score_places(10), 2);
        assert_eq!(high_score_places(99), 3);
        assert_eq!(high_score_places(1), 1);
        assert_eq!(high_score_x(1), Some(0x28 - 8));
    }

    #[test]
    fn max_streak_latch_never_falls() {
        let mut l = 0;
        for s in [3, 7, 2, 5, 1] {
            l = max_streak_latch(l, s);
        }
        assert_eq!(l, 7);
    }

    fn run(cab: &mut BakaCabinet, frames: usize, input: &CabinetInput) -> Vec<u32> {
        (0..frames).map(|_| cab.tick(input).state).collect()
    }

    #[test]
    fn boot_walks_to_the_player_select_screen() {
        let mut cab = BakaCabinet::new();
        let idle = CabinetInput {
            frame_step: 1,
            win_target: 2,
            ..Default::default()
        };
        assert_eq!(cab.tick(&idle).state, ST_ATTRACT);
        // The attract card waits for the start edge indefinitely.
        run(&mut cab, 500, &idle);
        assert_eq!(cab.state(), ST_ATTRACT);
        let start = CabinetInput {
            pad_edge: CABINET_START,
            ..idle
        };
        let f = cab.tick(&start);
        assert_eq!(f.state, ST_ATTRACT_OUT);
        assert_eq!(f.cues, vec![crate::baka_fighter::BAKA_CUE_CONFIRM]);
        run(&mut cab, 0x40, &idle);
        assert_eq!(cab.state(), ST_SELECT);
    }

    #[test]
    fn a_won_match_banks_the_prize_and_a_lost_one_forfeits_the_pot() {
        let mut cab = BakaCabinet::new();
        cab.enter_duel();
        let won = CabinetInput {
            frame_step: 1,
            player_round_wins: 2,
            win_target: 2,
            rung_prize: 40,
            ..Default::default()
        };
        // The round bracket has to elapse before the win is read.
        for _ in 0..0xB6 {
            cab.tick(&won);
        }
        assert_eq!(cab.pot(), 40);
        assert_eq!(cab.stage(), 3);
        assert_eq!(cab.state(), ST_PERFECT);
        assert_eq!(cab.rounds_dropped(), 0, "a shutout drops no rounds");

        // A match won 2-1 still reaches the flourish - the `state = 0x65`
        // store is in the delay slot of the shutout branch, so only the
        // dropped-round tally is gated.
        let mut messy = BakaCabinet::new();
        messy.enter_duel();
        let scrappy = CabinetInput {
            opponent_round_wins: 1,
            ..won
        };
        for _ in 0..0xB6 {
            messy.tick(&scrappy);
        }
        assert_eq!(messy.state(), ST_PERFECT);
        assert_eq!(messy.rounds_dropped(), 1);

        // Now lose one with a pot on the table.
        cab.enter_duel();
        let lost = CabinetInput {
            frame_step: 1,
            opponent_round_wins: 2,
            win_target: 2,
            ..Default::default()
        };
        for _ in 0..0xB6 {
            cab.tick(&lost);
        }
        assert_eq!(cab.state(), ST_LOSE);
        let mut forfeit = None;
        for _ in 0..0x200 {
            let f = cab.tick(&lost);
            if f.forfeit.is_some() {
                forfeit = f.forfeit;
            }
        }
        assert_eq!(forfeit, Some(40));
        assert_eq!(cab.pot(), 0);
        assert_eq!(cab.state(), ST_EXIT);
    }

    #[test]
    fn pay_out_banks_the_pot_and_next_game_keeps_it_at_risk() {
        let mut cab = BakaCabinet::new();
        cab.state = ST_CHOICE;
        cab.pot = 120;
        let idle = CabinetInput {
            frame_step: 1,
            win_target: 2,
            ..Default::default()
        };
        // Cursor right onto PAY OUT, then confirm.
        cab.tick(&CabinetInput {
            pad_edge: CABINET_RIGHT,
            ..idle
        });
        assert_eq!(cab.menu_cursor, 1);
        cab.tick(&CabinetInput {
            pad_edge: CABINET_CONFIRM,
            ..idle
        });
        assert_eq!(cab.state(), ST_EXIT);
        let mut payout = None;
        for _ in 0..0x80 {
            if let Some(p) = cab.tick(&idle).payout {
                payout = Some(p);
            }
        }
        assert_eq!(payout, Some(120));

        // NEXT GAME instead: the pot survives into the following rung.
        let mut cab = BakaCabinet::new();
        cab.state = ST_CHOICE;
        cab.pot = 120;
        cab.tick(&CabinetInput {
            pad_edge: CABINET_CONFIRM,
            ..idle
        });
        run(&mut cab, 0xD4, &idle);
        assert_eq!(cab.pot(), 120);
        assert_eq!(cab.state(), ST_ROUND_SETUP);
    }

    #[test]
    fn the_secret_rung_is_served_instead_of_advancing_the_stage() {
        let mut cab = BakaCabinet::new();
        cab.stage = SECRET_A_STAGE;
        cab.set_high_score(SECRET_A_SCORE + 1);
        cab.enter_duel();
        let won = CabinetInput {
            frame_step: 1,
            player_round_wins: 2,
            opponent_round_wins: 1,
            win_target: 2,
            rung_prize: 10,
            ..Default::default()
        };
        for _ in 0..0xB6 {
            cab.tick(&won);
        }
        assert_eq!(cab.secret_opponent(), SecretOpponent::First);
        assert_eq!(cab.stage(), SECRET_A_STAGE, "stage must not advance");
        assert_eq!(rung_fold(cab.stage(), cab.secret_opponent()), (3, 5));
    }

    #[test]
    fn the_duel_menu_edge_routes_to_the_dev_menu_only_when_enabled() {
        let mut cab = BakaCabinet::new();
        cab.enter_duel();
        let menu = CabinetInput {
            frame_step: 1,
            pad_edge: CABINET_DUEL_MENU,
            win_target: 2,
            ..Default::default()
        };
        cab.tick(&menu);
        assert_eq!(cab.state(), ST_PAUSE_3);

        let mut cab = BakaCabinet::new();
        cab.enter_duel();
        cab.tick(&CabinetInput {
            dev_menu_enabled: true,
            ..menu
        });
        assert_eq!(cab.state(), ST_DEV_MENU);
    }

    #[test]
    fn the_editor_state_dumps_the_installed_action_tables() {
        let tables = vec![BakaActionSet {
            index: 0,
            power: [1, 2, 3, 4, 5, 6, 7, 8, 9],
            keyframes: [0, 0, 1, 2, 3, 4, 0, 0, 0],
        }];
        let mut cab = BakaCabinet::new().with_action_tables(tables);
        cab.state = ST_DEV_EDITOR_RUN;
        let idle = CabinetInput {
            frame_step: 1,
            ..Default::default()
        };
        assert!(cab.tick(&idle).action_dump.is_none());
        let dump = cab
            .tick(&CabinetInput {
                pad_edge: CABINET_DEV_DUMP,
                ..idle
            })
            .action_dump
            .expect("L1 dumps the table");
        assert!(dump.contains("ACT_TBL 0"));
        assert_eq!(dump.lines().filter(|l| l.contains("act ")).count(), 9);
        // Select leaves the editor.
        cab.tick(&CabinetInput {
            pad_edge: CABINET_DEV_EXIT,
            ..idle
        });
        assert_eq!(cab.state(), ST_SELECT_POSE);
    }

    #[test]
    fn the_hud_pass_runs_only_in_the_duel_band() {
        let mut cab = BakaCabinet::new();
        cab.enter_duel();
        let input = CabinetInput {
            frame_step: 1,
            hp: [FULL_HP, FULL_HP / 2],
            combo_taken: [1, 4],
            win_target: 2,
            ..Default::default()
        };
        let f = cab.tick(&input);
        assert!(f.draw_hud && f.draw_arena);
        let hud = f.hud.expect("the duel draws the HUD");
        assert_eq!(hud.bars[0].x1 - hud.bars[0].x0, 0x64);
        // Crossed sides: index 0 shows slot 1's streak.
        assert_eq!(hud.combo_level[0], combo_counter_level(4));
        assert_eq!(hud.max_streak, 4);

        // The attract card draws neither.
        let mut cab = BakaCabinet::new();
        let f = cab.tick(&input);
        assert!(f.hud.is_none());
        assert!(!f.draw_hud && !f.draw_arena);
    }

    #[test]
    fn the_epilogue_draw_gates_are_narrower_than_the_state_list() {
        // The arena pass skips the attract pair, the tally, three all-clear
        // beats and the exit; the HUD pass is the round / duel / result band
        // only, and it excludes the tally screen.
        for s in [ST_BOOT, ST_ATTRACT, ST_ATTRACT_OUT, ST_TALLY, ST_EXIT] {
            assert!(!draws_arena(s), "{s:#x} should not draw the arena");
        }
        for s in [ST_CLEAR_B, ST_CLEAR_D, ST_CLEAR_E] {
            assert!(!draws_arena(s), "{s:#x} should not draw the arena");
        }
        for s in [ST_DUEL, ST_CHOICE, ST_DEV_MENU, ST_CLEAR_A, ST_CLEAR_C] {
            assert!(draws_arena(s), "{s:#x} should draw the arena");
        }
        assert!(!draws_arena(0x1234), "an unlisted state draws nothing");
        assert!(draws_hud(ST_DUEL) && draws_hud(ST_GAME_OVER));
        assert!(!draws_hud(ST_TALLY) && !draws_hud(ST_CHOICE));
        // Every HUD state also draws the arena.
        for s in CABINET_STATES {
            if draws_hud(s) {
                assert!(draws_arena(s), "{s:#x} draws a HUD over no arena");
            }
        }
    }
}
