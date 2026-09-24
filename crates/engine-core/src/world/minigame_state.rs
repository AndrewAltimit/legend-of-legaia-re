//! Minigame sessions (dance, fishing, slot machine, Baka Fighter, Muscle Dome) plus the casino coin / point-card wallet.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Minigame sessions (dance, fishing, slot machine, Baka Fighter, Muscle Dome) plus the casino coin / point-card wallet.
pub struct MinigameState {
    /// Noa dance (rhythm) minigame state. `Some` while `mode ==
    /// SceneMode::Dance`; the beat clock + hit judge run each tick. See
    /// [`crate::dance::DanceGame`] and [`crate::world::World::enter_dance`].
    pub dance: Option<crate::dance::DanceGame>,
    /// The scene mode to restore when the dance minigame ends
    /// ([`crate::world::World::enter_dance`] snapshots the mode it interrupted). Mirrors the
    /// pause-menu suspend/restore contract.
    pub dance_return_mode: SceneMode,
    /// The most recent dance-press judgement, kept for the host HUD (the
    /// score/gauge banner). Reset to `None` on [`crate::world::World::enter_dance`]; updated
    /// each frame a directional press is judged.
    pub dance_last_judge: Option<crate::dance::Judge>,
    /// Fishing minigame session. `Some` while `mode == SceneMode::Fishing`; the
    /// retail cast / wait / strike / fight / score loop runs each tick. See
    /// [`crate::fishing::PondSession`] and [`crate::world::World::enter_fishing`].
    pub fishing: Option<crate::fishing::PondSession>,
    /// The [`crate::fishing::PondEvent`]s the last
    /// [`crate::world::World::tick`] raised (hook, landed, snapped, cadence
    /// splash, recast). Refreshed every fishing frame; the play hosts seed
    /// their HUD banner one-shots from it, the way the minigames page does
    /// from its own tick's return.
    pub fishing_events: Vec<crate::fishing::PondEvent>,
    /// The scene mode to restore when the fishing minigame ends
    /// ([`crate::world::World::enter_fishing`] snapshots the interrupted mode).
    pub fishing_return_mode: SceneMode,
    /// Persistent fishing-point pool, mirroring retail's `_DAT_8008444C`
    /// counter: [`crate::world::World::exit_fishing`] banks the session record's points
    /// here, and the point exchange spends from it
    /// ([`crate::world::World::fishing_exchange_buy`]). Hosts seed a new session's
    /// [`crate::fishing::FishingRecord`] from this cell.
    pub fishing_points: i32,
    /// Persistent rod index (`0..`[`crate::fishing::ROD_KINDS`]), mirroring
    /// retail's `_DAT_80084454`: the cell the rod / lure select screen writes
    /// and the tension gauge divides by. The fishing bring-up re-points a
    /// stale value at an owned rod before a session reads it - see
    /// [`crate::world::World::resolve_fishing_entry_rod`].
    pub fishing_rod: u32,
    /// Persistent lifetime cast counter, mirroring retail's `_DAT_80084460`.
    /// The band-4 gate reads it, and its low bit picks the sign of the cast
    /// lure's walk-grid drift
    /// ([`crate::fishing_actors::LureActor::probe`]).
    pub fishing_casts: i32,
    /// Persistent one-time prize bitmask, mirroring retail's `_DAT_8008446C`:
    /// bit `row + venue * 8` latches when a `limit == 1` exchange row is
    /// bought (see [`legaia_asset::fishing_exchange`]).
    pub fishing_prizes_purchased: u32,
    /// Persistent equipped-lure row (`0..=2`), mirroring retail's
    /// `_DAT_80084450`: the spawn-table row, the HUD's lure label and the
    /// item whose count the lures-remaining row shows. The bring-up re-points
    /// it at an owned lure ([`crate::world::World::enter_fishing_session`]).
    pub fishing_lure: u32,
    /// Persistent best single-catch award, mirroring retail's `_DAT_80084458`.
    pub fishing_best_points: i32,
    /// Species id of the best catch, mirroring retail's `_DAT_8008445C`.
    pub fishing_best_fish: u32,
    /// Fishing point-exchange (prize shop) session. `Some` while the exchange
    /// list is open on the host's fishing screen; purchases commit through
    /// [`crate::world::World::fishing_exchange_buy`].
    pub fishing_exchange: Option<crate::fishing::PrizeExchange>,
    /// Slot-machine minigame session. `Some` while
    /// `mode == SceneMode::SlotMachine`; the reel state machine runs each
    /// tick. See [`crate::slot_machine::SlotMachine`] and
    /// [`crate::world::World::enter_slot_machine`].
    pub slot_machine: Option<crate::slot_machine::SlotMachine>,
    /// The scene mode to restore when the slot-machine minigame ends
    /// ([`crate::world::World::enter_slot_machine`] snapshots the interrupted mode).
    pub slot_return_mode: SceneMode,
    /// Baka Fighter duel state. `Some` while `mode ==
    /// SceneMode::BakaFighter`; the exchange / round / match state machine
    /// runs each tick. See [`crate::baka_fighter::BakaFight`] and
    /// [`crate::world::World::enter_baka_fighter`].
    pub baka_fighter: Option<crate::baka_fighter::BakaFight>,
    /// The scene mode to restore when the Baka Fighter match ends
    /// ([`crate::world::World::enter_baka_fighter`] snapshots the interrupted mode).
    pub baka_return_mode: SceneMode,
    /// Muscle Dome contest state. `Some` while `mode ==
    /// SceneMode::MuscleDome`; the hand-select / commit / resolve loop runs
    /// each tick. See [`crate::muscle_dome::MuscleDomeSession`] and
    /// [`crate::world::World::enter_muscle_dome`].
    pub muscle_dome: Option<crate::muscle_dome::MuscleDomeSession>,
    /// The scene mode to restore when the Muscle Dome contest ends
    /// ([`crate::world::World::enter_muscle_dome`] snapshots the interrupted mode).
    pub muscle_return_mode: SceneMode,
    /// The Muscle Dome **contest** - the ladder run above the individual
    /// legs: which `(course, round)` is staged, the running coin tally, and
    /// whether the run continues. `Some` for as long as a contest is open,
    /// which outlives any one `muscle_dome` session. See
    /// [`crate::muscle_dome::DomeContest`].
    pub muscle_contest: Option<crate::muscle_dome::DomeContest>,
    /// The last Muscle Dome contest's settlement, kept after the contest
    /// itself is gone so a host can put the payout on screen.
    pub muscle_settlement: Option<crate::muscle_dome::ContestSettlement>,
    /// The **ringside still** the last dome leg left resident in VRAM
    /// (`(384, 0)`), as an extraction PROT index - `1221` or `1222`, picked
    /// off the lead's HP at the leg's end by
    /// [`crate::muscle_ringside::still_prot_index`]. `None` until a leg
    /// ends. A re-entered hub draws it as its backdrop
    /// ([`crate::muscle_ringside::HubBackdrop`]).
    pub muscle_ringside_still: Option<u32>,
    /// The casino coin bank (`_DAT_800845A4`, the GameShark "Infinite
    /// Coins" cell). Read to seed the slot machine's playing balance and
    /// **assigned** its final balance on cash-out (the retail state-100
    /// commit is an assignment, not a delta). The coin counter
    /// ([`crate::world::World::open_coin_counter`]) credits it as a delta instead.
    pub casino_coins: u32,
    /// The **Point Card** bank (`_DAT_800845B4`), the third purse beside
    /// [`crate::world::PartyState::money`] and [`crate::world::MinigameState::casino_coins`]. A shop buy credits 5% of
    /// the gold spent while the party holds the Point Card
    /// ([`crate::shop::POINT_CARD_ITEM_ID`]); the total is what the pause
    /// Items screen's "Points Left" line and the shop's window-31 toast
    /// print, and it is clamped to [`crate::shop::POINT_CARD_CAP`].
    ///
    /// See [`crate::shop::point_card_credit`] for the accrual and
    /// `docs/subsystems/shop.md` for the retail chain.
    pub point_card: i32,
    /// The mode-24 minigame door-warp's backup of the active scene name
    /// (retail `0x8007BAE8`, written by the OTHER-INIT entry `FUN_80025980`
    /// from `0x80084548`). [`crate::world::World::minigame_return_warp`] restores it into
    /// [`crate::world::World::active_scene_label`] on exit. `None` while no warp is armed.
    pub scene_backup: Option<String>,
    /// The mode-24 session-winnings accumulator (retail `_DAT_80084440`,
    /// zeroed by the field-VM `0x3E` warp arm; the minigame overlays add
    /// their winnings here). [`crate::world::World::minigame_return_warp`] commits it
    /// into [`crate::world::MinigameState::casino_coins`].
    pub winnings: u32,
    /// Pending **mode-24 minigame door-warp** (field-VM op `0x3E`, `op0 >=
    /// 100`): `Some(sub_id)` for the frame the arm ran on. Drained by
    /// [`crate::scene::SceneHost::tick`], which decodes it with
    /// [`crate::minigame_entry::MinigameSubId`] and enters that minigame.
    ///
    /// **Not a map id.** `sub_id` selects a code overlay, not a scene - the op
    /// carries no destination name at all. It reads like a map id (a small
    /// dense integer on a warp opcode), which is exactly why the engine used
    /// to resolve it through a CDNAME-ordinal scene table and warp the player
    /// somewhere unrelated instead of into the minigame. See
    /// [`crate::minigame_entry`] for the arm's disassembly.
    pub pending_warp: Option<u8>,
    /// The dance's **pre-song count-in** (`FUN_801cf470`'s below-10 states),
    /// armed by [`crate::world::World::enter_dance`] and played out by the
    /// world's dance tick, which holds the beat clock off until it finishes.
    /// `None` once the song is running.
    pub dance_countin: Option<crate::dance::CountIn>,
    /// The count-in banner envelope the last dance tick produced, for a
    /// host's draw list. `None` outside the count-in.
    pub dance_countin_banner: Option<crate::dance::CountInBanner>,
    /// The global `music_01` track the dance's own overlay loads, held until
    /// the count-in ends (retail starts the song when the banner clears).
    /// Chosen by song length in [`crate::world::World::enter_dance`].
    pub dance_pending_bgm: Option<u16>,
    /// The Disco King **how-to** tutorial actor, installed by
    /// [`crate::world::World::enter_dance`] when the parsed game is a
    /// [`crate::dance::DanceMode::HowTo`] run and stepped beside the session.
    pub dance_tutorial: Option<crate::dance_tutorial::DanceTutorial>,
    /// The tutorial frame the last dance tick produced (captions / options /
    /// cursor seats), for a host's draw list.
    pub dance_tutorial_frame: Option<crate::dance_tutorial::TutorialFrame>,
    /// SFX cue ids the minigame sessions queued this frame (the count-in
    /// intro cue, the tutorial's cursor / confirm cues). Drained by
    /// [`crate::world::World::drain_minigame_sfx_cues`]; cosmetic.
    pub pending_sfx: Vec<u16>,
    /// Is the dance HUD's own texture page resident in the VRAM this host is
    /// drawing with?
    ///
    /// The dance samples one 4bpp page and one CLUT strip that belong to the
    /// hall scene (`crate::dance::DANCE_HUD_ART_PROT_ENTRY`), and retail has
    /// them because the dance **is** that scene. The port suspends whichever
    /// scene the player entered from, so each host has to stage those rects
    /// itself and says so here.
    ///
    /// It is a host-written flag on purpose. Whether the page is resident is
    /// the one fact about the dance frame that only the host holding the VRAM
    /// knows, and the *choice it drives* - retail's textured sprite, or the
    /// placeholder letterforms - must be the same choice on both hosts. Left
    /// per-host it would be two predicates over two spellings of "did the
    /// upload work", which is the shape a silent one-host regression hides in.
    pub dance_hud_art_staged: bool,
    /// The **effect-part pool** every minigame overlay's one-shot
    /// presentation spawns land in (the fishing venue's splash, ripples and
    /// catch bursts). Aged once per world tick, so every host that ticks the
    /// world drains the same parts - see [`crate::minigame_fx`] for why the
    /// pool is world state rather than a host's.
    ///
    /// The dance run keeps its own pool inside
    /// [`crate::dance::DanceGame`], because its spawns are gameplay
    /// (the sequence-clear banner fires from the judge) rather than
    /// presentation the host drives.
    pub fx: crate::minigame_fx::MinigameFxPool,
}

