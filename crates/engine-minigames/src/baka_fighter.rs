//! From-scratch **Baka Fighter duel minigame** rules engine.
//!
//! A faithful port of the fight logic in the Baka Fighter overlay (PROT 0976):
//! the rock-paper-scissors exchange resolver, the HP-tiered damage kernel, the
//! comeback-critical roll, the scripted-pattern CPU move picker, and the
//! best-of-3 round/match bookkeeping - driven by the already-parsed roster +
//! action tables ([`legaia_asset::baka_opponents`]). This is the *rules*
//! layer: it consumes chosen attack types (pad presses on the player side)
//! and produces resolved exchanges, damage, round wins and the gold prize,
//! exactly as the retail overlay does. The duel's 3D presentation (fighter
//! meshes, clips, banners, HUD) is a host concern and is not covered here.
//!
//! Every formula and constant is the reading from
//! [`docs/subsystems/minigame-baka-fighter.md`](../../../docs/subsystems/minigame-baka-fighter.md),
//! re-derived from the overlay dumps cited on each item.
//!
//! **Exchange timing.** A fight built from the disc tables
//! ([`BakaFight::from_tables`]) carries each fighter's action-record speed and
//! strike-frame column ([`StrikeTable`]) and runs retail's frame cursor over
//! the chosen attack ([`StrikeClock`]): the exchange is booked on the tick the
//! winner's strike keyframe is crossed. The special (type 4) is no player
//! choice: it is retail's auto-finisher, thrown by whoever just put the foe at
//! 0 HP, and the round is won by its *last* strike landing - a special with
//! strikes left keeps playing after its first. What the port still does not model is the clip **tail** - retail
//! plays each attack clip to its ANM frame count and only then drops back to
//! idle, while the port clears the exchange once it is booked and paces
//! re-entry with the retail cooldown decay. A fight built from bare configs
//! ([`BakaFight::new`]) has no strike data and resolves the tick both sides
//! have chosen; with no special clip to play, its knockout ends the round on
//! the spot.
//!
//! Chain: retail `FUN_801d3468` (match resolution SM) → `FUN_801d3a14`
//! (exchange win-condition) → `FUN_801d3b18` (damage) → `FUN_801d6660`
//! (comeback-crit roll); CPU picks via `FUN_801d487c`.

use legaia_asset::baka_opponents::{BakaActionSet, BakaOpponent, ROUND_WIN_TARGET};

use legaia_engine_vm::bios_rand::BiosRand;

/// Starting HP each round (`FUN_801d1744` round seed: `DAT_801dbfc4 = 0xc80`).
pub const HP_START: i32 = 0xC80;

/// HP at/above this uses stat tier `[0]` (the `0x8c1` threshold).
pub const HP_TIER_HIGH: i32 = 0x8C1;

/// HP at/above this (but below [`HP_TIER_HIGH`]) uses tier `[1]` (`0x3c1`).
pub const HP_TIER_MID: i32 = 0x3C1;

/// The comeback-crit roll only fires while `0 < HP < 0x280` (`FUN_801d6660`).
pub const CRIT_HP_BAND: i32 = 0x280;

/// Per-consecutive-hit damage bonus step (`(combo - 1) * 0x40`).
pub const COMBO_DAMAGE_STEP: i32 = 0x40;

/// Post-exchange cooldown seed (`FUN_801d3468` writes 200).
pub const COOLDOWN_RESET: i32 = 200;

/// Cooldown decay per frame step (`cooldown -= frame_step * 0x10`).
pub const COOLDOWN_DECAY: i32 = 0x10;

/// One of the duel's attack commitments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BakaAttack {
    /// Attack type 1 (beaten by 2, beats 3).
    A,
    /// Attack type 2 (beats 1, beaten by 3).
    B,
    /// Attack type 3 (beats 2, beaten by 1).
    C,
    /// Type 4 - the special / guard-break: an immediate exchange win for
    /// whoever throws it (fighter 0 has priority when both do).
    Special,
}

impl BakaAttack {
    /// The retail attack-type id (`DAT_801dbfe0` value space).
    pub fn type_id(self) -> u8 {
        match self {
            BakaAttack::A => 1,
            BakaAttack::B => 2,
            BakaAttack::C => 3,
            BakaAttack::Special => 4,
        }
    }

    /// Map a retail type id (`1..=4`) back to the attack.
    pub fn from_type_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(BakaAttack::A),
            2 => Some(BakaAttack::B),
            3 => Some(BakaAttack::C),
            4 => Some(BakaAttack::Special),
            _ => None,
        }
    }
}

/// Result of one exchange-resolution pass (`FUN_801d3a14` return space:
/// `-1` undecided / `0` fighter-0 wins / `1` fighter-1 wins / `3` draw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeOutcome {
    /// No resolution this frame (nobody committed, or the settle timer runs).
    Undecided,
    /// The indexed fighter wins the exchange (its opponent takes the damage).
    FighterWins(usize),
    /// Both chose the same type - both take damage, both reset.
    Draw,
}

/// What one resolved exchange did - surfaced for the host HUD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeReport {
    /// Winning fighter slot (the draw arm reports fighter 1, matching the
    /// retail SM's final crit-roll operand).
    pub winner: usize,
    /// `true` when the exchange was a same-type draw (both damaged).
    pub draw: bool,
    /// Damage applied to the (last) loser.
    pub damage: i32,
    /// The winning hit consumed a pending comeback critical.
    pub critical: bool,
    /// A fully-charged special landed - an immediate round win.
    pub special_round_win: bool,
}

/// Per-fighter static configuration, lifted from the parsed roster + action
/// tables. Build via [`FighterConfig::from_tables`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FighterConfig {
    /// Roster id (0..17) this fighter plays as.
    pub roster_id: usize,
    /// Record `+0x24` - the base defense value (`mod + mod*def/100`).
    pub damage_mod: i32,
    /// Record `+0x28..` - DEF tier % at HP high / mid / low.
    pub def_tiers: [i32; 3],
    /// Record `+0x34` - comeback-critical chance %.
    pub crit_chance: i32,
    /// Record `+0x38..` - ATK tier % at HP high / mid / low.
    pub atk_tiers: [i32; 3],
    /// Action-record `+0x18` powers, indexed by attack type id (1..=4).
    pub attack_power: [i32; 5],
    /// Record `+0x20` - gold paid out when this fighter is beaten.
    pub gold_reward: u32,
    /// Record `+0x4c` - the scripted CPU attack loop (empty = random only).
    pub ai_pattern: Vec<u8>,
}

impl FighterConfig {
    /// Lift a roster entry + its action set into a fight configuration.
    pub fn from_tables(opponent: &BakaOpponent, actions: &BakaActionSet) -> Self {
        let mut attack_power = [0i32; 5];
        for t in 1..=4u8 {
            attack_power[t as usize] = actions.attack_power(t).unwrap_or(0);
        }
        Self {
            roster_id: opponent.index,
            damage_mod: opponent.damage_mod,
            def_tiers: opponent.def_tiers,
            crit_chance: opponent.crit_chance,
            atk_tiers: opponent.atk_tiers,
            attack_power,
            gold_reward: opponent.gold_reward,
            ai_pattern: opponent.ai_pattern.clone(),
        }
    }

    /// The HP-keyed ATK tier (`>= 0x8c1` → `[0]`, `>= 0x3c1` → `[1]`, else `[2]`).
    fn atk_tier(&self, hp: i32) -> i32 {
        Self::tier(&self.atk_tiers, hp)
    }

    /// The HP-keyed DEF tier (same thresholds).
    fn def_tier(&self, hp: i32) -> i32 {
        Self::tier(&self.def_tiers, hp)
    }

    fn tier(tiers: &[i32; 3], hp: i32) -> i32 {
        if hp >= HP_TIER_HIGH {
            tiers[0]
        } else if hp >= HP_TIER_MID {
            tiers[1]
        } else {
            tiers[2]
        }
    }
}

/// Per-fighter mutable duel state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FighterState {
    /// Current round HP (`&DAT_801dbfc4[slot]`).
    hp: i32,
    /// Round wins (`&DAT_801dbff0[slot]`); 2 takes the match.
    round_wins: u32,
    /// Consecutive hits *taken* (`&DAT_801dbfec[slot]`) - feeds the escalating
    /// combo damage bonus and resets when this fighter wins an exchange.
    combo: i32,
    /// Total hits taken this match (`&DAT_801dbff4[slot]`).
    hits_taken: u32,
    /// Chosen attack this exchange (`&DAT_801dbfe0[slot]`, `None` = type 0).
    chosen: Option<BakaAttack>,
    /// "Already committed this exchange" flag (`&DAT_801dbfe8[slot]`).
    committed: bool,
    /// Pending comeback critical (`&DAT_801dc05c[slot]`).
    crit_pending: bool,
    /// Attack-rate cooldown (`DAT_801dbea0` / `DAT_801dbea4`).
    cooldown: i32,
    /// CPU scripted-pattern cursor (`&DAT_801dc044[slot]`, counts DOWN).
    ai_cursor: usize,
    /// The combat tick's frame cursor over the chosen attack's action record.
    clock: StrikeClock,
}

impl FighterState {
    fn new() -> Self {
        Self {
            hp: HP_START,
            round_wins: 0,
            combo: 0,
            hits_taken: 0,
            chosen: None,
            committed: false,
            crit_pending: false,
            cooldown: 0,
            ai_cursor: 0,
            clock: StrikeClock::default(),
        }
    }

    fn reset_round(&mut self) {
        self.hp = HP_START;
        self.combo = 0;
        self.chosen = None;
        self.committed = false;
        self.crit_pending = false;
        self.cooldown = COOLDOWN_RESET;
        self.clock = StrikeClock::default();
    }
}

/// Where a fighter's strike clock stands: the per-fighter word at block
/// `+0x0C` (`&DAT_801dbfc8[slot * 0x2a]`) the combat tick and the damage
/// kernel hand between them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StrikeState {
    /// `0` - a fresh commit; the keyframe lookup runs every tick.
    #[default]
    Armed,
    /// `1` - the cursor crossed a new sub-keyframe; the resolution SM may
    /// book the exchange (`FUN_801D3468` tests `== 1` before every damage
    /// call, `0x801D36DC` / `0x801D3730` / `0x801D378C`).
    Landed,
    /// `2` - the damage kernel consumed the strike (`FUN_801D3B18` writes it
    /// to the winner at `0x801D3EB0`); the lookup re-arms, so a later
    /// sub-keyframe of the same clip can land again (the special's second
    /// strike).
    Consumed,
}

/// One fighter's strike clock - the frame cursor the retail combat tick
/// `FUN_801D3F44` runs over the chosen attack, and the lookup it feeds.
///
/// Per tick, while an attack is chosen: unless a strike is already
/// [`StrikeState::Landed`], look the cursor's last step up through
/// [`keyframe_in_range`] (`0x801D4334`: `from` = the cursor before the step,
/// block `+0x90`; `to` = after it, actor `+0x68`), and a hit on a sub-keyframe
/// other than the one cached at block `+0x98` lands it. Then the step itself:
/// `from = to; to += frame_step * speed * 8 >> 3` (`+0x6A` from record `+0x04`
/// times the frame-rate divisor `DAT_1F80037D`, which the round setup seeds to
/// `8` at `0x801D01A4`; the clip selector `FUN_800204F8` adds it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StrikeClock {
    /// Actor `+0x68` - the clip cursor, 1/16-frame fixed point.
    pub cursor: i32,
    /// Block `+0x90` - the cursor before the last step.
    pub prev: i32,
    /// Block `+0x98` - the sub-keyframe last landed (`-1` = `None` at commit).
    pub landed: Option<usize>,
    /// Block `+0x0C`.
    pub state: StrikeState,
    /// Set on the commit tick: retail's lookup that tick ran *before* the
    /// commit, against the previous clip, so the new clip's first lookup is
    /// the next tick's `[0, step]`.
    fresh: bool,
}

/// The frame-rate divisor the round setup stores at `DAT_1F80037D`
/// (`0x801D01A4`); the cursor step is `speed * divisor >> 3`, so at `8` the
/// step is the record's speed verbatim. A special's commit lowers it to
/// [`crate::baka_fighter_chrome::SPECIAL_RATE_DIVISOR`] for the rest of the
/// round (`0x801D4568`), and the tick recomputes both fighters' steps from it
/// every frame (`0x801D4780..0x801D47CC`) - the special plays in slow motion.
pub const STRIKE_RATE_DIVISOR: i32 = 8;

/// The cameo's clip step: its animator forces `+0x6A = 8` every frame
/// (`0x801D6320..0x801D632C`) - half a frame per tick.
pub const CAMEO_STEP: i32 = 8;

/// The three ANM record header fields the clip selector `FUN_800204F8` and
/// the afterimage read off a fighter's clip record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipHeader {
    /// Record `+0x02` - the clip's frame count.
    pub frames: u16,
    /// Record `+0x01` bit 0: the selector steps the cursor by
    /// `(step * 2 + n - 1) / n` instead of `step` (`0x800205B4..0x800205E4`).
    pub double_step: bool,
    /// Record `+0x06` - the `n` of that formula.
    pub n: u8,
}

impl ClipHeader {
    /// From a parsed record's header words (`legaia_asset::player_anm`
    /// `PlayerAnmRecord { a, b, flag, .. }`: `a` = bytes `+0/+1`, `b` =
    /// `+2/+3`, `flag` = `+6/+7`).
    pub fn from_record_words(a: u16, b: u16, flag: u16) -> Self {
        ClipHeader {
            frames: b,
            double_step: (a >> 8) & 1 != 0,
            n: (flag & 0xFF) as u8,
        }
    }

    /// The cursor step the clip selector applies for an actor step `step`.
    pub fn selector_step(&self, step: i32) -> i32 {
        if self.double_step {
            let n = i32::from(self.n.max(1));
            (step * 2 + n - 1) / n
        } else {
            step
        }
    }
}

/// One fighter's nine clip headers, by action.
pub type ClipHeaders = [Option<ClipHeader>; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER];

/// A clip bank's headers for the nine actions starting at `first_record`.
pub fn clip_headers_from_bundle(
    bundle: &legaia_asset::player_anm::PlayerAnmBundle,
    first_record: usize,
) -> ClipHeaders {
    std::array::from_fn(|action| {
        let r = bundle.record(first_record + action).ok()?;
        Some(ClipHeader::from_record_words(r.a, r.b, r.flag))
    })
}

