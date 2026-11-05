//! Minigame states (`OTHER MODE`, game mode `0x19`): which minigame a capture
//! runs, how far the engine can be seeded into it, and the channels it is
//! scored on.
//!
//! The minigame is named by the mode-24 door warp's sub-id `_DAT_8007BA34`
//! (field-VM op `0x3E`, `0x801E07B8`), which selects the overlay
//! ([`legaia_engine_mode::minigame_entry::MinigameSubId`]); the scene label
//! stays the departure scene's. The seed lands the save in that scene and
//! takes the same warp (`World::request_minigame_warp`), so the engine enters
//! the session a cabinet installs.
//!
//! Only a session whose captured phase the warp itself reaches is seeded: a
//! phase that needs played input past the entry carries `rand()` history (a
//! slot machine's reel stops) the seed does not replay. That is the slot
//! machine at rest (reel state word `DAT_801D3C84 = 1`, the attract state the
//! init advances to), the dance's count-in (states `3..=5` of `FUN_801CF470`),
//! the contest hub's first visit up to the fight (`FUN_801CF870`), the fishing
//! venue's idle shore (state `0xC` of `FUN_801CF3BC`) and the Baka Fighter's
//! title card (cabinet state `1` of `FUN_801CF388`).

use super::*;
use legaia_engine_mode::minigame_entry::MinigameSubId;

/// The mode-24 door warp's sub-id (`_DAT_8007BA34`, `sh` at `0x801E07B8`).
pub(super) const MINIGAME_SUB_ID: u32 = 0x8007_BA34;
/// The slot machine's reel state word (`DAT_801D3C84`, PROT 0975).
const SLOT_STATE: u32 = 0x801D_3C84;
/// The slot machine's balance (`DAT_801D4114`), assigned from the coin bank
/// at entry.
const SLOT_BALANCE: u32 = 0x801D_4114;
/// The casino coin bank (`_DAT_800845A4`).
const COIN_BANK: u32 = 0x8008_45A4;
/// The battle-return flag the slot init tests (`_DAT_8007B8B8`).
const BATTLE_RETURN_FLAG: u32 = 0x8007_B8B8;
/// The slot machine's attract state.
const SLOT_IDLE: i32 = 1;
/// The dance's game state (`DAT_801D5334`, PROT 0980) and the READY banner
/// counter its state `3` draws (`DAT_801D5158`, `0x801D015C..0x801D0184`).
const DANCE_STATE: u32 = 0x801D_5334;
const DANCE_COUNTER: u32 = 0x801D_5158;
/// The contest hub's arm byte (`DAT_801D1A78`, PROT 0977) and the intro hold
/// counter arm `1` runs in `0x801D1A84`.
const HUB_ARM: u32 = 0x801D_1A78;
const HUB_HOLD: u32 = 0x801D_1A84;
/// The departure scene's define, backed up by the mode-24 init.
const DEPARTURE_DEFINE: u32 = 0x8007_BAC4;
/// The fishing driver's state word (`DAT_801D926C`, PROT 0972).
const FISHING_STATE: u32 = 0x801D_926C;
/// The dance's `GO!` accumulator (`DAT_801D515C`).
const DANCE_GO: u32 = 0x801D_515C;
/// The hub's arm-`5` hold (`0x801D1A70`).
const HUB_CARD_HOLD: u32 = 0x801D_1A70;
/// The Baka Fighter cabinet's state word (`DAT_801DBF44`, PROT 0976).
const BAKA_STATE: u32 = 0x801D_BF44;
/// The title card's clock (`DAT_801DBE94`), the `t` `FUN_801D59D4` draws from.
const BAKA_CARD_CLOCK: u32 = 0x801D_BE94;
/// The tick the seed takes the warp on, after the scene entry settles and
/// ahead of any talk the save's seat would walk into: `koin1`'s sign-up
/// counter opens its refusal line at tick `31` when the player is left there,
/// where retail's warp is the last op of the record that seated him.
const WARP_TICK: u64 = 20;
/// Ticks the entry may take to reach the captured phase.
const ENTRY_DEADLINE: u64 = 900;
/// The image child's frame, past the warp: the captures sample the attract
/// state 300 vsyncs after the field was left.
const FRAME_TICKS_PAST_WARP: u64 = 300;

