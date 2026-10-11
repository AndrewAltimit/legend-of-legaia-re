//! The [`SlotMachine`] session state, its sound latches and pacing constants, construction and read accessors.

use super::*;
use legaia_engine_system::rand::BiosRand;
use legaia_minigame_formats::slot_payout::{self, SlotPayoutTable};

/// The outcome of one evaluated spin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinResult {
    /// Winning payline index (`0` top / `1` middle / `2` bottom / `3`, `4` the
    /// two diagonals), or `None`. The index is also the medallion / lamp the
    /// machine lights - see [`legaia_minigame_formats::minigame_slot_scene`].
    pub line: Option<usize>,
    /// Winning symbol id, or `None`.
    pub symbol: Option<u8>,
    /// Coins credited for this spin (post-eval, pre-tally).
    pub payout: i32,
    /// `true` when this spin's win kicked off the bonus round (symbols 8/9).
    pub bonus_triggered: bool,
    /// `true` when this spin was a bonus-round free spin (product payout).
    pub bonus_spin: bool,
}

/// Which phase the machine is in. Mirrors the `DAT_801d3c84` state word at
/// the granularity the host drives: init/attract/spin/stop/payout, the
/// cash-out submenu and its two instruction pages (states `0x32..=0x39`), the
/// not-enough-coins prompt (`0x5A`) and the state-100 leave fade
/// ([`SlotMachine::retail_state`] carries the exact word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPhase {
    /// Attract / idle (state `1`): waiting for a bet.
    Idle,
    /// Spin-up (state `2`): reels accelerate until the spin timer expires.
    Spinning,
    /// Stopping (state `3`): reels stop one per stop input.
    Stopping,
    /// Payout tally (state `4`): a result is latched for collection.
    Payout,
    /// The cash-out submenu and its instruction pages (states `0x32..=0x39`,
    /// [`SlotMachine::cash_out_input`]).
    Menu,
    /// The not-enough-coins prompt (state `0x5A`): any face button leaves.
    NoCoins,
    /// The leave fade (state `100`): the screen fades to black, then the
    /// balance is committed ([`SlotPhase::CashedOut`]).
    Leaving,
    /// Cash-out committed (state `100`): the session is over.
    CashedOut,
}

