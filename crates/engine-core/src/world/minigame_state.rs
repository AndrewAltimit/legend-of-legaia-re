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
    pub(crate) dance_return_mode: SceneMode,
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
    pub(crate) fishing_return_mode: SceneMode,
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
    pub(crate) fishing_casts: i32,
    /// Persistent one-time prize bitmask, mirroring retail's `_DAT_8008446C`:
    /// bit `row + venue * 8` latches when a `limit == 1` exchange row is
    /// bought (see [`legaia_asset::fishing_exchange`]).
    pub fishing_prizes_purchased: u32,
    /// Persistent equipped-lure row (`0..=2`), mirroring retail's
    /// `_DAT_80084450`: the spawn-table row, the HUD's lure label and the
    /// item whose count the lures-remaining row shows. The bring-up re-points
    /// it at an owned lure ([`crate::world::World::enter_fishing_session`]).
    pub(crate) fishing_lure: u32,
    /// Persistent best single-catch award, mirroring retail's `_DAT_80084458`.
    pub(crate) fishing_best_points: i32,
    /// Species id of the best catch, mirroring retail's `_DAT_8008445C`.
    pub(crate) fishing_best_fish: u32,
    /// Fishing point-exchange (prize shop) session. `Some` while the exchange
    /// list is open on the host's fishing screen; purchases commit through
    /// [`crate::world::World::fishing_exchange_buy`].
    pub fishing_exchange: Option<crate::fishing::PrizeExchange>,
    /// The two point-exchange venue pages (`0` Buma, `1` Vidna) PROT 0972
    /// carries beside the session tables, decoded by
    /// [`crate::scene::SceneHost::enter_fishing_from_overlay`] - the one
    /// entry the door warp and both play hosts' launchers share. `None` until
    /// a fishing session has been entered on a disc whose pages decode.
    pub fishing_prize_venues: Option<[crate::fishing::PrizeExchange; 2]>,
    /// The venue hub's text (menu rows, help pages) off PROT 0972, decoded by
    /// the same entry as [`Self::fishing_prize_venues`]
    /// ([`crate::fishing_hub::FishingHubText::from_overlay`]).
    pub fishing_hub_text: Option<crate::fishing_hub::FishingHubText>,
    /// The fishing HUD's sprite table off PROT 0972
    /// ([`legaia_asset::fishing_sprites`]), decoded by the same entry as
    /// [`Self::fishing_prize_venues`]. Every HUD glyph, digit and gauge cap
    /// is one of its records drawn out of the venue's HUD page; `None` leaves
    /// a host on its text fallback.
    pub fishing_sprites: Option<Vec<legaia_asset::fishing_sprites::FishingSprite>>,
    /// The lure row's captions off PROT 0972, resolved against the item
    /// table ([`FishingCaptionText`]); `None` leaves a host on its
    /// placeholders.
    pub fishing_captions: Option<FishingCaptionText>,
    /// Slot-machine minigame session. `Some` while
    /// `mode == SceneMode::SlotMachine`; the reel state machine runs each
    /// tick. See [`crate::slot_machine::SlotMachine`] and
    /// [`crate::world::World::enter_slot_machine`].
    pub slot_machine: Option<crate::slot_machine::SlotMachine>,
    /// The scene mode to restore when the slot-machine minigame ends
    /// ([`crate::world::World::enter_slot_machine`] snapshots the interrupted mode).
    pub(crate) slot_return_mode: SceneMode,
    /// The slot machine's runtime SFX descriptor bank (`efect.dat`, the raw
    /// extraction PROT 1199 the overlay init loads), staged by the scene host
    /// on the warp - see [`crate::world::World::runtime_sfx_bundle`].
    pub(crate) slot_sfx_bundle: Vec<u8>,
    /// The Muscle Dome arena's runtime SFX descriptor bundle (extraction
    /// PROT 542, which the arena init points `_DAT_8007B8D0` at), staged by
    /// the scene host on the warp - see
    /// [`crate::world::World::runtime_sfx_bundle`].
    pub muscle_sfx_bundle: Vec<u8>,
    /// Baka Fighter duel state. `Some` while `mode ==
    /// SceneMode::BakaFighter`; the exchange / round / match state machine
    /// runs each tick. See [`crate::baka_fighter::BakaFight`] and
    /// [`crate::world::World::enter_baka_fighter`].
    pub baka_fighter: Option<crate::baka_fighter::BakaFight>,
    /// The scene mode to restore when the Baka Fighter match ends
    /// ([`crate::world::World::enter_baka_fighter`] snapshots the interrupted mode).
    pub(crate) baka_return_mode: SceneMode,
    /// Muscle Dome contest state. `Some` while `mode ==
    /// SceneMode::MuscleDome`; the hand-select / commit / resolve loop runs
    /// each tick. See [`crate::muscle_dome::MuscleDomeSession`] and
    /// [`crate::world::World::enter_muscle_dome`].
    pub muscle_dome: Option<crate::muscle_dome::MuscleDomeSession>,
    /// The scene mode to restore when the Muscle Dome contest ends
    /// ([`crate::world::World::enter_muscle_dome`] snapshots the interrupted mode).
    pub(crate) muscle_return_mode: SceneMode,
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
    /// The arena hub is between two legs of an open contest: a survived leg
    /// with the course not exhausted keeps the mode on the arena, and the
    /// hub's INTERVAL / ROUND screens play before the next fight opens
    /// ([`crate::world::World::begin_next_muscle_leg`]).
    pub(crate) muscle_hub_between_legs: bool,
    /// Ticks left of the resolved turn's **playback** - retail's action
    /// phases `0xFE` / `0xFF`, during which the queued plays animate before
    /// the round driver returns to its command cluster. The leg holds at
    /// [`crate::muscle_dome::MusclePhase::TurnOver`] until it drains
    /// ([`crate::world::World::muscle_playback_frames`]).
    pub(crate) muscle_playback_frames: u32,
    /// The arena hub's screen timers - the first visit, the leg-open ROUND
    /// card, the INTERVAL tally and the re-entered hub's backdrop - ticked by
    /// [`crate::world::World::tick_muscle_hub`]; both play hosts draw from
    /// it.
    pub muscle_hub: crate::muscle_ringside::HubTimers,
    /// Sounds the hub fired that the host has not drained yet
    /// ([`crate::world::World::take_muscle_hub_sounds`]).
    pub(crate) muscle_hub_sounds: crate::muscle_ringside::HubTimersFrame,
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
    pub(crate) dance_countin: Option<crate::dance::CountIn>,
    /// The count-in banner envelope the last dance tick produced, for a
    /// host's draw list. `None` outside the count-in.
    pub dance_countin_banner: Option<crate::dance::CountInBanner>,
    /// The `GO!` banner's brightness (`acc * 2`, `FUN_801cf470` states 4 / 5)
    /// the last dance tick produced. `None` outside those states.
    pub dance_countin_go: Option<i32>,
    /// The global `music_01` track the dance's own overlay loads, held until
    /// the count-in ends (retail starts the song when the banner clears).
    /// Chosen by song length in [`crate::world::World::enter_dance`].
    pub(crate) dance_pending_bgm: Option<u16>,
    /// The Disco King **how-to** tutorial actor, installed by
    /// [`crate::world::World::enter_dance`] when the parsed game is a
    /// [`crate::dance::DanceMode::HowTo`] run and stepped beside the session.
    pub(crate) dance_tutorial: Option<crate::dance_tutorial::DanceTutorial>,
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
    /// What the dance entry staged over the walked-in scene's globals, `Some`
    /// exactly while a dance runs - see [`crate::dance_venue::sync_dance_venue`].
    pub dance_venue: Option<crate::dance_venue::DanceVenueStage>,
    /// Staging counter behind [`crate::dance_venue::DanceVenueStage::generation`].
    pub(crate) dance_venue_generation: u32,
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
    /// The fishing **venue actors** (wander fish, reeling line, sub-screen
    /// sway), stepped by [`crate::fishing_venue::tick_fishing_venue`] from
    /// each host's minigame frame so the ripples, bursts, venue camera and
    /// swaying prize panel reach every host that runs the pond.
    pub fishing_venue: crate::fishing_venue::FishingVenue,
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
        self.dance.is_some() && self.dance_countin.is_none()
    }

    /// The dance HUD's retail textured quads for this frame
    /// ([`crate::dance::DanceGame::hud_draw_quads`]) when a host can draw
    /// them: the HUD is up ([`Self::dance_status_visible`]) and the hall's
    /// HUD page is resident ([`Self::dance_hud_art_staged`]). Empty
    /// otherwise, which is when a host draws the frame's text rows
    /// (`DanceGame::hud_frame_rows`) instead - the either/or the count-in
    /// banner takes, off the same flag, on every host.
    pub fn dance_hud_quads(&self) -> Vec<crate::dance::DanceHudQuad> {
        if !self.dance_hud_art_staged || !self.dance_status_visible() {
            return Vec::new();
        }
        self.dance
            .as_ref()
            .map(|g| g.hud_draw_quads(g.rival_hud_visible()))
            .unwrap_or_default()
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
            fishing_prize_venues: None,
            fishing_hub_text: None,
            fishing_sprites: None,
            fishing_captions: None,
            slot_machine: None,
            slot_return_mode: SceneMode::Field,
            slot_sfx_bundle: Vec::new(),
            muscle_sfx_bundle: Vec::new(),
            baka_fighter: None,
            baka_return_mode: SceneMode::Field,
            muscle_dome: None,
            muscle_return_mode: SceneMode::Field,
            muscle_contest: None,
            muscle_settlement: None,
            muscle_ringside_still: None,
            muscle_hub_between_legs: false,
            muscle_playback_frames: 0,
            muscle_hub: Default::default(),
            muscle_hub_sounds: Default::default(),
            casino_coins: 0,
            point_card: 0,
            scene_backup: None,
            winnings: 0,
            pending_warp: None,
            dance_countin: None,
            dance_countin_banner: None,
            dance_countin_go: None,
            dance_pending_bgm: None,
            dance_tutorial: None,
            dance_tutorial_frame: None,
            pending_sfx: Vec::new(),
            dance_hud_art_staged: false,
            dance_venue: None,
            dance_venue_generation: 0,
            fx: crate::minigame_fx::MinigameFxPool::new(),
            fishing_venue: crate::fishing_venue::FishingVenue::default(),
        }
    }
}

