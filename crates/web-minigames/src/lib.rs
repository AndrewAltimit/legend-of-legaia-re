//! `LegaiaMinigames` WASM bindings for site/minigames.html - the browser twin
//! of the play-window's minigame entry points (`window/minigames.rs`). The
//! play page's runtime (`legaia-web-play`) holds one of these for the
//! minigame art it draws over a door-warp minigame.
//!
//! Three of the game's side-games are ported rules engines in
//! `legaia-engine-core`, each driven entirely by a table baked into its own
//! runtime overlay:
//!
//! | Game | Overlay | Table |
//! |---|---|---|
//! | Noa's dance | PROT 0980 | step chart (`legaia_minigame_formats::dance_chart`) |
//! | Baka Fighter | PROT 0976 | roster + action tables (`legaia_minigame_formats::baka_opponents`) |
//! | Casino slots | PROT 0975 | per-symbol payout table (`legaia_minigame_formats::slot_payout`) |
//!
//! This crate is the thin JSON shell over those engines. The load path is
//! identical to the play-window's and to the disc-gated `*_minigame_real`
//! tests: read the raw PROT entry out of the user's own disc, lift it to its
//! statically-recovered loaded form via [`static_overlay::as_loaded`], and hand
//! the bytes to the table parser. No table is shipped with the site - every
//! number a game plays with is read out of the disc the visitor supplied, in
//! their browser, and never leaves it.
//!
//! The engines are the rules; the JS page is the presentation. Everything with
//! a `// PORT:` provenance tag lives in `legaia-engine-core`, not here.

#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::AudioSink;
use wasm_bindgen::prelude::*;

// Baka Fighter presentation exports (fighter meshes, HUD art, stage, poses)
// live in a child module so this file stays the rules-engine shell.
mod baka_presentation;
pub use baka_presentation::{baka_state_json_for, duel_surface};

// Dance presentation exports (PROT 1230 HUD art, the overlay's widget table,
// the dancer face-stamp rig, SFX + BGM) live in a child module too.
mod dance_presentation;

// Fishing + Muscle Dome are the other two ported rules engines
// (`legaia_engine_fishing::fishing` / `muscle_dome`). Each gets a thin JSON shell
// in its own child module, mirroring the dance/baka split above; the session
// state lives on `LegaiaMinigames` and the wasm-bindgen methods that drive it
// are in these files.
mod fishing_web;
// The fishing venue's 3D scene + angler body (the browser twin of the dance
// hall / Baka stage builds) lives in its own child module.
mod fishing_scene_web;
mod muscle_web;

mod baka_session;
mod bgm;
mod dance_session;
mod disc_load;
mod live_spu;
mod slot_presentation;
mod slot_session;
mod slot_sound;
pub use bgm::Music01Render;
pub(crate) use bgm::{music01_pair_ok, render_music01_bgm};

/// Log a line to the browser console. Every caller is a wasm-only path
/// (`web_sys::console` is a wasm-bindgen import, and calling it off-wasm
/// aborts the process), so the helper only exists on that target.
#[cfg(target_arch = "wasm32")]
fn console_log(s: &str) {
    web_sys::console::log_1(&JsValue::from_str(s));
}

use legaia_engine_minigames::baka_fighter::{BakaAttack, BakaFight, MatchPhase};
use legaia_engine_minigames::dance::{DanceDir, DanceEvent, DanceGame};
use legaia_engine_minigames::slot_machine::{SlotMachine, SlotPhase};
use legaia_minigame_formats::fishing_species::FishingSpecies;
use legaia_minigame_formats::minigame_art::{self, SlotHudWidget};
use legaia_minigame_formats::minigame_sfx::{self, SfxCueBank};
use legaia_minigame_formats::minigame_slot_scene::{self as slot_scene, SlotScene};
use legaia_overlay_images::static_overlay;
use legaia_tim::Tim;

/// The three side-games playable in the browser, plus the disc they read.
#[wasm_bindgen]
pub struct LegaiaMinigames {
    /// Extracted `PROT.DAT` bytes (the games only ever need PROT entries).
    prot: Vec<u8>,
    /// PROT TOC.
    entries: Vec<legaia_prot::toc_index::EntryMeta>,