/// Every roster fighter's clip headers, indexed by roster id, read off the
/// disc through `read_prot` (extraction PROT index -> entry bytes).
///
/// The party fighters (roster `0..3`) pose off the PROT 1203 battle-form bank,
/// nine records each (`id * 9 + action`); the ladder fighters (`3..`) off
/// their own pack's bank (PROT `1206 + id - 3`, record = action) - the same
/// two banks the duel's clip ids resolve into (see
/// `legaia_asset::baka_opponents::action_slot_label`). A fighter whose bank
/// does not decode gets all-`None` headers.
pub fn roster_clip_headers(read_prot: impl Fn(usize) -> Option<Vec<u8>>) -> Vec<ClipHeaders> {
    use legaia_asset::baka_opponents as bo;
    let party = read_prot(bo::BAKA_HUD_ART_PROT_INDEX).and_then(|e| {
        legaia_asset::player_anm::find_in_entry(&e, 4)
            .into_iter()
            .next()
    });
    (0..bo::OPPONENT_COUNT)
        .map(|roster| {
            if roster < bo::FIGHTER_PACK_FIRST_ROSTER_ID {
                party
                    .as_ref()
                    .map(|b| clip_headers_from_bundle(b, roster * bo::ACTIONS_PER_FIGHTER))
                    .unwrap_or([None; bo::ACTIONS_PER_FIGHTER])
            } else {
                bo::fighter_pack_prot_index(roster)
                    .and_then(&read_prot)
                    .and_then(|e| bo::parse_fighter_pack(&e))
                    .and_then(|p| legaia_asset::player_anm::parse(&p.anim_bytes).ok())
                    .map(|b| clip_headers_from_bundle(&b, 0))
                    .unwrap_or([None; bo::ACTIONS_PER_FIGHTER])
            }
        })
        .collect()
}

/// One fighter's strike data - the action records' `+0x04` speed and `+0x26`
/// strike-frame column, indexed by action (the attack types are actions
/// `1..=4`), plus the clip headers when a host staged the fighter's clip bank.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StrikeTable {
    pub speed: [i32; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER],
    pub frames: [Vec<i16>; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER],
    /// Per-action clip header; `None` until a host supplies the fighter's
    /// clip bank ([`BakaFight::set_clip_headers`]). Without it the strike
    /// clock steps by the record speed alone, which is exact for every clip
    /// whose record leaves the double-step bit clear.
    pub clips: [Option<ClipHeader>; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER],
    /// Per-action strike offsets - each live sub-keyframe's `+0x20 / +0x22 /
    /// +0x24` TRS, in slot order (the column the impact pair reads).
    pub offsets: [Vec<[i16; 3]>; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER],
}

impl StrikeTable {
    /// Lift a fighter's parsed action set.
    pub fn from_actions(actions: &BakaActionSet) -> Self {
        let mut frames: [Vec<i16>; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER] =
            Default::default();
        for (a, f) in frames.iter_mut().enumerate() {
            *f = actions.strike_frames(a);
        }
        let offsets = std::array::from_fn(|a| {
            actions
                .sub_keyframes
                .get(a)
                .map(|k| k.iter().map(|s| s.offset).collect())
                .unwrap_or_default()
        });
        StrikeTable {
            speed: actions.speed,
            frames,
            clips: [None; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER],
            offsets,
        }
    }

    /// The action record an attack type plays (types `1..=4` are actions
    /// `1..=4`: `record[+0x10] = anim - base - 1`).
    fn action_of(attack: BakaAttack) -> usize {
        attack.type_id() as usize
    }
}

impl StrikeClock {
    /// A commit: cursor to the clip start, nothing landed (`+0x98 = -1`,
    /// `+0x68 = 0`, both blocks' `+0x0C = 0` at `0x801D44B8..0x801D44D8`).
    fn commit(&mut self) {
        *self = StrikeClock {
            fresh: true,
            ..StrikeClock::default()
        };
    }

    /// One combat-tick step of the clock over `frames` at `speed`.
    ///
    /// REF: FUN_801d3f44 (`0x801D4304..0x801D435C` lookup, `0x801D47E0..
    /// 0x801D47EC` the `+0x90` store before the clip selector's advance)
    fn step(
        &mut self,
        frames: &[i16],
        speed: i32,
        divisor: i32,
        clip: Option<ClipHeader>,
        frame_step: i32,
    ) {
        if !self.fresh
            && self.state != StrikeState::Landed
            && let Some(i) = keyframe_in_range(frames, self.prev, self.cursor)
            && self.landed != Some(i)
        {
            self.landed = Some(i);
            self.state = StrikeState::Landed;
        }
        self.fresh = false;
        let step = sra3_round_to_zero(speed * divisor);
        let step = clip.map_or(step, |c| c.selector_step(step));
        self.prev = self.cursor;
        self.cursor = self.cursor.wrapping_add(frame_step * step);
    }
}

/// `x >> 3` rounding toward zero - the `bgez; addiu 7; sra 3` idiom at
/// `0x801D47AC..0x801D47C4`.
fn sra3_round_to_zero(v: i32) -> i32 {
    (if v < 0 { v + 7 } else { v }) >> 3
}

/// The base every host folds its frame count into to seed a cabinet
/// (`BAKA_RNG_BASE ^ frame`): the play hosts with the world frame, the
/// standalone minigames page with its own stepped-frame count. A port
/// choice, not a retail literal - one shared base keeps a replayed pad
/// stream deterministic on every host.
pub const BAKA_RNG_BASE: u32 = 0xBA4A_F19A;

/// What one [`BakaFight::frame`] hands the host's winnings accumulator
/// (retail's mode-24 `_DAT_80084440`, the coin prize - not party gold).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BakaFrameOutcome {
    /// Coins the result tally drained this frame (`FUN_801D239C`'s add).
    pub paid: u32,
    /// The game-over state forfeited the pot: the accumulator empties.
    pub forfeit: bool,
    /// The cabinet's exit state finished: the host leaves the minigame.
    pub exit: bool,
}

/// Match phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPhase {
    /// Exchanges run; fighters choose and resolve.
    Fighting,
    /// A round just ended (winner indexed); the next tick starts the next round.
    RoundOver(usize),
    /// The match is decided.
    MatchOver(usize),
}

/// The SFX cue the duel fires when an exchange's damage lands.
///
/// Retail queues sound by writing a cue id straight into the 4-entry ring at
/// `_DAT_8007B6D8`, which the drainer `FUN_80016B6C` resolves against the
/// static descriptor table (`&DAT_8006F198 + id*8`, see
/// `docs/formats/sfx-table.md`). The damage kernel `FUN_801D3B18` writes `9`.
///
/// It is the only cue the *fight* fires: a sweep of the whole duel overlay
/// finds exactly four ring writes - this one plus the menu / tally blips
/// ([`BAKA_CUE_CONFIRM`] / [`BAKA_CUE_CURSOR`] / [`BAKA_CUE_CANCEL`]), which
/// belong to the surrounding UI, not to [`BakaFight`]. Round-start banners,
/// KOs, draws and victory poses are **silent** in retail.
pub const BAKA_CUE_HIT: u8 = 0x09;
/// Menu confirm blip (duel menu SM). Not fired by [`BakaFight`] - the host's
/// UI owns it.
pub const BAKA_CUE_CONFIRM: u8 = 0x20;
/// Menu cursor-move blip, also the score-tally tick (`FUN_801D239C`).
pub const BAKA_CUE_CURSOR: u8 = 0x21;
/// Menu cancel blip.
pub const BAKA_CUE_CANCEL: u8 = 0x37;

/// The running Baka Fighter duel.
#[derive(Debug, Clone)]
pub struct BakaFight {
    cfg: [FighterConfig; 2],
    f: [FighterState; 2],
    /// SFX cue ids queued this tick, in fire order - the host's view of the
    /// retail cue-ring writes. Drained by [`BakaFight::take_cues`].
    cues: Vec<u8>,
    /// Which slots the CPU picker drives (slot 1 in retail; both for demos).
    ai_controlled: [bool; 2],
    /// Round index (`DAT_801dbf20`).
    round: u32,
    /// Per-exchange settle timer (`DAT_801dbf54`). No seeder exists in the
    /// dumped corpus - it only ever decays - so it starts (and stays) 0
    /// unless a host installs a pace.
    settle_timer: i32,
    phase: MatchPhase,
    rng: BiosRand,
    last_exchange: Option<ExchangeReport>,
    /// The end-of-match score tally, installed once the player takes the
    /// match (`FUN_801d239c`'s screen). `None` until then, and on a loss -
    /// a beaten player is paid nothing.
    tally: Option<BakaTally>,
    /// The two overlay score-bonus tables, when a host has parsed them.
    score_tables: Option<BakaScoreTables>,
    /// `DAT_801dbec8` - the running **maximum** of the player's hit streak,
    /// which retail latches once per frame in the HUD renderer
    /// (`FUN_801d2afc`: `if (DAT_801dbec8 <= DAT_801dc094) DAT_801dbec8 =
    /// DAT_801dc094;`). `DAT_801dc094` is `&DAT_801dbfec[1 * 0x2a]` - slot 1's
    /// consecutive-hits-taken counter, i.e. how long a streak the player is
    /// currently landing.
    max_combo: i32,
    /// The three score rows `FUN_801d2a28` accumulates into
    /// (`DAT_801dbee0` / `DAT_801dbed8` / `DAT_801dbedc`), which the
    /// end-of-match [`BakaTally`] then drains.
    score_rows: [i32; 3],
    /// The round chrome, advanced once per [`BakaFight::tick_with_input`].
    chrome: crate::baka_fighter_chrome::BakaChrome,
    /// The chrome frame the last tick produced.
    chrome_frame: crate::baka_fighter_chrome::ChromeFrame,
    /// The round-result banner sub-state `DAT_801DBF84`: non-zero once this
    /// round's result banners are up, so they rise once per round.
    result_latch: bool,
    /// `DAT_801DBF24` - the player has not been hit this round. Raised at the
    /// round's setup, cleared by the two exchange arms that damage slot 0;
    /// a win with it still up is the PERFECT!! banner.
    player_untouched: bool,
    /// The cabinet shell state machine (`FUN_801CF388`), stepped once per
    /// tick. Retail has the nesting the other way up - the cabinet SM owns the
    /// frame and the fight resolution runs under it - but the port's host
    /// enters through the duel, so the duel owns the frame and the cabinet
    /// runs as its shell. The state the cabinet occupies while a duel is live
    /// is the same one retail uses ([`crate::baka_cabinet::ST_DUEL`]), so the
    /// bracket it drives (round count, stage advance, the win / lose exits,
    /// the pot) is retail's.
    cabinet: crate::baka_cabinet::BakaCabinet,
    /// The cabinet frame the last tick produced.
    cabinet_frame: crate::baka_cabinet::CabinetFrame,
    /// The roster + action tables the fight was built from, kept so the
    /// cabinet's between-rung opponent install (`ST_OPPONENT_INSTALL`) can
    /// seat the next rung's fighter. `None` for a fight built from bare
    /// configs, which re-seats the same opponent instead.
    tables: Option<std::sync::Arc<(Vec<BakaOpponent>, Vec<BakaActionSet>)>>,
    /// The packed pad edge the host handed over for the next cabinet tick
    /// (consumed by it). See [`Self::set_cabinet_pad`].
    cabinet_pad: u16,
    /// Both fighters' strike data. With it installed an exchange is booked
    /// only on the tick the winner's strike lands (retail's `+0x0C == 1`
    /// gate); without it (a fight built from bare configs) the exchange
    /// resolves the tick both sides have chosen.
    strike: Option<[StrikeTable; 2]>,
    /// `DAT_1F80037D` - the frame-rate divisor both clips step by.
    rate_divisor: i32,
    /// The live afterimage actors (one per special commit).
    afterimages: Vec<crate::baka_fighter_chrome::AfterimageActor>,
    /// What each afterimage drew on the last tick, by owner slot.
    afterimage_frames: Vec<(usize, crate::baka_fighter_chrome::AfterimageFrame)>,
    /// Every roster fighter's clip headers, when a host staged them.
    roster_clips: Option<std::sync::Arc<Vec<ClipHeaders>>>,
    /// Each seat's display clip (actor `+0x5C` / `+0x68`), stepped every
    /// tick by [`Self::tick_presentation`].
    motion: [crate::baka_duel::FighterMotion; 2],
    /// The arena camera.
    camera: crate::baka_duel::DuelCamera,
    /// Each seat's roster `+0x44` stand-off (`0` for a bare-config fight).
    stand_off: [i32; 2],
    /// Each seat's Z off the duel line (the result close-up's step).
    stand_z: [i32; 2],
    /// The player's special-commit camera glides, by party fighter.
    special_cameras: Vec<[i16; 20]>,
    /// The packed held pad word (`_DAT_8007B850`) the host handed over for
    /// the next tick ([`Self::set_held_pad`]).
    held_pad: u16,
    /// A round setup (cabinet state `0x32`) is due on the next presentation
    /// tick - the first round's and every later one's.
    setup_pending: bool,
    /// Frames the player-select lineup has been on stage - the clock its
    /// three idle clips run off ([`Self::select_lineup`]).
    select_clock: i32,
    /// The round-start cameo's actor, while it walks: its phase `+0x22` and
    /// the clip cursor `+0x68` its clip selector advances.
    cameo: Option<CameoActor>,
    /// `DAT_801DBF50` - raised by a special's commit (`0x801D4578`), cleared
    /// by the round setup (`0x801CFFD8`). The impact pair reads it for its
    /// Z lift.
    special_latch: bool,
    /// The impact pair's effect parts ([`crate::baka_impact_fx`]).
    impact: crate::baka_impact_fx::ImpactFx,
}

/// The round-start cameo actor (`FUN_801D6310`'s record): its phase and the
/// clip cursor under the clip it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameoActor {
    /// `+0x22`.
    pub phase: i16,
    /// `+0x68`, 1/16 frame.
    pub cursor: i32,
    /// `+0x5C` the cursor belongs to.
    pub clip: i16,
}

impl CameoActor {
    /// This frame's pose ([`crate::baka_fighter_chrome::cameo_pose`]).
    pub fn pose(&self) -> Option<crate::baka_fighter_chrome::CameoPose> {
        crate::baka_fighter_chrome::cameo_pose(self.phase)
    }
}

/// The two per-round score-bonus tables the overlay carries as rodata: the
/// combo-bonus table `&DAT_801d70c4` (20 `i32` slots) and the health-bonus
/// table `&DAT_801d711c` (`i16` slots). They are Sony bytes with no parser in
/// `legaia_asset::baka_opponents`, so a host that wants the score rows
/// populated reads them out of the overlay image itself; with no tables the
/// rows stay at zero, which is what [`BakaFight`] does by default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BakaScoreTables {
    pub combo_bonus: Vec<i32>,
    pub health_bonus: Vec<i16>,
}

