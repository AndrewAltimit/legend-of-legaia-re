//! The battle half of the retail comparison corpus: read a battle save
//! state's encounter out of RAM, put the engine into the same fight through
//! its real encounter entry, and score the battle channels.
//!
//! Retail side (every address a battle-resident SCUS global or the battle
//! context it points at, `docs/subsystems/battle.md`):
//!
//! - **battle context** `*0x8007BD24` (`0x800EB654` while a fight is
//!   resident): `+0x00` party count, `+0x01` monster count, `+0x06` the
//!   command-flow byte, `+0x07` the action-state cursor;
//! - **formation cell** `0x8007BD0C[0..4]` - monster seat `m`'s id;
//! - **per-battle flags** `0x8007BD60` - bit `0x80` is the scripted-fight
//!   header bit the entity SM ORs in for a row whose `record[+0]` is set;
//! - **run state** `0x8007BD71` (`0xFF` running, `0xFE` ending);
//! - **combatants** through the eight-slot actor pointer table `0x801C9370`
//!   (party `0..2`, monsters `3..7`, fixed slots): HP `+0x14C` / max
//!   `+0x14E`, MP `+0x150` / max `+0x152`;
//! - **camera** the same rotation / translation globals the field uses
//!   (`0x8007B790/92`, `0x800840B8..C0`) plus GTE `H`.
//!
//! Engine side: the scene is entered through the card-load path and the
//! field settles [`legaia_engine_capture::SETTLE_TICKS`] frames (the fight
//! is entered from a running field, as retail's was: the entry scripts start
//! the scene's track first). The retail formation is matched against the
//! scene's registered MAN rows (the id space a region roll or a scripted
//! carrier produces) and, when no row carries it, registered as a formation
//! of its own - then [`legaia_engine_core::world::World::force_encounter`]
//! arms it through the ordinary transition, exactly as `play-window
//! --battle` does. Once the mode flips to battle the retail combatants' live
//! HP / MP are written over the engine's (the capture is mid-fight; the seed
//! carries its damage), and the session is placed at the capture's phase
//! ([`SeedPlan`]): sampled at the flip for an opening capture, parked on the
//! round prompt for a prompt capture, a summon-band cast replayed
//! ([`PhaseGate`]), and any other menu surface or action in flight reached
//! through the engine's own pad path ([`BattleDrive`]).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use legaia_engine_battle_menus::battle_flow::BattleFlowState;
use legaia_engine_capture::bar_seeds::*;
use legaia_engine_capture::battle_drive::*;
use legaia_engine_capture::hud_glide::*;
use legaia_engine_capture::phase_gate::*;
use legaia_engine_mode::mode::SceneMode;
use legaia_ram_image::game_anchors;
use serde::{Deserialize, Serialize};

use crate::retail_compare::{CameraObs, RetailObs};
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

mod camera_align;
mod camera_pool;
mod capture_drive;
mod display_lag;
mod effect_children;
mod engine_run;
mod frame_step;
mod hud_glide;
mod hud_glide_pool;
mod inflight_seeds;
mod popup_cells;
mod spirit_gauge;
pub use inflight_seeds::UndriftActing;
mod ram_read;
mod scoring;
mod seed_plan;
mod summon_fade;

use camera_align::*;
use camera_pool::*;
use capture_drive::*;
pub use display_lag::*;
use effect_children::*;
pub use engine_run::*;
pub use frame_step::*;
use hud_glide::*;
use hud_glide_pool::*;
pub use popup_cells::*;
use ram_read::*;
pub use scoring::*;
pub use seed_plan::*;
use spirit_gauge::*;
pub use summon_fade::*;