impl MinigameState {
    /// Whether the dance's **status readout** (score / groove gauge / lane,
    /// the called arrow, the last judgement, the scrolling beat track) is on
    /// screen this frame.
    ///
    /// False while the pre-song count-in banner is up. The banner is centred
    /// on stage row `0x40` and the status pen sits just below it, so the two
    /// overprint - and there is nothing for the readout to say yet: the beat
    /// clock is held, the score is zero, and the "current beat" is beat zero
    /// of a song that has not started. Retail never shows both, because its
    /// count-in runs before the dance screen's readout is armed at all.
    ///
    /// One predicate for every host: the rule belongs to the phase, not to a
    /// draw list.
    pub fn dance_status_visible(&self) -> bool {
        self.dance.is_some() && self.dance_countin_banner.is_none()
    }

    pub fn new() -> Self {
        Self {
            dance: None,
            dance_return_mode: SceneMode::Field,
            dance_last_judge: None,
            fishing: None,
            fishing_return_mode: SceneMode::Field,
            fishing_points: 0,
            fishing_rod: 0,
            fishing_casts: 0,
            fishing_prizes_purchased: 0,
            fishing_lure: 0,
            fishing_best_points: 0,
            fishing_best_fish: 0,
            fishing_events: Vec::new(),
            fishing_exchange: None,
            slot_machine: None,
            slot_return_mode: SceneMode::Field,
            baka_fighter: None,
            baka_return_mode: SceneMode::Field,
            muscle_dome: None,
            muscle_return_mode: SceneMode::Field,
            muscle_contest: None,
            muscle_settlement: None,
            muscle_ringside_still: None,
            casino_coins: 0,
            point_card: 0,
            scene_backup: None,
            winnings: 0,
            pending_warp: None,
            dance_countin: None,
            dance_countin_banner: None,
            dance_pending_bgm: None,
            dance_tutorial: None,
            dance_tutorial_frame: None,
            pending_sfx: Vec::new(),
            dance_hud_art_staged: false,
            fx: crate::minigame_fx::MinigameFxPool::new(),
        }
    }
}

impl Default for MinigameState {
    fn default() -> Self {
        Self::new()
    }
}