    /// Live dance run.
    dance: Option<DanceGame>,
    /// The pre-song count-in (READY, then `GO!`), the engine's
    /// `dance::CountIn` both play hosts run - armed by [`Self::dance_start`],
    /// stepped by [`Self::dance_countin_step`].
    dance_countin: Option<legaia_engine_minigames::dance::CountIn>,
    /// A how-to run's Disco King tutorial actor ([`Self::dance_start_mode`]).
    dance_tutorial: Option<legaia_engine_minigames::dance_tutorial::DanceTutorial>,
    /// The tutorial's last frame, for [`Self::dance_tutorial_rgba`].
    dance_tutorial_frame: Option<legaia_engine_minigames::dance_tutorial::TutorialFrame>,
    /// Live Baka Fighter duel.
    baka: Option<BakaFight>,
    /// The page's winnings accumulator for a cabinet run - retail's mode-24
    /// `_DAT_80084440`, the coin prize the play hosts keep on
    /// `World::minigames.winnings`. [`BakaFight::frame`] adds the tally's
    /// drain into it and clears it on a forfeit; the cabinet reads it back as
    /// the pot at risk on its NEXT GAME / PAY OUT sheet.
    baka_winnings: u32,
    /// Game frames this page has stepped ([`Self::drain_sim_steps`]), the
    /// frame count a cabinet's RNG seed folds in - the play hosts fold in the
    /// world frame the same way.
    sim_frames: u32,
    /// The duel's 3D surface (`engine-minigame-scenes::baka_duel_scene`) - the same
    /// per-host cache the native window and the play page pose the duel
    /// through, here over this page's own fight.
    baka_surface: legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelSurface,
    /// The dance floor's cast surface (`engine-core::dance_cast_scene`) over
    /// this page's run - the per-host cache the native window and the play
    /// page pose the bodies through, off the run's own clip driver.
    dance_surface: legaia_engine_minigame_scenes::dance_cast_scene::DanceCastSurface,
    /// The duel's CD-XA announcer lines (`XA32` / `XA33`), decoded at disc
    /// load while the raw sectors are still in hand.
    baka_xa: legaia_engine_audio::XaClipBank,
    /// The Muscle Dome hub's live first visit, stepped by the page
    /// (`muscle_first_visit_reset` / `muscle_first_visit_step`).
    muscle_hub: Option<legaia_engine_minigames::muscle_ringside::FirstVisitHub>,
    /// Announcer lines the live first visit has started.
    muscle_hub_xa_fired: u32,
    /// Announcer lines started (a read-out for the page's checks).
    baka_xa_fired: u32,
    /// Parsed Baka roster + action tables (cached; the roster picker reads them
    /// before a fight starts).
    baka_tables: Option<(
        Vec<legaia_minigame_formats::baka_opponents::BakaOpponent>,
        Vec<legaia_minigame_formats::baka_opponents::BakaActionSet>,
    )>,
    /// Live slot-machine session.
    slot: Option<SlotMachine>,
    /// Parsed slot payout table (cached; the paytable panel reads it before a
    /// session starts).
    slot_payouts: Option<legaia_minigame_formats::slot_payout::SlotPayoutTable>,
    /// The slot machine's five-TIM art pack (PROT 1200) - the reel symbols,
    /// cabinet, digit font and banners the retail machine draws with.
    slot_art: Option<Vec<Tim>>,
    /// The 3 HUD widget descriptors out of the slot overlay (PROT 0975).
    slot_hud: Option<Vec<SlotHudWidget>>,
    /// The machine's 3D scene graph: paylines, medallions, lamps, marquee
    /// billboards and the dot-matrix message bank (PROT 0975 + art page 3).
    slot_scene: Option<SlotScene>,
    /// The machine's whole resident set for the shared screen-prim builder
    /// (`legaia_engine_ui::slot_cabinet`): art-pack VRAM, scene graph,
    /// HUD records and the cabinet mesh - what the native window draws from.
    pub(crate) slot_cabinet: Option<legaia_engine_ui::slot_cabinet::SlotCabinetAssets>,
    /// The marquee's counters for this page's own machine (the play hosts'
    /// `SlotMarqueeClock` twin) and the dot buffer they composed this frame.
    slot_clock: legaia_engine_ui::slot_cabinet::SlotMarqueeClock,
    slot_dots: Vec<u8>,
    /// The retail dialog font (PROT.DAT font TIM + the SCUS width table), for
    /// the slot machine's rules pages - the play hosts draw the same text
    /// through their own copy.
    slot_font: Option<legaia_font::Font>,
    /// The slot machine's own SFX cue bank (descriptors from PROT 1199, samples
    /// from the PROT 1198 VAB).
    slot_sfx: Option<SfxCueBank>,
    /// The Baka Fighter roster's fighter names (roster record `+0x00`).
    baka_names: Option<Vec<String>>,
    /// The dance's presentation bundle: PROT 1230 art pack + the overlay's
    /// HUD widget table + face rigs + SFX bank (see `dance_presentation.rs`).
    dance_pres: Option<dance_presentation::DancePresentation>,
    /// The dance's floor cast: Noa's field-form mesh + the dance-hall scene's
    /// dedicated dancer NPCs, their choreography ANM bundle and the VRAM they
    /// sample (see `dance_presentation.rs`).
    dance_bodies: Option<dance_presentation::DanceBodies>,