impl Default for MinigameState {
    fn default() -> Self {
        Self::new()
    }
}

/// The fishing HUD's lure-row text as `FUN_801D13F0` prints it: the three
/// lure labels (each the item name its `0xC2` token names), the caption
/// before the count and the one after it - all off the user's disc
/// ([`legaia_asset::fishing_captions`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FishingCaptionText {
    pub lure_names: [String; 3],
    pub lures_left: String,
    pub suffix: String,
    /// The species names, by species id - each record's `+0x00` string
    /// (`legaia_asset::fishing_species`), which the landed catch's result
    /// plate draws (`FUN_801D5298` -> `FUN_801D73B8`). Empty until
    /// [`Self::with_species_names`] resolves them.
    pub species_names: Vec<String>,
}

impl FishingCaptionText {
    /// Resolve the overlay's raw row against the item-name table. Bytes
    /// outside printable ASCII are dropped (the HUD draws with the ASCII
    /// font layout).
    pub fn resolve(
        raw: &legaia_asset::fishing_captions::FishingCaptionsRaw,
        item_name: impl Fn(u8) -> Option<String>,
    ) -> Self {
        let ascii = |b: &[u8]| {
            b.iter()
                .filter(|c| c.is_ascii_graphic() || **c == b' ')
                .map(|&c| c as char)
                .collect::<String>()
        };
        Self {
            lure_names: raw.lure_items.map(|id| item_name(id).unwrap_or_default()),
            lures_left: ascii(&raw.lures_left),
            suffix: ascii(&raw.suffix),
            species_names: Vec::new(),
        }
    }

