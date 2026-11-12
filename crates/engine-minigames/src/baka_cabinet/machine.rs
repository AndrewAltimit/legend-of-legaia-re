//! The [`BakaCabinet`] state, its frame and input records, construction and accessors.

use super::*;
use legaia_minigame_formats::baka_opponents::BakaActionSet;

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
    /// The front end's own widget draws this frame: the attract card's
    /// "PRESS START" prompt and the "PLAYER SELECT" banner. Same shape as the
    /// tally sheet's cells, so a host draws both through one path.
    pub widgets: Vec<SheetCell>,
    /// The clock the attract arms called the title card `FUN_801D59D4` with
    /// this frame (`DAT_801DBE94` before its increment), or `None` when no
    /// arm called it.
    pub title_card: Option<i32>,
    /// The clock the round banner arms `0x33..0x36` called the banner
    /// `FUN_801D5C7C` with this frame (`DAT_801DC128` before its step), or
    /// `None` outside them.
    pub round_banner: Option<i32>,
    /// Roster id the player-select pose state seated (`DAT_801DBF70`).
    pub install_player: Option<i32>,
    /// The announcer line a cabinet arm started this frame
    /// (`FUN_8003D53C`): the FIGHT flash, NEXT GAME, GAME OVER and the
    /// all-clear announcer's lines. The chrome's own lines (intro, round
    /// banner, results) are the chrome's.
    pub xa: Option<crate::baka_fighter_chrome::XaCue>,
    /// The `0xFB -> 0xFC` edge spawned the all-clear fighter
    /// ([`crate::baka_fighter_chrome::clear_fighter_tick`]).
    pub spawn_clear_fighter: bool,
    /// The level of the full-screen subtractive quad the all-clear beats
    /// `0xFB..=0xFE` end on (`FUN_80024EE4(0xA, 2, level * 0x010101)`):
    /// `0xFF` is black, `0x50` the lightest the fighter is seen through.
    ///
    /// REF: FUN_80024EE4
    pub clear_fade: Option<u8>,
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
    /// Prize gold of the rung just cleared (roster record `+0x20`). The
    /// match-won arm copies it into `DAT_801DBEE8` for the tally to drain
    /// (`lw v1,0x20(v0)` / `sw v1,-0x4118(v0)` at `0x801D06FC`); it does
    /// not touch the pot.
    pub rung_prize: u32,
    /// Per-slot HP, for the HUD pass.
    pub hp: [i32; 2],
    /// Per-slot consecutive-hits-taken counters, for the HUD pass.
    pub combo_taken: [i32; 2],
    /// Per-slot chosen attack type (block `+0x24`), for the HUD's icons.
    pub chosen_type: [i32; 2],
    /// Per-slot held-type timer (block `+0x8C`), for the lit icon's flash.
    pub type_hold: [i32; 2],
    /// The round's result clock runs: the fight resolution SM's condition
    /// for advancing the round timer `DAT_801DBF88` this frame
    /// (`FUN_801D3468`, `0x801D3614..0x801D3668`) - either fighter's
    /// finisher flag `+0x2C` (`0x801DBFE8` / `0x801DC090`, raised when a
    /// special lands its full chain, `0x801D3C2C`) or both HPs at zero.
    pub round_clock: bool,
    /// The score tally `FUN_801D239C` still has a row to drain (its
    /// non-zero answer). The tally state holds its two-second count until
    /// it is clear.
    pub tally_draining: bool,
    /// The duel's pause gate `DAT_801DBEC4`: the player's combat tick raises
    /// it while the player stands on the idle clip (`actor+0x5C` one past
    /// `block+0x14`, seat `0` only, `0x801D47FC..0x801D4824`) and either tick drops it
    /// while a knockdown latch `+0x2C` is up (`0x801D4828..0x801D4848`). The
    /// duel state opens its menu only with it up (`0x801D08B0`), so the pause
    /// edge does nothing mid-exchange.
    pub pause_ready: bool,
    /// The round-start camera beat this frame ran left the camera short of
    /// its mark, so the arm holds: state `0x35`'s spin with the yaw not yet
    /// past `0x1000` (`slti a0,a0,0x1001` at `0x801D03A0`), or state
    /// `0x34`'s rise on a secret opponent's first round with eye Y not yet
    /// past `0xBB8` (`0x801D02F4`). The camera is the duel's
    /// ([`crate::baka_duel::DuelCamera::spin`]); a cabinet driven with no
    /// camera leaves this clear and steps straight through.
    pub camera_hold: bool,
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
    pub(super) state: u32,
    /// `DAT_801DC128` - the general per-state frame timer.
    pub(super) scene_timer: i32,
    /// `DAT_801DBF88` - the second per-state timer (round bracket, tally).
    pub(super) state_timer: i32,
    /// `DAT_801DBE94` - the attract card's own clock.
    pub(super) intro_timer: i32,
    /// `DAT_801DBE90` - the game-over hold.
    pub(super) lose_timer: i32,
    /// `DAT_801DBE9C` - the exit fade.
    pub(super) exit_timer: i32,
    /// `DAT_801DC12C` / `DAT_801DC130` - the attract blink phase and the
    /// cabinet's own frame counter.
    pub(super) blink: i32,
    pub(super) frames: i32,
    /// `DAT_801DBEB0` / `DAT_801DBEB4` - the all-clear sequence's two fades.
    pub(super) clear_fade_in: i32,
    pub(super) clear_fade_out: i32,
    /// `DAT_801DC134` / `DAT_801DC138` - the all-clear announcer walk
    /// (`FUN_801D21FC`), called from the five all-clear beats only. Nothing
    /// else in the overlay writes it, so it runs once per overlay load.
    pub(super) clear_announcer: crate::baka_fighter_chrome::Countdown,
    /// `DAT_801DBF70` - the player-select cursor.
    pub(super) select_cursor: i32,
    /// `DAT_801DBF90` - the menu cursor (tally choice, pause, developer menu).
    pub(super) menu_cursor: i32,
    /// `DAT_801DC10C` - the ladder's stage counter (retail seeds it `2`).
    pub(super) stage: i32,
    /// `DAT_801DC110` - the displayed rung number.
    pub(super) stage_display: i32,
    /// `DAT_801DBF06` - the secret-opponent override.
    pub(super) secret: SecretOpponent,
    /// `DAT_801DBEE4` - the running high score.
    pub(super) high_score: i32,
    /// `DAT_801DBEE8` - the rung prize the tally pays.
    pub(super) prize: i32,
    /// `DAT_801DBEE0` - the tally's match-bonus row.
    pub(super) bonus: i32,
    /// `DAT_801DBEB8` - the all-clear flag.
    pub(super) all_clear: i32,
    /// `DAT_801DBF8C` - the round index, clamped to `9`.
    pub(super) round: i32,
    /// `DAT_801DBF78` - the match phase the fight rules gate on.
    pub(super) match_phase: i32,
    /// `DAT_801DBED4` - suppresses the arena + HUD passes.
    pub(super) suppress_scene: bool,
    /// `DAT_801DBEC0` - the screen-shake amplitude.
    pub(super) shake: i32,
    /// `_DAT_80084440` - the casino prize accumulator the run banks into.
    pub(super) pot: u32,
    /// `DAT_801DBF58` / `DAT_801DBEC8` - the HUD's max-streak latch.
    pub(super) max_streak: i32,
    /// `DAT_801DBF28` - matches won without dropping a round (shutouts);
    /// zeroed at the player select.
    pub(super) perfect_matches: i32,
    /// The parsed action tables, when a host installed them: the only thing
    /// the developer dump state has to dump.
    pub(super) action_tables: Vec<BakaActionSet>,
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
            clear_announcer: Default::default(),
            select_cursor: 0,
            menu_cursor: 0,
            stage: 2,
            stage_display: 0,
            secret: SecretOpponent::None,
            high_score: 0,
            prize: 0,
            bonus: 0,
            all_clear: 0,
            round: 0,
            match_phase: 0,
            suppress_scene: false,
            shake: 0,
            pot: 0,
            max_streak: 0,
            perfect_matches: 0,
            action_tables: Vec::new(),
        }
    }

    /// Install the parsed action tables so the developer editor state has
    /// something to dump.
    pub fn with_action_tables(mut self, tables: Vec<BakaActionSet>) -> Self {
        self.action_tables = tables;
        self
    }

    /// Back to the boot state ([`Self::new`]), keeping the installed action
    /// tables.
    pub fn reboot(&mut self) {
        let tables = std::mem::take(&mut self.action_tables);
        *self = Self::new().with_action_tables(tables);
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

    /// `DAT_801DBE94` - the attract card's clock, which is the `t` the intro
    /// title card ([`crate::baka_fighter_chrome::IntroTitle`]) is a function
    /// of. Only the attract arms advance it.
    pub fn intro_clock(&self) -> i32 {
        self.intro_timer
    }

    /// Is the cabinet on one of the arms that animates the title card?
    ///
    /// The two `jal 0x801D59D4` sites in PROT 0976 are both inside this state
    /// machine, and both attract arms advance [`Self::intro_clock`].
    pub fn in_attract(&self) -> bool {
        matches!(self.state, ST_BOOT | ST_ATTRACT | ST_ATTRACT_OUT)
    }
    /// The cabinet's **front end**: the attract card and the player select,
    /// every state before the first opponent install. No fight runs here -
    /// retail spawns the round SM only at `0x0E`.
    pub fn front_end(&self) -> bool {
        front_end(self.state)
    }

    /// `DAT_801DBF70` - the player-select cursor (the roster id the pick
    /// seats).
    pub fn select_cursor(&self) -> i32 {
        self.select_cursor
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
    /// Stand the cabinet on the all-clear's first beat at displayed stage
    /// `stage_display`, the arm the ladder's last tally stores: a seam for what
    /// no fixture plays twelve rungs to reach.
    #[doc(hidden)]
    pub fn enter_all_clear(&mut self, stage_display: i32) {
        self.stage_display = stage_display;
        self.clear_fade_in = 0;
        self.clear_fade_out = 0;
        self.state_timer = 0;
        self.state = ST_CLEAR_A;
    }

    pub fn all_clear(&self) -> bool {
        self.all_clear != 0
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
    /// `DAT_801DBF28` - matches won without dropping a round.
    pub fn perfect_matches(&self) -> i32 {
        self.perfect_matches
    }

    /// Mirror the live casino prize accumulator (`_DAT_80084440`) into the
    /// cabinet before a tick. The word is global and the tally drain owns
    /// its increments, so a host that drains the tally elsewhere hands the
    /// value across here; the game-over zero and the exit's payout then act
    /// on the real amount.
    pub fn set_pot(&mut self, pot: u32) {
        self.pot = pot;
    }

    /// `DAT_801DBF90` - the menu cursor (tally choice, pause, developer).
    pub fn menu_cursor(&self) -> i32 {
        self.menu_cursor
    }

    /// `DAT_801DC12C` - the blink phase the choice screen's lit arrow reads.
    pub fn blink(&self) -> i32 {
        self.blink
    }

    /// The "NEXT GAME / PAY OUT" sheet this frame, when the cabinet is on
    /// it. Retail draws the sheet only while the confirm latch
    /// `DAT_801DBF88` is still zero (`bne v1,zero,0x801D0DD8` at
    /// `0x801D0BB4`), so it disappears the frame NEXT GAME is taken.
    pub fn choice_sheet(&self) -> Option<[SheetCell; 5]> {
        (self.state == ST_CHOICE && self.state_timer == 0)
            .then(|| choice_sheet(self.menu_cursor, self.blink))
    }

    /// `true` once the exit state's fade has run (`DAT_801DBE9C >= 0x3D`),
    /// the frame retail's return warp `FUN_80026018` leaves the mode.
    pub fn exit_done(&self) -> bool {
        self.state == ST_EXIT && self.exit_timer >= 0x3D
    }

    /// Publish the running score (`DAT_801DBEE4`) the secret-opponent gate reads.
    pub fn set_high_score(&mut self, score: i32) {
        self.high_score = score;
    }

    /// Kick the screen shake (`DAT_801DBEC0`); the epilogue decays it.
    pub fn shake_screen(&mut self, amplitude: i32) {
        self.shake = amplitude;
    }

    /// Every scalar the cabinet keeps, beside the overlay word it mirrors
    /// (all words but the secret override, a halfword): the view the retail
    /// kernel differential reads the machine through.
    pub fn retail_words(&self) -> [(u32, i32); 26] {
        [
            (0x801D_BF44, self.state as i32),
            (0x801D_C128, self.scene_timer),
            (0x801D_BF88, self.state_timer),
            (0x801D_BE94, self.intro_timer),
            (0x801D_BE90, self.lose_timer),
            (0x801D_BE9C, self.exit_timer),
            (0x801D_C12C, self.blink),
            (0x801D_C130, self.frames),
            (0x801D_BEB0, self.clear_fade_in),
            (0x801D_BEB4, self.clear_fade_out),
            (0x801D_BF70, self.select_cursor),
            (0x801D_BF90, self.menu_cursor),
            (0x801D_C10C, self.stage),
            (0x801D_C110, self.stage_display),
            (0x801D_BF06, i32::from(self.secret.raw())),
            (0x801D_BEE4, self.high_score),
            (0x801D_BEE8, self.prize),
            (0x801D_BEE0, self.bonus),
            (0x801D_BEB8, self.all_clear),
            (0x801D_BF8C, self.round),
            (0x801D_BF78, self.match_phase),
            (0x801D_BED4, i32::from(self.suppress_scene)),
            (0x801D_BEC0, self.shake),
            (0x801D_BF58, self.max_streak),
            (0x801D_BF28, self.perfect_matches),
            (0x8008_4440, self.pot as i32),
        ]
    }
}