/// The minigame a capture runs and its scored words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailMinigame {
    /// `_DAT_8007BA34`.
    pub sub_id: i16,
    /// The slot machine's state word and balance, when the slot overlay is
    /// the one resident.
    pub slot: Option<(i32, i32)>,
    /// `_DAT_800845A4`.
    pub coin_bank: u32,
    /// The battle-return flag `_DAT_8007B8B8` is up: the overlay was entered
    /// through the door path, which assigns the balance from the bank.
    pub door_entry: bool,
    /// The dance's, the hub's or the fishing driver's state word with its
    /// phase counter (the READY banner counter, the Welcome hold), when that
    /// overlay is the one resident.
    pub phase: Option<(u32, u32)>,
    /// The phase's second counter: the dance's `GO!` accumulator
    /// `DAT_801D515C`, the hub's arm-`5` hold `0x801D1A70`.
    pub phase2: u32,
    /// The departure scene's define the mode-24 init backed up
    /// (`_DAT_8007BAC4`): the venue overlays load their own block over the
    /// scene word, and the warp starts from this one.
    pub departure_define: u16,
    /// The dance's marker tiles as the displayed frame drew them, by script
    /// row: cursor, countdown and mesh ([`displayed_dance_markers`]).
    pub dance_markers: [Option<[i16; 3]>; legaia_engine_field::dance_marker::MARKER_SCRIPT_ROWS],
    /// The hall's animated props' clip cursors as the RAM holds them, one per
    /// clip ([`dance_prop_clips`]).
    pub dance_props: [Option<(u8, i16)>; DANCE_PROP_CLIPS],
}

/// The hall's animated plain floor actors (tick `FUN_801D4098`) by clip: each
/// clip's `+0x68` cursor. The floor pass spawns them all on one pass at one
/// rate, so the actors of a clip agree; the first found stands for them.
fn dance_prop_clips(ram: &[u8]) -> [Option<(u8, i16)>; DANCE_PROP_CLIPS] {
    let mut out: Vec<(u8, i16)> = Vec::new();
    for n in crate::retail_compare_script::actor_nodes(ram) {
        if game_anchors::u32_at(ram, n + 0x0C) != legaia_engine_minigames::dance::HALL_PROP_TICK {
            continue;
        }
        let Ok(clip) = u8::try_from(game_anchors::i16_at(ram, n + 0x5C)) else {
            continue;
        };
        if clip == 0 || out.iter().any(|&(c, _)| c == clip) {
            continue;
        }
        out.push((clip, game_anchors::i16_at(ram, n + 0x68)));
    }
    out.sort_unstable();
    let mut clips = [None; DANCE_PROP_CLIPS];
    for (slot, c) in clips.iter_mut().zip(out) {
        *slot = Some(c);
    }
    clips
}

/// How many distinct prop clips the seat carries; the hall runs three.
const DANCE_PROP_CLIPS: usize = 8;

/// The marker tile tick (`FUN_801D0640`, PROT 0980).
const DANCE_MARKER_TICK: u32 = 0x801D_0640;
/// Vsyncs a dance game frame steps (`DAT_1F800393`), which ages a tile's
/// countdown; in every dance state the library holds.
const DANCE_FRAME_STEP: i16 = 3;