/// Runtime VA of the combo-bonus table `&DAT_801D70C4`.
pub const COMBO_BONUS_TABLE_VA: u32 = 0x801D_70C4;
/// Runtime VA of the health-bonus table `&DAT_801D711C`.
pub const HEALTH_BONUS_TABLE_VA: u32 = 0x801D_711C;
/// Load base of the Baka Fighter overlay (PROT 0976).
pub const BAKA_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// Rows in the health-bonus table: the index is `hp / `
/// [`BAKA_HEALTH_BONUS_DIVISOR`], and `hp` tops out at [`HP_START`], so
/// `0xC80 / 0x140 = 10` is the last reachable row.
pub const HEALTH_BONUS_ROWS: usize = 11;

impl BakaScoreTables {
    /// Read both bonus tables out of a loaded Baka Fighter overlay image.
    ///
    /// `overlay_0976` is the entry at [`BAKA_OVERLAY_BASE_VA`]. Returns `None`
    /// when the image is too short to hold either table, which is what keeps
    /// the fixed VAs honest on an entry that is not this one.
    ///
    /// The lengths are the index spaces their consumers can produce:
    /// [`BAKA_COMBO_MAX`]` + 1` combo rows and [`HEALTH_BONUS_ROWS`] health
    /// rows. Reading further would walk into the neighbouring rodata, which is
    /// how a table with no terminator gets over-read.
    pub fn from_overlay(overlay_0976: &[u8]) -> Option<Self> {
        let at = |va: u32| -> Option<usize> {
            let off = va.checked_sub(BAKA_OVERLAY_BASE_VA)? as usize;
            (off < overlay_0976.len()).then_some(off)
        };
        let combo_at = at(COMBO_BONUS_TABLE_VA)?;
        let health_at = at(HEALTH_BONUS_TABLE_VA)?;
        let combo_rows = (BAKA_COMBO_MAX + 1) as usize;
        let mut combo_bonus = Vec::with_capacity(combo_rows);
        for i in 0..combo_rows {
            let b = overlay_0976.get(combo_at + i * 4..combo_at + i * 4 + 4)?;
            combo_bonus.push(i32::from_le_bytes(b.try_into().ok()?));
        }
        let mut health_bonus = Vec::with_capacity(HEALTH_BONUS_ROWS);
        for i in 0..HEALTH_BONUS_ROWS {
            let b = overlay_0976.get(health_at + i * 2..health_at + i * 2 + 2)?;
            health_bonus.push(i16::from_le_bytes(b.try_into().ok()?));
        }
        Some(Self {
            combo_bonus,
            health_bonus,
        })
    }
}

impl BakaFight {
    /// Start a best-of-3 match: `player_cfg` in slot 0 (pad-driven),
    /// `opponent_cfg` in slot 1 (CPU picker).
    pub fn new(player_cfg: FighterConfig, opponent_cfg: FighterConfig, seed: u32) -> Self {
        Self {
            cfg: [player_cfg, opponent_cfg],
            f: [FighterState::new(), FighterState::new()],
            cues: Vec::new(),
            ai_controlled: [false, true],
            round: 0,
            settle_timer: 0,
            phase: MatchPhase::Fighting,
            rng: BiosRand::new(seed),
            last_exchange: None,
            tally: None,
            score_tables: None,
            max_combo: 0,
            score_rows: [0; 3],
            chrome: crate::baka_fighter_chrome::BakaChrome::default(),
            chrome_frame: crate::baka_fighter_chrome::ChromeFrame::default(),
            result_latch: false,
            player_untouched: true,
            cabinet: {
                let mut c = crate::baka_cabinet::BakaCabinet::new();
                c.enter_duel();
                c
            },
            cabinet_frame: crate::baka_cabinet::CabinetFrame::default(),
            tables: None,
            cabinet_pad: 0,
            strike: None,
            rate_divisor: STRIKE_RATE_DIVISOR,
            afterimages: Vec::new(),
            afterimage_frames: Vec::new(),
            roster_clips: None,
            motion: Default::default(),
            camera: Default::default(),
            stand_off: [0; 2],
            stand_z: [0; 2],
            special_cameras: Vec::new(),
            held_pad: 0,
            setup_pending: true,
            select_clock: 0,
            cameo: None,
            special_latch: false,
            impact: crate::baka_impact_fx::ImpactFx::default(),
        }
    }

    /// Install the impact pair's four spawn templates
    /// ([`crate::baka_impact_fx::ImpactTemplates::from_overlay`]). Without
    /// them a decided exchange spawns no effect.
    pub fn with_impact_templates(mut self, t: crate::baka_impact_fx::ImpactTemplates) -> Self {
        self.impact = crate::baka_impact_fx::ImpactFx::with_templates(t);
        self
    }

    /// [`Self::with_impact_templates`] off the as-loaded PROT 0976 image; a
    /// no-op when the templates do not read.
    pub fn with_impact_overlay(self, overlay_0976: &[u8]) -> Self {
        match crate::baka_impact_fx::ImpactTemplates::from_overlay(overlay_0976) {
            Some(t) => self.with_impact_templates(t),
            None => self,
        }
    }

    /// The live impact effect parts.
    pub fn impact_fx(&self) -> &crate::baka_impact_fx::ImpactFx {
        &self.impact
    }

    /// `FUN_801D4DF8`'s call on the booking arm for `slot`: the pair spawns
    /// at the fighter's position offset by the current action's strike TRS -
    /// the landed sub-keyframe, or slot `0` when `reset_keyframe` (the
    /// draw's calls). A fight without strike tables has no offsets and
    /// spawns nothing.
    fn spawn_impact(&mut self, slot: usize, reset_keyframe: bool) {
        let Some(st) = self.strike.as_ref() else {
            return;
        };
        let Some(attack) = self.f[slot].chosen else {
            return;
        };
        let action = StrikeTable::action_of(attack);
        let live = self.f[slot].clock.landed.map_or(-1, |i| i as i32);
        let k = crate::baka_fighter_chrome::impact_keyframe_index(live, reset_keyframe);
        let Some(off) = usize::try_from(k)
            .ok()
            .and_then(|k| st[slot].offsets.get(action)?.get(k).copied())
        else {
            return;
        };
        let p = self.fighter_position(slot);
        let pos = (p[0] as i16, p[1] as i16, p[2] as i16);
        // The block's facing word `+0x28`: clear for the player's seat, set
        // for the opponent's (the round setup's `0x801CFFF8` / `0x801CFFFC`).
        let facing = slot & 1 == 1;
        let spawns = crate::baka_fighter_chrome::impact_effect_pair(
            slot,
            pos,
            (off[0], off[1], off[2]),
            facing,
            self.special_latch,
        );
        self.impact.spawn_pair(&spawns);
    }

    /// Install both fighters' strike data (action-record speed + strike
    /// frames), switching exchange booking to the retail keyframe gate.
    pub fn with_strike_tables(mut self, tables: [StrikeTable; 2]) -> Self {
        self.strike = Some(tables);
        self
    }

    /// A fighter's strike clock, for a host that paces the swing off it.
    pub fn strike_clock(&self, slot: usize) -> StrikeClock {
        self.f[slot].clock
    }

    /// Stage one fighter's clip headers (its ANM bank's records for actions
    /// `0..9`), so the strike clock honours the double-step bit and the
    /// afterimage expires on the special clip's real end. A no-op on a fight
    /// without strike tables.
    pub fn set_clip_headers(&mut self, slot: usize, clips: ClipHeaders) {
        if let Some(st) = self.strike.as_mut()
            && let Some(t) = st.get_mut(slot)
        {
            t.clips = clips;
        }
    }

    /// Stage every roster fighter's clip headers ([`roster_clip_headers`]):
    /// both seats take theirs now, and each rung the cabinet installs takes
    /// its own.
    pub fn with_roster_clip_headers(mut self, headers: Vec<ClipHeaders>) -> Self {
        for slot in 0..2 {
            if let Some(h) = headers.get(self.cfg[slot].roster_id) {
                self.set_clip_headers(slot, *h);
            }
        }
        self.roster_clips = Some(std::sync::Arc::new(headers));
        self
    }

    /// The frame-rate divisor both fighters' clips step by (`DAT_1F80037D`).
    pub fn rate_divisor(&self) -> i32 {
        self.rate_divisor
    }

    /// A seat's display clip ([`crate::baka_duel::FighterMotion`]).
    pub fn motion(&self, slot: usize) -> crate::baka_duel::FighterMotion {
        self.motion[slot & 1]
    }

    /// The arena camera.
    pub fn duel_camera(&self) -> &crate::baka_duel::DuelCamera {
        &self.camera
    }

    /// Stage the player's special-commit camera glides
    /// ([`crate::baka_duel::parse_special_cameras`]). Without them a
    /// player special leaves the camera where it is.
    pub fn with_special_cameras(mut self, rows: Vec<[i16; 20]>) -> Self {
        self.special_cameras = rows;
        self
    }

    /// Hand the duel this frame's **packed** held pad word (`_DAT_8007B850`,
    /// Legaia's layout). Its one reader is the round setup's cameo test
    /// (`0x801D0190..0x801D01C4`): Triangle held (`0x10`) at a round setup
    /// sends the ring girl on.
    pub fn set_held_pad(&mut self, packed: u16) {
        self.held_pad = packed;
    }

    /// The round-start cameo, while it is on stage.
    pub fn cameo(&self) -> Option<CameoActor> {
        self.cameo
    }

    /// The roster id in the player seat.
    pub fn player_roster(&self) -> usize {
        self.cfg[0].roster_id
    }

    /// A seat's world position: the round setup stands the player at
    /// `X = -(stand_off + 200)` and the opponent at `+(stand_off + 200)`, on
    /// the `Y = Z = 0` duel line (`0x801D005C..0x801D00F4`).
    pub fn fighter_position(&self, slot: usize) -> [f32; 3] {
        let x = (self.stand_off[slot & 1] + 200) as f32;
        let z = self.stand_z[slot & 1] as f32;
        [if slot & 1 == 0 { -x } else { x }, 0.0, z]
    }

    /// A seat's yaw: the combat tick writes `+0x26 = 0x400` while the block's
    /// facing word `+0x28` is clear and `-0x400` while it is set
    /// (`0x801D4070..0x801D4084`); the round setup clears it for the player
    /// and sets it for the opponent (`0x801CFFF8` / `0x801CFFFC`).
    pub fn fighter_yaw(&self, slot: usize) -> i32 {
        if slot & 1 == 0 { 0x400 } else { -0x400 }
    }

    /// The afterimages the last tick drew: `(owner slot, frame)` per live
    /// actor, with each ghost pass's lagged cursor and depth-cue level.
    pub fn afterimages(&self) -> &[(usize, crate::baka_fighter_chrome::AfterimageFrame)] {
        &self.afterimage_frames
    }

    /// The combat tick's commit, for either side: the clock restarts, and a
    /// special halves the round's clip rate and spawns its afterimage
    /// (`0x801D4538..0x801D4634`).
    fn commit(&mut self, slot: usize, attack: BakaAttack) {
        self.f[slot].clock.commit();
        // The clip store: `+0x5C` = the attack's display id, `+0x68 = 0`
        // (`0x801D44D8` / `0x801D44DC`), held on its last frame only for the
        // special (`ori v0,v0,0x8` at `0x801D4640`).
        self.motion[slot].play(
            StrikeTable::action_of(attack),
            attack == BakaAttack::Special,
        );
        if attack == BakaAttack::Special {
            self.special_latch = true;
        }
        if attack == BakaAttack::Special && self.strike.is_some() {
            // The special's camera glide (`0x801D4644..0x801D4740`): the
            // player's row of the per-fighter table, the opponent's fixed
            // record.
            let record = if slot == 0 {
                self.special_cameras.get(self.cfg[0].roster_id).copied()
            } else {
                Some(crate::baka_duel::OPPONENT_SPECIAL_GLIDE)
            };
            if let Some(r) = record {
                self.camera.arm_glide(&r);
            }
        }
        if attack == BakaAttack::Special
            && let Some(st) = self.strike.as_ref()
        {
            self.rate_divisor = crate::baka_fighter_chrome::SPECIAL_RATE_DIVISOR;
            let speed = st[slot].speed[legaia_asset::baka_opponents::ACTION_SPECIAL];
            self.afterimages
                .push(crate::baka_fighter_chrome::AfterimageActor::spawn(
                    slot, speed,
                ));
        }
    }

    /// Step every live afterimage one frame (retail's actor pool runs the
    /// `0x801D7684` callback once per frame per actor).
    fn tick_afterimages(&mut self, frame_step: i32) {
        use legaia_asset::baka_opponents::ACTION_SPECIAL;
        self.afterimage_frames.clear();
        let strike = self.strike.as_ref();
        let chosen = [self.f[0].chosen, self.f[1].chosen];
        let mut out = Vec::new();
        self.afterimages.retain_mut(|a| {
            let clip = strike
                .and_then(|st| st[a.owner].clips[ACTION_SPECIAL])
                .map(|c| (c.n, c.frames));
            // With no clip header nothing can expire the ghosts; they go with
            // the special's own exchange instead.
            if clip.is_none() && chosen[a.owner] != Some(BakaAttack::Special) {
                return false;
            }
            let f = a.tick(frame_step, clip);
            if f.retire {
                return false;
            }
            out.push((a.owner, f));
            true
        });
        self.afterimage_frames = out;
    }

    /// Hand the cabinet this frame's **packed** pad edge (`_DAT_8007B874`,
    /// Legaia's layout - `baka_cabinet::CABINET_*`).
    ///
    /// The cabinet reads it only once the match is decided **and** it has
    /// left its duel state: the tally, the "NEXT GAME / PAY OUT" choice and
    /// the exit. While the cabinet is in the duel state the port keeps
    /// feeding zero, because that state's one pad read is the pause edge
    /// `0x110`, and Triangle - one of its bits - is the button the round
    /// setup's cameo test reads held; the in-duel pause menu stays unreached
    /// on every host. The port's cabinet stays in the duel state up to `0xB5`
    /// frames past the deciding exchange (its round timer is its own), so
    /// gating on the match alone let a Triangle pressed in that window open
    /// the pause menu.
    pub fn set_cabinet_pad(&mut self, edge: u16) {
        self.cabinet_pad = edge;
    }

    /// The roster id of the fighter in the opponent seat.
    pub fn opponent_roster(&self) -> usize {
        self.cfg[1].roster_id
    }