    /// Parsed per-species table (PROT 0972 rodata; cached so the roster panel
    /// and each recast read it without re-decoding).
    fishing_species: Option<Vec<FishingSpecies>>,
    /// The as-loaded fishing overlay image, kept so the species-name pointers
    /// (`FishingSpecies::name`) resolve against it.
    fishing_overlay: Option<Vec<u8>>,
    /// Live venue-faithful pond session (the retail cast/band/strike/fight
    /// loop; see `fishing_web.rs`).
    fishing_pond: Option<legaia_engine_fishing::fishing::PondSession>,
    /// Parsed per-venue species-spawn tables (PROT 0972 rodata pages).
    fishing_spawn: Option<[Vec<[u32; 8]>; 2]>,
    /// Parsed reel-cadence gesture templates (PROT 0972 rodata).
    fishing_cadence: Option<Vec<legaia_minigame_formats::fishing_species::CadenceTemplate>>,
    /// Parsed point-exchange prize pages (PROT 0972 rodata).
    fishing_exchange: Option<legaia_minigame_formats::fishing_exchange::FishingExchange>,
    /// The pond session's HUD banner timers (engine-ui's retail animators).
    fishing_banners: legaia_engine_ui::FishingBanners,
    /// The fishing HUD's sprite table off PROT 0972
    /// ([`legaia_minigame_formats::fishing_sprites`]).
    fishing_sprites: Option<Vec<legaia_minigame_formats::fishing_sprites::FishingSprite>>,
    /// The last frame's fishing HUD draw list, as `fishing_pond_hud_json`
    /// built it - what `fishing_hud_rgba` rasterises.
    fishing_hud_last: Vec<legaia_engine_ui::HudDraw>,
    /// The lure row's captions off PROT 0972, resolved against the item
    /// table.
    fishing_captions: Option<legaia_engine_minigame_scenes::minigame_state::FishingCaptionText>,
    /// This page's **effect-part pool** - the standalone twin of
    /// `World::minigames.fx`, which this page cannot use because it drives
    /// the session types directly and holds no `World`. Same model
    /// ([`legaia_engine_minigames::minigame_fx`]), same draw builder, ticked from
    /// whichever per-minigame tick is running.
    fx: legaia_engine_minigames::minigame_fx::MinigameFxPool,
    /// Prizes collected through the point exchange this session
    /// (`item_id -> qty`; the browser has no live inventory to grant into).
    fishing_prizes: std::collections::HashMap<u32, u32>,
    /// The fishing venue's baked 3D scene + angler body
    /// (see `fishing_scene_web.rs`).
    fishing_scene: Option<fishing_scene_web::FishingScene>,
    /// The angler's facing (retail's actor `+0x26`), the angle the next cast
    /// offsets its lure along.
    fishing_angler_facing: i16,
    /// This page's lure stock per lure row (`0x9D..=0x9F`): it holds no
    /// bag, so each row opens at the stand-in count and a broken line
    /// debits the equipped row, as retail's break arm consumes one lure.
    fishing_lures_held: [i32; 3],
    /// The venue's shore actors (the lead's ambient ripple roll, the
    /// reeling-line actor and its catch bursts) - the step the play hosts
    /// run on `World::minigames.fishing_venue`, here over this page's own
    /// session and effect pool.
    fishing_venue_actors: legaia_engine_minigame_scenes::fishing_venue::FishingVenue,
    /// SCUS item-name table (present on full-disc loads only), naming the
    /// point-exchange prize rows.
    item_names: Option<legaia_game_tables::item_names::ItemNameTable>,