/// The dance's marker tiles on the frame the capture displays: the RAM's, taken
/// back the display lag's passes. A tile's flipbook is a clock of the hall's
/// game frames, and a game frame's length varies with what it draws: between
/// `minigame_dance_retail_go_in` and `_go_out` the `GO!` accumulator walks `11`
/// steps of `3` vsyncs while the ambient leader takes `8` rows of one pass each.
/// So the count-in's own counter does not name a tile's phase, and the seat reads
/// it.
fn displayed_dance_markers(
    ram: &[u8],
) -> [Option<[i16; 3]>; legaia_engine_field::dance_marker::MARKER_SCRIPT_ROWS] {
    use legaia_engine_field::dance_marker::{MARKER_SCRIPT_ROW_BYTES, MARKER_SCRIPT_VA};
    let mut out = [None; legaia_engine_field::dance_marker::MARKER_SCRIPT_ROWS];
    for n in crate::retail_compare_script::actor_nodes(ram) {
        if game_anchors::u32_at(ram, n + 0x0C) != DANCE_MARKER_TICK {
            continue;
        }
        let class = game_anchors::u16_at(ram, n + 0x50);
        let Some(slot) = out.get_mut(usize::from(class)) else {
            continue;
        };
        let row = MARKER_SCRIPT_VA + u32::from(class) * MARKER_SCRIPT_ROW_BYTES as u32;
        let pair = |i: i32| {
            let a = row + (i * 4) as u32;
            (
                game_anchors::i16_at(ram, a),
                game_anchors::i16_at(ram, a + 2),
            )
        };
        // The row's length: the sentinel that wraps the cursor.
        let len = (0..32).find(|&i| pair(i).0 < 0).unwrap_or(25);
        let mut cursor = i32::from(game_anchors::u16_at(ram, n + 0x9C));
        let mut timer = game_anchors::i16_at(ram, n + 0x54);
        let bias = legaia_scene_formats::field_objects::FIELD_ACTOR_PACK_BIAS as i16;
        let mut mesh = game_anchors::i16_at(ram, n + 0x64);
        for _ in 0..crate::retail_compare_battle::DISPLAY_LAG_PASSES {
            let current = (cursor / 2 - 1).rem_euclid(len);
            if timer + DANCE_FRAME_STEP <= pair(current).1 {
                timer += DANCE_FRAME_STEP;
            } else {
                // The pass that staged this step: before it, the last step
                // stood at zero with its cursor here.
                cursor = current * 2;
                mesh = pair((current - 1).rem_euclid(len)).0 + bias;
                timer = 0;
            }
        }
        *slot = Some([cursor as i16, timer, mesh]);
    }
    out
}

impl RetailMinigame {
    pub(super) fn from_ram(ram: &[u8]) -> Self {
        let sub_id = game_anchors::i16_at(ram, MINIGAME_SUB_ID);
        let slot = (u8::try_from(sub_id)
            .ok()
            .and_then(MinigameSubId::from_sub_id)
            == Some(MinigameSubId::SlotMachine))
        .then(|| {
            (
                game_anchors::u32_at(ram, SLOT_STATE) as i32,
                game_anchors::u32_at(ram, SLOT_BALANCE) as i32,
            )
        });
        let minigame = u8::try_from(sub_id)
            .ok()
            .and_then(MinigameSubId::from_sub_id);
        let phase = match minigame {
            Some(MinigameSubId::Dance) => Some((
                game_anchors::u32_at(ram, DANCE_STATE),
                game_anchors::u32_at(ram, DANCE_COUNTER),
            )),
            Some(MinigameSubId::MuscleDome) => Some((
                u32::from(game_anchors::u32_at(ram, HUB_ARM) as u8),
                game_anchors::u32_at(ram, HUB_HOLD),
            )),
            Some(MinigameSubId::Fishing) => Some((game_anchors::u32_at(ram, FISHING_STATE), 0)),
            Some(MinigameSubId::BakaFighter) => Some((
                game_anchors::u32_at(ram, BAKA_STATE),
                game_anchors::u32_at(ram, BAKA_CARD_CLOCK),
            )),
            _ => None,
        };
        let phase2 = match minigame {
            Some(MinigameSubId::Dance) => game_anchors::u32_at(ram, DANCE_GO),
            Some(MinigameSubId::MuscleDome) => game_anchors::u32_at(ram, HUB_CARD_HOLD),
            _ => 0,
        };
        Self {
            sub_id,
            slot,
            coin_bank: game_anchors::u32_at(ram, COIN_BANK),
            door_entry: game_anchors::u32_at(ram, BATTLE_RETURN_FLAG) != 0,
            phase,
            phase2,
            departure_define: game_anchors::u32_at(ram, DEPARTURE_DEFINE) as u16,
            dance_markers: if minigame == Some(MinigameSubId::Dance) {
                displayed_dance_markers(ram)
            } else {
                Default::default()
            },
            dance_props: if minigame == Some(MinigameSubId::Dance) {
                dance_prop_clips(ram)
            } else {
                [None; DANCE_PROP_CLIPS]
            },
        }
    }