    /// Species `id`'s name; empty when it did not resolve.
    pub fn species_name(&self, id: usize) -> &str {
        self.species_names.get(id).map(String::as_str).unwrap_or("")
    }

    /// The same text with the species names resolved out of the as-loaded
    /// fishing overlay. A record whose name pointer does not resolve keeps
    /// an empty name, so the indices stay species ids.
    pub fn with_species_names(mut self, overlay: &[u8]) -> Self {
        use legaia_asset::fishing_species;
        self.species_names = fishing_species::parse(overlay)
            .unwrap_or_default()
            .iter()
            .map(|sp| sp.name(overlay).unwrap_or_default().to_string())
            .collect();
        self
    }
}

/// The SFX cues one pond event raises, in order: the hook cue on a strike
/// (`_DAT_8007B6DA`), and on a catch the celebration cue plus whichever of
/// the four score-gated burst cues it unlocked (`FUN_801d4948`). Splash,
/// snap and recast raise none here (the splash is a spawn into the effect
/// pool, not a cue).
///
/// One answer for every host that runs a pond: [`World::tick`] queues these
/// for both play hosts, and the standalone minigames page keys them off its
/// own session's events - which played no hook or catch sound at all before.
pub fn pond_event_cues(e: &crate::fishing::PondEvent) -> Vec<u16> {
    use crate::fishing::PondEvent;
    match *e {
        PondEvent::Hooked(_) => vec![u16::from(crate::fishing_actors::HOOK_CUE)],
        PondEvent::Landed(points) => {
            let mut cues = vec![u16::from(crate::fishing_actors::CELEBRATE_CUE)];
            cues.extend(
                crate::fishing_actors::celebration_bursts(points)
                    .filter_map(|b| b.cue)
                    .map(u16::from),
            );
            cues
        }
        PondEvent::Splash | PondEvent::Snapped | PondEvent::Recast => Vec::new(),
    }
}