    /// Live Muscle Dome **leg** (see `muscle_web.rs`): the rules session
    /// plus the disc-derived fighter stats, RNG cursor and round play log.
    muscle: Option<muscle_web::MuscleContest>,
    /// The Muscle Dome **contest** the legs belong to - the ladder run that
    /// decides which `(course, round)` is staged, banks the coin tally and
    /// settles the payout. Shared kernel
    /// ([`legaia_engine_minigames::muscle_dome::DomeContest`]), so the browser and
    /// the native window run one model rather than two.
    muscle_run: Option<legaia_engine_minigames::muscle_dome::DomeContest>,
    /// The player's casino coin bank, so a settled contest has somewhere to
    /// pay into (the native host banks into `World::minigames.casino_coins`).
    muscle_coins: u32,
    /// The dome fighter's two saved command strings (record `+0x1A7` /
    /// `+0x1B7`): this page holds no record, so it keeps them across legs
    /// here - each leg opens over them and banks its review confirms back.
    muscle_auto_strings: ([u8; 16], [u8; 16]),
    /// The **special-battle word** `0x8007BAC0` the next leg opens on - the
    /// value retail's arena entry seeds from the three course-unlock story
    /// flags (`legaia_engine_minigames::muscle_dome::contest_entry_word`). The
    /// native host reads the flags out of `World`; this page has no save, so
    /// the word is latched from the `unlock` mask the page hands
    /// `muscle_contest_start`. `0` until a contest is opened, which forbids
    /// nothing.
    muscle_special: u32,
    /// The last settled contest, kept so the page can show the payout.
    muscle_settlement: Option<legaia_engine_minigames::muscle_dome::ContestSettlement>,
    /// The Muscle Dome's cached battle tables (deck hand ids, move-power table
    /// + id map, element-affinity matrix - all PROT 0898 rodata), decoded once
    /// per disc so each fresh contest rebuilds without re-decoding.
    muscle_tables: Option<muscle_web::MuscleTables>,
    /// The dome leg's 3D surface - the engine kernel the play hosts draw a
    /// leg with (`muscle_web/surface.rs`).
    muscle_surface: legaia_engine_minigame_scenes::muscle_dome_scene::MuscleDomeSurface,

    /// `SCUS_942.54` bytes, kept when the input was a full disc image (absent
    /// for a raw `PROT.DAT` load). The Muscle Dome reads the new-game
    /// starting-party template + per-level growth curves out of it to seed the
    /// player fighter's battle stats, and the spell-name table to name the
    /// reward; games degrade (and say so) without it.
    scus: Option<Vec<u8>>,