    fn minigame(&self) -> Option<MinigameSubId> {
        u8::try_from(self.sub_id)
            .ok()
            .and_then(MinigameSubId::from_sub_id)
    }

    /// Why the state is not seeded, or `None` when it is.
    pub fn unseeded_reason(&self) -> Option<String> {
        match (self.minigame(), self.slot) {
            // The door path's init assigns the balance from the coin bank
            // (`FUN_801CEC94`); with the battle-return flag clear it is the
            // 70-coin dev fallback instead, a launch outside the casino door
            // the warp does not take.
            (Some(MinigameSubId::SlotMachine), Some(_)) if !self.door_entry => Some(
                "slot machine launched outside the casino door (battle-return flag \
                 _DAT_8007B8B8 clear, the 70-coin dev balance)"
                    .to_string(),
            ),
            // Only a spin or a payout moves the balance off the bank: a
            // machine at rest off it has been played.
            (Some(MinigameSubId::SlotMachine), Some((SLOT_IDLE, balance)))
                if i64::from(balance) == i64::from(self.coin_bank) =>
            {
                None
            }
            (Some(MinigameSubId::SlotMachine), Some((SLOT_IDLE, balance))) => Some(format!(
                "slot machine at rest after play (balance {balance}, coin bank {}): the reel \
                 positions and the balance are rand() history the seed does not replay",
                self.coin_bank
            )),
            (Some(MinigameSubId::SlotMachine), Some((state, _))) => Some(format!(
                "slot machine in reel state {state:#X}: past the attract state the reel stops \
                 and feature rolls are rand() history the seed does not replay"
            )),
            // State `0xC`, the idle shore: the entry itself lands there, and
            // no cast has rolled anything. State `0xB` is the fade in front
            // of it, whose frame still holds what stood before.
            (Some(MinigameSubId::Fishing), _) if matches!(self.phase, Some((0xC, _))) => None,
            (Some(MinigameSubId::Fishing), _) => Some(
                "fishing past the idle shore: the cast, its drift and the bite rolls are \
                 rand() history the seed does not replay"
                    .to_string(),
            ),
            // States `3..=5`: the count-in the warp plays out on its own.
            (Some(MinigameSubId::Dance), _) if matches!(self.phase, Some((3..=5, _))) => None,
            (Some(MinigameSubId::Dance), _) => Some(
                "dance past the count-in: the chart's presses and the rivals' rolls are \
                 history the seed does not replay"
                    .to_string(),
            ),
            // The first visit's arms, no pad in them.
            (Some(MinigameSubId::MuscleDome), _)
                if matches!(self.phase, Some((0..=5 | 0x15, _))) =>
            {
                None
            }
            (Some(MinigameSubId::MuscleDome), _) => Some(
                "contest hub past the first visit: the legs are played input the seed does \
                 not replay"
                    .to_string(),
            ),
            // State `1`: the title card, before any press.
            (Some(MinigameSubId::BakaFighter), _) if matches!(self.phase, Some((1, _))) => None,
            (Some(MinigameSubId::BakaFighter), _) => Some(
                "Baka Fighter past the title card: the rounds are pad input and the AI's \
                 rand() picks"
                    .to_string(),
            ),
            (Some(m), _) => Some(format!(
                "no minigame session seeding path for {m:?} (sub-id {})",
                self.sub_id
            )),
            (None, _) => Some(format!("minigame sub-id {} names no overlay", self.sub_id)),
        }
    }
}