/// A live slot-machine session: the reel strips, the two RNG streams, the
/// feature state, and the overlay-local coin balance. This is the
/// host-facing composition of the confirmed kernels (`FUN_801cf0d8` in
/// miniature).
#[derive(Debug, Clone)]
pub struct SlotMachine {
    pub(super) payouts: SlotPayoutTable,
    /// Slot LCG (`DAT_801d3c80`): strips + (via [`stop_plan`]) landings.
    pub(super) rng: SlotRng,
    /// Feature-roll stream. Retail uses BIOS `rand` here; the engine
    /// substitutes the same [`BiosRand`] LCG seeded alongside the slot LCG so
    /// replays stay deterministic (reconstruction - see the module docs).
    pub(super) rand: BiosRand,
    /// Display reel strips (`DAT_801d3d50`): win eval + render read **these**,
    /// and only these. A row holds a symbol id (`0..=9`) or, once a bonus round
    /// has rotated it in, a bonus numeral value (`0x10..=0x19`).
    pub(super) strips: [[u8; STRIP_LEN]; REEL_COUNT],
    /// Source strip: the ten reel **symbols** (`DAT_801d3e90`).
    pub(super) symbol_strips: [[u8; STRIP_LEN]; REEL_COUNT],
    /// Source strip: the ten bonus **numerals** as values `0x10..=0x19`
    /// (`DAT_801d3fd0`). A bonus round does not relabel the symbols - it feeds
    /// the display strip from here instead.
    pub(super) bonus_strips: [[u8; STRIP_LEN]; REEL_COUNT],
    /// Per-reel **claimed value** (`DAT_801d3d20`): the payline value + 1,
    /// latched the frame the reel locks; `0` until the reel's stop is taken.
    /// The marquee's bonus tally is this array (see [`SlotMachine::tally`]), and
    /// the payout multiplies the very same rows - so they cannot disagree.
    pub(super) claimed: [i32; REEL_COUNT],
    /// Live fixed-point reel positions (`DAT_801d3cc0..`), wrap mod `0x1400`.
    pub(super) reel_pos: [i32; REEL_COUNT],
    /// Reel velocities during a spin (`DAT_801d3cd0..`). Magnitudes are
    /// visual pacing, not pinned - host-rate constants.
    pub(super) reel_vel: [i32; REEL_COUNT],
    /// Landed payline row per reel (`None` while still spinning).
    pub(super) stopped: [Option<usize>; REEL_COUNT],
    /// Spin-up timer (`DAT_801d3c90`), frames until stopping is allowed.
    pub(super) spin_timer: i32,
    pub(super) phase: SlotPhase,
    /// Feature mode (`DAT_801d3cac`): `0` normal … `6` bonus round.
    pub(super) feature_mode: u8,
    /// Bonus free-spin / multiplier counter (`DAT_801d3cb0`).
    pub(super) bonus_spins: i32,
    /// Net-take heat counter (`DAT_801d3d40`): `+6` per normal spin, `+1` per
    /// feature spin, minus each bonus-round payout. The feature-odds bracket
    /// input; never reset during a session.
    pub(super) net_take: i32,
    /// Normal-mode target symbol for this spin (`DAT_801d3cb8`).
    pub(super) normal_target: u8,
    /// Per-spin landing-line row (`DAT_801d4134`, `rand % 5`): the row of the
    /// `0x801d3630` table that picks which payline a forced stop lands its
    /// target on ([`LANDING_LINE_BY_JITTER`]). Its `* 0x10` in
    /// `FUN_801d2440` is the table's row stride, not a sub-row nudge.
    pub(super) jitter: i32,
    /// Overlay-local playing balance (`DAT_801d4114`).
    pub(super) balance: i32,
    /// The spin-up press latch (`DAT_801d3790`): raised by any face-button
    /// edge during the spin-up ([`Self::latch_spin_up`]), read by the next
    /// spin's feature roll and cleared straight after it. It **widens** every
    /// feature-entry denominator, so a press makes the next spin's features
    /// *rarer* - the word was once read as a "richer odds" flag, which has
    /// the effect backwards.
    pub(super) spin_up_pressed: bool,
    /// "The bonus round just ended" latch (`DAT_801d3798`): the next spin runs
    /// the long spin-up, so the display strip has time to rotate back to the
    /// symbols before it reaches the payline.
    pub(super) bonus_just_ended: bool,
    /// The last evaluated spin, latched through [`SlotPhase::Payout`].
    pub(super) last_result: Option<SpinResult>,
    /// `DAT_801d3d3c`: the coin figure the marquee's payout caption prints.
    /// Latched the frame the third reel locks; cleared on collect.
    pub(super) caption_payout: i32,
    /// `DAT_801d3c94`: frames since the caption came up. The composer slides the
    /// caption in over the first [`PAYOUT_SLIDE_ROWS`] frames, so this has to
    /// advance for the caption to finish arriving.
    pub(super) caption_frame: i32,
    /// The five payline segments' model-space geometry (`DAT_801d3680`), when
    /// the host staged it ([`Self::with_paylines`]).
    paylines: Vec<legaia_minigame_formats::minigame_slot_scene::PayLine>,
    /// The bonus-anticipation latch (`DAT_801d3ca4`): `1` while two landed
    /// reels show a punch (`9`) pair on any payline, `2` for a kick (`8`)
    /// pair, `0` otherwise - see [`Self::anticipation`].
    pub(super) anticipation: i32,
    /// The scanner's once-per-spin guard (`DAT_801d3ca8`) over its sting and
    /// its loop swap.
    pub(super) anticipation_sounded: bool,
    /// `DAT_801d3d38`: the coins the payout state still has to tally into the
    /// balance.
    pub(super) payout_left: i32,
    /// `DAT_801d3c8c`: the winning-line word the lamps and the payline pass
    /// compare against; `-1` lights nothing.
    pub(super) win_line: i32,
    /// The overlay's free-running frame counter whose parity paces the tally
    /// (`DAT_801d3c9c`, advanced by the reel renderer's tail).
    pub(super) frame: u32,
    /// The sounds this machine raised since the host last took them.
    pub(super) sounds: SlotSounds,
    /// The retail state word while the machine is in [`SlotPhase::Menu`]
    /// (`0x32..=0x39`); unused otherwise.
    pub(super) menu_state: u8,
    /// The submenu cursor `DAT_801d4110`, kept as retail stores it - an
    /// unsigned word reduced `% 3` (so Up on row 0 stays on row 0).
    pub(super) menu_cursor: u32,
    /// The screen-fade level `DAT_801d3c98` (`0..=0xFF`) the instruction
    /// pages and the leave fade ramp by `0x10` a frame.
    pub(super) fade: i32,
}