/// Battle context pointer.
const BATTLE_CTX: u32 = 0x8007_BD24;
/// Formation cell, one monster id per seat.
const FORMATION_CELL: u32 = 0x8007_BD0C;
/// Per-battle flags; bit `0x80` = the formation row's header byte was set.
const PER_BATTLE_FLAGS: u32 = 0x8007_BD60;
/// Battle run state (`0xFF` running, `0xFE` ending, `0x00` opening).
const RUN_STATE: u32 = 0x8007_BD71;
/// Battle stage id (`0` none; `1` the Tetsu tutorial module).
const STAGE_ID: u32 = 0x8007_B64A;
/// Battle init's keep-object-1 byte: set, `FUN_800513F0` keeps the backdrop
/// shell's object 1 (`0x80051ABC`).
const KEEP_BACKDROP_OBJECT_1: u32 = 0x8007_B64B;
/// `_DAT_8007BD84`, the Mystic Shield's effect handle.
const MYSTIC_SHIELD_HANDLE: u32 = 0x8007_BD84;
/// The backdrop draw's slot-1 Y angle: the second row's `+2` of the per-slot
/// angle table at `0x800891C8` (`FUN_8001ADA4`'s backdrop arm reads it,
/// `FUN_80046A20` winds it while [`KEEP_BACKDROP_OBJECT_1`] is up).
const BACKDROP_SLOT_1_YAW: u32 = 0x8008_91D2;
/// Present-party list: pool slot -> roster character id (1-based; `4` is
/// the AI-companion seat).
const SEAT_CHARS: u32 = 0x8007_BD10;
/// The battle-id word a development launcher writes ([`RetailBattle::battle_id`]).
const BATTLE_ID_WORD: u32 = 0x8007_B7FC;
/// The system flags the field entity SM's battle-entry tail writes
/// (`FUN_801DA51C`, `0x801DA6C4..0x801DA6EC`): `0` and the report flag under
/// its arm, and the victory latch it lowers.
pub(crate) const BATTLE_ENTRY_TAIL_FLAGS: [u16; 3] = [
    0,
    legaia_engine_battle::battle_tutorial::BATTLE_REPORT_FLAG,
    legaia_engine_battle::battle_tutorial::BATTLE_ENTRY_CLEARED_FLAG,
];
/// The options screen's Battle Camera config word.
const BATTLE_CAMERA_OPTION: u32 = 0x8008_46C0;
/// The options screen's Select Attack config word.
const SELECT_ATTACK_OPTION: u32 = 0x8008_46C4;
/// Eight-slot battle actor pointer table.
const ACTOR_TABLE: u32 = 0x801C_9370;
/// A battle actor's position history ring: 32 `(i32 X, i32 Z)` slots
/// (`FUN_80047430`, `0x80047E58..0x80048060`).
const POSITION_RING: u32 = 0x4C;
/// The ring slot the displayed frame was drawn on: the display lag is two
/// game frames ([`display_lag_vsyncs`]), one ring slot each.
const DISPLAYED_FRAMES_BACK: u32 = 2;
/// The tint-state byte `actor[+0x21C]`'s defeat-fade value.
const DEFEAT_FADE_STATE: u8 = legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE;
/// The SCUS frame driver's battle-entry counter `gp+0x330`: below `0x80`
/// the fight loads, `0x80..=0xC0` the entry sweep owns the camera, `0xFF`
/// the battle tick runs (`FUN_80046A20`, `0x80046EEC..0x8004700C`).
const ENTRY_COUNTER: u32 = 0x8007_B648;
/// Frames a forced encounter may take to reach battle mode (the intro
/// transition runs 132 display frames).
const ENTRY_TICKS: u32 = 400;
/// The command-flow bytes `ctx[+6]` whose frames the battle tick's idle
/// orbit owns: `FUN_801D0748`'s prologue decrements the shared yaw
/// `_DAT_8007B792` only on these (`0x801D07AC..0x801D07CC`), so a capture on
/// one of them holds an orbit sample - a clock reading, not a framing.
pub const ORBIT_FLOWS: [u8; 4] = [0x1E, 0x32, 0x6E, 0xFE];