    /// This page's **live SPU**, the way the play page's `play_sfx` owns one.
    ///
    /// Until it existed the page's whole audio surface was offline: a track
    /// rendered to PCM and handed to an `AudioBufferSourceNode`, and per-cue
    /// PCM decoded from a VAB and played the same way. That covers a cue that
    /// names itself by id and nothing else - and the Muscle Dome's between-leg
    /// tally names no id at all. `FUN_801D1288` resolves a whole
    /// `(voice, VAB id, program, tone, note, fine, vol_l, vol_r)` set, which
    /// is exactly `legaia_engine_audio::VoiceAttr`, and keying one needs an
    /// `Spu` to key it *into*.
    ///
    /// Opened lazily from a user gesture ([`Self::minigame_audio_open`]) -
    /// browser autoplay policy - and `None` off wasm32, where `WebAudioOut`
    /// does not exist.
    #[cfg(target_arch = "wasm32")]
    audio_out: Option<legaia_engine_audio::WebAudioOut>,
    /// The SFX program bank staged into that SPU, keyed by the descriptor
    /// table's VAB **slot** (`sfx_table::slot_for_category`). Uploaded once
    /// per slot out of one allocator so several banks pack rather than
    /// overlap - the same rule the play page's SFX region follows.
    #[cfg(target_arch = "wasm32")]
    sfx_vabs: std::collections::HashMap<u8, legaia_engine_audio::VabBank>,
    /// Allocator for that region.
    #[cfg(target_arch = "wasm32")]
    sfx_alloc: Option<legaia_engine_audio::spu::ram::SpuAllocator>,
    /// The static cue descriptors (`SCUS_942.54`'s `DAT_8006F198` table),
    /// lifted into the audio layer's bank once a disc with an executable
    /// loads. The catalog path ([`Self::minigame_sfx_cue`]) resolves through
    /// it; the voice-attr path does not need it.
    #[cfg(target_arch = "wasm32")]
    sfx_bank: Option<legaia_engine_audio::SfxBank>,
    /// Rotating voice counter for the tally cue - retail's `DAT_801D1AE4`,
    /// which is a free-running `u32` masked only when it picks the slot.
    /// Held on the page host because the ramp is replayed from its screen
    /// tick rather than stepped once per frame.
    #[cfg(target_arch = "wasm32")]
    muscle_tally_cue_counter: u32,
    /// How many tally steps this screen has already keyed, so a replay to
    /// `round` keys only the steps it has not.
    muscle_tally_voiced_steps: i32,
    /// The INTERVAL screen tick the tally's fast-forward latch was raised
    /// on - retail's `DAT_801D1AB4`, which the hub's tally arm sets on a
    /// press and nothing lowers until the hub is entered again. This page
    /// replays the roll from a tick count, so the latch is kept as the tick
    /// it rose on: every replayed step at or past it runs boosted.
    muscle_tally_boost_from: Option<i32>,
    /// The shared wall-clock to sim-tick rule
    /// ([`legaia_engine_field::frame_step::SimStepper`]) the native window and
    /// the play page drain through: the page's animation loop asks it how
    /// many 60 Hz game frames each display frame runs.
    sim_stepper: legaia_engine_field::frame_step::SimStepper,
}

impl Default for LegaiaMinigames {
    fn default() -> Self {
        Self::new()
    }
}

/// Read one overlay's **as-loaded** image out of the PROT bytes, exactly as the
/// SCUS loader would: the entry's full on-disc footprint (the web `parse_prot_toc`
/// already honours the extended window), lifted through the static-overlay map so
/// an LZS-form overlay decompresses to its runtime size.
fn overlay_image(
    prot: &[u8],
    entries: &[legaia_prot::toc_index::EntryMeta],
    prot_index: u32,
) -> Option<Vec<u8>> {
    let rec = static_overlay::overlay_map().by_prot_index(prot_index)?;
    let meta = entries.iter().find(|e| e.index == prot_index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    let raw = prot.get(off..end.min(prot.len()))?;
    static_overlay::as_loaded(raw, rec).ok()
}

/// Read one PROT entry's **raw** on-disc bytes (no static-overlay lift - this is
/// for plain data entries like the art packs, not code overlays).
fn entry_bytes<'a>(
    prot: &'a [u8],
    entries: &[legaia_prot::toc_index::EntryMeta],
    prot_index: u32,
) -> Option<&'a [u8]> {
    let meta = entries.iter().find(|e| e.index == prot_index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    prot.get(off..end.min(prot.len()))
}