    /// Seat the next rung's opponent, the port side of the cabinet's
    /// `ST_OPPONENT_INSTALL` (`0x1E`): the roster record the rung fold names
    /// replaces slot 1, and both fighters start a fresh match (retail's round
    /// setup `0x32` refills HP to `0xC80` and clears the bracket). The pot is
    /// untouched - it is the global accumulator, not the fight's.
    fn install_rung(&mut self, roster: usize) {
        if let Some(t) = self.tables.clone()
            && let (Some(opp), Some(act)) = (t.0.get(roster), t.1.get(roster))
        {
            self.cfg[1] = FighterConfig::from_tables(opp, act);
            if let Some(st) = self.strike.as_mut() {
                st[1] = StrikeTable::from_actions(act);
                if let Some(h) = self.roster_clips.as_ref().and_then(|r| r.get(roster)) {
                    st[1].clips = *h;
                }
            }
        }
        if let Some(opp) = self.tables.as_ref().and_then(|t| t.0.get(roster)) {
            self.stand_off[1] = i32::from(opp.stand_off);
        }
        self.f = [FighterState::new(), FighterState::new()];
        self.motion = Default::default();
        self.stand_z = [0; 2];
        self.camera.round_setup();
        self.setup_pending = true;
        self.round = 0;
        self.rate_divisor = STRIKE_RATE_DIVISOR;
        self.special_latch = false;
        self.afterimages.clear();
        self.afterimage_frames.clear();
        self.settle_timer = 0;
        self.phase = MatchPhase::Fighting;
        self.last_exchange = None;
        self.tally = None;
        self.score_rows = [0; 3];
    }

    /// Supply the two overlay score-bonus tables so the per-round score rows
    /// accumulate. Without them every [`baka_round_score`] lookup misses and
    /// the rows stay empty.
    pub fn with_score_tables(mut self, tables: BakaScoreTables) -> Self {
        self.score_tables = Some(tables);
        self
    }

    /// Arm the cabinet's intro title card so the first ticks run it.
    pub fn with_intro_card(mut self) -> Self {
        self.chrome = crate::baka_fighter_chrome::BakaChrome::with_intro();
        self
    }

    /// The announcer lines to stage ahead of use
    /// ([`crate::baka_fighter_chrome::BakaChrome::take_xa_prestage`]): the
    /// chrome's whole list on the first call, the next round's banner line
    /// on each round advance. Both play hosts drain it every duel tick.
    pub fn take_xa_prestage(&mut self) -> Vec<crate::baka_fighter_chrome::XaCue> {
        self.chrome.take_xa_prestage(self.round as i32)
    }

    /// The chrome frame the last tick produced - the round banner, countdown
    /// and title-card draws plus any announcer line they fired.
    pub fn chrome_frame(&self) -> &crate::baka_fighter_chrome::ChromeFrame {
        &self.chrome_frame
    }

    /// The chrome runner itself, for a host that wants its sprite pool.
    pub fn chrome(&self) -> &crate::baka_fighter_chrome::BakaChrome {
        &self.chrome
    }

    /// The cabinet frame the last tick produced - the shell state, the
    /// arena / HUD draw flags and the HUD layout
    /// ([`crate::baka_cabinet::BakaCabinet`], the `FUN_801CF388` port).
    pub fn cabinet_frame(&self) -> &crate::baka_cabinet::CabinetFrame {
        &self.cabinet_frame
    }

    /// The cabinet shell itself, for a host that wants its stage counter, its
    /// secret-opponent override or its prize pot.
    pub fn cabinet(&self) -> &crate::baka_cabinet::BakaCabinet {
        &self.cabinet
    }

    /// Mutable access to the cabinet shell, for a host driving the ladder
    /// (installing the running high score, seeding the stage, banking a prize).
    pub fn cabinet_mut(&mut self) -> &mut crate::baka_cabinet::BakaCabinet {
        &mut self.cabinet
    }

    /// Install the parsed action tables into the cabinet so its developer
    /// editor state has something to dump (`FUN_801D553C`).
    pub fn with_action_tables(
        mut self,
        tables: Vec<legaia_asset::baka_opponents::BakaActionSet>,
    ) -> Self {
        self.cabinet = std::mem::take(&mut self.cabinet).with_action_tables(tables);
        self.cabinet.enter_duel();
        self
    }

    /// Step the cabinet shell one frame off the duel's own live state.
    ///
    /// REF: FUN_801cf388 - retail's dispatcher reads exactly these globals
    /// each frame (`DAT_801DBFF0` / `DAT_801DC098` round wins,
    /// `DAT_801DBED0` the target, the per-slot HP and hits-taken counters for
    /// the HUD pass), so the port feeds them across rather than duplicating
    /// them inside the cabinet.
    fn tick_cabinet(&mut self, frame_step: i32) {
        let input = crate::baka_cabinet::CabinetInput {
            frame_step,
            // The host's packed edge, but only once the match is decided and
            // the cabinet has left the duel state (see `set_cabinet_pad`): the
            // duel band's one read is the pause edge `0x110`, which overlaps
            // the cameo's held Triangle, and the port's cabinet sits in the
            // duel state for up to `0xB5` frames after the deciding exchange.
            pad_edge: if self.cabinet.front_end()
                || matches!(self.phase, MatchPhase::MatchOver(_))
                    && self.cabinet.state() != crate::baka_cabinet::ST_DUEL
            {
                std::mem::take(&mut self.cabinet_pad)
            } else {
                self.cabinet_pad = 0;
                0
            },
            pad_edge_alt: 0,
            pad_held: 0,
            dev_menu_enabled: false,
            player_round_wins: self.f[0].round_wins,
            opponent_round_wins: self.f[1].round_wins,
            win_target: ROUND_WIN_TARGET,
            rung_prize: self.cfg[1].gold_reward,
            hp: [self.f[0].hp, self.f[1].hp],
            combo_taken: [self.f[0].combo, self.f[1].combo],
            // `FUN_801D3468` advances the round timer only once the round is
            // decided (a finisher flag or both HPs at zero) - in the port,
            // once the exchange that decided it has left the fight phase.
            round_clock: !matches!(self.phase, MatchPhase::Fighting),
        };
        self.cabinet_frame = self.cabinet.tick(&input);
        self.cues.append(&mut self.cabinet_frame.cues);
        if let Some(roster) = self
            .cabinet_frame
            .install_player
            .and_then(|r| usize::try_from(r).ok())
        {
            self.install_player(roster);
        }
        if let Some((roster, _mesh)) = self.cabinet_frame.install_opponent
            && let Ok(roster) = usize::try_from(roster)
        {
            self.install_rung(roster);
        }
        // The intro title card belongs to the cabinet, not to the duel: both
        // `jal 0x801D59D4` sites in PROT 0976 are attract arms of this same
        // state machine, and those arms are what advance its clock. Hand the
        // clock to the chrome while the cabinet is on one of them, so the card
        // runs for any host that enters at the cabinet's own boot rather than
        // only for one that armed a private timeline. The arms call the card
        // only on the frames they draw it (the fade-out stops after `0x1E`),
        // so the chrome takes exactly what this frame called.
        if self.cabinet.front_end() || self.cabinet_frame.title_card.is_some() {
            self.chrome.set_intro_clock(self.cabinet_frame.title_card);
        }
    }

    /// Seat the player-select pick in slot 0 - the port side of state `0x0E`,
    /// which spawns the chosen fighter's actor with the select cursor as its
    /// roster id (`sh a0,0x5a(t1)` at `0x801CFC50`) and reads that record's
    /// stand-off (`0x801CFC40`). The pick brings its own stats, action table,
    /// strike clips and special camera; the opponent install that follows
    /// starts the match.
    fn install_player(&mut self, roster: usize) {
        let Some(t) = self.tables.clone() else {
            return;
        };
        let (Some(rec), Some(act)) = (t.0.get(roster), t.1.get(roster)) else {
            return;
        };
        self.cfg[0] = FighterConfig::from_tables(rec, act);
        if let Some(st) = self.strike.as_mut() {
            st[0] = StrikeTable::from_actions(act);
            if let Some(h) = self.roster_clips.as_ref().and_then(|r| r.get(roster)) {
                st[0].clips = *h;
            }
        }
        self.stand_off[0] = i32::from(rec.stand_off);
    }

    /// The player-select lineup while it is on stage: the select cursor
    /// (`DAT_801DBF70`) and the frames the three idle clips have run. `None`
    /// outside states `0x0A` / `0x0B`.
    pub fn select_lineup(&self) -> Option<(usize, i32)> {
        crate::baka_cabinet::lineup_live(self.cabinet.state()).then(|| {
            (
                usize::try_from(self.cabinet.select_cursor()).unwrap_or(0),
                self.select_clock,
            )
        })
    }

    /// The cabinet's own widget draws this frame - the attract prompt, the
    /// "PLAYER SELECT" banner and the "NEXT GAME / PAY OUT" sheet - as one
    /// list every host draws the same way.
    pub fn cabinet_cells(&self) -> Vec<crate::baka_cabinet::SheetCell> {
        let mut out = self.cabinet_frame.widgets.clone();
        if let Some(sheet) = self.cabinet.choice_sheet() {
            out.extend_from_slice(&sheet);
        }
        out
    }

    /// Start the cabinet at its **boot** state instead of mid-duel, so the
    /// attract sequence - and with it the intro title card - runs before the
    /// fight. Retail's cabinet always enters here; the port's hosts enter
    /// through the duel, which is why the card had no production caller.
    pub fn with_attract(mut self) -> Self {
        self.cabinet.reboot();
        self
    }

    /// The running maximum player hit streak (`DAT_801dbec8`).
    pub fn max_combo(&self) -> i32 {
        self.max_combo
    }

    /// The three accumulated score rows.
    pub fn score_rows(&self) -> [i32; 3] {
        self.score_rows
    }

    /// Build both fighters straight from the parsed overlay tables. `None`
    /// when either roster id is out of range.
    pub fn from_tables(
        opponents: &[BakaOpponent],
        actions: &[BakaActionSet],
        player_roster: usize,
        opponent_roster: usize,
        seed: u32,
    ) -> Option<Self> {
        let p =
            FighterConfig::from_tables(opponents.get(player_roster)?, actions.get(player_roster)?);
        let o = FighterConfig::from_tables(
            opponents.get(opponent_roster)?,
            actions.get(opponent_roster)?,
        );
        // The cabinet's developer editor dumps these same tables, so a duel
        // built from the disc hands them straight over; the fight keeps both
        // for the between-rung installs.
        let strike = [
            StrikeTable::from_actions(&actions[player_roster]),
            StrikeTable::from_actions(&actions[opponent_roster]),
        ];
        let mut fight = Self::new(p, o, seed)
            .with_action_tables(actions.to_vec())
            .with_strike_tables(strike);
        fight.stand_off = [
            i32::from(opponents[player_roster].stand_off),
            i32::from(opponents[opponent_roster].stand_off),
        ];
        fight.tables = Some(std::sync::Arc::new((opponents.to_vec(), actions.to_vec())));
        Some(fight)
    }

    /// Current match phase.
    pub fn phase(&self) -> MatchPhase {
        self.phase
    }

    /// Round index (0-based).
    pub fn round(&self) -> u32 {
        self.round
    }

    /// A fighter's current HP.
    pub fn hp(&self, slot: usize) -> i32 {
        self.f[slot].hp
    }

    /// A fighter's round-win count.
    pub fn round_wins(&self, slot: usize) -> u32 {
        self.f[slot].round_wins
    }

    /// A fighter's consecutive-hits-taken combo counter.
    pub fn combo(&self, slot: usize) -> i32 {
        self.f[slot].combo
    }

    /// A fighter's chosen attack this exchange, if any.
    pub fn chosen(&self, slot: usize) -> Option<BakaAttack> {
        self.f[slot].chosen
    }

    /// Whether a fighter can commit an attack right now (fighting phase, no
    /// choice pending, cooldown elapsed).
    pub fn can_choose(&self, slot: usize) -> bool {
        self.phase == MatchPhase::Fighting
            && self.f[slot].hp > 0
            && self.f[slot].chosen.is_none()
            && !self.f[slot].committed
            && self.f[slot].cooldown <= 0
    }