/// Engine ticks run between the battle-mode flip and sampling.
pub const BATTLE_SETTLE_TICKS: u64 = 60;
/// Frames the battle opening may take to reach the first round prompt. The
/// bound is the longest retail opening, not a typical one: the evolved-Cort
/// arrival (PROT 0968) parks the flow byte at `0x0C` for about 3200 vsyncs
/// before it hands round one back (`docs/subsystems/battle.md`, flow `0x0C`
/// is the boss stage module's baton). Every other fight leaves the loop on
/// its first prompt, so the bound costs nothing there.
const OPENING_TICKS: u32 = 4800;
/// Frames a replayed cast may take to reach the capture's phase. The summon
/// band's longest pre-creature stretch is the caster's clip plus the `0x78`
/// flash-out; a creature's own choreography (`0x36`) runs longer.
const INFLIGHT_TICKS: u32 = 1200;
/// The extra capture deadline a phase-gated `play-window` child gets.
pub const INFLIGHT_DEADLINE: u64 = INFLIGHT_TICKS as u64;
/// The world stream's state at the encounter entry, tried in order on the
/// headless seed; the one used reaches the `play-window` child as
/// `LEGAIA_BATTLE_RNG_SEED`.
///
/// A capture's RNG state at the instant its fight began is not observable,
/// so the engine's stream at the entry is otherwise an arbitrary function of
/// how many field draws its settle window happened to take. Every battle
/// channel that rides the stream (monster picks, the camera and its framing,
/// the frame) then moved whenever a field-side port changed its draw count
/// or shape, with no change to the battle itself. Pinning the stream at the
/// entry - and holding it there through the field-side intro transition,
/// whose NPC and ambient programs keep drawing
/// (`EncounterState::rng_hold`) - keeps a battle state's scores a property
/// of the battle.
///
/// A capture taken on a monster's action (or on anything else a draw
/// decides) is one realisation of the stream. When the first seed's drive or
/// replayed cast never reaches the capture's phase, the next seed is tried,
/// so the state is scored at the event retail showed under some stream rather
/// than dropped for the one stream the corpus happened to hold.
///
/// The list only ever grows at its tail: a state keeps the first seed that
/// reaches, so an appended seed moves no state an earlier one already
/// reaches. The tail exists for captures whose round order is a tie a draw
/// breaks (`FUN_801DABA4`'s pick): any change in how long an action holds
/// shifts which draw breaks it, so a state can lose every early seed to a
/// party wipe without anything in the battle being wrong.
pub const BATTLE_RNG_SEEDS: [u32; 48] = [
    0x1234_5678,
    0x9E37_79B9,
    0x0BAD_F00D,
    0x7F4A_7C15,
    0xC0FF_EE01,
    0x2545_F491,
    0x6A09_E667,
    0xBB67_AE85,
    0x3C6E_F372,
    0xA54F_F53A,
    0x510E_527F,
    0x9B05_688C,
    0x1F83_D9AB,
    0x5BE0_CD19,
    0x428A_2F98,
    0x7137_4491,
    0xB5C0_FBCF,
    0xE9B5_DBA5,
    0x3956_C25B,
    0x59F1_11F1,
    0x923F_82A4,
    0xAB1C_5ED5,
    0xD807_AA98,
    0x1283_5B01,
    0x09E6_633D,
    0x3D95_588A,
    0x9DEB_E964,
    0x6369_1762,
    0x54F7_AE41,
    0x49C1_6D38,
    0x43DE_897D,
    0x9A3B_E366,
    0xAE2A_57C2,
    0x9115_780A,
    0xFBD4_4AC9,
    0xF0FE_EC45,
    0xBC5D_413F,
    0xE805_AD94,
    0xEA78_4A98,
    0xBAED_D782,
    0xB307_DC35,
    0x78A0_C04B,
    0x9EF3_D083,
    0x847A_F270,
    0x04EF_5376,
    0xCBAA_DB89,
    0x7B54_AE6C,
    0x8BAE_8BFB,
];
/// Per-state streams tried ahead of [`BATTLE_RNG_SEEDS`] ([`pinned_seed`]).
///
/// The capture's own stream word cannot be stepped back to its entry (the
/// engine's draw count to the phase moves with the stream it is dealt), so
/// which stream replays a drive or a cast stays a search result. These are
/// that search's results for the states whose first reaching stream frames
/// the capture's phase further from retail than another, or whose image
/// child never reaches the phase on it: each is the first stream, from
/// [`BATTLE_RNG_SEEDS`] in its order, that reaches the phase headlessly,
/// gets a frame from the child, and scores the camera and then the frame no
/// worse than the corpus held. The seed runs the retail-faithful mode the
/// child runs (`retail_compare::pin_seed_retail`), so a pin is searched under
/// it. A pin that stops reaching falls through to the list; the search is in
/// `docs/tooling/retail-compare.md` ("The stream at the entry").
pub const BATTLE_RNG_PINS: &[(&str, u32)] = &[
    ("battle_gaza2_park_0x19", 0xFBD4_4AC9),
    ("battle_gaza2_park_0x19_summon_melee", 0xB5C0_FBCF),
    ("battle_gaza2_park_0x19_target_vahn", 0x49C1_6D38),
    ("battle_melee_hit_spark", 0x9E37_79B9),
    ("battle_vahn_tri_somersault_super", 0x8BAE_8BFB),
    ("shiny_refactor_gimard_levelup", 0x1F83_D9AB),
];