/// Whether the engine's session stands at the phase the capture holds.
fn at_captured_phase(world: &legaia_engine_core::world::World, mg: &RetailMinigame) -> bool {
    use legaia_engine_minigames::muscle_ringside::FirstVisitArm;
    let m = &world.minigames;
    match (mg.minigame(), mg.phase) {
        (Some(MinigameSubId::SlotMachine), _) => {
            world.mode == SceneMode::SlotMachine
                && m.slot_machine.as_ref().map(|s| s.retail_state()) == mg.slot.map(|s| s.0)
        }
        (Some(MinigameSubId::Fishing), _) => {
            world.mode == SceneMode::Fishing
                && m.fishing
                    .as_ref()
                    .is_some_and(|f| f.phase() == legaia_engine_fishing::fishing::PondPhase::Idle)
        }
        (Some(MinigameSubId::Dance), Some((state, counter))) => {
            use legaia_engine_minigames::dance::CountInStage as S;
            // The frame on screen stands the display lag behind the RAM's
            // counters, each `DANCE_FRAME_STEP` a game frame.
            let lag = i64::from(
                crate::retail_compare_battle::DISPLAY_LAG_PASSES as i16 * DANCE_FRAME_STEP,
            );
            let go = i64::from(mg.phase2) * 2;
            world.mode == SceneMode::Dance
                && m.dance_countin.as_ref().is_some_and(|c| match state {
                    3 => i64::from(c.frame()) >= i64::from(counter) - lag,
                    4 => {
                        c.stage() == S::GoIn
                            && c.view().go.is_some_and(|b| i64::from(b) >= go - 2 * lag)
                    }
                    _ => {
                        c.stage() == S::GoOut
                            && c.view().go.is_some_and(|b| i64::from(b) <= go + 2 * lag)
                    }
                })
        }
        (Some(MinigameSubId::BakaFighter), Some((_, clock))) => {
            world.mode == SceneMode::BakaFighter
                && m.baka_fighter.as_ref().is_some_and(|f| {
                    f.cabinet().state() == 1
                        && i64::from(f.cabinet().intro_clock()) >= i64::from(clock)
                })
        }
        (Some(MinigameSubId::MuscleDome), Some((arm, count))) => {
            world.mode == SceneMode::MuscleDome
                && m.muscle_hub
                    .first_visit
                    .as_ref()
                    .is_some_and(|h| match arm {
                        0 => h.arm() == FirstVisitArm::IntroIn,
                        1 => h.arm() == FirstVisitArm::IntroHold && h.counter() >= count as i32,
                        2 => h.arm() == FirstVisitArm::IntroOut,
                        3 => h.arm() == FirstVisitArm::TitleZoom,
                        4 => h.arm() == FirstVisitArm::CardIn && h.counter() >= count as i32,
                        5 => h.arm() == FirstVisitArm::CardHold && h.hold() <= mg.phase2 as i32,
                        _ => h.arm() == FirstVisitArm::RoundIn && h.counter() >= count as i32,
                    })
        }
        _ => false,
    }
}

/// The scene the warp departs from: the label, or for a venue overlay that
/// loaded its own block over the scene word, the mode-24 init's backup.
fn departure_scene(
    retail: &RetailObs,
    mg: &RetailMinigame,
    cdname: &legaia_prot::cdname::IndexMap,
) -> String {
    match mg.minigame() {
        Some(MinigameSubId::Fishing) => cdname
            .get(&u32::from(mg.departure_define))
            .cloned()
            .unwrap_or_else(|| retail.scene.clone()),
        _ => retail.scene.clone(),
    }
}

/// The engine side of a seeded minigame state.
struct EngineMinigame {
    mode: SceneMode,
    scene: String,
    slot: Option<(i32, i32)>,
    coin_bank: u32,
    /// The tick the captured phase was reached on, past the warp.
    reached: Option<u64>,
}