/// One frame's worth of the machine's sound writes, as retail issues them:
/// stores straight into a cue-ring slot (`DAT_8007B6D8[slot] = id` - the
/// overlay never goes through the cursor producer), and the reel-motor voice
/// it keys and releases itself through `FUN_80065034` / `FUN_800653C8`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotSounds {
    /// `(ring slot, cue id)` stores, in order.
    pub ring: Vec<(u8, i16)>,
    /// Voices keyed on (`FUN_80065034`).
    pub voice_on: Vec<crate::other_game_overlay::VoiceAttrCue>,
    /// Voices released (`FUN_800653C8`).
    pub voice_off: Vec<u8>,
}

/// The SPU voice the reel motor loop keys (`FUN_80065034`'s `a0 = 0x13`).
pub const SPIN_VOICE: u8 = 0x13;
/// Reel stop click - one store into ring slot 0 per Stop press taken
/// (`0x801CF74C` / `0x801CF794` / `0x801CF7DC`).
pub const CUE_REEL_STOP: i16 = 0x20A;
/// Payout tally tick - one store into ring slot 0 per transfer
/// (`0x801CF900..0x801CF90C`).
pub const CUE_PAYOUT_TICK: i16 = 0x209;
/// The reach sting the scanner fires once per spin (`0x801D2050`).
pub const CUE_REACH: i16 = 0x200;
/// The spin-start stings of feature modes `1` / `2`, into ring slot 2
/// (`0x801CF5C0` / `0x801CF5DC`).
pub const CUE_FEATURE_1: i16 = 0x201;
pub const CUE_FEATURE_2: i16 = 0x202;
/// Caption timer the payout state starts at on a losing spin
/// (`li v0,0x6b` at `0x801CF85C`), so the state lasts
/// [`PAYOUT_HOLD_FRAMES`]` - 0x6B` frames.
pub const LOSING_PAYOUT_TIMER: i32 = 0x6B;
/// Caption timer at which a fully tallied payout state returns to idle
/// (`slti v0,v0,0x79` at `0x801CF918`).
pub const PAYOUT_HOLD_FRAMES: i32 = 0x79;

/// The reel motor's voice-attr call: `FUN_80065034(0x13, 2, 1, tone, 0x3C,
/// 0x40, 0x28, 0x28)` - class-2 VAB, program 1, tone `0` for the spin loop
/// (`0x801CF558`) and tone `1` for the reach loop the scanner swaps in
/// (`FUN_801D1AF4`, `0x801D2070`).
pub(super) fn spin_voice(tone: i32) -> crate::other_game_overlay::VoiceAttrCue {
    crate::other_game_overlay::VoiceAttrCue {
        voice: u32::from(SPIN_VOICE),
        vab_program_tone: (2, 1, tone),
        note_and_fine: (0x3C, 0x40),
        volume: (0x28, 0x28),
    }
}

/// Spin-up frames before the reels may be stopped (visual pacing constant;
/// the retail `DAT_801d3c90` magnitude is not pinned).
pub const SPIN_UP_FRAMES: i32 = 30;
/// Extra spin-up frames on a bonus spin, and on the first spin after a bonus
/// round ends (`FUN_801cf0d8` state 1: `DAT_801d3c90 = 0x18` when
/// `DAT_801d3cac == 6` or `DAT_801d3798`).
///
/// This is not decoration. The display strip is refilled one row per frame,
/// [`DISPLAY_REFRESH_LEAD`] rows ahead of the payline, so a reel has to *travel*
/// before the strip it swapped to reaches the row it pays on. The long spin-up
/// is what buys that travel - on both edges of the bonus round.
pub const BONUS_SPIN_UP_FRAMES: i32 = 0x18;
/// Per-reel spin velocities (visual pacing constants, staggered like the
/// retail ramp so the reels visibly desynchronize).
pub const SPIN_VELOCITY: [i32; REEL_COUNT] = [0x60, 0x70, 0x80];