    /// The last resolved exchange, for the host HUD.
    /// Drain the SFX cue ids the fight queued since the last call, in fire
    /// order (retail's cue-ring writes; see [`BAKA_CUE_HIT`]). Hosts route
    /// each through their SFX bank - the site's arts/minigame pages resolve
    /// them against the disc's class-2 sound bank.
    pub fn take_cues(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.cues)
    }

    /// Cue ids queued but not yet drained.
    pub fn pending_cues(&self) -> &[u8] {
        &self.cues
    }

    pub fn last_exchange(&self) -> Option<ExchangeReport> {
        self.last_exchange
    }

    /// The end-of-match tally, once the player has taken the match.
    pub fn tally(&self) -> Option<&BakaTally> {
        self.tally.as_ref()
    }

    /// Take the coins the tally has drained since the last call, for the
    /// host to add to party gold. `0` while no tally is running.
    pub fn take_tally_gold(&mut self) -> i32 {
        self.tally.as_mut().map(BakaTally::take_gold).unwrap_or(0)
    }

    /// One frame of the whole cabinet on this frame's **packed** pad words
    /// (`edge` = `_DAT_8007B874`, `held` = `_DAT_8007B850`) - the per-frame
    /// step every host runs, so the play hosts' world tick and the standalone
    /// minigames page drive one ladder, one tally and one input rule.
    ///
    /// - The front end (attract card, player select) reads the edge itself.
    /// - After a match the cabinet runs its tally, the "NEXT GAME / PAY OUT"
    ///   sheet, the next rung or the game-over / all-clear sequence off the
    ///   edge, with `pot` - the host's winnings accumulator - as the coins at
    ///   risk; any face button fast-forwards the tally.
    /// - In a duel, the frame's throw is retail's last-write-wins test read
    ///   back to front (Cross, then Circle, then Square), and the held word
    ///   feeds the round setup's cameo test.
    ///
    /// The returned outcome is what the host's accumulator does: add
    /// [`BakaFrameOutcome::paid`], clear it on a forfeit, and leave the
    /// cabinet on [`BakaFrameOutcome::exit`].
    pub fn frame(&mut self, edge: u16, held: u16, pot: u32) -> BakaFrameOutcome {
        use legaia_engine_vm::pad::{PACK_CIRCLE, PACK_CROSS, PACK_SQUARE, PACK_TRIANGLE};
        if self.cabinet.front_end() {
            self.set_cabinet_pad(edge);
            self.tick(1);
            return BakaFrameOutcome::default();
        }
        if self.match_over() {
            let face = edge & (PACK_TRIANGLE | PACK_CIRCLE | PACK_CROSS | PACK_SQUARE) != 0;
            self.cabinet.set_pot(pot);
            self.set_cabinet_pad(edge);
            self.tick_with_input(1, face);
            return BakaFrameOutcome {
                paid: self.take_tally_gold().max(0) as u32,
                forfeit: self.cabinet_frame.forfeit.is_some(),
                exit: self.cabinet.exit_done(),
            };
        }
        let attack = if edge & PACK_CROSS != 0 {
            Some(BakaAttack::C)
        } else if edge & PACK_CIRCLE != 0 {
            Some(BakaAttack::B)
        } else if edge & PACK_SQUARE != 0 {
            Some(BakaAttack::A)
        } else {
            None
        };
        if let Some(attack) = attack {
            self.choose(0, attack);
        }
        self.set_held_pad(held);
        self.tick(1);
        BakaFrameOutcome::default()
    }

    /// Coins the tally has not paid out yet - what a host owes the player if
    /// the duel is left before the tally finishes. `0` when no prize is due
    /// (a lost match) or the tally has fully drained.
    pub fn tally_gold_remaining(&self) -> i32 {
        self.tally
            .as_ref()
            .map(BakaTally::gold_remaining)
            .unwrap_or(0)
    }

    /// Gold prize for beating the slot-1 opponent (roster record `+0x20`).
    pub fn gold_reward(&self) -> u32 {
        self.cfg[1].gold_reward
    }

    /// The match winner, once decided.
    pub fn winner(&self) -> Option<usize> {
        match self.phase {
            MatchPhase::MatchOver(w) => Some(w),
            _ => None,
        }
    }

    /// `true` once the match is decided.
    pub fn match_over(&self) -> bool {
        matches!(self.phase, MatchPhase::MatchOver(_))
    }

    /// Commit an attack for `slot` this exchange. Returns `false` (ignored)
    /// while the fighter can't act - see [`Self::can_choose`] - and always
    /// for [`BakaAttack::Special`]: type 4 is not a button. Retail's combat
    /// tick throws it on its own, as the finisher against a foe already at
    /// 0 HP (see [`Self::finish_or_end_round`]).
    pub fn choose(&mut self, slot: usize, attack: BakaAttack) -> bool {
        if attack == BakaAttack::Special || !self.can_choose(slot) {
            return false;
        }
        self.f[slot].chosen = Some(attack);
        self.commit(slot, attack);
        true
    }

    /// The CPU move pick: random attack or a backward step of the scripted
    /// pattern, exactly as the retail picker rolls it.
    ///
    /// PORT: FUN_801d487c (opponent AI move picker)
    fn ai_pick(&mut self, slot: usize) -> BakaAttack {
        let roll = self.rng.next_u15() as i32;
        let r6 = roll % 6;
        let pattern_len = self.cfg[slot].ai_pattern.len();
        if r6 < 3 {
            if self.f[slot].ai_cursor == 0 {
                return BakaAttack::from_type_id((r6 % 3) as u8 + 1).unwrap();
            }
        } else if self.f[slot].ai_cursor == 0 {
            // Seed the cursor to the pattern length; the pattern then plays
            // back-to-front to exhaustion.
            if pattern_len > 0 && self.cfg[slot].ai_pattern[0] != 0 {
                self.f[slot].ai_cursor = pattern_len;
            }
            if self.f[slot].ai_cursor == 0 {
                return BakaAttack::from_type_id((r6 % 3) as u8 + 1).unwrap();
            }
        }
        self.f[slot].ai_cursor -= 1;
        let sym = self.cfg[slot].ai_pattern[self.f[slot].ai_cursor];
        BakaAttack::from_type_id((sym - 1) % 3 + 1).unwrap()
    }

    /// Resolve the current exchange.
    ///
    /// PORT: FUN_801d3a14 (exchange win-condition: settle timer, special
    /// priority, committed gates, and the 2>1 / 3>2 / 1>3 beats relation)
    fn resolve(&mut self, frame_step: i32) -> ExchangeOutcome {
        // Settle timer: while it hasn't elapsed the exchange stays open.
        if self.settle_timer - frame_step >= 0 {
            self.settle_timer -= frame_step;
            return ExchangeOutcome::Undecided;
        }
        self.settle_timer = 0;
        let p1 = self.f[0].chosen.map(BakaAttack::type_id).unwrap_or(0);
        let p2 = self.f[1].chosen.map(BakaAttack::type_id).unwrap_or(0);
        // The special is an unbeatable win, fighter 0 checked first - the
        // retail resolver's first two tests (`0x801D3A54` / `0x801D3A5C`: a
        // type of 4 on either side returns that side before any other
        // check). Only the auto-finisher ever throws it, and the keyframe
        // gate in the tick is what paces it.
        if p1 == 4 {
            return ExchangeOutcome::FighterWins(0);
        }
        if p2 == 4 {
            return ExchangeOutcome::FighterWins(1);
        }
        if p1 == 0 && p2 == 0 {
            return ExchangeOutcome::Undecided;
        }
        if self.f[0].committed || self.f[1].committed {
            return ExchangeOutcome::Undecided;
        }
        if p1 == p2 {
            return ExchangeOutcome::Draw;
        }
        // Beats relation from the dump: 2 beats 1, 3 beats 2, 1 beats 3.
        match (p1, p2) {
            (1, 2) | (2, 3) | (3, 1) => ExchangeOutcome::FighterWins(1),
            (2, 1) | (3, 2) | (1, 3) => ExchangeOutcome::FighterWins(0),
            // One side idle (type 0): an attack never lands on a non-attacker.
            _ => ExchangeOutcome::Undecided,
        }
    }

    /// Apply exchange damage to `loser`. Returns `(damage, critical,
    /// special_round_win)`.
    ///
    /// PORT: FUN_801d3b18 (damage application: HP-tiered ATK/DEF, combo bonus,
    /// crit override, special full-hit round win)
    fn apply_damage(&mut self, loser: usize) -> (i32, bool, bool) {
        let winner = loser ^ 1;
        if loser == 0 {
            // `sw zero,-0x40DC` in both arms that damage slot 0
            // (`0x801D375C`, `0x801D37B8`).
            self.player_untouched = false;
        }
        // The retail ring write (`_DAT_8007b6d8 = 9`) sits at the top of
        // FUN_801D3B18, before the damage arithmetic - so a double-KO draw
        // (which applies damage twice) queues the cue twice, as it does here.
        self.cues.push(BAKA_CUE_HIT);
        self.f[loser].hits_taken += 1;
        let winner_type = self.f[winner].chosen.map(BakaAttack::type_id).unwrap_or(0);

        // Special full-hit: the special scores the immediate round win only
        // when it lands on the action's final sub-keyframe - the winner's landed
        // sub-keyframe (block `+0x98`) is the special record's last
        // (`record[+0x1C] - 1`, `0x801D3C00..0x801D3C0C`). The damage kernel
        // also hands the winner's strike back to the lookup (`+0x0C = 2`,
        // `0x801D3EB0`), which is what lets the special's next strike land.
        let full_hit = self.strike.as_ref().is_some_and(|st| {
            let n = st[winner].frames[legaia_asset::baka_opponents::ACTION_SPECIAL].len();
            n > 0 && self.f[winner].clock.landed == Some(n - 1)
        });
        if self.f[winner].clock.state == StrikeState::Landed {
            self.f[winner].clock.state = StrikeState::Consumed;
        }
        let mut special_round_win = false;
        if winner_type == 4 && full_hit {
            self.f[winner].round_wins += 1;
            self.f[loser].committed = true;
            special_round_win = true;
        }

        let def_tier = self.cfg[loser].def_tier(self.f[loser].hp);
        let atk_tier = self.cfg[winner].atk_tier(self.f[winner].hp);
        let power = self.cfg[winner]
            .attack_power
            .get(winner_type as usize)
            .copied()
            .unwrap_or(0);
        let mod_ = self.cfg[loser].damage_mod;
        let combo = self.f[loser].combo;
        let hit = power + power * atk_tier / 100;
        let guard = mod_ + mod_ * def_tier / 100;
        let mut dmg = (hit * (200 - guard) * 0x20) / 100 + (combo - 1) * COMBO_DAMAGE_STEP;
        let critical = self.f[winner].crit_pending;
        if critical {
            dmg = power << 7;
        }
        // The special carries no HP payload. Its action record's `+0x18` power
        // is 0 for all 17 fighters (disc corpus), so retail's kernel would
        // compute `(combo-1)*0x40` = **-0x40** on a fresh combo - but retail
        // never applies it: type 4 is the auto-finisher, gated on the
        // opponent's HP already being 0 (`overlay_baka_fighter_801d3f44.txt`),
        // and the kernel's HP write is `hp > 0`-gated
        // (`overlay_baka_fighter_801d3b18.txt` `0x801d3e58..0x801d3e68`:
        // `blez` skips the `subu`). A special that wins against a foe still
        // standing (the other fighter's special, or a test driving the
        // resolver) would have the raw arithmetic *heal* the loser by 64;
        // the faithful HP delta for a special-won exchange is zero - its
        // payoff is the exchange / round win, never HP.
        if winner_type == 4 {
            dmg = 0;
        }
        if self.f[loser].hp > 0 {
            self.f[loser].hp -= dmg;
        }
        if self.f[loser].hp < 1 {
            self.f[loser].hp = 0;
        }
        self.f[loser].combo += 1;
        (dmg, critical, special_round_win)
    }

    /// Roll the comeback critical for a fighter that just took a hit: fires
    /// while `0 < HP < 0x280` on `rand() % 100 < crit_chance`.
    ///
    /// PORT: FUN_801d6660 (critical / lucky-hit roll)
    fn roll_comeback_crit(&mut self, slot: usize) {
        let hp = self.f[slot].hp;
        if hp > 0 && hp < CRIT_HP_BAND {
            let roll = self.rng.next_u15() as i32 % 100;
            if roll < self.cfg[slot].crit_chance {
                self.f[slot].crit_pending = true;
            }
        }
    }

    /// Clear the exchange state on both fighters (host pacing simplification:
    /// retail sequences the recovery through the per-action keyframes).
    fn end_exchange(&mut self) {
        for s in 0..2 {
            self.f[s].chosen = None;
            self.f[s].committed = false;
        }
    }

    /// A knockout: `winner` has just put the foe at 0 HP.
    ///
    /// With strike data this is retail's auto-finisher gate (own HP `!= 0`,
    /// foe HP `== 0`, round undecided - the slot-0 and slot-1 branches of
    /// `FUN_801d3f44` alike): the winner throws the special on its own, and
    /// the round is credited by its last strike landing, inside the damage
    /// kernel. A fight with no strike data, or a special with no strike
    /// frames to land, has no finisher to play and ends the round here.
    ///
    /// REF: FUN_801d3f44
    fn finish_or_end_round(&mut self, winner: usize) {
        let has_finisher = self.f[winner].hp > 0
            && self.strike.as_ref().is_some_and(|st| {
                !st[winner].frames[legaia_asset::baka_opponents::ACTION_SPECIAL].is_empty()
            });
        if !has_finisher {
            self.end_round(winner, false);
            return;
        }
        self.end_exchange();
        self.f[winner].chosen = Some(BakaAttack::Special);
        self.commit(winner, BakaAttack::Special);
    }

    /// End the current round with `winner` (KO path credits here; a landed
    /// full special already credited inside the damage kernel).
    fn end_round(&mut self, winner: usize, already_credited: bool) {
        if !already_credited {
            self.f[winner].round_wins += 1;
        }
        self.accumulate_round_score(winner);
        // The result banners rise first (retail's banner tail runs ahead of
        // the round setup), so the round banner asked for here waits out
        // their hold rather than drawing over them.
        self.tick_result_banner();
        self.chrome
            .start_round_banner(crate::baka_fighter_chrome::ROUND_BANNER_SPRITE);
        if self.f[winner].round_wins >= ROUND_WIN_TARGET {
            self.phase = MatchPhase::MatchOver(winner);
            if winner == 0 {
                // The retail end-of-match tally screen comes up on a player
                // win and drains the prize into gold.
                self.tally = Some(BakaTally::new([
                    self.score_rows[0],
                    self.score_rows[1],
                    self.score_rows[2],
                    self.cfg[1].gold_reward as i32,
                ]));
            }
        } else {
            self.phase = MatchPhase::RoundOver(winner);
        }
    }

    /// Fold this round's two bonus increments into the score rows, exactly
    /// as `FUN_801d2a28` does: the combo row takes the running-maximum
    /// streak's table entry, the bonus row the winner's end-HP entry.
    ///
    /// The score channel runs only when a host has supplied the two overlay
    /// bonus tables. Without them the port has no score channel at all - the
    /// rows stay empty and the tally carries the coin prize alone, which is
    /// the behaviour every disc-free oracle measures. Retail always has the
    /// tables, so supplying them is what turns the retail channel on.
    ///
    /// REF: FUN_801d2a28
    fn accumulate_round_score(&mut self, winner: usize) {
        let Some(tables) = self.score_tables.clone() else {
            return;
        };
        let gain = baka_round_score(
            self.max_combo,
            &tables.combo_bonus,
            self.f[winner].hp,
            &tables.health_bonus,
        );
        self.score_rows[1] += gain.combo_gain;
        self.score_rows[2] += gain.bonus_gain;
    }

    /// Advance the duel one frame: decay cooldowns, let the CPU pick, charge
    /// a held special, resolve the exchange, and book damage / rounds / the
    /// match, mirroring the retail resolution arm.
    ///
    /// PORT: FUN_801d3468 (round / match resolution state machine)
    pub fn tick(&mut self, frame_step: i32) {
        self.tick_with_input(frame_step, false);
    }

    /// [`Self::tick`] with the tally's fast-forward input: `face_button` is
    /// this frame's edge-triggered face-button test (`_DAT_8007b874 & 0xf0`),
    /// which snaps the end-of-match tally to its end state.
    pub fn tick_with_input(&mut self, frame_step: i32, face_button: bool) {
        self.tick_rules(frame_step, face_button);
        self.tick_presentation(frame_step);
    }

    /// The fighters' display clips and the arena camera, one frame: what the
    /// combat tick's clip store / idle reset and the clip selector do to each
    /// fighter actor (`FUN_801D3F44`, `FUN_800204F8`), and the camera spin /
    /// glide ([`crate::baka_duel::DuelCamera`]). Nothing here feeds the
    /// rules - the exchange books off the [`StrikeClock`].
    fn tick_presentation(&mut self, frame_step: i32) {
        use crate::baka_cabinet::{ST_CHOICE, ST_CHOICE_SECRET, ST_TALLY_OUT};
        use crate::baka_duel::MOTION_WIN;
        let st = self.cabinet.state();
        if crate::baka_cabinet::front_end(st) {
            // State `0x0A` writes the select camera (`0x801CF8F4..0x801CF924`)
            // as it spawns the lineup; the attract arms draw no 3D at all.
            if crate::baka_cabinet::lineup_live(st) {
                if self.select_clock == 0 {
                    self.camera.select_screen();
                }
                self.select_clock += frame_step;
            } else {
                self.select_clock = 0;
            }
            return;
        }
        if matches!(self.phase, MatchPhase::MatchOver(0))
            && matches!(st, ST_TALLY_OUT | ST_CHOICE | ST_CHOICE_SECRET)
            && !self.motion[0].pinned
        {
            // The tally's end (state `0x66` leaving, `0x801D0A38..0x801D0AB0`,
            // and the secret variant `0x6D` at `0x801D0FBC..0x801D1024`): the
            // result close-up, the player's win flourish held, and Noa one
            // step toward the camera.
            self.camera.result_close_up(st == ST_CHOICE_SECRET);
            self.motion[0].play(MOTION_WIN, true);
            self.motion[0].pinned = true;
            if self.cfg[0].roster_id == crate::baka_duel::RESULT_STEP_FIGHTER {
                self.stand_z[0] += crate::baka_duel::RESULT_STEP_Z;
            }
        }
        if std::mem::take(&mut self.setup_pending)
            && crate::baka_fighter_chrome::cameo_spawns(self.held_pad)
        {
            // The round setup's spawn of prototype `0x801D7624` (phase and
            // cursor zeroed by the allocator).
            self.cameo = Some(CameoActor {
                phase: 0,
                cursor: 0,
                clip: crate::baka_fighter_chrome::CAMEO_CLIP_WALK,
            });
        }
        self.tick_cameo(frame_step);
        if let Some(st) = self.strike.as_ref() {
            for (m, t) in self.motion.iter_mut().zip(st.iter()) {
                let r = m
                    .record
                    .min(legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER - 1);
                m.step(t.speed[r], self.rate_divisor, t.clips[r], frame_step);
            }
        }
        self.camera.tick(frame_step);
    }

    /// One frame of the cameo's actor: the animator's pose, the clip
    /// selector's advance at the forced step `+0x6A = 8`, then the phase
    /// advance by the frame step; the actor retires once the pose raises its
    /// retire bit.
    ///
    /// REF: FUN_801D6310
    fn tick_cameo(&mut self, frame_step: i32) {
        let Some(c) = self.cameo.as_mut() else {
            return;
        };
        let Some(pose) = crate::baka_fighter_chrome::cameo_pose(c.phase) else {
            self.cameo = None;
            return;
        };
        if pose.retire {
            self.cameo = None;
            return;
        }
        if pose.clip != c.clip {
            c.clip = pose.clip;
            c.cursor = 0;
        }
        c.cursor += CAMEO_STEP * frame_step;
        c.phase = c.phase.wrapping_add(frame_step as i16);
    }

    /// The rules half of [`Self::tick_with_input`], then the round-result
    /// banner tail the resolution SM runs after it.
    fn tick_rules(&mut self, frame_step: i32, face_button: bool) {
        self.tick_rules_body(frame_step, face_button);
        self.tick_result_banner();
    }

    /// The round-result banners (`FUN_801D3468`'s tail, `0x801D3864..`):
    /// once per round - the sub-state `DAT_801DBF84` - the first frame a
    /// fighter's HP is down, the chrome raises the banner the two HPs and
    /// the untouched flag name ([`crate::baka_fighter_chrome::RoundResult`]).
    /// They stand for the duel state's round-over hold
    /// ([`crate::baka_fighter_chrome::RESULT_HOLD_FRAMES`]) and the next
    /// round's banner waits for them, as retail draws it from the round
    /// setup after the hold; on the deciding round they stand through the
    /// win flourish / loss / GAME OVER screens and come down when the
    /// cabinet moves on to its tally. Retail's lifetime is the spawn
    /// template's (`0x801DB9C4`), which the port does not run, so the hold
    /// is the cabinet's state timer. The next round's start re-arms the latch
    /// and the untouched flag.
    ///
    /// PORT: FUN_801d3468 (`0x801D3864..0x801D39F0`)
    fn tick_result_banner(&mut self) {
        use crate::baka_cabinet::{ST_DUEL, ST_GAME_OVER, ST_LOSE, ST_PERFECT};
        use crate::baka_fighter_chrome::RoundResult;
        // Past the decided match (the tally, the sheet, the next rung) the
        // cabinet's own screens own the frame.
        if matches!(self.phase, MatchPhase::MatchOver(_))
            && !matches!(
                self.cabinet.state(),
                ST_DUEL | ST_PERFECT | ST_LOSE | ST_GAME_OVER
            )
        {
            self.chrome.clear_result();
            return;
        }
        if self.result_latch || self.cabinet.front_end() {
            return;
        }
        let result = match (self.f[0].hp <= 0, self.f[1].hp <= 0) {
            (true, true) => RoundResult::Draw,
            (true, false) => RoundResult::Lose,
            (false, true) if self.player_untouched => RoundResult::Perfect,
            (false, true) => RoundResult::Win,
            (false, false) => return,
        };
        self.chrome.raise_result(result);
        self.result_latch = true;
    }

    fn tick_rules_body(&mut self, frame_step: i32, face_button: bool) {
        // The HUD renderer latches the running maximum once per frame, so it
        // runs whatever the phase.
        if self.max_combo <= self.f[1].combo {
            self.max_combo = self.f[1].combo;
        }
        let chrome_tick = crate::baka_fighter_chrome::ChromeTick {
            frame_step,
            loading: false,
            match_phase: i32::from(matches!(self.phase, MatchPhase::MatchOver(_))),
            round: self.round as i32,
            round_counter: self.round as i32,
            focus: (0, 1),
        };
        self.chrome_frame = self.chrome.step(&chrome_tick, (&[], &[]));
        self.tick_cabinet(frame_step);
        if self.cabinet.front_end() {
            // Attract and player select: retail spawns the round SM only at
            // `0x0E`, so nothing of the fight runs yet.
            return;
        }
        match self.phase {
            MatchPhase::MatchOver(_) => {
                // The match is decided: the tally screen runs (FUN_801d239c).
                if let Some(t) = self.tally.as_mut() {
                    t.tick(frame_step, face_button);
                    self.cues.extend(t.take_cues());
                }
                return;
            }
            MatchPhase::RoundOver(_) => {
                // The round stays decided - fighters, HP and the result
                // banner as they were - until the cabinet's duel state has
                // run the round timer to `0xB5` and moved to its round setup
                // (`0x801D0620..0x801D063C`, the timer zeroed with it); the
                // next round starts there.
                if self.cabinet.state() == crate::baka_cabinet::ST_DUEL {
                    return;
                }
                self.round += 1;
                self.f[0].reset_round();
                self.f[1].reset_round();
                // The round setup (`0x32`) restores the clip rate the
                // special lowered (`0x801D01A4`), seeds both clips back to
                // the idle (`0x801CFFB0` / `0x801CFFC0`) and snaps the camera.
                self.rate_divisor = STRIKE_RATE_DIVISOR;
                self.special_latch = false;
                // The banner sub-state and the untouched flag re-arm with the
                // round (`DAT_801DBF84`, `DAT_801DBF24`).
                self.result_latch = false;
                self.player_untouched = true;
                self.motion = Default::default();
                self.camera.round_setup();
                self.setup_pending = true;
                self.phase = MatchPhase::Fighting;
                return;
            }
            MatchPhase::Fighting => {}
        }

        // Cooldown decay (`cooldown -= frame_step * 0x10`, floored at 0).
        for s in 0..2 {
            if self.f[s].cooldown > 0 {
                self.f[s].cooldown -= frame_step * COOLDOWN_DECAY;
            }
            if self.f[s].cooldown < 0 {
                self.f[s].cooldown = 0;
            }
        }

        // CPU commits once its cooldown elapses.
        for s in 0..2 {
            if self.ai_controlled[s] && self.can_choose(s) {
                let pick = self.ai_pick(s);
                self.f[s].chosen = Some(pick);
                self.commit(s, pick);
            }
        }

        // The strike clocks (retail's per-fighter combat tick, which runs
        // before the resolution SM reads the `+0x0C` words it leaves).
        if let Some(st) = self.strike.as_ref() {
            for (fs, table) in self.f.iter_mut().zip(st.iter()) {
                if let Some(attack) = fs.chosen {
                    let a = StrikeTable::action_of(attack);
                    fs.clock.step(
                        &table.frames[a],
                        table.speed[a],
                        self.rate_divisor,
                        table.clips[a],
                        frame_step,
                    );
                }
            }
        }
        self.tick_afterimages(frame_step);
        // The impact parts ride the actor-pool walk at the same
        // `frame step x DAT_1F80037D` factor as every part tick.
        let delta = (frame_step.max(0) * self.rate_divisor.max(0)).min(i32::from(u16::MAX));
        self.impact.tick(delta as u16);

        let outcome = self.resolve(frame_step);
        // The keyframe gate: the resolution SM books a decided exchange only
        // while the winner's strike has landed (`0x801D36DC` / `0x801D3730`),
        // and a draw while either side's has (`0x801D378C..0x801D37A4`).
        let outcome = match (self.strike.is_some(), outcome) {
            (true, ExchangeOutcome::FighterWins(w))
                if self.f[w].clock.state != StrikeState::Landed =>
            {
                ExchangeOutcome::Undecided
            }
            (true, ExchangeOutcome::Draw)
                if self.f[0].clock.state != StrikeState::Landed
                    && self.f[1].clock.state != StrikeState::Landed =>
            {
                ExchangeOutcome::Undecided
            }
            (_, o) => o,
        };
        match outcome {
            ExchangeOutcome::Undecided => {}
            ExchangeOutcome::FighterWins(w) => {
                let l = w ^ 1;
                // Phase gate: the winner must actually be mid-attack.
                if self.f[w].chosen.is_none() {
                    return;
                }
                // The booking arm's impact pair runs ahead of the damage
                // kernel (`0x801D36F0` / `0x801D3744`).
                self.spawn_impact(w, false);
                let (damage, critical, special_round_win) = self.apply_damage(l);
                // The damage kernel's clip store on the struck side: the hit
                // reaction, or the knockdown when the special's last strike
                // landed, held on its last frame (`0x801D3C60..0x801D3CA0`).
                self.motion[l].play(
                    if special_round_win {
                        crate::baka_duel::MOTION_KNOCKDOWN
                    } else {
                        crate::baka_duel::MOTION_HIT
                    },
                    true,
                );
                self.motion[l].down = special_round_win;
                // Winner's own hit streak clears; crit flags reset; the loser
                // rolls the comeback crit (retail: FUN_801d6660(loser)).
                self.f[w].combo = 0;
                self.f[0].crit_pending = false;
                self.f[1].crit_pending = false;
                self.roll_comeback_crit(l);
                // Cooldowns: fighter-0 win leaves slot 0 free (retail writes
                // 0/200); a fighter-1 win slows both (200/200).
                if w == 0 {
                    self.f[0].cooldown = 0;
                    self.f[1].cooldown = COOLDOWN_RESET;
                } else {
                    self.f[0].cooldown = COOLDOWN_RESET;
                    self.f[1].cooldown = COOLDOWN_RESET;
                }
                // A special that has strikes left keeps playing: retail's
                // clip runs on and its next sub-keyframe lands again (the
                // kernel re-armed it). Only the struck side's exchange ends.
                let special_continues = self.strike.as_ref().is_some_and(|st| {
                    self.f[w].chosen == Some(BakaAttack::Special)
                        && !special_round_win
                        && self.f[w].clock.landed.map_or(0, |i| i + 1)
                            < st[w].frames[legaia_asset::baka_opponents::ACTION_SPECIAL].len()
                });
                if special_continues {
                    self.f[l].chosen = None;
                    self.f[l].committed = false;
                } else {
                    self.end_exchange();
                }
                self.last_exchange = Some(ExchangeReport {
                    winner: w,
                    draw: false,
                    damage,
                    critical,
                    special_round_win,
                });
                if special_round_win {
                    self.end_round(w, true);
                } else if self.f[l].hp == 0 && self.f[w].chosen != Some(BakaAttack::Special) {
                    self.finish_or_end_round(w);
                }
            }
            ExchangeOutcome::Draw => {
                // The draw arm spawns both pairs with the keyframe reset
                // (`0x801D37B4` / `0x801D37C0`), then runs both damage calls.
                self.spawn_impact(0, true);
                self.spawn_impact(1, true);
                // Both take damage, both streaks reset, both roll comebacks.
                let (d0, c0, _) = self.apply_damage(0);
                let (d1, c1, _) = self.apply_damage(1);
                for m in &mut self.motion {
                    m.play(crate::baka_duel::MOTION_HIT, true);
                }
                self.f[0].combo = 0;
                self.f[1].combo = 0;
                self.f[0].crit_pending = false;
                self.f[1].crit_pending = false;
                self.roll_comeback_crit(0);
                self.roll_comeback_crit(1);
                self.f[0].cooldown = COOLDOWN_RESET;
                self.f[1].cooldown = COOLDOWN_RESET;
                self.end_exchange();
                self.last_exchange = Some(ExchangeReport {
                    winner: 1,
                    draw: true,
                    damage: d0.max(d1),
                    critical: c0 || c1,
                    special_round_win: false,
                });
                match (self.f[0].hp == 0, self.f[1].hp == 0) {
                    // Double KO replays the round: no round win is credited.
                    (true, true) => self.phase = MatchPhase::RoundOver(0),
                    (true, false) => self.finish_or_end_round(1),
                    (false, true) => self.finish_or_end_round(0),
                    (false, false) => {}
                }
            }
        }
    }
}