fn run_engine_minigame(
    extracted: &Path,
    retail: &RetailObs,
    departure: &str,
) -> Result<EngineMinigame> {
    let mg = retail.minigame.context("no minigame observables")?;
    let sub = mg.minigame().context("sub-id names no overlay")?;
    let cfg = BootConfig {
        scene: departure.to_string(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(extracted, &cfg).context("open boot session")?;
    crate::retail_compare::pin_seed_retail(&mut session.host.world.toggles, false);
    let save = retail
        .seed_save()
        .context("retail state has no liftable save window")?;
    let _landing = session.resume_save(save, departure, &FieldLiveOpts::default());
    for _ in 0..WARP_TICK {
        session.tick()?;
    }
    session.host.world.request_minigame_warp(sub.sub_id());
    let mut reached = None;
    for t in 0..ENTRY_DEADLINE {
        session.tick()?;
        if at_captured_phase(&session.host.world, &mg) {
            reached = Some(t);
            break;
        }
    }
    let w = &session.host.world;
    Ok(EngineMinigame {
        mode: w.mode,
        scene: session
            .host
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default(),
        slot: w
            .minigames
            .slot_machine
            .as_ref()
            .map(|m| (m.retail_state(), m.balance())),
        coin_bank: w.minigames.casino_coins,
        reached,
    })
}

/// Seed a minigame state and score it: `mode`, `scene`, `minigame` (the
/// session's own words) and `image`.
pub(super) fn run_minigame(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    retail: &RetailObs,
    cdname: &legaia_prot::cdname::IndexMap,
    report: &mut StateReport,
) {
    let Some(mg) = retail.minigame else {
        report.unseeded = StateClass::Minigame.unseeded_reason().to_string();
        return;
    };
    if let Some(why) = mg.unseeded_reason() {
        report.unseeded = why;
        return;
    }
    let departure = departure_scene(retail, &mg, cdname);
    let engine = match run_engine_minigame(opts.extracted, retail, &departure) {
        Ok(e) => e,
        Err(e) => {
            report.unseeded = format!("seeding failed: {e:#}");
            return;
        }
    };
    let mut ch = BTreeMap::new();
    let mode_ok = mg
        .minigame()
        .and_then(MinigameSubId::scene_mode)
        .is_some_and(|m| engine.mode == m);
    ch.insert("mode".to_string(), f64::from(u8::from(mode_ok)));
    report.detail.insert(
        "mode".into(),
        format!("retail 0x19 sub-id {}; engine {:?}", mg.sub_id, engine.mode),
    );
    ch.insert(
        "scene".to_string(),
        f64::from(u8::from(engine.scene == departure)),
    );
    let words = [
        // The phase reached stands for the session's word when there is no
        // balance to compare.
        engine.reached.is_some() && engine.slot.map(|s| s.0) == mg.slot.map(|s| s.0),
        engine.slot.map(|s| s.1) == mg.slot.map(|s| s.1),
        engine.coin_bank == mg.coin_bank,
    ];
    ch.insert(
        "minigame".to_string(),
        round3(words.iter().filter(|&&w| w).count() as f64 / words.len() as f64),
    );
    report.detail.insert(
        "minigame".into(),
        format!(
            "slot state/balance retail {:?} engine {:?}; coin bank retail {} engine {}; retail \
             phase {:?}, reached {:?} ticks past the warp",
            mg.slot, engine.slot, mg.coin_bank, engine.coin_bank, mg.phase, engine.reached
        ),
    );
    if let (Some(exe), Some(rf), Some(save)) =
        (opts.engine_exe, &retail.frame, retail.seed_save().as_ref())
    {
        let warp = WARP_TICK;
        let (key, past_warp) = match mg.minigame() {
            // The capture's own word names the instant; the slot machine's
            // attract state holds still, its frame sampled where the capture
            // stood.
            Some(MinigameSubId::Dance) => ("K", engine.reached.unwrap_or(0) + 1),
            Some(MinigameSubId::MuscleDome) => ("M", engine.reached.unwrap_or(0) + 1),
            Some(MinigameSubId::Fishing) => ("L", FRAME_TICKS_PAST_WARP),
            Some(MinigameSubId::BakaFighter) => ("B", engine.reached.unwrap_or(0) + 1),
            _ => ("O", FRAME_TICKS_PAST_WARP),
        };
        let extra = vec!["--key-script".to_string(), format!("{warp}:{key}")];
        let mut seats: Vec<(&'static str, String)> = mg
            .dance_markers
            .iter()
            .any(Option::is_some)
            .then(|| {
                (
                    "LEGAIA_SEAT_DANCE_MARKERS",
                    legaia_engine_capture::field_seeds::dance_markers_env(&mg.dance_markers),
                )
            })
            .into_iter()
            .collect();
        // The hall's animated props: their clip cursors are a clock of the
        // hall's game frames too.
        let props: Vec<(u8, i16)> = mg.dance_props.iter().flatten().copied().collect();
        if !props.is_empty() {
            seats.push((
                "LEGAIA_SEAT_DANCE_PROPS",
                legaia_engine_capture::field_seeds::dance_props_env(
                    &props,
                    crate::retail_compare_battle::DISPLAY_LAG_PASSES,
                ),
            ));
        }
        // The hall's light show is palette work: its cyclers and scrollers are
        // a clock of game frames, whose lengths the capture measured and the
        // engine's fixed tick does not.
        if mg.minigame() == Some(MinigameSubId::Dance) {
            if !retail.cell_fx.is_empty() {
                seats.push((
                    "LEGAIA_SEAT_CLUT_FX",
                    legaia_engine_capture::field_seeds::cell_fx_env(&retail.cell_fx),
                ));
            }
            seats.extend(retail.vram_rects_env(opts.out_dir, &entry.label));
            seats.extend(retail.cell_sources_env(opts.out_dir, &entry.label));
        }
        match crate::retail_compare_image::engine_frame_with(
            exe,
            opts.extracted,
            &departure,
            None,
            &extra,
            &seats,
            warp + past_warp,
            opts.out_dir,
            &entry.label,
            crate::retail_compare_image::FrameEntry::Resume(save),
        ) {
            Ok(ef) => {
                if let Some(dir) = opts.out_dir {
                    let _ = crate::retail_compare_image::write_side_by_side(
                        &dir.join(format!("{}.png", entry.label)),
                        rf,
                        &ef,
                    );
                }
                let score = crate::retail_compare_image::score(rf, &ef);
                ch.insert("image".into(), round3(score.within));
                report.detail.insert(
                    "image".into(),
                    format!(
                        "mae={:.1} within={:.3} ({})",
                        score.mae, score.within, score.note
                    ),
                );
                report.image = Some(score);
            }
            Err(e) => {
                report
                    .detail
                    .insert("image".into(), format!("engine frame failed: {e:#}"));
            }
        }
    }
    report.score = state_score(&ch);
    report.channels = ch;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(state: i32, balance: i32, bank: u32, door: bool) -> RetailMinigame {
        RetailMinigame {
            sub_id: 3,
            slot: Some((state, balance)),
            coin_bank: bank,
            door_entry: door,
            phase: None,
            phase2: 0,
            departure_define: 0,
            dance_markers: Default::default(),
            dance_props: [None; DANCE_PROP_CLIPS],
        }
    }

    #[test]
    fn only_a_fresh_door_entry_slot_machine_is_seeded() {
        assert_eq!(slot(1, 9_999_999, 9_999_999, true).unseeded_reason(), None);
        // A decided spin, a played machine, a dev launch.
        assert!(
            slot(4, 9_999_996, 9_999_999, true)
                .unseeded_reason()
                .is_some()
        );
        assert!(slot(1, 120, 100, true).unseeded_reason().is_some());
        assert!(slot(1, 70, 0, false).unseeded_reason().is_some());
        // Baka Fighter has no path yet.
        let baka = RetailMinigame {
            sub_id: 4,
            slot: None,
            coin_bank: 0,
            door_entry: true,
            phase: Some((0x64, 0)),
            phase2: 0,
            departure_define: 0,
            dance_markers: Default::default(),
            dance_props: [None; DANCE_PROP_CLIPS],
        };
        assert!(baka.unseeded_reason().unwrap().contains("Baka Fighter"));
    }

    #[test]
    fn the_entry_phases_are_seeded_and_the_played_ones_are_not() {
        let mg = |sub_id, state| RetailMinigame {
            sub_id,
            slot: None,
            coin_bank: 0,
            door_entry: true,
            phase: Some((state, 0)),
            phase2: 0,
            departure_define: 0,
            dance_markers: Default::default(),
            dance_props: [None; DANCE_PROP_CLIPS],
        };
        assert_eq!(mg(6, 3).unseeded_reason(), None);
        assert!(mg(6, 0xA).unseeded_reason().is_some());
        assert_eq!(mg(5, 1).unseeded_reason(), None);
        assert_eq!(mg(5, 0x15).unseeded_reason(), None);
        assert!(mg(5, 0xA).unseeded_reason().is_some());
        assert_eq!(mg(4, 1).unseeded_reason(), None);
        assert_eq!(mg(0, 0xC).unseeded_reason(), None);
        assert!(mg(0, 0xB).unseeded_reason().is_some());
        assert!(mg(0, 0x19).unseeded_reason().is_some());
    }
}