impl SlotMachine {
    /// A fresh machine over the parsed payout table, seeded (retail reseeds
    /// from BIOS `rand` at init) and holding `balance` coins loaded from the
    /// casino coin bank.
    pub fn new(payouts: SlotPayoutTable, seed: u32, balance: i32) -> Self {
        let mut rng = SlotRng::new(seed);
        // Retail builds BOTH strips for each reel in one interleaved pass - the
        // symbol strip and the bonus-numeral strip, off the same RNG stream -
        // and then clones the symbol strip into the display copy the win eval
        // and the renderer read (`FUN_801cf0d8` case 0).
        let mut symbol_strips = [[0u8; STRIP_LEN]; REEL_COUNT];
        let mut bonus_strips = [[0u8; STRIP_LEN]; REEL_COUNT];
        for reel in 0..REEL_COUNT {
            let (symbols, bonus) = build_reel(&mut rng);
            symbol_strips[reel] = symbols;
            bonus_strips[reel] = bonus;
        }
        Self {
            payouts,
            rng,
            rand: BiosRand::new(seed ^ 0x5A5A_5A5A),
            strips: symbol_strips,
            symbol_strips,
            bonus_strips,
            claimed: [0; REEL_COUNT],
            reel_pos: [0; REEL_COUNT],
            reel_vel: [0; REEL_COUNT],
            stopped: [None; REEL_COUNT],
            spin_timer: 0,
            phase: SlotPhase::Idle,
            feature_mode: 0,
            bonus_spins: 0,
            net_take: 0,
            normal_target: 2,
            jitter: 0,
            balance: balance.clamp(0, BALANCE_CAP),
            spin_up_pressed: false,
            bonus_just_ended: false,
            last_result: None,
            caption_payout: 0,
            caption_frame: 0,
            paylines: Vec::new(),
            anticipation: 0,
            anticipation_sounded: false,
            payout_left: 0,
            win_line: -1,
            frame: 0,
            sounds: SlotSounds::default(),
            menu_state: 0,
            menu_cursor: 0,
            fade: 0,
        }
    }

    /// Stage the overlay's payline geometry
    /// ([`legaia_minigame_formats::minigame_slot_scene::parse_paylines`]) so every host
    /// draws the paylines off the machine itself ([`Self::payline_segments`]).
    pub fn with_paylines(
        mut self,
        paylines: Vec<legaia_minigame_formats::minigame_slot_scene::PayLine>,
    ) -> Self {
        self.paylines = paylines;
        self
    }

    /// The winning-line word the payline pass compares against
    /// (`DAT_801d3c8c`): the last spin's line, or `-1` - which lights none.
    pub fn winning_line_word(&self) -> i32 {
        self.win_line
    }

    /// Take the sounds raised since the last call ([`SlotSounds`]).
    pub fn take_sounds(&mut self) -> SlotSounds {
        std::mem::take(&mut self.sounds)
    }

    /// Coins the payout state still has to tally (`DAT_801d3d38`).
    pub fn payout_left(&self) -> i32 {
        self.payout_left
    }

    /// This frame's paylines, built by the ported pass and projected onto the
    /// 640x240 slot framebuffer. Empty when no geometry was staged.
    pub fn payline_segments(&self) -> Vec<ProjectedPayline> {
        projected_paylines(&self.paylines, self.winning_line_word())
    }

    /// Current phase.
    pub fn phase(&self) -> SlotPhase {
        self.phase
    }

    /// Overlay-local playing balance (`DAT_801d4114`).
    pub fn balance(&self) -> i32 {
        self.balance
    }

    /// Active feature mode (`DAT_801d3cac`; `0` normal, `6` bonus round).
    pub fn feature_mode(&self) -> u8 {
        self.feature_mode
    }

    /// Remaining bonus free spins (`DAT_801d3cb0`).
    pub fn bonus_spins(&self) -> i32 {
        self.bonus_spins
    }

    /// Net-take heat counter (`DAT_801d3d40`; the feature-odds bracket
    /// input).
    pub fn net_take(&self) -> i32 {
        self.net_take
    }

    /// The display strips (win eval + render source).
    pub fn strips(&self) -> &[[u8; STRIP_LEN]; REEL_COUNT] {
        &self.strips
    }

    /// The last evaluated spin (latched through [`SlotPhase::Payout`]).
    pub fn last_result(&self) -> Option<SpinResult> {
        self.last_result
    }