/// The stream a state's search tries first, when it has one.
pub fn pinned_seed(label: &str) -> Option<u32> {
    BATTLE_RNG_PINS
        .iter()
        .find(|(l, _)| *l == label)
        .map(|&(_, s)| s)
}

/// The battle projection's `H` (`FUN_8003D254`; `battle_cam_script::GTE_H`).
const BATTLE_H: i16 = 256;

/// One combatant's HP / MP, both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combatant {
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
}

/// One pool slot as the capture held it: the live pair `+0x34` / `+0x38`,
/// the body pair `+0x3C` / `+0x40`, the current clip `+0x1D9` and - for a
/// monster seat - the size class its record carries (`+0x1F` through the
/// record-pointer table `0x801C9348[slot - 3]`, the byte the range law
/// `FUN_8004E2F0` reads).
pub type CapturedActor = ([i16; 2], [i16; 2], u8, u8);

/// Everything the battle channels read off a retail battle state.
#[derive(Debug, Clone)]
pub struct RetailBattle {
    pub party_count: u8,
    pub monster_count: u8,
    /// `ctx[+0x06]`.
    pub flow: u8,
    /// `ctx[+0x07]`.
    pub action_state: u8,
    /// The frame step the capture's frame ran at (`*(0x1F800393)`, rebuilt
    /// from the frame-time ring - [`frame_step`]).
    pub frame_step: u8,
    pub run_state: u8,
    pub stage_id: u8,
    pub scripted: bool,
    /// The battle-id word `0x8007B7FC`: non-zero on a fight a development
    /// launcher entered by monster id rather than the field entity SM
    /// `FUN_801DA51C` (the boss casts captured in `jou`). Such a fight never
    /// ran the SM's entry tail, so the system flags the tail writes are the
    /// save's ([`BATTLE_ENTRY_TAIL_FLAGS`]).
    pub battle_id: u8,
    /// `0x8007BD60 & 0x1F` - the battle-stage variant the region reader
    /// stamped for the tile the fight started on; battle init loads the
    /// backdrop from entry `scene_index + variant` (`FUN_800513F0`).
    pub stage_variant: u8,
    /// `0x8007B64B != 0` - battle init kept the backdrop shell's object 1
    /// (the region reader's long-layout `+8` bit 5).
    pub keep_backdrop_object_1: bool,
    /// `0x8007BD84 != 0` - Cort's Mystic Shield is up: the handle of the
    /// sphere PROT 0940's arm 4 seated on an earlier turn. Nothing in the
    /// replayed fight casts it again before the captured phase, so the seed
    /// raises it on the first battle tick (`World::raise_mystic_shield`).
    pub mystic_shield: bool,
    /// The state's live slot-B cast parts (`FUN_80021DF4` nodes whose
    /// record `+0x48` lies in the module image), each as the halfwords the
    /// image child seats over the replay's part of the same record on the
    /// capture frame (`LEGAIA_SEAT_CAST_PARTS`).
    pub cast_parts: Vec<legaia_engine_effects::summon::CastPartSeed>,
    /// The state's turning effect-script table parts (`FUN_80021DF4` nodes
    /// whose record lies in the battle overlay's prototype band), each as
    /// its displayed rotation trio, which the image child seats over the
    /// replay's part of the same record on the capture frame
    /// (`LEGAIA_SEAT_ACTION_FX_PARTS`).
    pub action_fx_parts: Vec<legaia_engine_effects::summon::CastPartSeed>,
    /// The state's live effect-pool children (`_DAT_8007BD30 + 0x10`) as
    /// the displayed frame held them, which the image child stands in its
    /// own pool on the capture frame (`LEGAIA_SEAT_EFFECT_CHILDREN`) -
    /// an empty pool included. `None` when the state holds no pool.
    pub effect_children: Option<Vec<legaia_engine_core::world::EffectChildSeed>>,
    /// The after-image ghosts the displayed frame drew, as each body's
    /// history ring held them, which the image child seats over the
    /// replay's ring on the capture frame (`LEGAIA_SEAT_GHOST_RING`).
    pub ghost_ring: Vec<legaia_engine_capture::ghost_ring::GhostRingSeed>,
    /// `0x800891D2` - the Y angle the backdrop draw turns its slot 1 by, in
    /// 4096ths of a turn. Time spent in keeping fights since boot, so a
    /// replay carries it over rather than winding its own.
    pub backdrop_slot_1_yaw: u16,
    /// `0x8007BD10[0..party_count]`.
    pub seat_chars: Vec<u8>,
    /// Each party seat's committed action category `+0x1DE` (`4` = Spirit).
    /// For the seats a menu capture's member follows, this round's commit.
    pub party_categories: Vec<u8>,
    /// Formation-cell ids, trimmed to the monster count.
    pub monster_ids: Vec<u8>,
    /// Party slots `0..party_count` (fixed pool slots).
    pub party: Vec<Option<Combatant>>,
    /// Monster seats (pool slots `3..3 + monster_count`).
    pub monsters: Vec<Option<Combatant>>,
    /// `ctx[+0x13]` - the seat the action SM is running; ahead of the seed
    /// pass ([`PRE_SEED_STATES`]) the seat it is about to run, `ctx[+0x274]`.
    pub active_actor: u8,
    /// The active seat's queued action id `+0x1DF` (a spell id on a cast).
    pub queued_action: u8,
    /// The active seat's target byte `+0x1DD`.
    pub target_code: u8,
    /// The active seat's committed action category `+0x1DE` (`1` item, `2`
    /// magic, `3` attack, `4` spirit, `5` run).
    pub queued_category: u8,
    /// The summon band's live flash, when one is up ([`RetailFade`]).
    pub summon_fade: Option<RetailFade>,
    /// `ctx[+0x279]` - the resident summon module's phase byte.
    pub module_phase: u8,
    /// Vsyncs the capture's displayed frame lags its RAM
    /// ([`display_lag_vsyncs`]).
    pub display_lag: u16,
    /// Vsyncs the two passes the displayed frame lags by spanned, each on
    /// its own step ([`display_pass_steps`]).
    pub display_pass_lag: u16,
    /// `ctx[+0x87C]` - the close-up accumulator the active actor's clip
    /// commit zeroes.
    pub cam_accum: u32,
    /// The active seat's committed clip `+0x1D9`.
    pub caster_clip: u8,
    /// `ctx[+0x6DA]` - the yaw base a module walk arm swings.
    pub walk_yaw_base: u16,
    /// `ctx[+0x26D]` - the per-action track coin `FUN_8004E13C` rolls on a
    /// clip commit (`rand() % 2`).
    pub track_coin: u8,
    /// PROT 0903's countdown word
    /// ([`legaia_engine_cast::cast_module_camera::GIMARD_COUNTDOWN_VA`]) - only
    /// meaningful while that module is resident.
    pub gimard_countdown: i32,
    /// `ctx[+0xD]` - the acting action's framing style.
    pub cam_style: u8,
    /// Retail's live camera and the endpoints its tween table
    /// (`ctx[+0x118C]`) steps toward, in the engine's pose space
    /// ([`retail_cam_tween`]): `(live, end)`.
    pub cam_tween: (
        legaia_engine_camera::projection::BattleCamPose,
        legaia_engine_camera::projection::BattleCamPose,
    ),
    /// [`Self::cam_tween`] with the live pose walked back to the frame the
    /// display shows ([`displayed_cam_live`]).
    pub displayed_cam_live: legaia_engine_camera::projection::BattleCamPose,
    /// [`Self::cam_tween`]'s live pose walked back to the displayed frame
    /// along a table built once, as a slot-B module's shot is
    /// ([`displayed_module_cam_live`]).
    pub displayed_module_cam_live: legaia_engine_camera::projection::BattleCamPose,
    /// Whether every tweened component of retail's camera reads its
    /// endpoint ([`camera_landed`]).
    pub cam_landed: bool,
    /// The frame driver's entry counter `gp+0x330` ([`ENTRY_COUNTER`]).
    pub entry_counter: u8,
    /// The options screen's Battle Camera word `0x800846C0` (Close `0` /
    /// Normal `1` / Far `2`), which the action shots read. It sits in the
    /// saved game-state window, but the save lift does not carry options,
    /// so the seed stamps it (`World::toggles.battle_camera`).
    pub camera_option: u8,
    /// No HUD widget glide is in flight: every tracked record
    /// `ctx[+0x11B4 + slot * 0xC]` reads `total == 0`, which is what
    /// `FUN_801D9BBC` leaves once a glide has snapped onto its target
    /// (`0x801D9BE4` skips a record whose `total` byte is zero).
    pub hud_glides_landed: bool,
    /// The HUD widget glides in flight, each as the displayed frame shows
    /// it ([`HudGlideSeat`]); empty when every record has landed.
    pub hud_glides: Vec<HudGlideSeat>,
    /// The glide table holds records and every one of them was spawned
    /// after the displayed frame was built: its raw `elapsed` is under
    /// [`Self::display_lag`]. The menu state the RAM holds opened its
    /// surface on a pass the display has not shown yet
    /// ([`Self::displayed_menu_flow`]).
    pub hud_glides_unseen: bool,
    /// The newest damage numeral as the displayed frame drew it: its struck
    /// pool slot and the engine popup age whose rise matches retail's
    /// (`hud_glide::popup_age_seat`); `None` with no live numeral.
    pub popup_age: Option<(u8, u16)>,
    /// The numeral cells the displayed frame drew, off the packet arenas
    /// ([`displayed_popup_cells`]).
    pub popup_cells: Option<Vec<DigitQuad>>,
    /// A Spirit turn's AP bar width and AP plate value as the displayed
    /// frame shows them (`spirit_gauge::retail_spirit_gauge`); `None`
    /// outside one.
    pub spirit_gauge: Option<(i16, i16)>,
    /// The options screen's Select Attack word `0x800846C4` (Select `0` /
    /// Automatic `1` / Command `2`): whether the ring's Attack arm opens
    /// the `Auto | Command` prompt `0x78`.
    pub select_attack: u8,
    /// `ctx[+0x269]` - the Seru a killing blow absorbed this action, staged
    /// for the Done band's grant (`sb v0,0x269(a0)` at `0x801EE2E8`) and
    /// cleared when `0x52` leaves.
    pub absorbed_seru: u8,
    /// `ctx[+0x26] == 0x65` - the summon-magic level check `FUN_801E70BC`
    /// levelled the acting seat's cast spell this action (`sb v0,0x26(v1)`
    /// at `0x801E723C`, beside the level byte's own `sb v0,0x729(a2)` at
    /// `0x801E7224`), and the next action seed clears it.
    pub magic_level_up: bool,
    /// Whether the active seat's committed queue `+0x1DF..+0x1EE` holds an
    /// art starter (`0x19` / `0x1A`): the turn was entered through the
    /// directional command entry, not the Auto swing.
    pub arts_queue: bool,
    /// The active seat's committed queue `+0x1DF..+0x1EE` itself - the
    /// tokenized turn the player entered ([`ActionSteer::queue`]).
    pub committed_queue: [u8; 16],
    /// `ctx[+0x15]` - the strike cursor into that queue.
    pub strike_cursor: u8,
    /// The active seat's live action gauge `+0x154` and its base `+0x156`.
    /// A Spirit turn's round boundary extends the live gauge
    /// (`(base * 7) / 5 + 8`), and the extension decides both the arts
    /// entry's AP pool and which saved command band it preseeds.
    pub acting_gauge: Option<(u16, u16)>,
    /// The battle-end sequence's position, when the capture is past the end
    /// signal ([`SpanGate`]).
    pub span_gate: SpanGate,
    /// The win pose the battle-end sequence latched on its pose actor
    /// (`ctx[+0x13]`'s `+0x1DB`), for a capture past the end signal. The
    /// results framing is that pose's script ([`legaia_engine_battle_vm::battle_cam_script`]'s
    /// `battle_over_script`), and the sequencer draws the pose from the
    /// stream, so a seed that reaches the phase on another pose frames
    /// another shot.
    pub win_pose: Option<u8>,
    /// The hits the action in flight has already landed, as `(pool slot,
    /// amount)` pairs off the damage-number ring ([`action_hits`]). The
    /// replay re-deals them, so the seed takes them back off the bars
    /// ([`Self::pre_action_hp`]).
    pub action_hits: Vec<(u8, u16)>,
    /// The same ring in push order ([`action_hit_sequence`]): a party melee
    /// replay lands these amounts rather than its own rolls
    /// ([`Self::melee_hit_seeds`]).
    pub action_hit_sequence: Vec<(u8, u16)>,
    /// The string's running total `DAT_8007BD14` (the `TOTAL` the combo
    /// cluster draws; the melee kernel's HP write stores it,
    /// `0x801EEA64..0x801EEA78`).
    pub combo_total: u32,
    /// Each pool slot's live `+0x34` / `+0x38` pair (party `0..=2`,
    /// monsters `3..=7`), `None` for an empty slot.
    pub ground: Vec<Option<[i16; 2]>>,
    /// The system flags the fight's own record raised on its way into the
    /// entry op and the capture therefore holds ([`entry_latches`]): held
    /// back from the scene entry the seed runs and raised once the field
    /// has settled, where retail's record raised them. Empty as read; the
    /// corpus run fills it.
    pub entry_latches: Vec<u16>,
    /// The state was made on a shiny-Seru patched disc (its `resident_patch`
    /// names `shiny-seru`): that build's battle init takes one `rand()` draw
    /// for the shiny roll ahead of the formation draws
    /// (`legaia_code_hooks::shiny_seru`, the setup routine's `jal rand`), so
    /// the engine stands in for it with `WorldToggles::shiny_seru` on, on
    /// both the headless seed and the image child. Off, both sides run
    /// retail's entry with no such draw. `false` as read; the corpus run
    /// fills it.
    pub resident_shiny_seru: bool,
    /// Each pool slot's captured live pair, body pair `+0x3C` / `+0x40` and
    /// current clip `+0x1D9`, as read - [`Self::ground`] is what a seed
    /// places and [`Self::undrift`] moves it; this is what the capture
    /// held. The post-strike framings read the body pair (cases 7 and 8).
    pub captured: Vec<Option<CapturedActor>>,
    /// Each pool slot's live pair on the **displayed** frame: slot
    /// [`DISPLAYED_FRAMES_BACK`] of the position history ring `+0x4C` (an
    /// `i32` X / Z pair every 8 bytes, slot 0 the live pair, shifted one
    /// slot a battle frame by the anim tick `FUN_80047430`,
    /// `0x80047E58..0x80048060`). The frame on screen is the one built two
    /// game frames before the RAM's ([`display_lag_vsyncs`]), and a seat
    /// that moved inside those two frames drew where the ring says.
    pub displayed_ground: Vec<Option<[i16; 2]>>,
    /// Each pool slot's playing clip `+0x1D9` and the cursor of its anim
    /// node (`*(+0x22C) + 0x68`, 12.4 sixteenths of a keyframe) taken back
    /// to the **displayed** frame, two game frames before the RAM's: as
    /// `(pool slot, clip, cursor)`, the cursor possibly below zero (the
    /// clip wraps). The frame the capture shows each body on
    /// ([`legaia_engine_capture::clip_cursors`]).
    pub clip_cursors: Vec<(u8, u8, i32)>,
    /// The idle-loop cursor each engine battle slot takes at an in-flight
    /// cast's dispatch ([`legaia_engine_core::world::InflightCastSeed::idle`]):
    /// empty on the RAM read, filled from a first replay's measured advance
    /// ([`Self::with_inflight_idle`]).
    pub inflight_idle: [Option<i32>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
    /// Each pool slot's heading `+0x46` (12-bit), beside [`Self::ground`]:
    /// the attack band's facing recompute stores it every frame of a
    /// swing and nothing turns the actor back, so a member stands facing
    /// the last target it struck. The results framing reads it (case 6's
    /// battle-over yaw is `0x800 - actor[+0x46]`).
    pub facing: Vec<Option<u16>>,
    /// Each pool slot's colour lanes `+0x04`, for a slot whose tint state
    /// `+0x21C` is the defeat fade (`2`): a monster killed earlier has
    /// stepped its lanes to black (`noa_levelup_banner`'s Gobu reads `0`)
    /// and the per-actor draw skips it, so the seed lands it there rather
    /// than standing a body retail no longer draws.
    pub defeat_lanes: Vec<Option<u32>>,
    /// Each pool slot's initiative key `+0x16C`. The round's seeder
    /// `FUN_801DA780` gives every living combatant a non-zero key and the
    /// action SM's `0x0C` dispatch zeroes the one it takes
    /// (`sh zero,0x16c(s3)` at `0x801E2CDC`), so a living combatant reading
    /// `0` has already acted in the capture's round.
    pub init_keys: Vec<Option<u16>>,
    /// `ctx[+0x274]` as read: the actor the initiative pick `FUN_801DABA4`
    /// handed the turn to. Inside a party member's attack band it names that
    /// member, except on a counterattack, where it still names the monster
    /// whose swing the counter answers (`battle_vahn_tri_somersault_super`:
    /// Vahn's strike loop on seat `0`, the turn seat `4`'s).
    pub turn_owner: u8,
    /// Each pool slot's status word `+0x16E` (the ailment bits the tint
    /// pass colours a body by - `0x1` Venom `0xFF2020`, `0x2` Toxic, ...,
    /// [`legaia_engine_battle_vm::status_effects::display_flags`]), `None` for an
    /// empty slot.
    pub status: Vec<Option<u16>>,
    /// The timed message up in the capture (HUD element `0x66`): the
    /// battle-overlay string its content word `0x800775B4` points at, and
    /// the hold `0x801F6964` left on it. `None` when the hold is spent.
    pub timed_message: Option<(u32, i16)>,
    /// Record `0x51`'s content word `0x800773BC` is zero: the strike loop's
    /// counter swap cleared the target plaque.
    pub target_plate_cleared: bool,
    /// The active seat's target (`+0x1DD`) in the defeat fade (`+0x21C ==
    /// 2`, written by the death arm of `FUN_8004AD80` at `0x8004B66C`): its
    /// colour word `+0x04`'s first 10-bit lane, which the fade walks down
    /// from `0x200` by `dt << 3` a frame - so it dates the death commit.
    pub target_fade_lane: Option<u16>,
    /// The death-spoils caption is up: `ctx[+0x18]` holds HUD element
    /// `0x5B`, which the steal / thief's-loot arm of `FUN_8004AD80` raises.
    pub spoils_caption: bool,
}

mod results_undo;
pub use results_undo::*;

/// Ticks `play-window --battle` runs before the capture: the intro
/// transition, the opening to the first round prompt, and the same settle
/// the headless side takes.
pub const BATTLE_CAPTURE_TICK: u64 = 320;

/// Retail's present list as 0-based roster slots (`0x8007BD10` holds 1-based
/// roster ids; `4` is the guest / AI-companion record).
fn retail_roster_slots(battle: &RetailBattle) -> Vec<u8> {
    battle
        .seat_chars
        .iter()
        .filter(|&&c| (1..=4).contains(&c))
        .map(|&c| c - 1)
        .collect()
}

impl RetailBattle {
    /// The fight's formation cell, as [`FormationCell::register`] stands it
    /// up where no MAN row carries it.
    ///
    /// [`FormationCell::register`]: legaia_engine_capture::formation_cell::FormationCell::register
    pub fn formation_cell(&self) -> legaia_engine_capture::formation_cell::FormationCell {
        legaia_engine_capture::formation_cell::FormationCell {
            ids: self.monster_ids.clone(),
            scripted: self.scripted,
        }
    }
}

/// The `play-window` arguments that reproduce this fight: the MAN row (or,
/// with none, the formation cell the headless seed registered) and the
/// retail party composition (`0x8007BD10` ids are 1-based roster ids).
pub fn play_window_args(battle: &RetailBattle, row: Option<u16>) -> Vec<String> {
    let entry = row.map_or_else(|| battle.formation_cell().to_spec(), |r| r.to_string());
    let mut args = vec!["--battle".to_string(), entry];
    let party: Vec<String> = retail_roster_slots(battle)
        .iter()
        .map(|c| c.to_string())
        .collect();
    if !party.is_empty() {
        args.push("--party".into());
        args.push(party.join(","));
    }
    args
}

#[cfg(test)]
mod tests;
