//! The cabinet state ids, the dispatcher's state list and the per-state draw predicates.

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

/// The attract + player-select band: every state before the first opponent
/// install (`0x00`..`0x0E`).
pub fn front_end(state: u32) -> bool {
    state < ST_OPPONENT_INSTALL
}

/// Whether the three player-select fighters are on stage. They spawn in
/// `0x0A` from the template at `0x801D760C`, whose tick `FUN_801D3390`
/// raises the retire bit once the match phase `DAT_801DBF78` is non-zero -
/// which the confirm state `0x0C` sets.
pub fn lineup_live(state: u32) -> bool {
    matches!(state, ST_SELECT_SETUP | ST_SELECT)
}