    /// The display-strip **value** currently on the payline of `reel`
    /// (`(pos >> 8) mod 0x14`). A symbol id `0..=9` in the normal game; a bonus
    /// numeral value `0x10..=0x19` once a bonus round has rotated the numbers in.
    pub fn payline_symbol(&self, reel: usize) -> u8 {
        let row = self.payline_row(reel);
        self.strips[reel][row]
    }

    /// `true` when a bonus round is running (`DAT_801d3cac == 6`): the reels
    /// carry numbers, every spin pays, and the payout is a product.
    pub fn in_bonus_round(&self) -> bool {
        self.feature_mode == FEATURE_MODE_BONUS
    }

    /// `true` when a bonus round has *just* ended (`DAT_801d3798`), so the next
    /// spin still runs the long spin-up - the symbols have to rotate back onto
    /// the payline before it can pay on them.
    pub fn bonus_just_ended(&self) -> bool {
        self.bonus_just_ended
    }

    /// Frames of spin-up the next spin will arm: the long one during a bonus
    /// round and on the spin straight after one, the short one otherwise.
    pub fn next_spin_up_frames(&self) -> i32 {
        SPIN_UP_FRAMES
            + if self.in_bonus_round() || self.bonus_just_ended {
                BONUS_SPIN_UP_FRAMES
            } else {
                0
            }
    }

    /// The raw claimed value of `reel` (`DAT_801d3d20[reel]`): the payline value
    /// **+ 1**, latched the frame the reel's stop was taken; `0` while the reel
    /// is still spinning.
    pub fn claimed(&self, reel: usize) -> i32 {
        self.claimed[reel]
    }

    /// The **bonus tally** - what the machine's marquee prints across the top of
    /// a bonus round, one column per reel.
    ///
    /// A column reads `0` until that reel's stop is claimed, and its landed
    /// number `1..=10` after: `0 x 0 x 0` at the start of a round, `9 x 5 x 0`
    /// with two reels down. The arithmetic is retail's, verbatim
    /// (`FUN_801cfff0`): print the message at `claimed - 0x10` when the claimed
    /// value clears `0xF`, else the `"0"` glyph.
    ///
    /// This is *the same latch* the payout multiplies - not a parallel display
    /// copy - so once all three columns are in, their product **is** the coins
    /// the round pays ([`SlotMachine::tally_product`]).
    // PORT: FUN_801cfff0 (the marquee's bonus tally row)
    pub fn tally(&self) -> [u32; REEL_COUNT] {
        core::array::from_fn(|r| {
            let claimed = self.claimed[r];
            if claimed > slot_payout::BONUS_VALUE_BIAS as i32 {
                (claimed - slot_payout::BONUS_VALUE_BASE as i32).max(0) as u32
            } else {
                0
            }
        })
    }

    /// `true` once every reel's stop has been claimed - i.e. the tally is
    /// complete and its product is the round's payout.
    pub fn tally_complete(&self) -> bool {
        self.claimed
            .iter()
            .all(|&c| c > slot_payout::BONUS_VALUE_BIAS as i32)
    }

    /// The product of the [`SlotMachine::tally`]'s three numbers, or `0` until
    /// all three columns are claimed. For a bonus round this is exactly the
    /// coins the spin pays.
    pub fn tally_product(&self) -> u32 {
        if !self.tally_complete() {
            return 0;
        }
        self.tally().iter().product()
    }

    /// The payline row index of `reel`.
    pub fn payline_row(&self, reel: usize) -> usize {
        ((self.reel_pos[reel] >> 8) as usize) % STRIP_LEN
    }

    /// The raw reel position of `reel` - a fixed-point angle whose high byte is
    /// the strip row and whose low byte is the sub-symbol fraction
    /// (`DAT_801d3cc0`). The renderer needs the fraction: retail's reel is a 3D
    /// cylinder and the fraction is what rotates it between symbols.
    pub fn reel_pos(&self, reel: usize) -> i32 {
        self.reel_pos[reel]
    }

    /// Whether `reel`'s stop is still open - the per-reel flag
    /// `DAT_801d3d00[reel]`, which the bet charge sets and the reel's Stop
    /// press clears (`FUN_801d2114`). The furniture pass `FUN_801d08e4` reads
    /// it to light that reel's pedestal and, ORed over the three, to brighten
    /// the medallions and marquee while any stop is still to be taken.
    // REF: FUN_801d08e4 (the pedestal pass that reads the flag)
    pub fn reel_stop_open(&self, reel: usize) -> bool {
        matches!(self.phase, SlotPhase::Spinning | SlotPhase::Stopping)
            && self.stopped.get(reel).is_some_and(|s| s.is_none())
    }

