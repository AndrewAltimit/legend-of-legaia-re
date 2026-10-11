//! The [`BakaFight`] session state, its builders and read accessors.

use super::*;
use legaia_engine_system::rand::BiosRand;
use legaia_minigame_formats::baka_opponents::{BakaActionSet, BakaOpponent};

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
    pub(super) cfg: [FighterConfig; 2],
    pub(super) f: [FighterState; 2],
    /// SFX cue ids queued this tick, in fire order - the host's view of the
    /// retail cue-ring writes. Drained by [`BakaFight::take_cues`].
    pub(super) cues: Vec<u8>,
    /// Which slots the CPU picker drives (slot 1 in retail; both for demos).
    pub(super) ai_controlled: [bool; 2],
    /// Round index (`DAT_801dbf20`).
    pub(super) round: u32,
    /// Per-exchange settle timer (`DAT_801dbf54`). No seeder exists in the
    /// dumped corpus - it only ever decays - so it starts (and stays) 0
    /// unless a host installs a pace.
    pub(super) settle_timer: i32,
    pub(super) phase: MatchPhase,
    pub(super) rng: BiosRand,
    pub(super) last_exchange: Option<ExchangeReport>,
    /// The end-of-match score tally, installed once the player takes the
    /// match (`FUN_801d239c`'s screen). `None` until then, and on a loss -
    /// a beaten player is paid nothing.
    pub(super) tally: Option<BakaTally>,
    /// The two overlay score-bonus tables, when a host has parsed them.
    pub(super) score_tables: Option<BakaScoreTables>,
    /// `DAT_801dbec8` - the running **maximum** of the player's hit streak,
    /// which retail latches once per frame in the HUD renderer
    /// (`FUN_801d2afc`: `if (DAT_801dbec8 <= DAT_801dc094) DAT_801dbec8 =
    /// DAT_801dc094;`). `DAT_801dc094` is `&DAT_801dbfec[1 * 0x2a]` - slot 1's
    /// consecutive-hits-taken counter, i.e. how long a streak the player is
    /// currently landing.
    pub(super) max_combo: i32,
    /// The three score rows `FUN_801d2a28` accumulates into
    /// (`DAT_801dbee0` / `DAT_801dbed8` / `DAT_801dbedc`), which the
    /// end-of-match [`BakaTally`] then drains.
    pub(super) score_rows: [i32; 3],
    /// The round chrome, advanced once per [`BakaFight::tick_with_input`].
    pub(super) chrome: crate::baka_fighter_chrome::BakaChrome,
    /// The chrome frame the last tick produced.
    pub(super) chrome_frame: crate::baka_fighter_chrome::ChromeFrame,
    /// The round-result banner sub-state `DAT_801DBF84`: non-zero once this
    /// round's result banners are up, so they rise once per round.
    pub(super) result_latch: bool,
    /// `DAT_801DBF24` - the player has not been hit this round. Raised at the
    /// round's setup, cleared by the two exchange arms that damage slot 0;
    /// a win with it still up is the PERFECT!! banner.
    pub(super) player_untouched: bool,
    /// The cabinet shell state machine (`FUN_801CF388`), stepped once per
    /// tick. Retail has the nesting the other way up - the cabinet SM owns the
    /// frame and the fight resolution runs under it - but the port's host
    /// enters through the duel, so the duel owns the frame and the cabinet
    /// runs as its shell. The state the cabinet occupies while a duel is live
    /// is the same one retail uses ([`crate::baka_cabinet::ST_DUEL`]), so the
    /// bracket it drives (round count, stage advance, the win / lose exits,
    /// the pot) is retail's.
    pub(super) cabinet: crate::baka_cabinet::BakaCabinet,
    /// The cabinet frame the last tick produced.
    pub(super) cabinet_frame: crate::baka_cabinet::CabinetFrame,
    /// The roster + action tables the fight was built from, kept so the
    /// cabinet's between-rung opponent install (`ST_OPPONENT_INSTALL`) can
    /// seat the next rung's fighter. `None` for a fight built from bare
    /// configs, which re-seats the same opponent instead.
    pub(super) tables: Option<std::sync::Arc<(Vec<BakaOpponent>, Vec<BakaActionSet>)>>,
    /// The packed pad edge the host handed over for the next cabinet tick
    /// (consumed by it). See [`Self::set_cabinet_pad`].
    pub(super) cabinet_pad: u16,
    /// Both fighters' strike data. With it installed an exchange is booked
    /// only on the tick the winner's strike lands (retail's `+0x0C == 1`
    /// gate); without it (a fight built from bare configs) the exchange
    /// resolves the tick both sides have chosen.
    pub(super) strike: Option<[StrikeTable; 2]>,
    /// `DAT_1F80037D` - the frame-rate divisor both clips step by.
    pub(super) rate_divisor: i32,
    /// The live afterimage actors (one per special commit).
    pub(super) afterimages: Vec<crate::baka_fighter_chrome::AfterimageActor>,
    /// What each afterimage drew on the last tick, by owner slot.
    pub(super) afterimage_frames: Vec<(usize, crate::baka_fighter_chrome::AfterimageFrame)>,
    /// Every roster fighter's clip headers, when a host staged them.
    pub(super) roster_clips: Option<std::sync::Arc<Vec<ClipHeaders>>>,
    /// Each seat's display clip (actor `+0x5C` / `+0x68`), stepped every
    /// tick by [`Self::tick_presentation`].
    pub(super) motion: [crate::baka_duel::FighterMotion; 2],
    /// The arena camera.
    pub(super) camera: crate::baka_duel::DuelCamera,
    /// Each seat's roster `+0x44` stand-off (`0` for a bare-config fight).
    pub(super) stand_off: [i32; 2],
    /// Each seat's Z off the duel line (the result close-up's step).
    pub(super) stand_z: [i32; 2],
    /// The player's special-commit camera glides, by party fighter.
    pub(super) special_cameras: Vec<[i16; 20]>,
    /// The packed held pad word (`_DAT_8007B850`) the host handed over for
    /// the next tick ([`Self::set_held_pad`]).
    pub(super) held_pad: u16,
    /// A round setup (cabinet state `0x32`) is due on the next presentation
    /// tick - the first round's and every later one's.
    pub(super) setup_pending: bool,
    /// Frames the player-select lineup has been on stage - the clock its
    /// three idle clips run off ([`Self::select_lineup`]).
    pub(super) select_clock: i32,
    /// The round-start cameo's actor, while it walks: its phase `+0x22` and
    /// the clip cursor `+0x68` its clip selector advances.
    pub(super) cameo: Option<CameoActor>,
    /// `DAT_801DBF50` - raised by a special's commit (`0x801D4578`), cleared
    /// by the round setup (`0x801CFFD8`). The impact pair reads it for its
    /// Z lift.
    pub(super) special_latch: bool,
    /// The impact pair's effect parts ([`crate::baka_impact_fx`]).
    pub(super) impact: crate::baka_impact_fx::ImpactFx,
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
        tables: Vec<legaia_minigame_formats::baka_opponents::BakaActionSet>,
    ) -> Self {
        self.cabinet = std::mem::take(&mut self.cabinet).with_action_tables(tables);
        self.cabinet.enter_duel();
        self
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
}