/// JSON-escape a string for the hand-rolled object writers below.
fn jstr(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl LegaiaMinigames {
    /// The slot cabinet's decoded art, once a slot session has loaded it.
    /// The play page's slot overlay draws the cabinet chrome from it.
    pub fn slot_cabinet(&self) -> Option<&legaia_engine_ui::slot_cabinet::SlotCabinetAssets> {
        self.slot_cabinet.as_ref()
    }

    /// The slot cabinet's art seat, for a host that completes the decoded
    /// art from a source the minigames page does not hold (the play page's
    /// full `PROT.DAT` head).
    pub fn slot_cabinet_mut(
        &mut self,
    ) -> &mut Option<legaia_engine_ui::slot_cabinet::SlotCabinetAssets> {
        &mut self.slot_cabinet
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// How many 60 Hz game frames this display frame runs, given the wall
    /// time since the previous call in **milliseconds** - the shared
    /// [`legaia_engine_field::frame_step::SimStepper`], the play page's
    /// `play_drain_sim_steps` and the native window's redraw drain. A gap
    /// longer than the stepper's backlog cap is dropped, not carried.
    ///
    /// The page stepped every game once per `requestAnimationFrame`, so on a
    /// 120 Hz display the dance song, the Baka duel, the reels and the dome
    /// all ran at twice retail speed (and the fishing loop's own wall clock
    /// rounded a half frame up to a whole one).
    pub fn drain_sim_steps(&mut self, elapsed_ms: f64) -> u32 {
        let n = self.sim_stepper.drain(elapsed_ms / 1000.0);
        self.sim_frames = self.sim_frames.wrapping_add(n);
        n
    }

    /// The RNG seed a session of `game` (`"fishing"`, `"muscle"`) starts on:
    /// the salt the play hosts fold the world frame into
    /// ([`legaia_engine_mode::minigame_entry::FISHING_SEED_SALT`] /
    /// [`legaia_engine_mode::minigame_entry::DOME_SEED_SALT`]), folded with
    /// this page's stepped-frame count. The page used to seed off
    /// `Date.now()` / `Math.random()`, a rule of its own. `0` for an unknown
    /// game.
    pub fn minigame_seed(&self, game: &str) -> u32 {
        use legaia_engine_mode::minigame_entry::{DOME_SEED_SALT, FISHING_SEED_SALT};
        match game {
            "fishing" => FISHING_SEED_SALT ^ self.sim_frames,
            "muscle" => DOME_SEED_SALT ^ self.sim_frames,
            _ => 0,
        }
    }

    /// Drop the undrained backlog - a game was (re)started or the tab was
    /// hidden, and the gap must not come back as catch-up frames.
    pub fn resync_sim_clock(&mut self) {
        self.sim_stepper.resync();
    }

    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        Self {
            prot: Vec::new(),
            entries: Vec::new(),
            sim_stepper: legaia_engine_field::frame_step::SimStepper::new(),
            dance: None,
            dance_countin: None,
            dance_tutorial: None,
            dance_tutorial_frame: None,
            baka: None,
            baka_winnings: 0,
            sim_frames: 0,
            baka_surface: Default::default(),
            dance_surface: Default::default(),
            baka_xa: legaia_engine_audio::XaClipBank::new(),
            muscle_hub: None,
            muscle_hub_xa_fired: 0,
            baka_xa_fired: 0,
            baka_tables: None,
            slot: None,
            slot_payouts: None,
            slot_art: None,
            slot_hud: None,
            slot_scene: None,
            slot_cabinet: None,
            slot_clock: Default::default(),
            slot_dots: Vec::new(),
            slot_font: None,
            slot_sfx: None,
            baka_names: None,
            dance_pres: None,
            dance_bodies: None,
            fishing_species: None,
            fishing_overlay: None,
            fishing_pond: None,
            fishing_spawn: None,
            fishing_cadence: None,
            fishing_exchange: None,
            fishing_banners: Default::default(),
            fishing_sprites: None,
            fishing_hud_last: Vec::new(),
            fishing_captions: None,
            fx: Default::default(),
            fishing_prizes: Default::default(),
            fishing_scene: None,
            // The lead's rest facing (`0x800`, into the water).
            fishing_angler_facing: 0x800,
            fishing_lures_held: [fishing_web::MINIGAMES_TACKLE_HELD; 3],
            fishing_venue_actors: Default::default(),
            item_names: None,
            muscle: None,
            muscle_run: None,
            muscle_coins: 0,
            muscle_auto_strings: ([0; 16], [0; 16]),
            muscle_special: 0,
            muscle_settlement: None,
            muscle_surface: Default::default(),
            muscle_tables: None,
            scus: None,
            #[cfg(target_arch = "wasm32")]
            audio_out: None,
            #[cfg(target_arch = "wasm32")]
            sfx_vabs: std::collections::HashMap::new(),
            #[cfg(target_arch = "wasm32")]
            sfx_alloc: None,
            #[cfg(target_arch = "wasm32")]
            sfx_bank: None,
            #[cfg(target_arch = "wasm32")]
            muscle_tally_cue_counter: 0,
            muscle_tally_voiced_steps: 0,
            muscle_tally_boost_from: None,
        }
    }

    /// This frame's live **effect parts**, for the page to draw:
    ///
    /// ```json
    /// [ { "x": 152, "y": 112, "sprite": 256, "fade": 255 }, ... ]
    /// ```
    ///
    /// `x` / `y` are stage pixels in the retail 320x240 frame and `fade` is
    /// `0..=255`. The pool, its bound and its ramp are
    /// [`legaia_engine_minigames::minigame_fx`], the same model the play page and
    /// the native window drain - so a part spawned by a ported kernel ages
    /// identically on all three surfaces.
    pub fn minigame_fx_json(&self) -> String {
        let rows = self
            .fx
            .frames()
            .iter()
            .map(|p| {
                format!(
                    r#"{{"x":{},"y":{},"sprite":{},"fade":{}}}"#,
                    p.x, p.y, p.sprite, p.fade
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }
}

// ---------------------------------------------------------------- save bar

#[wasm_bindgen]
impl LegaiaMinigames {
    /// One of the three retail 16x16 **save-file portrait** TIMs as a 1024-byte
    /// RGBA8 buffer: `0` = Vahn, `1` = Noa, `2` = Gala.
    ///
    /// These are the load-screen slot-grid portraits, pinned in the unindexed
    /// pre-`init_data` gap of `PROT.DAT` (offset `0x1AC90`, 192-byte stride;
    /// `legaia_ui_formats::title_pak::extract_overlay_load_portrait_tim`). Retail
    /// bakes the lead's copy into every SC save block as the memory-card icon,
    /// so these are exactly the faces a retail save carries. The site's save
    /// bar decodes them once from the visitor's own disc and caches the pixels
    /// locally - no art ships with the page. Empty when no disc is loaded or
    /// the TIM doesn't parse.
    pub fn save_portrait_rgba(&self, char_id: usize) -> Vec<u8> {
        use legaia_ui_formats::title_pak::{
            OVERLAY_LOAD_PORTRAIT_COUNT, OVERLAY_LOAD_PORTRAIT_STRIDE,
            OVERLAY_LOAD_PORTRAIT_TIM_OFFSET,
        };
        if char_id >= OVERLAY_LOAD_PORTRAIT_COUNT {
            return Vec::new();
        }
        let off = OVERLAY_LOAD_PORTRAIT_TIM_OFFSET + char_id * OVERLAY_LOAD_PORTRAIT_STRIDE;
        let Some(tim_bytes) = self.prot.get(off..off + OVERLAY_LOAD_PORTRAIT_STRIDE) else {
            return Vec::new();
        };
        let Ok(parsed) = legaia_tim::parse(tim_bytes) else {
            return Vec::new();
        };
        legaia_tim::decode_rgba8(&parsed, 0).unwrap_or_default()
    }
}

/// Blit `draws` (320x240 stage pixels, [`legaia_engine_ui::TextDraw`]) onto a
/// `w x h` RGBA8 frame through `font`'s atlas - the CPU twin of the play
/// hosts' text pass, nearest-sampled and alpha-blended.
fn blit_text(
    px: &mut [u8],
    w: u32,
    h: u32,
    font: &legaia_font::Font,
    draws: &[legaia_engine_ui::TextDraw],
) {
    let (aw, ah) = font.atlas_dimensions();
    let atlas = font.atlas_rgba();
    let sx = w as f32 / 320.0;
    let sy = h as f32 / 240.0;
    for d in draws {
        let (dx, dy, dw, dh) = d.dst;
        let (u, v, uw, vh) = d.src;
        let x0 = (dx as f32 * sx) as i32;
        let x1 = ((dx + dw as i32) as f32 * sx) as i32;
        let y0 = (dy as f32 * sy) as i32;
        let y1 = ((dy + dh as i32) as f32 * sy) as i32;
        for y in y0.max(0)..y1.min(h as i32) {
            for x in x0.max(0)..x1.min(w as i32) {
                let tu = u + ((x - x0) as u32 * uw) / (x1 - x0).max(1) as u32;
                let tv = v + ((y - y0) as u32 * vh) / (y1 - y0).max(1) as u32;
                if tu >= aw || tv >= ah {
                    continue;
                }
                let a_i = ((tv * aw + tu) * 4) as usize;
                // The shared overlay tint: retail's 5-bit modulation and the
                // font CLUTs' own shadows (`ui_tint`), as both play hosts draw.
                let tinted = legaia_render_kernels::ui_tint::tint_texel(
                    [atlas[a_i], atlas[a_i + 1], atlas[a_i + 2], atlas[a_i + 3]],
                    [d.color[0], d.color[1], d.color[2]],
                    true,
                );
                let a = tinted[3] as f32 / 255.0 * d.color[3];
                if a <= 0.0 {
                    continue;
                }
                let o = ((y as u32 * w + x as u32) * 4) as usize;
                for k in 0..3 {
                    let src = f32::from(tinted[k]);
                    px[o + k] = (px[o + k] as f32 * (1.0 - a) + src * a) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod countin_tests {
    use super::*;

    /// The minigames page counts in through the engine's `CountIn`: READY
    /// with the intro cue, then `GO!` with the run-start cue, then `done`.
    #[test]
    fn the_page_count_in_is_the_engine_count_in() {
        let mut mg = LegaiaMinigames::new();
        assert_eq!(mg.dance_countin_step(), vec![0; 8], "no run, no count-in");
        mg.dance_countin = Some(legaia_engine_minigames::dance::CountIn::new());
        let (mut cues, mut saw_ready, mut saw_go, mut vsyncs) = (Vec::new(), false, false, 0);
        loop {
            let s = mg.dance_countin_step();
            vsyncs += 1;
            saw_ready |= s[1] == 1;
            saw_go |= s[5] >= 0;
            if s[6] != 0 {
                cues.push(s[6]);
            }
            if s[7] == 1 {
                break;
            }
        }
        assert!(saw_ready && saw_go);
        assert_eq!(cues, vec![0x200, 0x201]);
        assert_eq!(vsyncs, legaia_engine_minigames::dance::COUNTIN_TOTAL_VSYNCS);
        assert_eq!(mg.dance_countin_step()[0], 0, "the song runs after done");
    }

    /// The page's tracks are the engine's: the literals it used to carry are
    /// what `standalone_bgm_id` answers.
    #[test]
    fn the_page_reads_its_tracks_from_the_engine() {
        let mg = LegaiaMinigames::new();
        assert_eq!(mg.minigame_bgm_id("baka"), 2055);
        assert_eq!(mg.minigame_bgm_id("slot"), 2018);
        assert_eq!(mg.minigame_bgm_id("muscle"), 2026);
        assert_eq!(mg.minigame_bgm_id("dance"), 2060);
        assert_eq!(mg.minigame_bgm_id("fishing"), -1);
        assert_eq!(mg.minigame_bgm_id("nope"), -1);
    }
}