/// The roster id the cabinet serves first on every visit: the rung fold of
/// the stage counter `FUN_801CF00C` seeds (`DAT_801DC10C = 2`, so roster
/// `2 + 3 = 5`). Every later rung comes from the cabinet's own stage advance
/// through [`crate::baka_cabinet::rung_fold`].
pub fn first_rung_roster() -> usize {
    let cab = crate::baka_cabinet::BakaCabinet::new();
    crate::baka_cabinet::rung_fold(cab.stage(), cab.secret_opponent()).0 as usize
}

// --- End-of-match score tally ----------------------------------------------

/// Remainder above which the tally drains at [`TALLY_DIVISOR_FAST`] per step.
pub const TALLY_FAST_THRESHOLD: i32 = 5;
/// Remainder below which the tally drains one unit per step.
pub const TALLY_SLOW_THRESHOLD: i32 = 3;
/// Divisor applied to a large remainder (`> TALLY_FAST_THRESHOLD`).
pub const TALLY_DIVISOR_FAST: i32 = 5;
/// Divisor applied to a mid-sized remainder.
pub const TALLY_DIVISOR_MID: i32 = 2;

/// How much the end-of-match tally moves out of a counter this frame, given
/// the amount still to drain.
///
/// The tally screen animates four score counters emptying into the running
/// total and the player's gold. The step is proportional, not linear, so a big
/// remainder empties fast and the last few units tick over one at a time:
/// `> 5` drains a fifth per frame, `3..=5` a half, and `< 3` exactly one - so
/// the counter always reaches zero rather than asymptotically approaching it.
///
/// `skip` is the tally's fast-forward flag (`DAT_801dbf00`): when set the whole
/// remainder moves in one step, which is what makes holding the button snap
/// the tally to its end state.
// PORT: FUN_801d6710 (tally drain step; the doc's "digit drawer" reading is
// wrong - this function draws nothing, it is the per-frame drain rate)
// REF: FUN_801d14b0 (the same routine linked into the PROT 0977 hub overlay)
// Wired: [`BakaTally::tick`] (the port of `FUN_801d239c`) calls this for
// every counter step, and [`BakaFight`] runs a tally once the match is
// decided, so the drain rate paces the prize actually reaching party gold.
//
// `FUN_801D6710` and the hub overlay's `FUN_801D14B0` are **one routine linked
// twice**: the two dumps are 24 instructions each and agree opcode for opcode
// and register for register, differing only in the `lui`/`lw` pair that loads
// the bypass flag (`DAT_801DBF00` here, `DAT_801D1AB4` there) and in the
// relocated branch targets. So the port holds one implementation and this entry
// delegates to it - the shape the `sin_4096` incident argues for, where two
// reproductions of one table disagreed and nothing compared them.
pub fn tally_drain_step(remaining: i32, skip: bool) -> i32 {
    crate::other_game_overlay::step_scale(remaining, skip)
}