    /// The bonus-anticipation latch (`DAT_801d3ca4`): `1` while two landed
    /// reels show a punch pair, `2` for a kick pair, `0` otherwise. Read by
    /// the marquee's legend tail, which scrolls message `4` / `5` for it
    /// ([`legaia_minigame_formats::minigame_slot_scene::attract_legend`]).
    pub fn anticipation(&self) -> i32 {
        self.anticipation
    }

    /// The bonus-symbol scanner (`FUN_801d1af4`), run while exactly two stops
    /// are in: on each of the five paylines it compares the three reel pairs
    /// `(0,1)`, `(1,2)`, `(0,2)`, each only when both reels have landed
    /// (`DAT_801d3d10[r]`, ANDed per pair), and notes an equal pair of `9`s
    /// (punch) or `8`s (kick). A punch pair anywhere wins: the latch reads
    /// `1`, else `2` for a kick pair, else nothing is written (`0`, the state
    /// frame's clear). The scanner's rows are retail's `+0x11 / +0x10 / +0x0F`
    /// and the two diagonals - [`PAYLINE_ROW_OFFSETS`] around the payline row.
    ///
    /// The scanner also raises SFX cue `0x200` and calls `FUN_80065034`
    /// once per spin behind the guard `DAT_801d3ca8`; that half rides
    /// [`Self::tick`], which raises the sting and the reach loop.
    ///
    /// [`PAYLINE_ROW_OFFSETS`]: legaia_minigame_formats::minigame_slot_scene::PAYLINE_ROW_OFFSETS
    // PORT: FUN_801d1af4 (PROT 0975)
    pub(super) fn anticipation_scan(&self) -> i32 {
        use legaia_minigame_formats::slot_payout::{KICK_SYMBOL_ID, PUNCH_SYMBOL_ID};
        let landed: [bool; REEL_COUNT] = core::array::from_fn(|r| self.stopped[r].is_some());
        let (mut punch, mut kick) = (false, false);
        for offs in legaia_minigame_formats::minigame_slot_scene::PAYLINE_ROW_OFFSETS.iter() {
            let v = |r: usize| {
                let row = (self.payline_row(r) as isize + offs[r] as isize)
                    .rem_euclid(STRIP_LEN as isize) as usize;
                self.strips[r][row]
            };
            for (a, b) in [(0, 1), (1, 2), (0, 2)] {
                if landed[a] && landed[b] && v(a) == v(b) {
                    punch |= v(a) == PUNCH_SYMBOL_ID;
                    kick |= v(a) == KICK_SYMBOL_ID;
                }
            }
        }
        if punch {
            1
        } else if kick {
            2
        } else {
            0
        }
    }

    /// How many reels are stopped this spin (`DAT_801d3d2c`).
    pub fn reels_stopped(&self) -> usize {
        self.stopped.iter().filter(|s| s.is_some()).count()
    }

    /// `true` when the spin timer has expired and stop inputs are accepted.
    pub fn can_stop(&self) -> bool {
        self.phase == SlotPhase::Stopping
    }

    /// The coin cost of the next spin: flat [`SPIN_COST_NORMAL`] in modes
    /// 0..=3, [`SPIN_COST_FEATURE`] in feature modes 4..=6 (there is no
    /// bet-line selection - all five paylines always play).
    pub fn spin_cost(&self) -> i32 {
        if (4..=6).contains(&self.feature_mode) {
            SPIN_COST_FEATURE
        } else {
            SPIN_COST_NORMAL
        }
    }

    /// `true` when a spin is accepted: idle and the balance clears the
    /// state-1 "not enough coins" gate (applied in every mode - retail
    /// checks `< 3` before looking at the feature mode, so even a 1-coin
    /// feature spin needs 3 banked).
    pub fn can_spin(&self) -> bool {
        self.phase == SlotPhase::Idle && self.balance >= MIN_SPIN_BALANCE
    }

    /// This spin's landing-line row (`DAT_801d4134`): which payline a forced
    /// stop lands its target on.
    pub fn jitter(&self) -> i32 {
        self.jitter
    }
}