/// Run one counter of the tally to empty, returning the per-frame steps it
/// takes. Each step is [`tally_drain_step`] of what is left; the sum is the
/// original `amount`.
///
/// Retail drains a negative counter by the same rule, which would run away
/// from zero - no call site produces one, and the port treats it as empty.
pub fn tally_drain_sequence(amount: i32) -> Vec<i32> {
    let mut left = amount.max(0);
    let mut steps = Vec::new();
    while left > 0 {
        let step = tally_drain_step(left, false).clamp(1, left);
        steps.push(step);
        left -= step;
    }
    steps
}

/// Frame-steps a tally row must have been on screen before its counter is
/// allowed to drain (`fade < 0x11` stalls the row in `FUN_801d239c`).
pub const TALLY_FADE_GATE: i32 = 0x11;

/// Number of counters the end-of-match tally drains.
pub const TALLY_COUNTERS: usize = 4;

/// Index of the tally counter that pays into the player's gold rather than
/// into the on-screen score total (`DAT_801dbee8` → `_DAT_80084440`).
pub const TALLY_GOLD_COUNTER: usize = 3;

/// The end-of-match **score tally**: four counters draining, strictly in
/// order, into the running total and the player's gold.
///
/// PORT: FUN_801d239c (end-of-match score tally). The retail screen holds
/// four counters (`DAT_801dbee0` / `DAT_801dbed8` / `DAT_801dbedc` for the
/// score rows and `DAT_801dbee8` for the coin prize). Each row has its own
/// fade counter that advances by the frame step only once every *earlier*
/// row has emptied; a row starts draining when its fade reaches
/// [`TALLY_FADE_GATE`], moves [`tally_drain_step`] out per frame and queues
/// the tick blip ([`BAKA_CUE_CURSOR`]) on every step. The first three rows
/// feed the score total (`DAT_801dbee4`); the fourth feeds party gold.
///
/// The fast-forward latch is the retail one: `FUN_801d239c` opens by testing
/// the edge-triggered pad word `_DAT_8007b874 & 0xf0` (any face button) and
/// setting `DAT_801dbf00`, which makes [`tally_drain_step`] move each whole
/// remainder in a single step - so holding a button snaps the tally to its
/// end state. The latch is never cleared inside the tally, matching retail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakaTally {
    counters: [i32; TALLY_COUNTERS],
    fade: [i32; TALLY_COUNTERS],
    total: i32,
    gold_drained: i32,
    gold_pending: i32,
    fast_forward: bool,
    cues: Vec<u8>,
}

impl BakaTally {
    /// Start a tally over the four counters, in retail row order (the three
    /// score rows first, the coin prize last).
    pub fn new(counters: [i32; TALLY_COUNTERS]) -> Self {
        Self {
            counters: counters.map(|c| c.max(0)),
            fade: [0; TALLY_COUNTERS],
            total: 0,
            gold_drained: 0,
            gold_pending: 0,
            fast_forward: false,
            cues: Vec::new(),
        }
    }

    /// Advance the tally one frame. `frame_step` is the global frame-rate
    /// step; `face_button` is this frame's edge-triggered face-button mask
    /// test (`_DAT_8007b874 & 0xf0`), which latches the fast-forward.
    pub fn tick(&mut self, frame_step: i32, face_button: bool) {
        if face_button {
            self.fast_forward = true;
        }
        for i in 0..TALLY_COUNTERS {
            // A row is only reached once every earlier row has emptied.
            if self.counters[..i].iter().any(|&c| c != 0) {
                break;
            }
            self.fade[i] += frame_step;
            if self.counters[i] == 0 {
                continue;
            }
            if self.fade[i] < TALLY_FADE_GATE {
                break;
            }
            let step =
                tally_drain_step(self.counters[i], self.fast_forward).clamp(1, self.counters[i]);
            self.cues.push(BAKA_CUE_CURSOR);
            self.counters[i] -= step;
            if i == TALLY_GOLD_COUNTER {
                self.gold_drained += step;
                self.gold_pending += step;
            } else {
                self.total += step;
            }
            // No break: retail falls straight through into the next row's
            // section, so the frame that empties a row also advances the
            // following row's fade. That row cannot drain on the same frame
            // (its fade is still under the gate), but it does start a frame
            // earlier than a break here would allow. If this row did *not*
            // empty, the next iteration's own guard stops the sweep.
        }
    }

    /// `true` once every counter has emptied.
    pub fn done(&self) -> bool {
        self.counters.iter().all(|&c| c == 0)
    }

    /// The counters still to drain.
    pub fn counters(&self) -> [i32; TALLY_COUNTERS] {
        self.counters
    }

    /// The on-screen score total accumulated so far (`DAT_801dbee4`).
    pub fn total(&self) -> i32 {
        self.total
    }

    /// Coins moved out of the prize counter so far.
    pub fn gold_drained(&self) -> i32 {
        self.gold_drained
    }

    /// Coins the prize counter has not paid out yet.
    pub fn gold_remaining(&self) -> i32 {
        self.counters[TALLY_GOLD_COUNTER]
    }

    /// Take the coins drained since the last call, for the host to add to
    /// party gold (retail adds each step straight into `_DAT_80084440`).
    pub fn take_gold(&mut self) -> i32 {
        std::mem::take(&mut self.gold_pending)
    }

    /// Drain the tick blips queued since the last call.
    pub fn take_cues(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.cues)
    }
}

/// A resolved HUD widget quad - the renderer-agnostic form of the POLY_GT4
/// packet the retail emitter builds (12-word GP0 `0x3C`/`0x3E` shaded
/// textured quad).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudWidgetQuad {
    /// GP0 polygon code (`(semi << 1) | 0x3C`).
    pub poly_code: u8,
    /// Quad corners, inclusive: `(x0, y0)` top-left, `(x1, y1)` bottom-right.
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
    /// Per-corner texture coordinates in vertex order (TL, TR, BL, BR).
    pub uv: [(u8, u8); 4],
    /// Brightness-scaled gouraud colours: verts 0/1 take `rgb_top`, verts
    /// 2/3 take `rgb_bottom`.
    pub rgb_top: [u8; 3],
    pub rgb_bottom: [u8; 3],
    /// CLUT id (packet uv0 hi-half).
    pub clut: u16,
    /// Texpage attribute after the ABR fold (`texpage + abr * 0x20`).
    pub tpage_attr: u16,
}

/// The MIPS `mult`/`sra` scale idiom the emitter applies to every colour
/// channel and half-extent: signed multiply, round toward zero at the given
/// shift (`bgez` skip + `addiu (1 << shift) - 1`).
fn mips_scale(value: i32, factor: i32, shift: u32) -> i32 {
    let p = value * factor;
    let p = if p < 0 { p + ((1 << shift) - 1) } else { p };
    p >> shift
}

// WIRED ON ONE HOST, and the split is deliberate rather than an oversight.
//
// **Browser**: `LegaiaMinigames::baka_hud_quad_json` calls this emitter, and
// the duel page's `_widget` draws the quad it returns - the corners, the
// inclusive UV span and the mirror arm are now this function's, not the page's.
// That is a behavioural change, not a rename: the page computed its half-extent
// as `(cell * scale) / 0x1000 / 2` in floating point with **no `size` term**,
// where retail is `((cell * scale) >> 13) * size >> 12` with both shifts
// rounding toward zero over a span of `x - hw ..= x + hw - 1`.
//
// **Native**: still unwired, and the blocker there is a texel source. The play
// window draws each `ChromeDraw` as font text because no PROT 1203 art page is
// resident in engine VRAM; the quad sink itself is not missing
// (`legaia_engine_ui::screen_prim::ScreenQuad` carries POLY_GT4 corners,
// per-vertex gouraud, CLUT/texpage, ABR and an OT bucket, and both hosts
// consume its `build_geometry` output). The earlier reason on this row said the
// sink was the gap; it is the page.
//
// One half of the emitter is surfaced but not applied on either host: the
// brightness-scaled gouraud pair. PSX modulation is a per-texel
// `texel * c / 128`, which the duel page's 2D canvas cannot express in one
// pass, so the page draws geometry + UVs and the colours ride along in the
// JSON.
//
// (Everything upstream was already present:
// `legaia_asset::baka_opponents::parse_baka_hud` is called by the play window -
// `window/minigames.rs` stages the 51 records for the duel - and by the browser
// duel page, which also decodes the PROT 1203 art pack the widgets sample; and
// `BakaChrome` emits one
// [`ChromeDraw`](crate::baka_fighter_chrome::ChromeDraw) per call of this
// emitter with the same `(widget, x, y, brightness, size)`, the play window
// resolving its `u` column through
// [`crate::baka_fighter_chrome::glyph_u`].)
/// PORT: FUN_801d5ed0 - the Baka Fighter HUD textured-quad emitter.
///
/// `FUN_801d5ed0(x, y, id, brightness, size)` draws widget `id` of the
/// 51-record descriptor table `DAT_801d7160`
/// ([`legaia_asset::baka_opponents::parse_baka_hud`]) as a POLY_GT4 centred
/// on `(x, y)`:
///
/// - half-extent per axis = `((cell * scale) >> 13) * size >> 12` (both
///   shifts round toward zero), spanning `x - hw ..= x + hw - 1`;
/// - every colour channel = `channel * brightness >> 8` (round toward
///   zero); verts 0/1 carry `rgb_top`, verts 2/3 `rgb_bottom`;
/// - UVs cover the cell inclusively (`u ..= u + w - 1`); `mirror` swaps the
///   left/right texture columns (retail's one-shot flag `DAT_801dbe98`,
///   consumed - zeroed - by every call);
/// - texpage attribute = `texpage + abr * 0x20` (the ABR blend fold), CLUT
///   passes through.
///
/// Retail then links the packet into the OT bucket `_DAT_801DBEBC` and
/// bumps that slot to 3 - host-side scheduling this kernel leaves to the
/// renderer.
pub fn hud_widget_quad(
    widget: &legaia_asset::baka_opponents::BakaHudWidget,
    x: i16,
    y: i16,
    brightness: i32,
    size: i32,
    mirror: bool,
) -> HudWidgetQuad {
    let scale8 = |c: u8| mips_scale(c as i32, brightness, 8) as u8;
    let half = |cell: u8| {
        let base = mips_scale(cell as i32, widget.scale, 13);
        mips_scale(size, base, 12)
    };
    let hw = half(widget.w) as i16;
    let hh = half(widget.h) as i16;
    let (u0, v0) = (widget.u, widget.v);
    let (u1, v1) = (
        widget.u.wrapping_add(widget.w).wrapping_sub(1),
        widget.v.wrapping_add(widget.h).wrapping_sub(1),
    );
    let uv = if mirror {
        [(u1, v0), (u0, v0), (u1, v1), (u0, v1)]
    } else {
        [(u0, v0), (u1, v0), (u0, v1), (u1, v1)]
    };
    HudWidgetQuad {
        poly_code: (widget.semi << 1) | 0x3C,
        x0: x - hw,
        y0: y - hh,
        x1: x + hw - 1,
        y1: y + hh - 1,
        uv,
        rgb_top: widget.rgb_top.map(scale8),
        rgb_bottom: widget.rgb_bottom.map(scale8),
        clut: widget.clut,
        tpage_attr: widget.texpage + widget.abr as u16 * 0x20,
    }
}

/// A minigame effect-part spawn spec - the argument set the tiny spawn
/// wrappers pass to the shared part-spawn API (`FUN_80021B04`) plus the
/// fields they stamp on the returned part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectSpawnSpec {
    /// Screen position handed to the spawn API.
    pub x: i16,
    pub y: i16,
    /// Fixed-point scale (`0x1000` = 1.0).
    pub scale: i32,
    /// Sprite/animation id stamped into the spawned part's `+0x50`.
    pub sprite_id: u16,
}

/// PORT: FUN_801d6e04 - the round chrome's screen-centre effect spawn:
/// retail zero-fills a spawn record, plants it at the fixed screen centre
/// `(0xA0, 0x78)` through the shared part-spawn API `FUN_80021B04` at scale
/// `0x1000`, then stamps `sprite_id` into the spawned part's `+0x50`. The
/// dance overlay's cell-placed twin is `FUN_801d3fd0`
/// (`legaia_engine_core::dance::step_mark_effect_spawn`).
pub fn center_effect_spawn(sprite_id: u16) -> EffectSpawnSpec {
    EffectSpawnSpec {
        x: 0xA0,
        y: 0x78,
        scale: 0x1000,
        sprite_id,
    }
}

// --- Action-table keyframe lookup ------------------------------------------

/// Arithmetic shift-right-by-4 rounding toward zero - the retail
/// `bgez v, skip; addiu v, v, 0xf; skip: sra v, v, 4` idiom (a plain `>> 4`
/// on a negative would round toward minus infinity).
fn sra4_round_to_zero(v: i32) -> i32 {
    (if v < 0 { v + 0xF } else { v }) >> 4
}

/// PORT: FUN_801d6e5c - action-table keyframe lookup by frame range.
///
/// Wired: the combat tick's call at `0x801D4334` is [`StrikeClock`]'s
/// per-tick lookup (`a0` = the fighter's table `s2[+0x94]`, `a1` = the action
/// `s4[+0x5C] - (s2[+0x14] + 1)`, `a2` = the pre-step cursor `s2[+0x90]`,
/// `a3` = the cursor `s4[+0x68]`), which every duel built from the disc
/// tables runs on all three hosts through [`BakaFight::tick_with_input`].
///
/// Returns the index of the first sub-keyframe whose whole-frame index (the
/// action record's `+0x26` field, one per `0x08`-byte sub-keyframe) falls
/// within the query range, or `None` when the range is inverted (`to < from`),
/// the action has no sub-keyframes, or none match.
///
/// The fixed point sits on the **query**, not on the record: `from` and `to`
/// are shifted right by 4 (rounding toward zero) and compared against the raw
/// frame indices, so callers pass a `<< 4` fixed-point frame range against the
/// whole-frame keyframe values. `frame_indices` is the action record's
/// per-sub-keyframe `+0x26` column (its length is the record's `+0x1c` count);
/// the retail function reaches it through `PTR_DAT_801db8b8[char][action]`.
pub fn keyframe_in_range(frame_indices: &[i16], from: i32, to: i32) -> Option<usize> {
    if to < from {
        return None;
    }
    let lo = sra4_round_to_zero(from);
    let hi = sra4_round_to_zero(to);
    frame_indices
        .iter()
        .position(|&f| lo <= f as i32 && (f as i32) <= hi)
}

// --- Decimal number drawers ------------------------------------------------

/// Cells in the Baka Fighter number drawers' fixed-width right-aligned field:
/// eight decimal places (`10_000_000` down to `1`). Leading-zero places are
/// blank; the units place always draws (so a zero value shows a single `0`).
pub const DIGIT_FIELD_CELLS: usize = 8;

/// Widget id of the 8px digit glyph (`FUN_801d69e4` / `FUN_801d6a18`).
pub const NUMBER_WIDGET: u8 = 0x13;
/// X advance per drawn cell for the 8px number drawer (`s1 += 8`).
pub const NUMBER_CELL_STRIDE: i16 = 8;
/// Widget id of the coin-count digit glyph (`FUN_801d6f44`, HUD widget 47).
pub const COIN_WIDGET: u8 = 0x2F;
/// X advance per drawn cell for the coin-strip drawer (`s1 += 0x10`).
pub const COIN_CELL_STRIDE: i16 = 0x10;
/// Base `u` texel column of the coin digit cell row (`u = 0x58 + digit*0x10`).
pub const COIN_U_BASE: u8 = 0x58;

/// One decimal glyph a Baka Fighter number drawer emits: which HUD widget to
/// draw, its place in the field, the x offset from the field's left edge, and
/// the `u` texel column patched into the widget descriptor for the digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigitCell {
    /// Field place, `0` (leftmost / highest) .. `DIGIT_FIELD_CELLS` (units).
    pub cell: usize,
    /// The decimal digit `0..=9` drawn here.
    pub digit: u8,
    /// HUD widget id drawn for this glyph.
    pub widget: u8,
    /// X offset from the field's left edge (`cell * stride`).
    pub x_offset: i16,
    /// The `u` texel column stamped into the widget descriptor.
    pub u: u8,
}

/// Lay out an integer as a right-aligned decimal field. Shared kernel of the
/// two overlay number drawers: the retail code stores each place's truncated
/// quotient `value / 10^(7-cell)` into a scratch array (skipping the ones that
/// come out zero, except the units place which is pre-seeded so it always
/// draws), then draws each surviving place as `quotient % 10`. Negative input
/// has no retail call site (score / coin counts are non-negative) and is
/// clamped to zero here.
fn field_cells(value: i32, widget: u8, stride: i16, u_of: impl Fn(u8) -> u8) -> Vec<DigitCell> {
    let value = value.max(0);
    let mut out = Vec::new();
    let mut divisor = 10_000_000i32;
    for cell in 0..DIGIT_FIELD_CELLS {
        let quotient = value / divisor;
        if quotient != 0 || cell == DIGIT_FIELD_CELLS - 1 {
            let digit = (quotient % 10) as u8;
            out.push(DigitCell {
                cell,
                digit,
                widget,
                x_offset: cell as i16 * stride,
                u: u_of(digit),
            });
        }
        divisor /= 10;
    }
    out
}

/// Wired: the duel HUD's score-total row in the play window lays its digits out
/// through this. The cells are drawn as font glyphs at their ported x offsets,
/// since the widget descriptor they patch has no sprite page behind it.
///
/// PORT: FUN_801d6a18 - the right-aligned 8px decimal number drawer.
///
/// Lays `value` out across the eight-place field as widget [`NUMBER_WIDGET`]
/// glyphs, each cell `8` px to the right of the last ([`NUMBER_CELL_STRIDE`])
/// with the digit's `u` column patched to `digit * 8` (`DAT_801d72e4`). Retail
/// also biases the glyph CLUT for the run (`DAT_801d72e2 = clut + 0x7d87`,
/// restored after) and draws through [`hud_widget_quad`] / `FUN_801d5ed0`; the
/// CLUT bias and OT-bucket scheduling are host-side, so this returns just the
/// per-digit cell layout.
pub fn right_aligned_number_cells(value: i32) -> Vec<DigitCell> {
    field_cells(value, NUMBER_WIDGET, NUMBER_CELL_STRIDE, |d| d * 8)
}

/// Wired: the duel HUD's prize row, alongside [`right_aligned_number_cells`].
///
/// PORT: FUN_801d6f44 - the coin-count digit-strip drawer (HUD widget 47).
///
/// Same right-aligned decimal decomposition as [`right_aligned_number_cells`],
/// but drawing widget [`COIN_WIDGET`] glyphs `0x10` px apart
/// ([`COIN_CELL_STRIDE`]) with the digit `u` column patched to
/// `0x58 + digit * 0x10` (`DAT_801d7514`) - the "GET COIN" numeral row on the
/// PROT 1203 tally sheet.
pub fn coin_digit_cells(value: i32) -> Vec<DigitCell> {
    field_cells(value, COIN_WIDGET, COIN_CELL_STRIDE, |d| {
        COIN_U_BASE.wrapping_add(d.wrapping_mul(0x10))
    })
}

/// Wired: the duel HUD's round-number glyph.
///
/// PORT: FUN_801d69e4 - the single 8px digit draw.
///
/// The one-glyph form the right-aligned drawer calls per place: patch widget
/// [`NUMBER_WIDGET`]'s `u` column to `digit << 3` (`DAT_801d72e4`) and draw it
/// through `FUN_801d5ed0`. Retail leaves the caller to position `x`; this
/// reports the cell with a zero x offset.
pub fn single_digit_cell(digit: u8) -> DigitCell {
    DigitCell {
        cell: 0,
        digit,
        widget: NUMBER_WIDGET,
        x_offset: 0,
        u: digit.wrapping_shl(3),
    }
}

/// The combo count is clamped to this before it indexes the combo-bonus table
/// (`FUN_801d2a28`: `if (0x13 < combo) combo = 0x13`).
pub const BAKA_COMBO_MAX: i32 = 0x13;

/// A round that ends with the winner still at full HP ([`HP_START`]) pays this
/// flat perfect-clear bonus instead of a health-scaled one
/// (`FUN_801d2a28`: `if (DAT_801dbfc4 == 0xc80) bonus += 0xc350`).
pub const BAKA_PERFECT_BONUS: i32 = 50_000;

/// The end-of-round HP is divided by this to index the health-bonus table
/// (`FUN_801d2a28`: `DAT_801dbfc4 / 0x140`). [`HP_START`] (`0xc80`) / `0x140`
/// is `10`, so the top table slot is reachable only via the perfect path.
pub const BAKA_HEALTH_BONUS_DIVISOR: i32 = 0x140;

/// The per-round score increment a completed round contributes to the two
/// score rows the end-of-match tally later drains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BakaRoundScore {
    /// Added to the combo-score row (`DAT_801dbed8`).
    pub combo_gain: i32,
    /// Added to the bonus row (`DAT_801dbedc`).
    pub bonus_gain: i32,
}

/// Clamp a raw combo count to the combo-bonus table index space.
///
/// PORT: FUN_801d2a28 (`0x801d2a34..0x801d2a40`). Retail keeps the count when
/// it is below `0x14` and otherwise pins it to [`BAKA_COMBO_MAX`]; the compare
/// is signed, so a (never-produced) negative count passes through unclamped,
/// exactly as the `slti` does.
pub fn baka_combo_index(combo: i32) -> i32 {
    if combo < BAKA_COMBO_MAX + 1 {
        combo
    } else {
        BAKA_COMBO_MAX
    }
}

/// Resolve the two score-row increments a finished round contributes.
///
/// PORT: FUN_801d2a28 (per-round score accumulation).
///
/// The combo input is [`BakaFight::max_combo`], the running maximum
/// `DAT_801dbec8` that the HUD renderer latches each frame from
/// `DAT_801dc094`. That address resolves: the per-fighter hits-taken array is
/// `&DAT_801dbfec[slot * 0x2a]` (an `int` index, so `0xa8` bytes per slot) and
/// `0x801dbfec + 0xa8 == 0x801dc094`, i.e. **slot 1's** consecutive-hits-taken
/// counter - how long a streak the player is currently landing. The two bonus
/// tables are still overlay rodata with no parser, so [`BakaFight`] keeps them
/// optional ([`BakaScoreTables`]): absent, every lookup misses and the rows
/// stay at zero. The retail routine reads
/// the running maximum combo (`DAT_801dbec8`) and the winner's remaining HP
/// (`DAT_801dbfc4`) and folds two increments into the running score rows the
/// end-of-match tally ([`BakaTally`]) later drains:
///
/// - the **combo** row gains `combo_bonus[clamp(combo)]`, indexed by
///   [`baka_combo_index`] into the overlay combo-bonus table
///   (`&DAT_801d70c4`, 20 `i32` slots);
/// - the **bonus** row gains [`BAKA_PERFECT_BONUS`] when the round ended at
///   full HP ([`HP_START`]), else `health_bonus[hp / `[`BAKA_HEALTH_BONUS_DIVISOR`]`]`
///   indexed into the overlay health-bonus table (`&DAT_801d711c`, `i16`
///   slots). The HP divide is the retail signed `/0x140`.
///
/// The two tables are disc data (`FUN_801d2a28`'s overlay), so they are passed
/// in by the caller rather than baked here; the caller supplies the slices it
/// parsed from the Baka Fighter overlay. Out-of-range indices are treated as a
/// zero contribution, which cannot happen with the retail table sizes but
/// keeps the kernel total.
pub fn baka_round_score(
    combo: i32,
    combo_bonus: &[i32],
    end_hp: i32,
    health_bonus: &[i16],
) -> BakaRoundScore {
    let combo_gain = combo_bonus
        .get(baka_combo_index(combo).max(0) as usize)
        .copied()
        .unwrap_or(0);

    let bonus_gain = if end_hp == HP_START {
        BAKA_PERFECT_BONUS
    } else {
        let idx = end_hp / BAKA_HEALTH_BONUS_DIVISOR;
        health_bonus
            .get(idx.max(0) as usize)
            .map(|&v| v as i32)
            .unwrap_or(0)
    };

    BakaRoundScore {
        combo_gain,
        bonus_gain,
    }
}

// ------------------------------------------------------- per-fighter installer

/// PROT TOC base the per-fighter mesh installer adds the folded roster index
/// to. Entry `0x4b6` is extraction 1204, the battle-form party pack.
pub const FIGHTER_PACK_TOC_BASE: u32 = 0x4B6;
/// Roster ids at or above this fold down by the same amount - the three party
/// fighters share the one party pack.
pub const FIGHTER_PACK_FOLD: usize = 3;
/// Scratch buffer the installer allocates for the pack before walking it.
pub const FIGHTER_PACK_BUFFER: usize = 0x4_6000;
/// Mask that separates a chunk header word's size from its type byte.
pub const FIGHTER_CHUNK_SIZE_MASK: u32 = 0x00FF_FFFF;

/// One chunk of a fighter pack, as the installer's walk sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FighterChunk {
    /// Header byte `3` - the asset type the dispatcher switches on.
    pub kind: u8,
    /// Byte offset of the payload inside the pack.
    pub offset: usize,
    /// Payload length as the header declares it.
    pub len: usize,
}

/// Which PROT TOC entry the installer loads for a roster index, and the folded
/// index it uses.
///
/// REF: FUN_801d4c50 (`0x801D4D58..0x801D4D70`)
pub fn fighter_pack_entry(roster: usize) -> (usize, u32) {
    let folded = if roster >= FIGHTER_PACK_FOLD {
        roster - FIGHTER_PACK_FOLD
    } else {
        roster
    };
    (folded, FIGHTER_PACK_TOC_BASE + folded as u32)
}

// REPLACED-BY: `legaia_asset::baka_opponents::parse_fighter_pack`. The port
// resolves the fighter's PROT entry on demand and walks the same
// `[u32 (type<<24)|size][payload]` chunk chain into typed sub-asset bytes
// (TIM / TMD / ANM), which the browser duel host consumes directly; retail's
// transient `0x46000` buffer, its dev-vs-CD load fork and its dispatcher
// hand-off have no analogue to host.
/// PORT: FUN_801d4c50 - the per-fighter **mesh installer**'s chunk walk.
///
/// The installer allocates [`FIGHTER_PACK_BUFFER`] bytes, fills it either from
/// a named `data_field` path (the dev arm, taken only while the streaming flag
/// `_DAT_8007b8c2` is `0`) or from raw PROT entry [`fighter_pack_entry`] (the
/// retail arm), then walks the buffer as a chain of
/// `[u32 (type << 24) | size][payload]` chunks, handing each to the asset
/// dispatcher `FUN_8001F05C` with the "already decompressed" flag. The chain
/// ends at the first header whose size field is zero, and the buffer is freed
/// on the way out - the pack is transient, and whatever the dispatcher keeps
/// it has already copied.
///
/// The stride is the retail one and it is **not** `4 + size`: the size is
/// rounded down to a multiple of four first (`sra 2` then `sll 2`), so a chunk
/// whose declared length is not word-aligned advances short of its own end.
pub fn fighter_pack_chunks(pack: &[u8]) -> Vec<FighterChunk> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= pack.len() {
        let hdr = u32::from_le_bytes([pack[pos], pack[pos + 1], pack[pos + 2], pack[pos + 3]]);
        let size = (hdr & FIGHTER_CHUNK_SIZE_MASK) as usize;
        if size == 0 {
            break;
        }
        out.push(FighterChunk {
            kind: (hdr >> 24) as u8,
            offset: pos + 4,
            len: size,
        });
        pos += (size / 4) * 4 + 4;
    }
    out
}

#[cfg(test)]
mod tests;
