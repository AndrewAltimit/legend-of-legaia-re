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
//! field settles [`crate::retail_compare::SETTLE_TICKS`] frames (the fight
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
use legaia_engine_core::battle_flow::BattleFlowState;
use legaia_engine_core::world::SceneMode;
use legaia_mednafen::game_anchors;
use serde::{Deserialize, Serialize};

use crate::boot::{BootConfig, BootSession, FieldLiveOpts};
use crate::retail_compare::{CameraObs, RetailObs};

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
/// Present-party list: pool slot -> roster character id (1-based; `4` is
/// the AI-companion seat).
const SEAT_CHARS: u32 = 0x8007_BD10;
/// The options screen's Battle Camera config word.
const BATTLE_CAMERA_OPTION: u32 = 0x8008_46C0;
/// Eight-slot battle actor pointer table.
const ACTOR_TABLE: u32 = 0x801C_9370;
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
/// entry keeps a battle state's scores a property of the battle.
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
pub const BATTLE_RNG_SEEDS: [u32; 12] = [
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
];
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

/// Everything the battle channels read off a retail battle state.
#[derive(Debug, Clone)]
pub struct RetailBattle {
    pub party_count: u8,
    pub monster_count: u8,
    /// `ctx[+0x06]`.
    pub flow: u8,
    /// `ctx[+0x07]`.
    pub action_state: u8,
    pub run_state: u8,
    pub stage_id: u8,
    pub scripted: bool,
    /// `0x8007BD60 & 0x1F` - the battle-stage variant the region reader
    /// stamped for the tile the fight started on; battle init loads the
    /// backdrop from entry `scene_index + variant` (`FUN_800513F0`).
    pub stage_variant: u8,
    /// `0x8007B64B != 0` - battle init kept the backdrop shell's object 1
    /// (the region reader's long-layout `+8` bit 5).
    pub keep_backdrop_object_1: bool,
    /// `0x8007BD10[0..party_count]`.
    pub seat_chars: Vec<u8>,
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
    /// ([`legaia_engine_vm::cast_module_camera::GIMARD_COUNTDOWN_VA`]) - only
    /// meaningful while that module is resident.
    pub gimard_countdown: i32,
    /// `ctx[+0xD]` - the acting action's framing style.
    pub cam_style: u8,
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
    /// results framing is that pose's script ([`legaia_engine_vm::battle_cam_script`]'s
    /// `battle_over_script`), and the sequencer draws the pose from the
    /// stream, so a seed that reaches the phase on another pose frames
    /// another shot.
    pub win_pose: Option<u8>,
    /// Each pool slot's live `+0x34` / `+0x38` pair (party `0..=2`,
    /// monsters `3..=7`), `None` for an empty slot.
    pub ground: Vec<Option<[i16; 2]>>,
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
    /// The timed message up in the capture (HUD element `0x66`): the
    /// battle-overlay string its content word `0x800775B4` points at, and
    /// the hold `0x801F6964` left on it. `None` when the hold is spent.
    pub timed_message: Option<(u32, i16)>,
    /// Record `0x51`'s content word `0x800773BC` is zero: the strike loop's
    /// counter swap cleared the target plaque.
    pub target_plate_cleared: bool,
}

/// The summon band's live full-screen flash in a capture: which of the two
/// templates it is and how many vsyncs it has run. Read off the SCUS fade
/// actor's `+0x7C` block ([`legaia_engine_core::fade_ramp`]), whose
/// countdowns give the age back exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailFade {
    /// `true` for the `0x33` flash-in (black to white), `false` for the
    /// `0x34` flash-out.
    pub to_white: bool,
    /// Vsyncs since the spawn, the start delay included.
    pub age: u16,
}

/// The SCUS fade actor's tick (`FUN_80025000`), stored at actor `+0x0C`.
const FADE_ACTOR_TICK: u32 = 0x8002_5000;
/// The actor-list "done" bit (`actor[+0x10] |= 8`) a killed fade carries.
const ACTOR_DONE: u32 = 0x8;
/// Where the effect-actor pool lives (`FUN_80020DE0`'s allocations).
const ACTOR_POOL: std::ops::Range<u32> = 0x8007_0000..0x800A_0000;

/// The block's per-frame red delta for a template (`FUN_80020B00`).
fn template_delta(t: &legaia_engine_vm::battle_action::SummonFadeTemplate) -> i16 {
    (((i32::from(t.end_rgb[0]) - i32::from(t.start_rgb[0])) * 0x40) / i32::from(t.duration)) as i16
}

/// Find the live summon flash among the fade actors: tick word
/// `FUN_80025000`, not done, kind `1`, id `1`, and the delta of one of the
/// two summon templates (the creature's own fades share the actor and the
/// kind, never the delta and duration pair).
pub fn summon_fade(ram: &[u8]) -> Option<RetailFade> {
    use legaia_engine_vm::battle_action::{SUMMON_FADE_ID, SUMMON_FLASH_IN, SUMMON_FLASH_OUT};
    let s16 = |va: u32| game_anchors::u16_at(ram, va) as i16;
    let mut a = ACTOR_POOL.start;
    while a + 0xA0 < ACTOR_POOL.end {
        let base = a;
        a += 4;
        if game_anchors::u32_at(ram, base + 0x0C) != FADE_ACTOR_TICK
            || game_anchors::u32_at(ram, base + 0x10) & ACTOR_DONE != 0
        {
            continue;
        }
        let b = base + 0x7C;
        if s16(b + 0x18) != 1 || s16(b + 0x22) != SUMMON_FADE_ID {
            continue;
        }
        let (delta, delay, duration) = (s16(b + 0x10), s16(b + 0x1C), s16(b + 0x20));
        for (t, to_white) in [(&SUMMON_FLASH_IN, true), (&SUMMON_FLASH_OUT, false)] {
            if delta != template_delta(t) {
                continue;
            }
            // `FUN_80020C14` counts the delay down first; the frame it lands
            // also steps the duration, so a landed block's age is one short
            // of the two countdowns' sum.
            let age = if delay > 0 {
                i32::from(t.delay) - i32::from(delay)
            } else if t.delay > 0 {
                i32::from(t.delay) + i32::from(t.duration) - i32::from(duration) - 1
            } else {
                i32::from(t.duration) - i32::from(duration)
            };
            return Some(RetailFade {
                to_white,
                age: age.clamp(0, i32::from(u16::MAX)) as u16,
            });
        }
    }
    None
}

/// Retail's per-frame duration history (`0x80084098`, sixteen halfwords in
/// hsync units, `gp = 0x8007B318`): the frame driver at `0x80017098` stores
/// each frame's duration there (clamped to `0x2BC`) and derives the frame
/// step from the largest of the sixteen.
const FRAME_HISTORY: u32 = 0x8008_4098;
/// A forced frame step (`gp+0x5D8`); non-zero skips the history.
const FORCED_STEP: u32 = 0x8007_B8F0;
/// The frame-rate mode word (`gp+0x4CE`); only mode `0x10` steps adaptively,
/// every other mode stores step `1`.
const STEP_MODE: u32 = 0x8007_B7E6;
/// The step floor (`0x8007B9D8`, read at `0x80017178`).
const STEP_FLOOR: u32 = 0x8007_B9D8;

/// The adaptive frame step `*(0x1F800393)` - vsyncs per game frame - rebuilt
/// from main RAM, since the scratchpad byte itself is not in a main-RAM
/// image. The thresholds are the frame driver's (`0x80017108..0x80017198`):
/// under `0xF1` hsyncs step `1`, under `0x1FF` step `2`, under `0x2D1` step
/// `3`, else `4`, then raised to the floor.
pub fn frame_step(ram: &[u8]) -> u8 {
    let forced = game_anchors::u32_at(ram, FORCED_STEP);
    if forced != 0 {
        return forced as u8;
    }
    if game_anchors::i16_at(ram, STEP_MODE) != 0x10 {
        return 1;
    }
    let longest = (0..16)
        .map(|i| game_anchors::i16_at(ram, FRAME_HISTORY + i * 2))
        .max()
        .unwrap_or(0);
    let step = if longest < 0xF1 {
        1
    } else if longest < 0x1FF {
        2
    } else if longest < 0x2D1 {
        3
    } else {
        4
    };
    let floor = game_anchors::u32_at(ram, STEP_FLOOR).min(4) as u8;
    step.max(floor)
}

/// How far the displayed frame lags the RAM a capture holds: **two** game
/// frames. Retail double-buffers - the CPU builds frame `N` while the GPU
/// draws `N - 1` and the display scans out `N - 2` - so the VRAM display
/// area a capture's frame is cropped from shows the state two ticks before
/// the RAM's. The fade actor steps `*(0x1F800393)` vsyncs a tick
/// (`FUN_80020C14`, `lbu v0,0x393(v0)` at `0x80020C34`), so the flash
/// visible in the frame is `2 * step` vsyncs younger than the block says.
/// Measured on the summon captures: the two packet pools each hold the
/// flash's full-screen `POLY_F4` - one at the block's value (the frame
/// being built), the other one step behind - and the displayed frame is one
/// step older still. A flash-in block at `178` (packets `178` / `153`)
/// shows `123` on screen, one at `255` shows `206`, one at `102` shows
/// `49`, and a block `6` vsyncs past its delay on a step-`3` frame shows no
/// flash at all.
pub fn display_lag_vsyncs(ram: &[u8]) -> u16 {
    2 * u16::from(frame_step(ram))
}

/// The caster's invoke clip through the summon band (`0x32` stages
/// `actor[+0x1DA] = 9`).
const SUMMON_INVOKE_CLIP: u8 = 9;
/// PROT 0903 (Gimard) and its walk-in arm, the one module arm that hands
/// the camera to case 6 (`FUN_801D5854(7, 6)`).
const GIMARD_MODULE: u32 = 903;
const GIMARD_WALK_ARM: u8 = 11;

/// Where in the summon band a capture sits, as an engine frame has to be
/// gated to match it: the action-SM state, and - while a flash is up - the
/// same flash the same number of vsyncs in. `play-window` reads it as
/// `LEGAIA_CAPTURE_GATE` and captures the first frame it holds; the headless
/// seed samples on the same predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseGate {
    pub action_state: u8,
    pub fade: Option<RetailFade>,
    /// The summon module's phase `ctx[+0x279]`, gated on only in `0x35` /
    /// `0x36` and only for a module whose arms the engine paces on retail's
    /// own countdown (`cast_module_camera::module_director`): there the band's
    /// length is the module's, so the state alone does not place the frame.
    pub module_phase: Option<u8>,
    /// For a `0x33` capture with no flash up yet: retail's close-up
    /// accumulator `ctx[+0x87C]`, which the invoke clip's commit zeroed and
    /// which then gains `8` a vsync. The state alone does not place such a
    /// frame - `0x33` runs until the clip's first effect record fires - so
    /// the engine frame is the first one where its caster has committed the
    /// same clip and the engine's own `ctx[+0x87C]` has reached the value.
    pub cam_accum: Option<u32>,
    /// For a capture inside PROT 0903's walk arm (`11`): retail's yaw base
    /// `ctx[+0x6DA]`, which arm 10 seats at `0x200` and the walk swings by
    /// `6 * scalar * delta` a pass. The module phase is the arm's entry; this
    /// is how far into the walk the frame is.
    pub walk_yaw: Option<u16>,
    /// For a capture in the Done band's hold (`0x51` / `0x52`): retail's
    /// countdown `ctx[+0x6D8]` ([`SpanGate::DoneHold`]). The band holds
    /// `0x3C` frames, or `0x96` behind a magic level-up banner, so the state
    /// alone places the frame at the hold's first vsync -
    /// `shiny_refactor_gimard_levelup` was taken 46 vsyncs in.
    pub done_hold: Option<i16>,
    /// For a capture inside one of PROT 0903's countdown-paced arms
    /// (`1..=10`): retail's module countdown
    /// ([`legaia_engine_vm::cast_module_camera::GIMARD_COUNTDOWN_VA`]). The
    /// arm alone places the frame at the arm's first pass - the
    /// `shiny_refactor_gimard_plus35` frame was taken at arm 9's entry, 16
    /// vsyncs before retail's (countdown `928` against the arm's opening
    /// value) - so the engine frame is the first one whose own countdown
    /// has drained as far.
    pub module_countdown: Option<i32>,
}

/// How far a walk-arm yaw base has swung from its seat (`0x200`), in
/// 12-bit units.
fn walk_swing(yaw: i32) -> i32 {
    (yaw - legaia_engine_vm::cast_module_camera::GIMARD_WALK_YAW_BASE) & 0xFFF
}

impl PhaseGate {
    /// `state[,white|black,age]`.
    pub fn to_env(&self) -> String {
        let mut s = self.env_state_and_fade();
        if let Some(p) = self.module_phase {
            s.push_str(&format!(",m{p}"));
        }
        if let Some(a) = self.cam_accum {
            s.push_str(&format!(",a{a}"));
        }
        if let Some(y) = self.walk_yaw {
            s.push_str(&format!(",y{y}"));
        }
        if let Some(d) = self.done_hold {
            s.push_str(&format!(",d{d}"));
        }
        if let Some(c) = self.module_countdown {
            s.push_str(&format!(",c{c}"));
        }
        s
    }

    fn env_state_and_fade(&self) -> String {
        match self.fade {
            None => format!("{}", self.action_state),
            Some(f) => format!(
                "{},{},{}",
                self.action_state,
                if f.to_white { "white" } else { "black" },
                f.age
            ),
        }
    }

    pub fn from_env(s: &str) -> Option<Self> {
        let mut parts: Vec<&str> = s.split(',').map(str::trim).collect();
        let module_countdown = match parts.last() {
            Some(t) if t.starts_with('c') => {
                let c = t[1..].parse().ok()?;
                parts.pop();
                Some(c)
            }
            _ => None,
        };
        let done_hold = match parts.last() {
            Some(t) if t.starts_with('d') => {
                let d = t[1..].parse().ok()?;
                parts.pop();
                Some(d)
            }
            _ => None,
        };
        let walk_yaw = match parts.last() {
            Some(t) if t.starts_with('y') => {
                let y = t[1..].parse().ok()?;
                parts.pop();
                Some(y)
            }
            _ => None,
        };
        let cam_accum = match parts.last() {
            Some(t) if t.starts_with('a') => {
                let a = t[1..].parse().ok()?;
                parts.pop();
                Some(a)
            }
            _ => None,
        };
        let module_phase = match parts.last() {
            Some(t) if t.starts_with('m') => {
                let p = t[1..].parse().ok()?;
                parts.pop();
                Some(p)
            }
            _ => None,
        };
        let mut it = parts.into_iter();
        let action_state = it.next()?.parse().ok()?;
        let fade = match (it.next(), it.next()) {
            (Some(dir), Some(age)) => Some(RetailFade {
                to_white: dir == "white",
                age: age.parse().ok()?,
            }),
            _ => None,
        };
        Some(Self {
            action_state,
            fade,
            module_phase,
            cam_accum,
            walk_yaw,
            done_hold,
            module_countdown,
        })
    }

    /// Whether `world` is at this phase: the action SM on the same state
    /// and, when the capture had a flash up, the same flash at least as far
    /// in.
    pub fn met(&self, world: &legaia_engine_core::world::World) -> bool {
        if world.mode != SceneMode::Battle || world.battle_ctx.action_state != self.action_state {
            return false;
        }
        if let Some(p) = self.module_phase
            && world.casting.module_phase < p
        {
            return false;
        }
        // A walk that arrives sooner than retail's leaves the arm before its
        // yaw gets as far; the arm's exit is then the nearest frame.
        if let Some(y) = self.walk_yaw
            && world.casting.module_phase <= GIMARD_WALK_ARM
            && walk_swing(world.casting.module_cam.yaw_base) < walk_swing(i32::from(y))
        {
            return false;
        }
        // Same arm, and the engine's countdown drained at least as far; a
        // later arm has passed the frame and takes it at once.
        if let Some(c) = self.module_countdown
            && self.module_phase == Some(world.casting.module_phase)
            && world.casting.module_cam.countdown.0 > c
        {
            return false;
        }
        if let Some(acc) = self.cam_accum {
            let caster = world
                .actors
                .get(usize::from(world.battle_ctx.active_actor))
                .map(|a| &a.battle);
            let committed = caster.is_some_and(|b| {
                b.current_anim == SUMMON_INVOKE_CLIP && b.queued_anim == SUMMON_INVOKE_CLIP
            });
            // The engine's own `ctx[+0x87C]`: the commit zeroes it and the
            // framing prologue adds `8` on the commit frame itself, so it
            // reads `8 * (frames since the commit + 1)` - counting frames
            // from the commit instead is one vsync late, and on a capture
            // taken on the band's last `0x33` frame the gate was never met.
            let accum = world.battle.camera.as_ref().map_or_else(
                || {
                    let since = world
                        .clock
                        .display_frames
                        .saturating_sub(world.battle_ctx.active_clip_commit_frame);
                    since.saturating_add(1).saturating_mul(8)
                },
                |c| u64::from(c.close_up_accum()),
            );
            if !committed || accum < u64::from(acc) {
                return false;
            }
        }
        if let Some(t) = self.done_hold
            && world.battle_ctx.frame_timer > t
        {
            return false;
        }
        let Some(want) = self.fade else {
            return true;
        };
        world.presentation.fade.as_ref().is_some_and(|f| {
            f.kind == 1
                && (f.target_rgb()[0] == 0xFF) == want.to_white
                && f.age_vsyncs() >= want.age
        })
    }
}

impl RetailBattle {
    /// The in-flight cast this capture holds, when the engine can replay it:
    /// a party seat running the summon band (`0x32..=0x36`) on a spell id.
    pub fn inflight_cast(&self) -> Option<legaia_engine_core::world::InflightCastSeed> {
        // The summon band itself, or the Done band a cast hands on to
        // (`0x37` / `0x38` then `0x50..=0x52`) while the caster still holds
        // the frame with its category `2` queued.
        let in_band = (0x32..=0x36).contains(&self.action_state)
            || (matches!(self.action_state, 0x37 | 0x38 | 0x50..=0x52)
                && self.queued_category == 2);
        (in_band
            && self.flow == 0xFF
            && self.active_actor < self.party_count
            && self.queued_action >= legaia_engine_vm::battle_action::SPELL_TRIGGER_SUMMON_MIN_ID)
            .then(|| legaia_engine_core::world::InflightCastSeed {
                caster: self.active_actor,
                spell_id: self.queued_action,
                target: self.target_code,
                ground: self.engine_ground(),
            })
    }

    /// [`Self::ground`] re-keyed to engine battle slots: the party keeps its
    /// seats, monster pool slot `3 + m` is engine slot `party_count + m`.
    pub fn engine_ground(
        &self,
    ) -> [Option<[i16; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        let pc = usize::from(self.party_count);
        for (s, o) in out.iter_mut().enumerate().take(pc) {
            *o = self.ground.get(s).copied().flatten();
        }
        for m in 0..usize::from(self.monster_count) {
            if let Some(o) = out.get_mut(pc + m) {
                *o = self.ground.get(3 + m).copied().flatten();
            }
        }
        out
    }

    /// The ground pairs the seed puts on its first battle tick, by engine
    /// slot ([`BarSeed::ground`]).
    ///
    /// Retail walks nobody home after an action
    /// (`docs/subsystems/battle-action.md`, "Where an action leaves its
    /// combatants"), so a capture of a running fight stands its combatants
    /// wherever earlier rounds left them - a monster that struck earlier
    /// casts from beside the party - and every case that frames an actor
    /// (case 6 on a caster, case 0 on a member, case 9's formation box)
    /// frames that ground. A fresh entry's authored seats frame it elsewhere.
    /// The replayed cast carries the same pairs at its dispatch
    /// ([`Self::inflight_cast`]).
    ///
    /// Not placed on an opening capture, which is sampled at the flip before
    /// any round ran. The acting seat is placed too, even on a captured
    /// Attack whose `+0x34` / `+0x38` is a point on the walk the drive is
    /// about to replay: the walk ends at the target whatever it starts from,
    /// and leaving that seat home measured worse over the corpus.
    pub fn seeded_ground(
        &self,
    ) -> [Option<[i16; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        if self.seed_plan() == SeedPlan::Opening {
            return [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        }
        self.engine_ground()
    }

    /// This capture with every combatant's ground pair moved back by what
    /// the engine's replay of the action moved it
    /// (`EngineBattle::ground_drift`), or `None` when nothing moved enough
    /// to matter ([`UNDRIFT_MIN`]).
    ///
    /// A capture inside an action holds its combatants where the action had
    /// **already** moved them - the knockback a strike landed, a target
    /// shoved back by a hit - and the seed places them there before the
    /// drive replays that same action from its start, so every push lands a
    /// second time (`battle_gimard_tail_fire_a`: Vahn ends 129 units behind
    /// his captured pair, and the framing that follows him loses Gimard off
    /// the edge). No word in the capture holds the pre-action ground, but
    /// the engine's own replay measures the push: seeding
    /// `captured - drift` stands each combatant on retail's pair at the
    /// phase.
    ///
    /// The acting seat is the exception. Its own drift is its approach,
    /// which ends at its target from wherever it starts, and the direction
    /// it walks in is the heading every framing case subtracts - so taking
    /// the walk off its start turns the shot. An acting seat that walked
    /// moves with its **target's** drift instead: the pair keeps the
    /// geometry of the first run (`battle_melee_hit_spark`'s Vahn stays on
    /// his side of the monster, at the same distance) and lands on retail's
    /// ground. One that did not walk (a caster) stays put.
    pub fn undrift(
        &self,
        drift: &[Option<[i32; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
    ) -> Option<Self> {
        // Past the end signal the sequencer poses the winners, and a win
        // pose's own travel is not a push the capture holds twice.
        if self.span_gate.is_end() {
            return None;
        }
        let pc = usize::from(self.party_count);
        let significant =
            |d: Option<[i32; 2]>| d.filter(|[dx, dz]| dx.abs().max(dz.abs()) >= UNDRIFT_MIN);
        let acting = usize::from(engine_seat(self.active_actor, self.party_count));
        let target = (self.target_code < 8)
            .then(|| usize::from(engine_seat(self.target_code, self.party_count)));
        let measured = *drift;
        let mut drift = measured;
        if let Some(own) = drift.get_mut(acting) {
            let target_drift = target
                .filter(|&t| t != acting)
                .and_then(|t| measured.get(t).copied().flatten());
            *own = significant(*own).and_then(|_| significant(target_drift));
        }
        let mut out = self.clone();
        let mut moved = false;
        for (slot, d) in drift.iter().enumerate() {
            let Some([dx, dz]) = significant(*d) else {
                continue;
            };
            let pool = if slot < pc { slot } else { 3 + (slot - pc) };
            let Some(Some([x, z])) = out.ground.get_mut(pool) else {
                continue;
            };
            let clamp = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            *x = clamp(i32::from(*x) - dx);
            *z = clamp(i32::from(*z) - dz);
            moved = true;
        }
        moved.then_some(out)
    }

    /// [`Self::defeat_lanes`] re-keyed to engine battle slots.
    pub fn engine_defeat_lanes(
        &self,
    ) -> [Option<u32>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        let pc = usize::from(self.party_count);
        for (s, o) in out.iter_mut().enumerate() {
            let pool = if s < pc { s } else { 3 + (s - pc) };
            *o = self.defeat_lanes.get(pool).copied().flatten();
        }
        out
    }

    /// [`Self::facing`] re-keyed to engine battle slots, placed where
    /// [`Self::seeded_ground`] places a ground pair - on a capture past the
    /// end signal only. There the sequencer has stopped the action SM, so
    /// nothing recomputes a heading before the results framing reads it. A
    /// capture of a running fight replays its rounds, whose own swings set
    /// the headings; seeding the captured ones there moved the corpus both
    /// ways (camera up on most, frames down on more than rose).
    pub fn seeded_facing(&self) -> [Option<u16>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        if !self.span_gate.is_end() {
            return out;
        }
        let ground = self.seeded_ground();
        let pc = usize::from(self.party_count);
        for (s, o) in out.iter_mut().enumerate() {
            if ground[s].is_none() {
                continue;
            }
            let pool = if s < pc { s } else { 3 + (s - pc) };
            *o = self.facing.get(pool).copied().flatten();
        }
        out
    }

    /// The phase the capture's **displayed frame** sits at: [`Self::phase_gate`]
    /// with the flash's age taken back by [`Self::display_lag`]. The RAM
    /// channels are sampled on the RAM's phase; the image is the frame the
    /// display was scanning out, two game frames older.
    pub fn display_phase_gate(&self) -> Option<PhaseGate> {
        let mut g = self.phase_gate()?;
        if let Some(f) = g.fade.as_mut() {
            f.age = f.age.saturating_sub(self.display_lag);
        }
        if let Some(a) = g.cam_accum.as_mut() {
            *a = a.saturating_sub(u32::from(self.display_lag) * 8);
        }
        if let Some(d) = g.done_hold.as_mut() {
            // The countdown drains by the frame step a frame; the displayed
            // frame is that many vsyncs older, so its countdown was higher.
            *d = d.saturating_add(self.display_lag as i16);
        }
        if let Some(c) = g.module_countdown.as_mut() {
            // The countdown drains `scalar` a vsync; the displayed frame's
            // word was that much higher.
            *c += i32::from(self.display_lag)
                * legaia_engine_vm::cast_module_camera::MODULE_DRAIN_PER_TICK;
        }
        if let Some(y) = g.walk_yaw.as_mut() {
            // `6 * scalar` a vsync, taken back no further than the seat.
            let per_vsync = 6 * legaia_engine_vm::cast_module_camera::MODULE_DRAIN_PER_TICK;
            let back = (i32::from(self.display_lag) * per_vsync).min(walk_swing(i32::from(*y)));
            *y = ((i32::from(*y) - back) & 0xFFF) as u16;
        }
        Some(g)
    }

    /// The phase an in-flight capture's engine frame is gated on.
    pub fn phase_gate(&self) -> Option<PhaseGate> {
        self.inflight_cast()?;
        // The player summon block is linear: PROT `903 + (id - 0x81)`.
        let entry = u32::from(self.queued_action).wrapping_sub(0x81) + 903;
        let directed = (0x81..=0xA0).contains(&self.queued_action)
            && legaia_engine_vm::cast_module_camera::module_profile(entry)
                .is_some_and(|p| p.paces_band());
        let module_phase =
            (directed && (0x35..=0x36).contains(&self.action_state)).then_some(self.module_phase);
        // A `0x33` frame before the flash-in is placed by how long the invoke
        // clip has run: the accumulator its commit zeroed.
        let cam_accum = (self.action_state == 0x33
            && self.summon_fade.is_none()
            && self.caster_clip == SUMMON_INVOKE_CLIP)
            .then_some(self.cam_accum);
        // PROT 0903's walk arm hands the camera to case 6 on the walking
        // creature; its yaw base says how far in the frame is.
        let walk_yaw = (entry == GIMARD_MODULE && module_phase == Some(GIMARD_WALK_ARM))
            .then_some(self.walk_yaw_base);
        let done_hold = match self.span_gate {
            SpanGate::DoneHold { timer } => Some(timer),
            _ => None,
        };
        // PROT 0903's countdown-paced arms: how far into the arm the frame is.
        let module_countdown = (entry == GIMARD_MODULE
            && module_phase.is_some_and(|p| (1..GIMARD_WALK_ARM).contains(&p)))
        .then_some(self.gimard_countdown);
        Some(PhaseGate {
            action_state: self.action_state,
            fade: self.summon_fade,
            module_phase,
            cam_accum,
            walk_yaw,
            done_hold,
            module_countdown,
        })
    }
}

/// Where in the fight a capture sits, as far as the seed has to place the
/// engine to compare the same phase ([`RetailBattle::seed_plan`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedPlan {
    /// The fight is still opening: `ctx[+0x06]` holds one of the entry
    /// values below the round prompt. The engine is sampled once its
    /// entry sweep has run as far as retail's ([`entry_sweep_reached`]).
    Opening,
    /// A command-selection surface above the round prompt, on party seat
    /// `seat` (`ctx[+0x13]`, the member cursor): the engine's command flow
    /// is driven there through its pad path.
    Menu { flow: BattleFlowState, seat: u8 },
    /// A party cast in the summon band: replayed through
    /// [`legaia_engine_core::world::InflightCastSeed`] and gated on
    /// [`PhaseGate`].
    Cast,
    /// Any other action in flight (flow `0xFF`): the round is played out
    /// through the pad path until the engine's action SM holds the same
    /// state on the same seat.
    Action { seat: u8, state: u8 },
    /// The round prompt: park on it and settle.
    Prompt,
}

/// Ticks an opening seed may run waiting for the engine's entry sweep to
/// reach retail's counter ([`entry_sweep_reached`]); the sweep spans `0x41`
/// display frames.
const ENTRY_SWEEP_TICKS: u32 = 120;

/// Whether the engine's battle-entry sweep
/// ([`legaia_engine_vm::battle_cam_script::BattleCamera::start_entry_sweep`])
/// has run as far as retail's frame-driver counter `entry` reads: a counter
/// inside the sweep (`0x80..=0xC0`) is met once the engine's has reached it,
/// one past it (`0xFF`) once the engine's sweep is over, and one below it
/// (the fight still loading) at once.
pub fn entry_sweep_reached(world: &legaia_engine_core::world::World, entry: u8) -> bool {
    if entry < 0x80 {
        return true;
    }
    world.battle.camera.as_ref().is_some_and(|c| {
        c.entry_sweep_counter()
            .is_none_or(|n| entry != 0xFF && n >= u32::from(entry))
    })
}

/// `ctx[+0x06]` values below the round prompt: SCUS battle init's `0xFD`
/// (`FUN_80055B6C`, `sb v0,0x6(v1)` at `0x80055FA8`), the overlay's init
/// `0x00`, the intro timer `0x0A` / `0x0B`, the boss stage module's baton
/// `0x0C`, and the one-frame turn setup `0x14` (`FUN_801D0748`,
/// `docs/subsystems/battle.md`).
pub const OPENING_FLOWS: [u8; 6] = [0xFD, 0x00, 0x0A, 0x0B, 0x0C, 0x14];

impl RetailBattle {
    /// How the seed has to place the engine for this capture.
    pub fn seed_plan(&self) -> SeedPlan {
        if OPENING_FLOWS.contains(&self.flow) {
            return SeedPlan::Opening;
        }
        if self.inflight_cast().is_some() {
            return SeedPlan::Cast;
        }
        if self.flow == 0xFF {
            return SeedPlan::Action {
                seat: self.active_actor,
                state: self.action_state,
            };
        }
        match BattleFlowState::from_raw(self.flow) {
            BattleFlowState::Idle | BattleFlowState::TurnPrompt => SeedPlan::Prompt,
            flow => SeedPlan::Menu {
                flow,
                seat: self.active_actor,
            },
        }
    }
}

/// The battle-end signal byte the `0x5A` wipe gate raises.
const END_SIGNAL: u32 = 0x8007_BD71;
/// The results sequencer's phase word (`_DAT_8007BD2C`; the wipe cause at
/// the signal).
const END_PHASE_WORD: u32 = 0x8007_BD2C;
/// The results hold `gp+0xA54`.
const END_RESULTS_HOLD: u32 = 0x8007_BD6C;

/// Read the [`SpanGate`] off a capture.
fn span_gate(ram: &[u8], ctx: u32) -> SpanGate {
    if game_anchors::u8_at(ram, END_SIGNAL) != 0xFE {
        if matches!(game_anchors::u8_at(ram, ctx + 7), 0x51 | 0x52) {
            return SpanGate::DoneHold {
                timer: game_anchors::u16_at(ram, ctx + 0x6D8) as i16,
            };
        }
        if PRE_SEED_STATES.contains(&game_anchors::u8_at(ram, ctx + 7)) && camera_landed(ram, ctx) {
            return SpanGate::Landed;
        }
        if CAPTURE_FADE_STATES.contains(&game_anchors::u8_at(ram, ctx + 7)) {
            // In the module tick `0x70`, a directed module's arm and the
            // countdown word it gates on place the capture inside the run.
            let arm = (game_anchors::u8_at(ram, ctx + 7) == 0x70)
                .then(|| {
                    let actor = game_anchors::u32_at(
                        ram,
                        ACTOR_TABLE + u32::from(game_anchors::u8_at(ram, ctx + 0x13).min(7)) * 4,
                    );
                    let action = if in_ram(actor) {
                        game_anchors::u8_at(ram, actor + 0x1DF)
                    } else {
                        0
                    };
                    legaia_engine_vm::cast_module_camera::capture_countdown_va(action).map(|va| {
                        (
                            game_anchors::u8_at(ram, ctx + 0x279),
                            game_anchors::u32_at(ram, va) as i32,
                        )
                    })
                })
                .flatten();
            // The yaw counter is a clock only while the action SM drifts it
            // (any Battle Camera option but Far, `0x801E29D4`).
            let yaw = (game_anchors::u8_at(ram, BATTLE_CAMERA_OPTION) != 2)
                .then(|| game_anchors::u16_at(ram, ctx + 0x6DA));
            return SpanGate::CaptureFade {
                height: game_anchors::u16_at(ram, ctx + 0x6D0),
                accum: game_anchors::u32_at(ram, ctx + 0x87C).min(u32::from(u16::MAX)) as u16,
                arm,
                yaw,
            };
        }
        return SpanGate::Age {
            accum: game_anchors::u32_at(ram, ctx + 0x87C).min(u32::from(u16::MAX)) as u16,
        };
    }
    let half = game_anchors::u16_at(ram, ctx + 0x6CE);
    match (game_anchors::u32_at(ram, END_PHASE_WORD), half) {
        (_, h) if h >= 2 => SpanGate::Exit { phase: h },
        (5, 1) => SpanGate::Results {
            hold: game_anchors::u32_at(ram, END_RESULTS_HOLD) as u16,
        },
        _ => SpanGate::Loading,
    }
}

/// The action SM's states ahead of the seed pass `0x0C`: the round-begin
/// `0x00`, the pre-action wait `0x0A` and the menu-queued hold `0x0B`. None of
/// them has picked the next actor into `ctx[+0x13]` yet - the seed pass copies
/// it there from `ctx[+0x274]` (`lbu v0,0x274(v1)` / `sb v0,0x2(s5)` at
/// `0x801E2C50..0x801E2C5C`) - so a capture in one reads the **previous**
/// actor (or the last ring member, at a round's start) in `ctx[+0x13]`.
pub const PRE_SEED_STATES: [u8; 3] = [0x00, 0x0A, 0x0B];

/// `ctx[+0x274]` - the next actor the initiative pick `FUN_801DABA4` chose.
const NEXT_ACTOR: u32 = 0x274;

/// The HUD's tracked widget glides `FUN_801D9BBC` walks: `ctx[+0x11B4]`,
/// `0xC`-byte records `[total][elapsed] .. [target x, y][start x, y]`, one
/// per handle slot `ctx[+0x1074]` (forty).
const HUD_GLIDE_TABLE: u32 = 0x11B4;
const HUD_GLIDE_STRIDE: u32 = 0xC;
const HUD_GLIDE_SLOTS: u32 = 40;

/// One HUD widget glide in flight, as a capture's **displayed** frame shows
/// it: the record's target seat (`+0x04` / `+0x06`, which names the widget -
/// the actor plaque lands on `(16, 12)`, the readout bar on `(16, 192)`, the
/// combo cluster's anchor on `(168, 168)`) and its `elapsed` byte less the
/// display lag ([`display_lag_vsyncs`]). `FUN_801D9BBC` adds the frame step
/// to `elapsed` a battle pass, so the byte counts vsyncs, the unit the lag is
/// in; a glide younger than the lag had not left its start on the displayed
/// frame (`0`).
///
/// The image child seats the engine's glides on these
/// (`LEGAIA_SEAT_HUD_GLIDES`): the replay reaches an action's phase on its
/// own clock, which the plates' sixteen-vsync raise does not share
/// (`nivora_duel_mid_blazing_slash` holds plaque and bar ten vsyncs into the
/// raise, six on screen, where the engine's had landed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudGlideSeat {
    pub target: [i16; 2],
    pub elapsed: u8,
    pub total: u8,
}

impl HudGlideSeat {
    /// `x:y:elapsed:total`, comma-separated - the form
    /// [`Self::list_from_env`] reads back.
    pub fn to_env(seats: &[Self]) -> String {
        seats
            .iter()
            .map(|g| format!("{}:{}:{}:{}", g.target[0], g.target[1], g.elapsed, g.total))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The inverse of [`Self::to_env`]; malformed entries are dropped.
    pub fn list_from_env(v: &str) -> Vec<Self> {
        v.split(',')
            .filter_map(|e| {
                let mut it = e.trim().split(':').map(str::parse::<i32>);
                let (Some(Ok(x)), Some(Ok(y)), Some(Ok(elapsed)), Some(Ok(total))) =
                    (it.next(), it.next(), it.next(), it.next())
                else {
                    return None;
                };
                Some(Self {
                    target: [x as i16, y as i16],
                    elapsed: elapsed.clamp(0, 255) as u8,
                    total: total.clamp(0, 255) as u8,
                })
            })
            .collect()
    }
}

/// Every in-flight record of the HUD glide table, lag-corrected
/// ([`HudGlideSeat`]).
fn hud_glide_seats(ram: &[u8], ctx: u32, lag: u16) -> Vec<HudGlideSeat> {
    (0..HUD_GLIDE_SLOTS)
        .filter_map(|s| {
            let r = ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE;
            let total = game_anchors::u8_at(ram, r);
            (total != 0).then(|| HudGlideSeat {
                target: [
                    game_anchors::i16_at(ram, r + 4),
                    game_anchors::i16_at(ram, r + 6),
                ],
                elapsed: u16::from(game_anchors::u8_at(ram, r + 1)).saturating_sub(lag) as u8,
                total,
            })
        })
        .collect()
}

/// The tween table `FUN_801D829C` builds (`ctx[+0x118C]`, nine
/// `{u16 step, u16 endpoint}` records: pitch / yaw / roll, the translation
/// trio, the focus trio).
const TWEEN_TABLE: u32 = 0x118C;

/// Whether retail's camera sits on its framing: every tweened component with
/// a step reads its endpoint. The yaw is left out - the idle orbit writes it
/// beside the walker. The rotation pair and the translation / focus words
/// compare as the low halfwords the walker steps.
fn camera_landed(ram: &[u8], ctx: u32) -> bool {
    let live = [
        game_anchors::u16_at(ram, 0x8007_B790),
        0,
        game_anchors::u16_at(ram, 0x8007_B794),
        game_anchors::u16_at(ram, 0x8008_40B8),
        game_anchors::u16_at(ram, 0x8008_40BC),
        game_anchors::u16_at(ram, 0x8008_40C0),
        game_anchors::u16_at(ram, 0x8008_9118),
        game_anchors::u16_at(ram, 0x8008_911C),
        game_anchors::u16_at(ram, 0x8008_9120),
    ];
    (0..9u32).filter(|&k| k != 1).all(|k| {
        let rec = ctx + TWEEN_TABLE + k * 4;
        let end = game_anchors::u16_at(ram, rec + 2);
        let mask = if k < 3 { 0xFFF } else { 0xFFFF };
        live[k as usize] & mask == end & mask
    })
}

fn in_ram(p: u32) -> bool {
    (0x8000_0000..0x8020_0000).contains(&p)
}

fn combatant(ram: &[u8], slot: u32) -> Option<Combatant> {
    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
    if !in_ram(p) {
        return None;
    }
    Some(Combatant {
        hp: game_anchors::u16_at(ram, p + 0x14C),
        hp_max: game_anchors::u16_at(ram, p + 0x14E),
        mp: game_anchors::u16_at(ram, p + 0x150),
        mp_max: game_anchors::u16_at(ram, p + 0x152),
    })
}

impl RetailBattle {
    /// Read the battle observables; `Err` names why the state is not a
    /// seedable battle.
    pub fn from_ram(ram: &[u8]) -> std::result::Result<Self, String> {
        let ctx = game_anchors::u32_at(ram, BATTLE_CTX);
        if !in_ram(ctx) {
            return Err(format!(
                "battle context pointer 0x{ctx:08X} not resident (battle still loading)"
            ));
        }
        let party_count = game_anchors::u8_at(ram, ctx);
        let monster_count = game_anchors::u8_at(ram, ctx + 1);
        if !(1..=3).contains(&party_count) || !(1..=5).contains(&monster_count) {
            return Err(format!(
                "battle context counts out of range (party {party_count}, monsters {monster_count})"
            ));
        }
        let monster_ids: Vec<u8> = (0..u32::from(monster_count).min(4))
            .map(|m| game_anchors::u8_at(ram, FORMATION_CELL + m))
            .collect();
        if monster_ids.iter().all(|&id| id == 0) {
            return Err("formation cell empty".into());
        }
        let action_state = game_anchors::u8_at(ram, ctx + 7);
        let active_actor = if game_anchors::u8_at(ram, ctx + 6) == 0xFF
            && PRE_SEED_STATES.contains(&action_state)
        {
            game_anchors::u8_at(ram, ctx + NEXT_ACTOR)
        } else {
            game_anchors::u8_at(ram, ctx + 0x13)
        };
        let active = Some(game_anchors::u32_at(
            ram,
            ACTOR_TABLE + u32::from(active_actor.min(7)) * 4,
        ))
        .filter(|&p| in_ram(p));
        Ok(Self {
            party_count,
            monster_count,
            flow: game_anchors::u8_at(ram, ctx + 6),
            action_state,
            run_state: game_anchors::u8_at(ram, RUN_STATE),
            stage_id: game_anchors::u8_at(ram, STAGE_ID),
            scripted: game_anchors::u32_at(ram, PER_BATTLE_FLAGS) & 0x80 != 0,
            stage_variant: game_anchors::u8_at(ram, PER_BATTLE_FLAGS) & 0x1F,
            keep_backdrop_object_1: game_anchors::u8_at(ram, KEEP_BACKDROP_OBJECT_1) != 0,
            monster_ids,
            seat_chars: (0..u32::from(party_count))
                .map(|s| game_anchors::u8_at(ram, SEAT_CHARS + s))
                .collect(),
            party: (0..u32::from(party_count))
                .map(|s| combatant(ram, s))
                .collect(),
            monsters: (0..u32::from(monster_count))
                .map(|m| combatant(ram, 3 + m))
                .collect(),
            active_actor,
            queued_action: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DF)),
            target_code: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DD)),
            queued_category: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DE)),
            summon_fade: summon_fade(ram),
            display_lag: display_lag_vsyncs(ram),
            cam_accum: game_anchors::u32_at(ram, ctx + 0x87C),
            caster_clip: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1D9)),
            walk_yaw_base: game_anchors::u16_at(ram, ctx + 0x6DA),
            track_coin: game_anchors::u8_at(ram, ctx + 0x26D) & 1,
            gimard_countdown: game_anchors::u32_at(
                ram,
                legaia_engine_vm::cast_module_camera::GIMARD_COUNTDOWN_VA,
            ) as i32,
            cam_style: game_anchors::u8_at(ram, ctx + 0xD),
            camera_option: game_anchors::u8_at(ram, BATTLE_CAMERA_OPTION),
            hud_glides_landed: (0..HUD_GLIDE_SLOTS).all(|s| {
                game_anchors::u8_at(ram, ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE) == 0
            }),
            hud_glides: hud_glide_seats(ram, ctx, display_lag_vsyncs(ram)),
            entry_counter: game_anchors::u8_at(ram, ENTRY_COUNTER),
            absorbed_seru: game_anchors::u8_at(ram, ctx + 0x269),
            magic_level_up: game_anchors::u8_at(ram, ctx + 0x26) == MAGIC_LEVEL_BANNER,
            strike_cursor: game_anchors::u8_at(ram, ctx + 0x15),
            committed_queue: std::array::from_fn(|i| {
                active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DF + i as u32))
            }),
            arts_queue: active.is_some_and(|p| {
                (0..0x10).any(|i| matches!(game_anchors::u8_at(ram, p + 0x1DF + i), 0x19 | 0x1A))
            }),
            acting_gauge: active.map(|p| {
                (
                    game_anchors::u16_at(ram, p + 0x154),
                    game_anchors::u16_at(ram, p + 0x156),
                )
            }),
            span_gate: span_gate(ram, ctx),
            win_pose: span_gate(ram, ctx)
                .is_end()
                .then(|| active.map(|p| game_anchors::u8_at(ram, p + 0x1DB)))
                .flatten(),
            module_phase: game_anchors::u8_at(ram, ctx + 0x279),
            ground: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    in_ram(p).then(|| {
                        [
                            game_anchors::u16_at(ram, p + 0x34) as i16,
                            game_anchors::u16_at(ram, p + 0x38) as i16,
                        ]
                    })
                })
                .collect(),
            facing: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    in_ram(p).then(|| game_anchors::u16_at(ram, p + 0x46) & 0xFFF)
                })
                .collect(),
            defeat_lanes: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    (in_ram(p)
                        && game_anchors::u8_at(ram, p + 0x21C)
                            == legaia_engine_vm::battle_formulas::STATE_DEFEAT_FADE)
                        .then(|| game_anchors::u32_at(ram, p + 4))
                })
                .collect(),
            timed_message: {
                let hold = game_anchors::u32_at(ram, TIMED_MESSAGE_HOLD) as i32;
                let va = game_anchors::u32_at(ram, TIMED_MESSAGE_WORD);
                (hold > 0 && va != 0).then_some((va, hold.min(i32::from(i16::MAX)) as i16))
            },
            target_plate_cleared: game_anchors::u32_at(ram, TARGET_PLATE_WORD) == 0,
        })
    }
}

/// The timed message's hold `0x801F6964` (`FUN_80046A20` counts it down).
const TIMED_MESSAGE_HOLD: u32 = 0x801F_6964;
/// Record `0x66`'s content word, `0x80076C10 + 0x66 * 0x18 + 0x14`.
const TIMED_MESSAGE_WORD: u32 = 0x8007_75B4;
/// Record `0x51`'s content word, `0x80076C10 + 0x51 * 0x18 + 0x14`.
const TARGET_PLATE_WORD: u32 = 0x8007_73BC;

/// What the engine shows after the battle seed.
pub struct EngineBattle {
    /// The mid-fight HP / MP this run seeded on its first battle tick
    /// ([`apply_bar_seeds`]); the image child seeds the same bars
    /// (`LEGAIA_BATTLE_BARS`).
    pub hp_seed: Vec<BarSeed>,
    pub scene: Option<String>,
    pub mode: SceneMode,
    /// How the formation was reached: `man row N` or `synthesized`.
    pub formation_source: String,
    /// The scene's MAN formation row the fight was entered through, when one
    /// carries the retail cell (what `play-window --battle` can name).
    pub man_row: Option<u16>,
    /// Ticks from the battle-mode flip to the first command prompt; `None`
    /// when the opening never reached one.
    pub prompt_tick: Option<u32>,
    /// `Some` when the capture's cast was replayed: the ticks from the
    /// dispatch to the capture's phase, `None` inside when the engine never
    /// reached it.
    pub inflight: Option<Option<u32>>,
    /// `Some` when the capture's menu state or in-flight action was driven
    /// to through the pad path ([`SeedPlan::Menu`] / [`SeedPlan::Action`]):
    /// the ticks from the first prompt to the capture's phase, `None` inside
    /// when the engine never reached it.
    pub driven: Option<Option<u32>>,
    /// The world stream's state at the encounter entry
    /// ([`BATTLE_RNG_SEEDS`]).
    pub rng_seed: u32,
    /// The formation advantage the entry rolled (`ctx+0x290`) was a back
    /// attack or a pre-emptive strike. A capture of a running fight is not
    /// its opening round, so such an opening hands one side a round - the
    /// monsters' swings on the party, or the party's on the monsters - that
    /// is not in the history the capture's HP / MP were read from
    /// ([`RetailBattle::seed_plan`] other than the opening).
    pub surprise_opening: bool,
    /// For a [`SpanGate::Age`] drive whose state the engine left before it
    /// was as old as retail's: the engine accumulator on its last tick
    /// there, which a re-run gates on instead (and which the re-run's own
    /// result carries as the gate it was sampled on).
    pub age_short: Option<u16>,
    /// The win pose the engine's battle-end sequence picked, when it runs.
    pub win_pose: Option<u8>,
    /// The engine's action-SM state when sampled.
    pub action_state: u8,
    /// The engine's active seat `ctx[+0x13]` when sampled.
    pub active_actor: u8,
    pub monster_ids: Vec<Option<u16>>,
    pub party: Vec<Combatant>,
    pub monsters: Vec<Combatant>,
    pub flow: BattleFlowState,
    pub camera: CameraObs,
    pub bgm_id: Option<u16>,
    /// The engine's track word and `World::audio.current_bgm` just before
    /// the encounter was forced.
    pub field_word: Option<u16>,
    pub field_current: Option<u16>,
    /// The track the engine plays during the fight.
    pub battle_bgm: Option<u16>,
    pub save: legaia_save::SaveFile,
    /// Per engine slot, how far the replay moved a combatant the seed
    /// placed (`[x, z]`, the phase's ground pair less the seeded one), for
    /// a driven action that reached its phase ([`RetailBattle::undrift`]).
    pub ground_drift: [Option<[i32; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
}

/// The smallest replay drift, on either axis, [`RetailBattle::undrift`]
/// takes back - under it the push is noise against the framings' own
/// tolerance and a second run would only re-sample the same frame.
pub const UNDRIFT_MIN: i32 = 16;

/// How far a run left its placed combatants from the capture's own ground
/// pairs at the phase, summed over the slots `seeded` placed (`seeded` is
/// the capture the run was seeded from - `captured` itself, or its
/// [`RetailBattle::undrift`]). `None` when the run measured no drift.
pub fn ground_residual(
    captured: &RetailBattle,
    seeded: &RetailBattle,
    run: &EngineBattle,
) -> Option<i64> {
    let want = captured.seeded_ground();
    let from = seeded.seeded_ground();
    let mut sum = None;
    for slot in 0..legaia_engine_core::world::INFLIGHT_GROUND_SLOTS {
        let (Some([wx, wz]), Some([fx, fz]), Some([dx, dz])) =
            (want[slot], from[slot], run.ground_drift[slot])
        else {
            continue;
        };
        let ex = i64::from(fx) + i64::from(dx) - i64::from(wx);
        let ez = i64::from(fz) + i64::from(dz) - i64::from(wz);
        *sum.get_or_insert(0) += ex.abs() + ez.abs();
    }
    sum
}

/// The first registered formation row whose monster list equals `ids`.
fn matching_row(world: &legaia_engine_core::world::World, ids: &[u8]) -> Option<u16> {
    world.registered_formation_ids().into_iter().find(|fid| {
        world
            .tables
            .formation_table
            .formation(*fid)
            .is_some_and(|d| {
                d.slots.len() == ids.len()
                    && d.slots
                        .iter()
                        .zip(ids)
                        .all(|(s, &id)| s.monster_id == u16::from(id))
            })
    })
}

/// One combatant's mid-fight bars as the capture read them: engine actor
/// slot, HP, MP - and, where the seed places it, the live ground pair
/// `+0x34` / `+0x38` ([`RetailBattle::seeded_ground`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarSeed {
    pub slot: u8,
    pub hp: u16,
    pub mp: u16,
    pub ground: Option<[i16; 2]>,
    /// The heading `+0x46` that goes with [`Self::ground`].
    pub facing: Option<u16>,
    /// A body seeded inside (or past) its defeat fade: the colour lanes it
    /// holds ([`RetailBattle::defeat_lanes`]).
    pub defeat_lanes: Option<u32>,
}

/// Seed the capture's HP / MP onto the engine actors (the engine enters a
/// fight on full bars). Run on the first battle tick by the headless seed and
/// by the `play-window` image child alike: the monster AI's picks and every
/// kill read these bars, so a child on full bars plays a different fight
/// from the one its seed scored.
pub fn apply_bar_seeds(world: &mut legaia_engine_core::world::World, seeds: &[BarSeed]) {
    for s in seeds {
        let Some(a) = world.actors.get_mut(usize::from(s.slot)) else {
            continue;
        };
        a.battle.hp = s.hp;
        a.battle.liveness = a.battle.hp;
        if a.battle.hp_display.is_some() {
            a.battle.hp_display = Some(a.battle.hp);
        }
        a.battle.mp = s.mp;
        if let Some([x, z]) = s.ground {
            a.move_state.world_x = x;
            a.move_state.world_z = z;
            if a.battle.seat.is_some() {
                a.battle.seat = Some((x, z));
            }
        }
        if let Some(f) = s.facing {
            a.battle.facing_angle = f;
        }
        if let Some(lanes) = s.defeat_lanes
            && a.battle.hp == 0
        {
            a.battle.render_flag = legaia_engine_vm::battle_formulas::STATE_DEFEAT_FADE;
            a.battle.render_color = lanes;
        }
    }
}

/// `slot:hp:mp[:x:z],...` for `LEGAIA_BATTLE_BARS`.
pub fn bar_seeds_to_env(seeds: &[BarSeed]) -> String {
    seeds
        .iter()
        .map(|s| {
            let head = match (s.ground, s.facing) {
                (Some([x, z]), Some(f)) => format!("{}:{}:{}:{x}:{z}:{f}", s.slot, s.hp, s.mp),
                (Some([x, z]), None) => format!("{}:{}:{}:{x}:{z}", s.slot, s.hp, s.mp),
                _ => format!("{}:{}:{}", s.slot, s.hp, s.mp),
            };
            match s.defeat_lanes {
                Some(l) => format!("{head}:d{l:x}"),
                None => head,
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The inverse of [`bar_seeds_to_env`]; malformed entries are skipped.
pub fn bar_seeds_from_env(v: &str) -> Vec<BarSeed> {
    v.split(',')
        .filter_map(|e| {
            let (e, defeat_lanes) = match e.trim().rsplit_once(":d") {
                Some((head, l)) => (head, Some(u32::from_str_radix(l, 16).ok()?)),
                None => (e.trim(), None),
            };
            let mut it = e.split(':');
            let slot = it.next()?.parse().ok()?;
            let hp = it.next()?.parse().ok()?;
            let mp = it.next()?.parse().ok()?;
            let ground = match (it.next(), it.next()) {
                (Some(x), Some(z)) => Some([x.parse().ok()?, z.parse().ok()?]),
                _ => None,
            };
            let facing = match it.next() {
                Some(f) => Some(f.parse().ok()?),
                None => None,
            };
            Some(BarSeed {
                slot,
                hp,
                mp,
                ground,
                facing,
                defeat_lanes,
            })
        })
        .collect()
}

/// Seed the engine into `retail`'s battle and sample it.
pub fn run_engine_battle(
    extracted: &Path,
    retail: &RetailObs,
    battle: &RetailBattle,
    rng_seed: u32,
    revive_victims: bool,
) -> Result<EngineBattle> {
    let cfg = BootConfig {
        scene: retail.scene.clone(),
        enable_audio: false,
    };
    let mut session = BootSession::open(extracted, &cfg).context("open boot session")?;
    // Player-driven: retail parks every party turn on the command flow, so an
    // auto-resolving engine would spend the settle window fighting.
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        ..Default::default()
    };
    let save = retail
        .save
        .clone()
        .context("retail state has no liftable save window")?;
    let _landing = session.resume_save(save, &retail.scene, &opts);
    let mut director = crate::retail_compare::RecordingDirector::default();
    session.host.route_bgm_events(&mut director)?;
    // The fight is entered from a settled field, as retail's was: the
    // scene's entry scripts start its track and raise its entry state first.
    for _ in 0..crate::retail_compare::SETTLE_TICKS {
        // The settle is seeding, not elapsed game time: the script timers
        // hold the counts retail's field had when the fight began (the battle
        // does not run the field VM, so the capture's slot table is the
        // field's), else a short timer runs out inside the window and fires
        // an arm retail had not reached (`jouind`'s spawn delay, 24 of 50).
        crate::retail_compare::hold_slot_table(&mut session, retail);
        session.tick()?;
        session.fog_render_tick();
        session.host.route_bgm_events(&mut director)?;
    }
    let world = &mut session.host.world;
    let source;
    let man_row = matching_row(world, &battle.monster_ids);
    let fid = match man_row {
        Some(row) => {
            source = format!("man row {row}");
            // A scripted carrier's record raises its one-shot system-flag
            // arm on the way into the fight (the sparring tutorial's `50 19`);
            // the forced entry replays it, as `play-window --battle` does.
            world.replay_scripted_battle_arm(row);
            // The same record's op-0x35 words pick the fight's music: a
            // scripted boss's event starts its theme and selects the battle
            // sound set before the entry op (korb3's Gaza: `2028`, set
            // `-1`), so the forced entry replays them too.
            world.replay_scripted_battle_score(row);
            row
        }
        None => {
            // No row of this scene carries the retail cell: a scripted
            // fight installed from another table, or a scene whose rows the
            // engine does not register. Register the cell as its own
            // formation, with the archive's stats for its ids.
            let rec = legaia_engine_core::encounter_record::EncounterRecord::new(
                battle.monster_ids.len() as u8,
                &battle.monster_ids,
            )
            .context("formation cell does not fit a record")?;
            let mut def = rec.to_formation_def(retail.scene.clone());
            let mut id = def.formation_id;
            while world.tables.formation_table.formation(id).is_some() {
                id = id.wrapping_add(1);
            }
            def.formation_id = id;
            def.header_flags = u8::from(battle.scripted);
            world.tables.formation_table.insert(def);
            source = "synthesized".to_string();
            id
        }
    };
    let ids: Vec<u16> = battle.monster_ids.iter().map(|&i| u16::from(i)).collect();
    if ids
        .iter()
        .any(|id| session.host.world.tables.monster_catalog.get(*id).is_none())
    {
        let archive = session.host.index.entry_bytes_extended(867)?;
        let cat = legaia_engine_core::monster_catalog::catalog_from_monster_archive(&archive, &ids);
        for def in cat.by_id.into_values() {
            session.host.world.tables.monster_catalog.insert(def);
        }
    }
    // The fight's own composition: retail's present list `0x8007BD10` is
    // the battle's seat table, not the save window's field party. They
    // differ on a guest seat (id `4`) and on a battle-id fight, whose init
    // (`FUN_80055B6C` with `DAT_8007B7FC != 0`) re-seeds the fallback trio
    // `{1, 2, 3}` through `FUN_80055B20` whatever the field party was. The
    // image side passes the same list as `play-window --party`.
    // The stage the fight is staged in: the region reader's variant for the
    // tile it started on. The capture's player actor is no longer the field
    // walker, so the seed cannot stand on that tile; it stamps the variant.
    session
        .host
        .world
        .seed_battle_stage_variant(battle.stage_variant);
    session
        .host
        .world
        .seed_battle_backdrop_keep_object_1(battle.keep_backdrop_object_1);
    session.host.world.toggles.battle_camera =
        legaia_engine_core::options::BattleCameraOpt::from_word(battle.camera_option);
    // Hand any replayed BGM words to the director before reading the word.
    session.host.route_bgm_events(&mut director)?;
    let seats = retail_roster_slots(battle);
    if !seats.is_empty() && seats != session.host.world.party.active_party {
        session.host.world.set_active_party(seats);
    }
    // The field's own track word and the world's current track, as the
    // battle swap will find them.
    let field_word = session.host.bgm_track_word.or(director.last);
    let field_current = session.host.world.audio.current_bgm;
    session.host.world.rng_state = rng_seed;
    if !session.host.world.force_encounter(fid) {
        bail!("force_encounter({fid}) refused ({source})");
    }
    let mut entered = false;
    for _ in 0..ENTRY_TICKS {
        session.tick()?;
        session.fog_render_tick();
        session.host.route_bgm_events(&mut director)?;
        if session.host.world.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    if !entered {
        bail!(
            "forced encounter ({source}) never reached battle mode in {ENTRY_TICKS} ticks (mode {:?})",
            session.host.world.mode
        );
    }
    // Read on the latch as well: the entry's own round-one Begin may have
    // run already and moved `+0x290` into `+0x291`.
    let surprise_opening = {
        let w = &session.host.world;
        let none = legaia_engine_vm::battle_formulas::FormationAdvantage::None;
        w.battle_formation() != none || w.battle_formation_latched() != none
    };
    // Reconstruct the mid-fight HP / MP (the engine seeds full bars).
    //
    // A capture taken on a party action in flight whose target already
    // reads `0` HP (`+0x14C`) is the swing that killed it: the drive replays
    // that action from the prompt, and a monster seeded dead would end the
    // fight at the first `0x5A` wipe gate before the swing ever started. Such
    // a victim is seeded at `1` HP, so the replayed swing makes the kill the
    // capture shows, and its HP is read at the phase rather than at the
    // prompt. The caller asks for it ([`RetailBattle::action_victims`]) only
    // after the capture's phase was not reached with the HP as read.
    let pc_seed = session.host.world.party.party_count.clamp(1, 3) as usize;
    let victims = if revive_victims {
        battle.action_victims()
    } else {
        Vec::new()
    };
    let ground = battle.seeded_ground();
    let facing = battle.seeded_facing();
    let defeat_lanes = battle.engine_defeat_lanes();
    let hp_seed: Vec<BarSeed> = {
        let world = &session.host.world;
        let pc = world.party.party_count.clamp(1, 3) as usize;
        battle
            .party
            .iter()
            .enumerate()
            .chain(battle.monsters.iter().enumerate().map(|(m, c)| (pc + m, c)))
            .filter_map(|(slot, c)| {
                let c = c.as_ref()?;
                let hp = if slot >= pc && victims.contains(&(slot - pc)) {
                    1
                } else {
                    c.hp
                };
                Some(BarSeed {
                    slot: u8::try_from(slot).ok()?,
                    hp,
                    mp: c.mp,
                    ground: ground.get(slot).copied().flatten(),
                    facing: facing.get(slot).copied().flatten(),
                    defeat_lanes: (hp == 0)
                        .then(|| defeat_lanes.get(slot).copied().flatten())
                        .flatten(),
                })
            })
            .collect()
    };
    apply_bar_seeds(&mut session.host.world, &hp_seed);
    // Place the engine at the capture's phase ([`SeedPlan`]).
    //
    // An opening capture is sampled at retail's entry-sweep counter. Everything else
    // runs the opening (banner, intro camera, initiative) to the first round
    // prompt, the earliest point a retail capture of a running fight can
    // share with a fresh entry, and then:
    //
    // - a prompt capture settles there;
    // - a menu capture is driven to the same surface on the same seat
    //   through the pad path ([`BattleDrive::Menu`]);
    // - a mid-cast capture is replayed: the cast is seeded to dispatch the
    //   moment the prompt opens, and the session runs until it reaches the
    //   capture's phase ([`PhaseGate`]);
    // - any other action in flight is reached by playing the round out
    //   through the pad path until the action SM holds the capture's state
    //   on the capture's seat ([`BattleDrive::Action`]).
    let plan = battle.seed_plan();
    let seed = battle.inflight_cast();
    let mut prompt_tick = None;
    if plan != SeedPlan::Opening {
        // Retail's battle tick runs only once the frame driver's entry sweep
        // is over, so no capture of a running fight was taken under it. The
        // port's tick does not wait; the seed does, and a cast is seeded to
        // dispatch only from there.
        let mut seeded = false;
        for t in 0..OPENING_TICKS {
            if !seeded && entry_sweep_reached(&session.host.world, 0xFF) {
                session.host.world.battle.inflight_seed = seed;
                seeded = true;
            }
            let w = &session.host.world;
            let reached = seeded
                && match seed {
                    Some(_) => w.battle.inflight_seed.is_none(),
                    None => w.battle.flow != BattleFlowState::Idle,
                };
            if reached {
                prompt_tick = Some(t);
                break;
            }
            session.tick()?;
            session.fog_render_tick();
            session.host.route_bgm_events(&mut director)?;
        }
    }
    let mut phase_tick = None;
    let mut driven = None;
    // A driven capture plays rounds the retail history did not (a Spirit
    // here, a strike there), so its combatant, bag, flag and track channels
    // are read at the first prompt, before the drive - the moment the seed
    // placed them - and only phase and camera at the phase itself.
    let mut pre_drive = None;
    let mut age_short = None;
    match (plan, battle.phase_gate()) {
        // The entry sweep owns retail's camera until the frame driver's
        // counter passes `0xC0`; run the engine's to the same count.
        (SeedPlan::Opening, _) => {
            for _ in 0..ENTRY_SWEEP_TICKS {
                if entry_sweep_reached(&session.host.world, battle.entry_counter) {
                    break;
                }
                session.tick()?;
                session.fog_render_tick();
                session.host.route_bgm_events(&mut director)?;
            }
        }
        (SeedPlan::Cast, Some(gate)) if prompt_tick.is_some() => {
            for t in 0..INFLIGHT_TICKS {
                if std::env::var_os("LEGAIA_RC_DRIVE_TRACE").is_some() {
                    let w = &session.host.world;
                    eprintln!(
                        "[cast] t={t} st=0x{:02X} act={} yb={:?} acc={:?} facing={:?} cam={:?}",
                        w.battle_ctx.action_state,
                        w.battle_ctx.active_actor,
                        w.battle.camera.as_ref().map(|c| c.action_yaw_base()),
                        w.battle.camera.as_ref().map(|c| c.close_up_accum()),
                        w.actors
                            .iter()
                            .take(8)
                            .map(|a| a.battle.facing_angle & 0xFFF)
                            .collect::<Vec<_>>(),
                        w.battle
                            .camera
                            .as_ref()
                            .map(|c| (c.phase(), w.battle_cam_pose())),
                    );
                }
                if gate.met(&session.host.world) {
                    phase_tick = Some(t);
                    break;
                }
                session.tick()?;
                session.fog_render_tick();
                session.host.route_bgm_events(&mut director)?;
            }
        }
        (SeedPlan::Menu { .. } | SeedPlan::Action { .. }, _) if prompt_tick.is_some() => {
            if let Some(drive) = battle.battle_drive() {
                let track = session.host.bgm_track_word.or(director.last);
                pre_drive = Some(combat_snapshot(&mut session.host.world, track));
                driven = Some(run_drive(
                    &mut session,
                    &mut director,
                    drive,
                    &mut age_short,
                )?);
            }
        }
        _ => {
            for _ in 0..BATTLE_SETTLE_TICKS {
                session.tick()?;
                session.fog_render_tick();
                session.host.route_bgm_events(&mut director)?;
            }
        }
    }
    if matches!(driven, Some(Some(_)))
        && let Some(snap) = pre_drive.as_mut()
    {
        let world = &session.host.world;
        for &m in &victims {
            if let (Some(c), Some(a)) = (snap.monsters.get_mut(m), world.actors.get(pc_seed + m)) {
                c.hp = a.battle.hp;
            }
        }
    }
    let snap = match pre_drive.take() {
        Some(snap) => snap,
        None => {
            let track = session.host.bgm_track_word.or(director.last);
            combat_snapshot(&mut session.host.world, track)
        }
    };
    // The idle orbit's yaw is a clock (`-4` a camera step from whatever
    // azimuth the field left): on a capture whose flow byte hands the frame
    // to the orbit, phase-align it to retail's reading, as the image child
    // does (`LEGAIA_BATTLE_ORBIT_YAW`), so the channel scores the framing
    // rather than the instant. `align_orbit_yaw` refuses unless the orbit
    // really owns the yaw (the far framing, no yaw glide in flight).
    if battle.orbit_owns_yaw()
        && let Some(cam) = session.host.world.battle.camera.as_mut()
    {
        cam.align_orbit_yaw(f32::from(retail.camera.yaw));
    }
    let world = &session.host.world;
    // How far the drive moved each placed combatant: a capture of a running
    // action stands its combatants where the action had already moved them,
    // and the drive replays the action from there.
    let mut ground_drift = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
    if matches!(driven, Some(Some(_))) {
        for (slot, d) in ground_drift.iter_mut().enumerate() {
            let (Some([gx, gz]), Some(a)) = (ground[slot], world.actors.get(slot)) else {
                continue;
            };
            *d = Some([
                a.move_state.world_x as i32 - i32::from(gx),
                a.move_state.world_z as i32 - i32::from(gz),
            ]);
        }
    }
    let pose = world.battle_cam_pose();
    let camera = CameraObs {
        pitch: pose.pitch.round() as i16,
        yaw: (pose.yaw.round() as i32).rem_euclid(4096) as i16,
        h: BATTLE_H,
        eye: pose.tr.map(|v| v.round() as i32),
        // The engine holds the focus un-negated; retail's words are negated.
        focus: [
            -(pose.focus[0].round() as i32),
            -(pose.focus[2].round() as i32),
        ],
    };
    Ok(EngineBattle {
        hp_seed,
        rng_seed,
        surprise_opening,
        age_short,
        scene: session.host.scene.as_ref().map(|s| s.name.clone()),
        mode: snap.mode,
        formation_source: source,
        man_row,
        prompt_tick,
        inflight: seed.map(|_| phase_tick),
        driven,
        win_pose: world.battle.victory.and_then(|v| v.pose_id),
        action_state: world.battle_ctx.action_state,
        active_actor: world.battle_ctx.active_actor,
        monster_ids: snap.monster_ids,
        party: snap.party,
        monsters: snap.monsters,
        flow: world.battle.flow,
        camera,
        bgm_id: snap.bgm_id,
        field_word,
        field_current,
        battle_bgm: snap.battle_bgm,
        save: snap.save,
        ground_drift,
    })
}

/// The combatant-side observables a battle state is scored on.
struct CombatSnapshot {
    mode: SceneMode,
    monster_ids: Vec<Option<u16>>,
    party: Vec<Combatant>,
    monsters: Vec<Combatant>,
    bgm_id: Option<u16>,
    battle_bgm: Option<u16>,
    save: legaia_save::SaveFile,
}

fn combat_snapshot(
    world: &mut legaia_engine_core::world::World,
    field_track: Option<u16>,
) -> CombatSnapshot {
    let pc = world.party.party_count.clamp(1, 3) as usize;
    // Battle ordinal `s` holds roster record `party_roster_slot(s)` (the
    // present-party list): a solo duel's Gala is ordinal 0 but record 2, and
    // retail's battle init copies the max from *that* record (`+0x108`,
    // `FUN_80053CB8` at `0x80053E50`).
    let roster = world.save_full().party;
    let party_maxes: Vec<(u16, u16)> = (0..pc)
        .map(|s| {
            roster
                .members
                .get(world.party_roster_slot(s))
                .map_or((0, 0), |m| {
                    let v = m.hp_mp_sp();
                    (v.hp_max, v.mp_max)
                })
        })
        .collect();
    let comb = |a: &legaia_engine_core::world::Actor, mp_max: u16| Combatant {
        hp: a.battle.hp,
        hp_max: a.battle.max_hp,
        mp: a.battle.mp,
        mp_max,
    };
    let party = (0..pc)
        .filter_map(|s| {
            world
                .actors
                .get(s)
                .map(|a| comb(a, party_maxes.get(s).map_or(0, |m| m.1)))
        })
        .collect();
    let mcount = world
        .battle
        .active_formation
        .as_ref()
        .map_or(0, |f| f.slots.len().min(5));
    let monster_ids = (0..mcount)
        .map(|m| world.actors.get(pc + m).and_then(|a| a.battle_monster_id))
        .collect();
    let monsters = (0..mcount)
        .filter_map(|m| {
            let a = world.actors.get(pc + m)?;
            let mp_max = a
                .battle_monster_id
                .and_then(|id| world.tables.monster_catalog.get(id))
                .map_or(0, |d| d.mp);
            Some(comb(a, mp_max))
        })
        .collect();
    CombatSnapshot {
        mode: world.mode,
        monster_ids,
        party,
        monsters,
        // Retail's track-select word keeps the field track through a fight:
        // the battle theme is started without the op-0x35 store, so the word
        // the fight holds is the one the field resumes. The engine routes its
        // battle swap through the op-0x35 start (which rewrites its copy of
        // the word), so the comparand is the track it stashed to resume.
        bgm_id: if world.audio.battle_bgm_active {
            world.audio.field_bgm_resume
        } else {
            field_track
        },
        battle_bgm: world.audio.current_bgm,
        save: world.save_full(),
    }
}

/// The engine battle ordinal of retail pool slot `seat`: party slots are
/// shared, but retail's monsters sit at fixed pool slots `3..` whatever the
/// party size while the engine seats them straight after the party.
pub fn engine_seat(seat: u8, party_count: u8) -> u8 {
    if seat >= 3 {
        party_count + (seat - 3)
    } else {
        seat
    }
}

/// Ticks the pad path may take to reach a menu capture's surface.
const MENU_DRIVE_TICKS: u32 = 600;
/// Ticks a menu capture's surface is held, with no input, before it is
/// sampled. A retail menu capture is a surface the player was sitting on,
/// so its camera has finished whatever transition opened it: the case-`0`
/// glide onto a member (`FUN_801D829C`, `a3 = 0xC`: 12 display frames), or
/// the submenu-exit swing and return to the far framing that the commit
/// confirm opens on (6 + 7 camera steps, 26 frames). The drive reaches the
/// surface on the tick it opens, so sampling then reads the transition's
/// first step - a clock reading, not the framing. Once the camera is in the
/// hold changes nothing: the surface waits for input.
pub const MENU_HOLD_TICKS: u32 = 32;
/// Ticks the pad path may take to reach an in-flight capture's action-SM
/// state: long enough for several rounds, since the seat's turn comes up in
/// initiative order.
const ACTION_DRIVE_TICKS: u32 = 4000;
/// The extra capture deadline a driven `play-window` child gets.
pub const DRIVE_DEADLINE: u64 = ACTION_DRIVE_TICKS as u64;

/// How a capture that sits past the round prompt is reached through the
/// engine's own pad path - the headless seed and the `play-window` image
/// child run the same driver (`LEGAIA_BATTLE_DRIVE`), so the frame is taken
/// at the phase the state channels were scored at.
///
/// Seats are **retail** pool slots (`ctx[+0x13]`); [`engine_seat`] maps
/// them onto the engine's seating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleDrive {
    /// An opening capture ([`SeedPlan::Opening`]): nothing is pressed, and
    /// the frame is the first one in battle mode whose entry sweep has run
    /// as far as retail's - the instant the headless seed samples. Retail's opening holds the fight on its intro (the
    /// tutorial's first speech, the formation reveal) while the engine's
    /// round prompt opens with it, so a frame taken past the prompt shows a
    /// surface retail had not reached.
    ///
    /// `swept` is set for a capture whose flow byte is past the intro timer
    /// (`0x0C` / `0x14`): the enemy-name labels `0x0B` sweeps are gone there,
    /// so the frame waits for the engine's own intro names to clear
    /// (`World::battle.intro_names_frames`).
    ///
    /// `entry` is retail's frame-driver counter `gp+0x330`: the frame also
    /// waits for the engine's battle-entry sweep to run as far
    /// ([`entry_sweep_reached`]).
    Opening { swept: bool, entry: u8 },
    /// A command-selection surface on party seat `seat`: members ahead of
    /// it commit a plain Attack, the seat itself takes the arm that leads
    /// to `flow`.
    Menu { flow: BattleFlowState, seat: u8 },
    /// An action in flight: rounds are committed (a plain Attack each, the
    /// capture's own seat Spirit when that is what it had committed) until
    /// the action SM holds `state` on `seat`. A monster seat that was
    /// casting (`category == 2`) casts the capture's spell `queued` on its
    /// next turn ([`legaia_engine_core::world::BattleState::forced_monster_cast`]);
    /// the party commits Spirit throughout, so the caster is still standing
    /// when that turn comes (Zeto's captures sit in a party able to kill him
    /// in two swings).
    ///
    /// `spare` is set on a killing-blow capture
    /// ([`RetailBattle::action_victims`]): the victim is replayed at `1` HP,
    /// so any member's swing would kill it, and the members other than the
    /// capture's seat commit Spirit instead of Attack - the kill is then the
    /// seat's own, whatever order initiative put them in.
    ///
    /// `absorbed` is the Seru a capture on the Done band already staged
    /// (`ctx[+0x269]`, [`RetailBattle::absorbed_seru`]). Retail's grant ran
    /// before the capture, so the lifted save's spell list already holds
    /// spell `absorbed + 0x80` - and the replayed kill's absorb lookup would
    /// answer "known" and stage nothing. [`Self::prime`] takes it back off
    /// the seat's list, the twin of crediting a cast's MP back.
    ///
    /// `style` is the capture's framing style `ctx[+0xD]`, a draw the action
    /// seed rolls per action (`rand() % 2 * 2`, `rand() % 4`, ...). A
    /// replay on another stream rolls its own, and the post-strike cases
    /// fork on it (pitch `0x80` / `TR.y 0x400` against level, a half-turn),
    /// so [`Self::steer`] sets the acting seat's style to retail's while it
    /// holds the capture's state (outside the capture band, [`CAPTURE_BAND`])
    /// - the camera twin of the orbit-yaw alignment.
    Action {
        seat: u8,
        state: u8,
        category: u8,
        queued: u8,
        spare: bool,
        absorbed: u8,
        end: SpanGate,
        style: Option<u8>,
        steer: ActionSteer,
    },
}

/// The two further capture facts an action drive steers by
/// ([`BattleDrive::Action`]).
///
/// `target` is the capture's target seat `actor[+0x1DD]`, set only on a
/// killing-blow capture whose target is one of its victims
/// ([`RetailBattle::action_victims`]). The phase is then reached only with
/// the acting seat on that target: with the victim seeded at its read HP of
/// `0` the auto-target picks the next standing monster, and a run that held
/// the right state against the wrong body scored the camera of a different
/// shot (`player_steal_skeleton_banner` framed the live skeleton where
/// retail's case 8 frames the falling one), so the search moves on to the
/// `1`-HP re-run that makes the kill. On a monster's cast it is the party
/// seat the cast aimed at, which the replayed cast is steered onto
/// (`BattleState::forced_monster_target`): the caster turns to face its
/// target as the cast begins, and the module's shots frame from that facing.
///
/// `yaw` is the capture's yaw counter `ctx[+0x6DA]`. A party attacker's
/// first swing-clip commit re-seeds it to `(rand() % 2) * 0x800 + 0x280`
/// (`FUN_8004E13C`, `0x8004E288..0x8004E2B4`) - a coin on a stream the replay
/// does not share, and the half-turn it picks is the side cases 6, 7 and 8
/// film from. [`BattleDrive::steer`] keeps the engine's counter on retail's
/// half while the seat's action runs - the twin of the style alignment.
///
/// `message` is the timed message (HUD element `0x66`) the capture holds -
/// its battle-overlay string pointer and remaining hold - and `plate_cleared`
/// the counter swap's cleared target plaque. Both are the HUD's half of a
/// counterattack the replay's own monster turn does not roll (the drive
/// reaches the counterer's strike loop through its own turn), raised on the
/// engine when it holds the capture's state.
///
/// `arts` marks a party capture whose committed queue holds an art starter
/// ([`RetailBattle::arts_queue`]): the player entered that turn through
/// `Command`, so the drive opens the arts entry and confirms the string it
/// preseeds from the record (`FUN_801DA34C`) instead of taking `Auto`, which
/// builds a different queue under the same state byte. `gauge` is that
/// seat's live gauge over its base ([`RetailBattle::acting_gauge`]), set when
/// a Spirit turn the replay does not play had extended it: the extension is
/// what selects the saved string's band and pays for its arrows, so
/// [`BattleDrive::prime`] restores it.
///
/// `clip` is the acting party seat's committed clip `+0x1D9` on an
/// [`SpanGate::Age`] capture, when that is a swing clip. The age is frames
/// since the seat's **last** clip commit, and the strike loop `0x1E` spans
/// one commit per queued swing, so an age alone matches the first swing
/// that runs as long; the clip names which one retail sits in
/// (`rim_elm_gimard_victory`: `0x1E` at `16` on `0x0E`, the second swing -
/// the age alone took the first, `0x0F`). Art clips are compared on the
/// dynamic slot (`0x10` / `0x11`) the anim commit remaps them onto, which
/// both sides store. An idle `0` is not gated: a party seat's idle between
/// the approach and its first strike has no engine twin (the engine holds
/// the walk clip there).
///
/// `aim` is the monster row a party attack capture's target byte `+0x1DD`
/// names: the player picked that monster, so the drive walks the target
/// cursor onto it rather than confirming the picker's default. Which body
/// the swing lands on moves the framing (the strike shots look at the
/// target), and a default pick lands on whichever monster the earlier turns
/// left first in the ring (`battle_vahn_tri_somersault_super`: retail row
/// `1`, the default row `0`, which Noa's and Gala's swings had knocked far
/// back).
///
/// `queue` is an arts capture's committed queue `+0x1DF..`. The string a
/// bare confirm replays is the record's saved band, and that band is not
/// what the player entered on the captured turn: `battle_melee_hit_spark`'s
/// Vahn holds `0F 0E 0F 0E` (Up Down Up Down) in band A while his committed
/// queue `0D 0F 0E 19 27` is Right Up Down Up, tokenized with Somersault -
/// so the replay struck Up, Down, Somersault, Down and sat in `0x20` on a
/// Down swing where retail plays the Somersault. [`BattleDrive::prime`]
/// recovers the entered arrows from the queue ([`entered_arrows`]) and,
/// when neither saved band holds them, writes them into both, so the replay
/// tokenizes the captured turn.
///
/// `cursor` is that queue's strike cursor `ctx[+0x15]` on an
/// [`SpanGate::Age`] capture inside the strike loop. With the queue
/// replayed byte for byte, the age alone matches the first clip of the turn
/// that runs as long - `battle_vahn_tri_somersault_super` sits at age `160`
/// on the Somersault at cursor `5`, and the drive took the turn's first Down
/// swing at cursor `3` - so the cursor names the clip the art slot's `clip`
/// gate cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActionSteer {
    pub target: Option<u8>,
    pub yaw: Option<u16>,
    /// The track coin `ctx[+0x26D]` the capture holds - a `rand() % 2`
    /// draw like the yaw counter's half-turn, aligned the same way
    /// (`BattleCamera::align_phase_cursor`).
    pub coin: Option<u8>,
    pub message: Option<(u32, i16)>,
    pub plate_cleared: bool,
    pub arts: bool,
    pub gauge: Option<u16>,
    pub clip: Option<u8>,
    pub aim: Option<u8>,
    pub queue: Option<[u8; 16]>,
    pub cursor: Option<u8>,
}

/// Where inside a state that spans many frames a capture sits.
///
/// **The Done band's continuation `0x52`** holds for its own countdown
/// `ctx[+0x6D8]` (`0xB4` frames when a Seru absorb stages it), and the engine
/// enters it at the top: the capture's countdown places it, the drive holding
/// until the engine's has run down to it. **The fade-down `0x51`** ticks the
/// same word - the `0x3C` tail timer `0x50` seeds - and is placed by it too,
/// not by the close-up accumulator: in the Done band that word counts frames
/// since the acting actor's last clip commit, which there is whichever idle or
/// return commit the actor's clip lengths put last, so the engine's
/// accumulator in `0x51` may not have been reset since its cast clip
/// (`zora_glare_petrify_post`: retail `24`, engine past `4000`) and the
/// accumulator gate held the band's first tick.
///
/// **The battle-end sequence**, for a capture taken after the `0x5A` gate
/// raised the end signal (`DAT_8007BD71 == 0xFE`):
/// Retail stops stepping the action SM on the signal and runs the results
/// sequencer `FUN_8004E568` instead, so `ctx[+0x07]` stays `0x5A` (and
/// `ctx[+0x13]` on the pose actor) through the whole load hold, results
/// hold and exit fade. The action-SM state then names a span of several
/// hundred vsyncs; the sequencer's own words place the capture in it
/// ([`legaia_engine_core::world::VictoryPhase`] is the engine's twin):
/// the phase word `_DAT_8007BD2C`, the phase halfword `ctx[+0x6CE]` and the
/// results hold `gp+0xA54` (`0x8007BD6C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanGate {
    /// No end signal: the action SM's own state places the capture.
    #[default]
    None,
    /// Phases `0..=4`, the CD loads (`ctx[+0x6CE] == 0`).
    Loading,
    /// Phase 5 with `ctx[+0x6CE] == 1`: the results frame onward, `hold`
    /// vsyncs into the hold.
    Results { hold: u16 },
    /// `ctx[+0x6CE] >= 2`: the exit fade, at that phase halfword.
    Exit { phase: u16 },
    /// Action-SM state `0x51` or `0x52`, its countdown `ctx[+0x6D8]` down to
    /// `timer`.
    DoneHold { timer: i16 },
    /// The capture band's CD holds `0x6E` / `0x6F` (or the module tick
    /// `0x70` they lead to), with retail's framing depth `ctx[+0x6D0]` at
    /// `height` and its close-up accumulator `ctx[+0x87C]` at `accum`.
    ///
    /// Both holds wait on the disc, not on a counter: `0x6E` on the CD-ready
    /// poll `FUN_8003DE7C(1)` (`0x801E4F08`), `0x6F` on `FUN_8003F2B8(1)`
    /// (`0x801E5024`). Every frame of either first calls
    /// `FUN_801D5854(ctx[+0x13], 6)`, whose prologue adds
    /// `8 * frame_step` to `ctx[+0x87C]` (`0x801D5900..0x801D5920`), and
    /// `0x6F` also ramps `ctx[+0x6D0]` down by `16 * frame_step`
    /// (`0x801E4FFC..0x801E5014`); `0x70` does not write the depth, and the
    /// `0x71` store re-seeds it. So the depth counts `0x6F`'s frames and
    /// the accumulator, less half the depth's ramp, counts `0x6E`'s since
    /// the caster's last clip commit zeroed it - clocks of how long
    /// retail's reads took, a disc-timing property the engine, whose polls
    /// are always ready, does not have. Through them the case-6 glide onto
    /// the caster has landed in every such capture. The drive holds the
    /// engine's polls busy ([`BattleDrive::steer`]) until its own words
    /// have run as far.
    ///
    /// `arm` is `(ctx[+0x279], countdown)` for a `0x70` capture of a module
    /// whose countdown the engine directs
    /// ([`legaia_engine_vm::cast_module_camera::capture_countdown_va`]): the
    /// phase is held until the engine's module sits in that arm with its
    /// countdown run down at least as far - the module's own clock, which
    /// places a camera shot or drift in flight.
    ///
    /// `yaw` is retail's yaw counter `ctx[+0x6DA]` (`None` under the Far
    /// option, which stops it): the action seed `0x0C` stores `0x800` and
    /// the SM's prologue adds about one a display frame, so it counts the
    /// frames since the seed - including the ones `0x6E` spent waiting on
    /// the drive (`FUN_8003DE7C(1)` at `0x801E4F08`) before the caster's
    /// clip commit zeroed the accumulator. The accumulator alone cannot see
    /// those; the three Delilas special captures sit `34` frames past the
    /// accumulator's reading of them.
    CaptureFade {
        height: u16,
        accum: u16,
        arm: Option<(u8, i32)>,
        yaw: Option<u16>,
    },
    /// Any other action-SM state, `accum` (`ctx[+0x87C]`) into it.
    ///
    /// The action SM's states span frames, and the drive reaches each on
    /// its first tick, while a retail capture sits wherever the save was
    /// made. The close-up accumulator places it: the acting actor's clip
    /// commit zeroes it (`FUN_8004AD80`) and every framing call adds
    /// `8 * frame_step` (`FUN_801D5854`, `0x801D5900..0x801D5920`), so it is
    /// the frames since the actor's last clip commit. The drive takes the
    /// first tick in the state whose engine accumulator has run as far; a
    /// state the engine leaves sooner is re-run to its last tick
    /// ([`EngineBattle::age_short`]).
    Age { accum: u16 },
    /// A capture ahead of the seed pass ([`PRE_SEED_STATES`]) whose camera
    /// has landed on its framing ([`camera_landed`]). `0x0A` waits on the CD
    /// (`FUN_8003F2B8(1)` at `0x801E2B78`) for as long as the next actor's
    /// data takes, which the engine's always-ready poll does not; the
    /// `evil_medallion_rage_battle` capture sits there with the commit
    /// confirm's case-9 glide finished. The drive holds the engine's wait
    /// until its own glide lands ([`BattleDrive::steer`]).
    Landed,
}

/// The capture band `0x6E..=0x71`. `0x70` pins the style to `1`
/// (`sb v0,0xd(v1)` at `0x801E50CC`) without re-arming a framing, so a
/// capture there reads `1` over a camera the rolled style placed; the style
/// is not aligned in the band.
const CAPTURE_BAND: std::ops::RangeInclusive<u8> = 0x6E..=0x71;

/// The capture band's states placed by its CD holds
/// ([`SpanGate::CaptureFade`]).
pub const CAPTURE_FADE_STATES: [u8; 3] = [0x6E, 0x6F, 0x70];

/// How far the engine's `0x6F` depth ramp still has to come down to reach
/// retail's `height` (`ctx[+0x6D0]`, an unsigned halfword the ramp wraps),
/// `0` once it is there.
fn capture_ramp_left(world: &legaia_engine_core::world::World, height: u16) -> u16 {
    ((world.battle.camera_frame_height as u16).wrapping_sub(height) as i16).max(0) as u16
}

/// Whether the engine's close-up accumulator has run as far through `0x6E`
/// as retail's `accum` (`ctx[+0x87C]`) says retail's did: the capture's
/// value less what the `0x6F` frames still to come will add (half the depth
/// ramp left, `8` against `16` per frame step).
fn capture_accum_done(world: &legaia_engine_core::world::World, height: u16, accum: u16) -> bool {
    let want = accum.saturating_sub(capture_ramp_left(world, height) / 2);
    world
        .battle
        .camera
        .as_ref()
        .is_none_or(|c| c.close_up_accum() >= u32::from(want))
}

/// Whether the engine's yaw counter has run as far through `0x6E` as
/// retail's `yaw` (`ctx[+0x6DA]`) says retail's did: the capture's value
/// less the `0x6F` frames still to come (the depth ramps `16` a display
/// frame against the counter's one). `None` - the Far option - holds
/// nothing.
fn capture_yaw_done(
    world: &legaia_engine_core::world::World,
    height: u16,
    yaw: Option<u16>,
) -> bool {
    let Some(yaw) = yaw else {
        return true;
    };
    let want = i32::from(yaw) - i32::from(capture_ramp_left(world, height) / 16);
    world
        .battle
        .camera
        .as_ref()
        .is_none_or(|c| c.action_yaw_base() >= want)
}

/// Whether the engine's `0x6F` pull-in has come down at least as far as
/// retail's `height` (`ctx[+0x6D0]`, an unsigned halfword the ramp wraps).
fn capture_ramp_done(world: &legaia_engine_core::world::World, height: u16) -> bool {
    capture_ramp_left(world, height) == 0
}

impl SpanGate {
    fn to_env(self) -> (u8, u16) {
        match self {
            Self::None => (0, 0),
            Self::Loading => (1, 0),
            Self::Results { hold } => (2, hold),
            Self::Exit { phase } => (3, phase),
            Self::DoneHold { timer } => (4, timer as u16),
            Self::CaptureFade { .. } => (5, 0),
            Self::Age { accum } => (6, accum),
            Self::Landed => (7, 0),
        }
    }

    /// The env value field: one number, or `height/accum` for a capture
    /// hold.
    fn env_value(self) -> String {
        match self {
            Self::CaptureFade {
                height,
                accum,
                arm,
                yaw,
            } => {
                let mut s = format!("{height}/{accum}");
                if let Some((phase, countdown)) = arm {
                    s.push_str(&format!("/{phase}/{countdown}"));
                }
                if let Some(y) = yaw {
                    s.push_str(&format!("/y{y}"));
                }
                s
            }
            _ => self.to_env().1.to_string(),
        }
    }

    fn from_env(kind: &str, value: &str) -> Option<Self> {
        if kind.trim() == "5" {
            let mut parts: Vec<&str> = value.trim().split('/').collect();
            let yaw = match parts.last() {
                Some(t) if t.starts_with('y') => {
                    let y = t[1..].parse().ok()?;
                    parts.pop();
                    Some(y)
                }
                _ => None,
            };
            let arm = match parts.as_slice() {
                [_, _] => None,
                [_, _, p, c] => Some((p.parse().ok()?, c.parse().ok()?)),
                _ => return None,
            };
            return Some(Self::CaptureFade {
                height: parts[0].parse().ok()?,
                accum: parts[1].parse().ok()?,
                arm,
                yaw,
            });
        }
        let value: u16 = value.trim().parse().ok()?;
        Some(match kind.trim() {
            "0" => Self::None,
            "1" => Self::Loading,
            "2" => Self::Results { hold: value },
            "3" => Self::Exit { phase: value },
            "4" => Self::DoneHold {
                timer: value as i16,
            },
            "6" => Self::Age { accum: value },
            "7" => Self::Landed,
            _ => return None,
        })
    }

    /// Whether a gate past the end signal.
    pub fn is_end(self) -> bool {
        matches!(
            self,
            Self::Loading | Self::Results { .. } | Self::Exit { .. }
        )
    }

    /// Whether the engine's battle-end sequence holds this gate.
    fn met(self, world: &legaia_engine_core::world::World) -> bool {
        use legaia_engine_core::world::VictoryPhase;
        let Some(seq) = world.battle.victory else {
            return false;
        };
        match (self, seq.phase) {
            (Self::Loading, VictoryPhase::Loading { .. }) => true,
            (Self::Results { hold }, VictoryPhase::Results { hold: h }) => h == hold,
            (Self::Exit { phase }, VictoryPhase::Exit { phase: p }) => p == phase,
            _ => false,
        }
    }
}

impl BattleDrive {
    /// `menu,<flow>,<seat>` or
    /// `action,<seat>,<state>,<category>,<queued>[,<spare>,<absorbed>[,<end kind>,<end value>]]`.
    pub fn to_env(&self) -> String {
        match *self {
            Self::Opening { swept, entry } => format!("opening,{},{entry}", u8::from(swept)),
            Self::Menu { flow, seat } => format!("menu,{},{seat}", flow.raw()),
            Self::Action {
                seat,
                state,
                category,
                queued,
                spare,
                absorbed,
                end,
                style,
                steer,
            } => {
                let (kind, _) = end.to_env();
                let value = end.env_value();
                let mut s = format!(
                    "action,{seat},{state},{category},{queued},{},{absorbed},{kind},{value}",
                    u8::from(spare)
                );
                if let Some(style) = style {
                    s.push_str(&format!(",{style}"));
                }
                if let Some(t) = steer.target {
                    s.push_str(&format!(",t{t}"));
                }
                if let Some(y) = steer.yaw {
                    s.push_str(&format!(",y{y}"));
                }
                if let Some(o) = steer.coin {
                    s.push_str(&format!(",o{o}"));
                }
                if let Some((va, hold)) = steer.message {
                    s.push_str(&format!(",m{va:x}/{hold}"));
                }
                if steer.plate_cleared {
                    s.push_str(",c");
                }
                if let Some(g) = steer.gauge {
                    s.push_str(&format!(",g{g}"));
                }
                if steer.arts {
                    s.push_str(",a");
                }
                if let Some(k) = steer.clip {
                    s.push_str(&format!(",k{k}"));
                }
                if let Some(p) = steer.aim {
                    s.push_str(&format!(",p{p}"));
                }
                if let Some(u) = steer.cursor {
                    s.push_str(&format!(",u{u}"));
                }
                if let Some(q) = steer.queue {
                    s.push_str(",q");
                    for b in q {
                        s.push_str(&format!("{b:02x}"));
                    }
                }
                s
            }
        }
    }

    pub fn from_env(s: &str) -> Option<Self> {
        match s.trim() {
            "opening" | "opening,0" => {
                return Some(Self::Opening {
                    swept: false,
                    entry: 0,
                });
            }
            "opening,1" => {
                return Some(Self::Opening {
                    swept: true,
                    entry: 0,
                });
            }
            t if t.starts_with("opening,") => {
                let mut it = t.split(',').skip(1);
                let swept = it.next()? == "1";
                let entry = it.next()?.parse().ok()?;
                return Some(Self::Opening { swept, entry });
            }
            _ => {}
        }
        // The tagged steering tokens ride at the end (`t<seat>`, `y<yaw>`).
        let mut steer = ActionSteer::default();
        let mut fields: Vec<&str> = s.split(',').collect();
        while fields.len() > 1 {
            let last = fields[fields.len() - 1].trim();
            if let Some(t) = last.strip_prefix('t') {
                steer.target = Some(t.parse().ok()?);
            } else if let Some(y) = last.strip_prefix('y') {
                steer.yaw = Some(y.parse().ok()?);
            } else if let Some(o) = last.strip_prefix('o') {
                steer.coin = Some(o.parse().ok()?);
            } else if let Some(m) = last.strip_prefix('m') {
                let (va, hold) = m.split_once('/')?;
                steer.message = Some((u32::from_str_radix(va, 16).ok()?, hold.parse().ok()?));
            } else if last == "c" {
                steer.plate_cleared = true;
            } else if last == "a" {
                steer.arts = true;
            } else if let Some(g) = last.strip_prefix('g') {
                steer.gauge = Some(g.parse().ok()?);
            } else if let Some(k) = last.strip_prefix('k') {
                steer.clip = Some(k.parse().ok()?);
            } else if let Some(p) = last.strip_prefix('p') {
                steer.aim = Some(p.parse().ok()?);
            } else if let Some(u) = last.strip_prefix('u') {
                steer.cursor = Some(u.parse().ok()?);
            } else if let Some(q) = last.strip_prefix('q') {
                if q.len() != 32 {
                    return None;
                }
                let mut queue = [0u8; 16];
                for (i, b) in queue.iter_mut().enumerate() {
                    *b = u8::from_str_radix(q.get(i * 2..i * 2 + 2)?, 16).ok()?;
                }
                steer.queue = Some(queue);
            } else {
                break;
            }
            fields.pop();
        }
        if steer != ActionSteer::default() {
            let mut d = Self::from_env(&fields.join(","))?;
            if let Self::Action { steer: st, .. } = &mut d {
                *st = steer;
            }
            return Some(d);
        }
        if fields.len() == 10 && fields[0].trim() == "action" {
            let mut d = Self::from_env(&fields[..9].join(","))?;
            if let Self::Action { style, .. } = &mut d {
                *style = Some(fields[9].trim().parse().ok()?);
            }
            return Some(d);
        }
        if fields.len() == 9 && fields[0].trim() == "action" {
            let mut d = Self::from_env(&fields[..7].join(","))?;
            if let Self::Action { end, .. } = &mut d {
                *end = SpanGate::from_env(fields[7], fields[8])?;
            }
            return Some(d);
        }
        let parts: Vec<u8> = s
            .split(',')
            .skip(1)
            .map(|p| p.trim().parse().ok())
            .collect::<Option<_>>()?;
        match (s.split(',').next()?.trim(), parts.as_slice()) {
            ("menu", &[flow, seat]) => Some(Self::Menu {
                flow: BattleFlowState::from_raw(flow),
                seat,
            }),
            ("action", &[seat, state, category, queued]) => Some(Self::Action {
                seat,
                state,
                category,
                queued,
                spare: false,
                absorbed: 0,
                end: SpanGate::None,
                style: None,
                steer: ActionSteer::default(),
            }),
            ("action", &[seat, state, category, queued, spare, absorbed]) => Some(Self::Action {
                seat,
                state,
                category,
                queued,
                spare: spare != 0,
                absorbed,
                end: SpanGate::None,
                style: None,
                steer: ActionSteer::default(),
            }),
            _ => None,
        }
    }

    /// An action drive's steering facts.
    fn steering(&self) -> Option<ActionSteer> {
        match *self {
            Self::Action { steer, .. } => Some(steer),
            _ => None,
        }
    }

    /// The tick budget the drive gets past the first prompt.
    pub fn budget(&self) -> u32 {
        match self {
            Self::Opening { .. } => 0,
            Self::Menu { .. } => MENU_DRIVE_TICKS,
            Self::Action { .. } => ACTION_DRIVE_TICKS,
        }
    }

    /// Ticks the reached phase is held before it is sampled
    /// ([`MENU_HOLD_TICKS`]); an action phase moves on by itself, so it is
    /// sampled the tick it is reached.
    pub fn hold_ticks(&self) -> u32 {
        match self {
            Self::Menu { .. } => MENU_HOLD_TICKS,
            Self::Action { .. } | Self::Opening { .. } => 0,
        }
    }

    /// Arm the drive's one-shot world seed (a monster cast to replay, an
    /// absorbed Seru to take back off the seat's spell list). Called once,
    /// on the first battle tick.
    pub fn prime(&self, world: &mut legaia_engine_core::world::World) {
        if let Self::Action { seat, steer, .. } = *self
            && let Some(live) = steer.gauge
            && seat < 3
        {
            let pc = world.party.party_count.clamp(1, 3);
            if let Some(a) = world.actors.get_mut(usize::from(engine_seat(seat, pc))) {
                a.battle.agl = live;
            }
        }
        if let Self::Action { seat, steer, .. } = *self
            && seat < 3
            && let Some(queue) = steer.queue
        {
            seed_entered_arrows(world, seat, &queue);
        }
        if let Self::Action { seat, absorbed, .. } = *self
            && absorbed != 0
            && seat < 3
        {
            let roster = world.party_roster_slot(usize::from(seat));
            if let Some(rec) = world.party.roster.members.get_mut(roster) {
                unlearn_spell(rec, absorbed.wrapping_add(0x80));
            }
        }
        if let Self::Action {
            seat,
            category: 2,
            queued,
            ..
        } = *self
        {
            let pc = world.party.party_count.clamp(1, 3);
            let seat = engine_seat(seat, pc);
            if seat >= pc {
                world.battle.forced_monster_cast = Some((seat, queued));
                // A party seat keeps its number; the caster's own retail
                // seat is re-keyed to its engine seat.
                world.battle.forced_monster_target = self
                    .steering()
                    .and_then(|s| s.target)
                    .and_then(|t| match t {
                        0..3 => Some(t),
                        t if engine_seat(t, pc) == seat => Some(seat),
                        _ => None,
                    });
            }
        }
    }

    /// Whether the engine holds the capture's phase.
    pub fn reached(&self, world: &legaia_engine_core::world::World) -> bool {
        if world.mode != SceneMode::Battle {
            return false;
        }
        let pc = world.party.party_count.clamp(1, 3);
        match *self {
            // The first frame whose monsters are drawable: the bodies bind
            // a few ticks after the mode flip.
            Self::Opening { swept, entry } => {
                let ok = (!swept || world.battle.intro_names_frames == 0)
                    && entry_sweep_reached(world, entry)
                    && world.actors.iter().enumerate().all(|(i, a)| {
                        a.battle_monster_id.is_none()
                            || !a.active
                            || a.tmd_binding.is_some()
                                && world
                                    .battle_actor_draw_plan(i, None, 4.0, false)
                                    .is_none_or(|p| p.drawn)
                    });
                if std::env::var_os("LEGAIA_RC_DRIVE_TRACE").is_some() {
                    for (i, a) in world.actors.iter().enumerate().take(8) {
                        eprintln!(
                            "[op] {i} mon={:?} act={} bind={:?} rf={} rc={:#x} plan={:?} anim={} pose={} cam={:?} pos={:?}",
                            a.battle_monster_id,
                            a.active,
                            a.tmd_binding,
                            a.battle.render_flag,
                            a.battle.render_color,
                            world
                                .battle_actor_draw_plan(i, None, 4.0, false)
                                .map(|p| p.drawn),
                            a.battle_animation.is_some(),
                            a.battle_pose.is_some(),
                            world
                                .battle
                                .camera
                                .as_ref()
                                .map(|_| world.battle_cam_pose().tr),
                            (
                                a.move_state.world_x,
                                a.move_state.world_y,
                                a.move_state.world_z
                            )
                        );
                    }
                }
                ok
            }
            // A menu capture is a surface the player sat on: the camera has
            // arrived at its framing, so the frame waits out the glide.
            Self::Menu { flow, seat } => {
                world.battle.flow == flow
                    && (!menu_seat_matters(flow) || world.battle_ctx.active_actor == seat)
                    && !world.battle.camera.as_ref().is_some_and(|c| c.is_gliding())
            }
            Self::Action { seat, end, .. } if end.is_end() => {
                // The sequencer frames its pose actor `ctx[+0x13]`.
                end.met(world)
                    && world
                        .battle
                        .victory
                        .is_some_and(|v| v.pose_actor == usize::from(engine_seat(seat, pc)))
            }
            Self::Action {
                seat,
                state,
                end,
                steer,
                ..
            } => {
                world.battle.flow == BattleFlowState::Idle
                    && world.battle.command.is_none()
                    && world.battle_ctx.active_actor == engine_seat(seat, pc)
                    && world.battle_ctx.action_state == state
                    && steer.target.is_none_or(|t| {
                        world
                            .actors
                            .get(usize::from(engine_seat(seat, pc)))
                            .is_some_and(|a| a.battle.active_target == engine_seat(t, pc))
                    })
                    && match end {
                        SpanGate::DoneHold { timer } => world.battle_ctx.frame_timer <= timer,
                        SpanGate::CaptureFade {
                            arm: Some((phase, countdown)),
                            ..
                        } if state == 0x70 => {
                            world.casting.module_phase > phase
                                || world.casting.module_phase == phase
                                    && world.casting.module_cam.countdown.0 <= countdown
                        }
                        SpanGate::CaptureFade {
                            height, accum, yaw, ..
                        } if state == 0x6E => {
                            capture_accum_done(world, height, accum)
                                && capture_yaw_done(world, height, yaw)
                        }
                        SpanGate::CaptureFade { height, .. } if state == 0x6F => {
                            capture_ramp_done(world, height)
                        }
                        SpanGate::Landed => {
                            !world.battle.camera.as_ref().is_some_and(|c| c.is_gliding())
                        }
                        SpanGate::Age { accum } => {
                            world
                                .battle
                                .camera
                                .as_ref()
                                .is_none_or(|c| c.close_up_accum() >= u32::from(accum))
                                && steer.clip.is_none_or(|k| {
                                    world.battle_current_anim(usize::from(engine_seat(seat, pc)))
                                        == k
                                })
                                && steer.cursor.is_none_or(|u| {
                                    world
                                        .actors
                                        .get(usize::from(engine_seat(seat, pc)))
                                        .is_some_and(|a| a.battle.strike_index == u)
                                })
                        }
                        _ => true,
                    }
            }
        }
    }

    /// Whether the engine sits in an [`SpanGate::Age`] action phase's state
    /// at all, whatever its age.
    fn in_aged_state(&self, world: &legaia_engine_core::world::World) -> bool {
        let mut base = *self;
        match &mut base {
            Self::Action { end, .. } if matches!(end, SpanGate::Age { .. }) => {
                *end = SpanGate::None;
                base.reached(world)
            }
            _ => false,
        }
    }

    /// Per-tick world steering the drive owns besides the pad: a
    /// [`SpanGate::CaptureFade`] capture holds the engine's capture-band CD
    /// polls busy while its acting seat sits in `0x6E` with the close-up
    /// accumulator short of retail's, or in `0x6F` with the pull-in not yet
    /// down to retail's depth, and releases them otherwise.
    pub fn steer(&self, world: &mut legaia_engine_core::world::World) {
        let Self::Action {
            seat,
            state: want,
            end,
            style,
            steer,
            ..
        } = *self
        else {
            return;
        };
        // The yaw counter's half-turn coin ([`ActionSteer::yaw`]): from the
        // seat's seed pass to the capture's state.
        {
            let pc = world.party.party_count.clamp(1, 3);
            let state = world.battle_ctx.action_state;
            if let Some(yaw) = steer.yaw
                && world.mode == SceneMode::Battle
                && world.battle_ctx.active_actor == engine_seat(seat, pc)
                && (0x0C..=want).contains(&state)
                && let Some(cam) = world.battle.camera.as_mut()
            {
                cam.align_action_yaw_half(i32::from(yaw));
                if let Some(coin) = steer.coin {
                    cam.align_phase_cursor(coin);
                }
            }
        }
        let pc = world.party.party_count.clamp(1, 3);
        let ours = world.mode == SceneMode::Battle
            && world.battle_ctx.active_actor == engine_seat(seat, pc);
        let state = world.battle_ctx.action_state;
        // The capture's counterattack HUD, on the frame the engine holds its
        // state ([`ActionSteer::message`] / [`ActionSteer::plate_cleared`]).
        if ours && state == want {
            if let Some((va, hold)) = steer.message
                && world.battle.message_banner.is_none()
            {
                world.raise_timed_message(va, i32::from(hold));
            }
            if steer.plate_cleared {
                world.battle.target_plate_cleared = true;
                // The swap's HUD: the monster seed's bar for the counterer,
                // and no combo cluster.
                world.battle.counter_hud = Some(engine_seat(seat, pc));
            }
        }
        if let Some(style) = style
            && ours
            && (state == want || seat < 3 && (0x0C..=want).contains(&state))
            && !CAPTURE_BAND.contains(&state)
        {
            world.battle_ctx.camera_variant = style;
        }
        if end == SpanGate::Landed {
            let gliding = world.battle.camera.as_ref().is_some_and(|c| c.is_gliding());
            world.battle.prev_action_cleared = !(ours && state == want && state == 0x0A && gliding);
            return;
        }
        let SpanGate::CaptureFade {
            height, accum, yaw, ..
        } = end
        else {
            return;
        };
        world.audio.sound_bank_ready = !(ours
            && state == 0x6E
            && !(capture_accum_done(world, height, accum) && capture_yaw_done(world, height, yaw)));
        world.battle.prev_action_cleared =
            !(ours && state == 0x6F && !capture_ramp_done(world, height));
    }

    /// The press that walks the engine one step toward the phase, or `None`
    /// when nothing on screen wants one (the action SM owns the frame).
    pub fn press(
        &self,
        world: &legaia_engine_core::world::World,
    ) -> Option<legaia_engine_core::input::PadButton> {
        use legaia_engine_core::battle_input::CommandPhase;
        use legaia_engine_core::input::PadButton;
        if world.mode != SceneMode::Battle || matches!(self, Self::Opening { .. }) {
            return None;
        }
        // A battle message box parks the whole battle until it is dismissed.
        if !world.battle.tutorial_boxes.is_empty() {
            return Some(PadButton::Cross);
        }
        // The arts entry the capture seat opened to replay its saved
        // command string: a bare confirm takes the string, and the
        // confirms after it begin the turn and pick the target.
        if let Self::Action { seat, steer, .. } = *self
            && let Some(want) = steer.aim
        {
            let picker = world
                .battle
                .arts_input
                .as_ref()
                .filter(|a| a.party_slot == seat)
                .and_then(|a| a.picker())
                .or_else(|| {
                    world
                        .battle
                        .command
                        .as_ref()
                        .filter(|c| c.actor == seat)
                        .and_then(|c| c.picker())
                });
            if let Some(picker) = picker
                && let legaia_engine_core::target_picker::PickerState::Cursor {
                    row: legaia_engine_core::target_picker::CursorRow::Enemy,
                    slot,
                } = picker.state()
                && slot != want
                && aim_alive(world, want)
            {
                return Some(PadButton::Right);
            }
        }
        if matches!(self, Self::Action { steer, .. } if steer.arts)
            && world.battle.arts_input.is_some()
        {
            return Some(PadButton::Cross);
        }
        let cmd = world.battle.command.as_ref()?;
        match *self {
            Self::Opening { .. } => None,
            Self::Menu { flow, seat } => {
                let ours = cmd.actor == seat && flow != BattleFlowState::CommitBegin;
                match cmd.phase {
                    CommandPhase::RoundPrompt { .. } => Some(PadButton::Left),
                    CommandPhase::Menu { .. } if !ours => Some(PadButton::Left),
                    CommandPhase::Menu { .. } => match flow {
                        BattleFlowState::ItemWindow => Some(PadButton::Up),
                        BattleFlowState::MagicWindow => Some(PadButton::Right),
                        BattleFlowState::ArtsCommandEntry
                        | BattleFlowState::AttackModePrompt
                        | BattleFlowState::TargetSelect => Some(PadButton::Left),
                        _ => None,
                    },
                    CommandPhase::AttackMode { .. } if !ours => Some(PadButton::Left),
                    CommandPhase::AttackMode { .. } => match flow {
                        BattleFlowState::ArtsCommandEntry => Some(PadButton::Right),
                        BattleFlowState::TargetSelect => Some(PadButton::Left),
                        _ => None,
                    },
                    CommandPhase::Targeting { .. } if !ours => Some(PadButton::Cross),
                    _ => None,
                }
            }
            Self::Action {
                seat,
                category,
                spare,
                steer,
                ..
            } => Some(match cmd.phase {
                CommandPhase::Menu { .. } if cmd.actor == seat && category == 4 => PadButton::Down,
                CommandPhase::Menu { .. } if cmd.actor != seat && spare => PadButton::Down,
                // A capture mid monster cast: the party commits Spirit, so no
                // swing lands on the caster before its replayed turn comes up
                // (a strong party otherwise kills it first, and the cast the
                // seed names is never taken).
                CommandPhase::Menu { .. } if seat >= 3 && category == 2 => PadButton::Down,
                CommandPhase::AttackMode { .. }
                    if cmd.actor == seat && steer.arts && saved_command_string(world, seat) =>
                {
                    PadButton::Right
                }
                CommandPhase::RoundPrompt { .. }
                | CommandPhase::Menu { .. }
                | CommandPhase::AttackMode { .. } => PadButton::Left,
                _ => PadButton::Cross,
            }),
        }
    }

    /// This tick's pad word: the step's press on even ticks, released on odd
    /// ones, so every press is an edge.
    pub fn pad_word_at(&self, world: &legaia_engine_core::world::World, tick: u64) -> u16 {
        if !tick.is_multiple_of(2) {
            return 0;
        }
        self.press(world).map_or(0, |b| b.mask())
    }
}

/// Whether party seat `seat`'s character record carries a saved auto
/// command string - the arrows the arts entry preseeds its window from
/// (`FUN_801DA34C`) and a bare confirm replays.
///
/// A party capture's queue `+0x1DF` is the turn the player committed, and a
/// record that holds a string is a player who entered (and so saved) one: a
/// replay through `Command` rebuilds that queue, where `Auto` rebuilds the
/// queue from the direction commands and learned arts and plays a different
/// action under the same state byte.
fn saved_command_string(world: &legaia_engine_core::world::World, seat: u8) -> bool {
    use legaia_save::character::AutoCommandBand;
    let slot = world.party_roster_slot(usize::from(seat));
    world.party.roster.members.get(slot).is_some_and(|rec| {
        rec.auto_command_string(AutoCommandBand::Primary)[0] != 0
            || rec.auto_command_string(AutoCommandBand::Secondary)[0] != 0
    })
}

/// The arrows a committed arts queue was entered as, as the swing bytes
/// `0x0C..=0x0F` the record's saved band holds.
///
/// The queue builder keeps a matched art's leading arrows as swings and
/// rewrites only its last one into the starter + constant pair (`0x19` /
/// `0x1A`, then the art), so each pair stands for its art's final arrow and
/// every other byte is an arrow already. `combo_of` names an art's arrows;
/// an art it does not know - a Super or Miracle replacement, whose tail the
/// finish rewrote whole - leaves the entry unrecoverable (`None`).
pub fn entered_arrows(
    queue: &[u8; 16],
    combo_of: impl Fn(u8) -> Option<Vec<legaia_art::Command>>,
) -> Option<[u8; 16]> {
    let mut out = [0u8; 16];
    let mut n = 0;
    let mut i = 0;
    while i < queue.len() && queue[i] != 0 {
        let arrow = match queue[i] {
            b @ 0x0C..=0x0F => b,
            0x19 | 0x1A => {
                i += 1;
                let last = *combo_of(*queue.get(i)?)?.last()?;
                0x0B + last as u8
            }
            _ => return None,
        };
        *out.get_mut(n)? = arrow;
        n += 1;
        i += 1;
    }
    (n > 0).then_some(out)
}

/// Make `seat`'s saved command string the arrows `queue` was entered as, so
/// the arts entry preseeds the captured turn.
///
/// Which band the preseed reads is the live gauge's choice at the arts
/// entry, which a replayed round can move after this runs (the seat's gauge
/// restore is the drive's, not the band's). A record that already holds the
/// arrows in either band is left alone - the capture's own preseed then
/// reads the string retail did - and otherwise both bands take them.
fn seed_entered_arrows(world: &mut legaia_engine_core::world::World, seat: u8, queue: &[u8; 16]) {
    use legaia_save::character::AutoCommandBand;
    let roster = world.party_roster_slot(usize::from(seat));
    let character = legaia_engine_core::battle_arts::character_for_slot(roster as u8);
    let Some(arrows) = entered_arrows(queue, |art| {
        let action = legaia_art::ActionConstant::from_byte(art)?;
        world
            .tables
            .art_records
            .get(&(character, action))
            .map(|r| r.commands.clone())
    }) else {
        return;
    };
    let Some(rec) = world.party.roster.members.get_mut(roster) else {
        return;
    };
    let bands = [AutoCommandBand::Primary, AutoCommandBand::Secondary];
    if bands.iter().any(|&b| rec.auto_command_string(b) == arrows) {
        return;
    }
    for band in bands {
        rec.set_auto_command_string(band, arrows);
    }
}

/// The member's EXP share the results sequencer hands out, `gp+0xA04`
/// (`sw s6,0xA04(gp)` at `0x8004F684`).
const END_XP_SHARE: u32 = 0x8007_BD1C;

/// Take a results-frame capture's rewards back off the party it seeds.
///
/// The results sequencer `FUN_8004E568` grants the fight's EXP and runs the
/// level-up applier `FUN_801E9504` when it opens the results frame, so a
/// capture on that frame or after it (`SpanGate::Results` / `Exit`) holds the
/// party past the grant. The seed replays the fight from that party, and the
/// engine grants again on its own results frame: `noa_levelup_banner`'s Noa,
/// already level 3 in the capture, gained nothing the second time, and the
/// engine frame carried no "level increased" line.
///
/// Every living member (`+0x14C > 0` on its seat) loses the share. A member
/// the applier levelled is recognised by its record stat window
/// (`+0x11C` HP max, `+0x11E` MP max, `+0x122..+0x12D` the six stats)
/// standing apart from the live window it is mirrored into one phase later
/// (`+0x104`, `+0x108`, `+0x110..+0x11B` -
/// `docs/subsystems/level-up.md#phase-split-multi-frame-writes`); that member
/// gets the live values back in its record window and its level byte
/// `+0x130` one lower. A capture past the live copy keeps the growth, which
/// the engine's grant does not repeat because the level stays where it is.
pub fn ungrant_results_rewards(
    save: &mut legaia_save::SaveFile,
    battle: &RetailBattle,
    ram: &[u8],
) {
    if !matches!(
        battle.span_gate,
        SpanGate::Results { .. } | SpanGate::Exit { .. }
    ) {
        return;
    }
    let share = game_anchors::u32_at(ram, END_XP_SHARE);
    for (seat, &char_id) in battle.seat_chars.iter().enumerate() {
        let alive = battle
            .party
            .get(seat)
            .and_then(|c| c.as_ref())
            .is_some_and(|c| c.hp > 0);
        let Some(rec) = usize::from(char_id)
            .checked_sub(1)
            .and_then(|i| save.party.members.get_mut(i))
        else {
            continue;
        };
        if alive {
            rec.set_cumulative_xp(rec.cumulative_xp().saturating_sub(share));
        }
        // (record window, live window) halfword pairs.
        const PAIRS: [(usize, usize); 8] = [
            (0x11C, 0x104),
            (0x11E, 0x108),
            (0x122, 0x110),
            (0x124, 0x112),
            (0x126, 0x114),
            (0x128, 0x116),
            (0x12A, 0x118),
            (0x12C, 0x11A),
        ];
        let raw = &mut rec.raw;
        if raw.len() < 0x130 || PAIRS.iter().all(|&(r, l)| raw[r..r + 2] == raw[l..l + 2]) {
            continue;
        }
        for (r, l) in PAIRS {
            raw.copy_within(l..l + 2, r);
        }
        let level = rec.level();
        rec.set_level(level.saturating_sub(1).max(1));
    }
}

/// The magic-level-increased screen element `FUN_801E70BC` raises and stores
/// on `ctx[+0x26]`.
const MAGIC_LEVEL_BANNER: u8 = 0x65;

/// Take a cast capture's magic level-up back off the caster it seeds.
///
/// The summon return's level check (`FUN_801E70BC`) bumps the cast spell's
/// level byte (`record[+0x161 + slot]`) and raises the "magic level
/// increased" banner, so a capture taken after it in the same action
/// ([`RetailBattle::magic_level_up`]) holds the caster already a level up,
/// with XP past the old threshold. The seed replays the cast from that
/// record, and the engine's check then compares the XP against the **next**
/// level's threshold: `shiny_refactor_gimard_levelup` levelled nothing the
/// second time and the engine frame carried no banner. The level goes back
/// one; the XP stays, and still clears the old threshold, so the replay's
/// own check levels it again.
pub fn ungrant_magic_level_up(save: &mut legaia_save::SaveFile, battle: &RetailBattle) {
    if !battle.magic_level_up || battle.queued_category != 2 {
        return;
    }
    let Some(&char_id) = battle.seat_chars.get(usize::from(battle.active_actor)) else {
        return;
    };
    let Some(rec) = usize::from(char_id)
        .checked_sub(1)
        .and_then(|i| save.party.members.get_mut(i))
    else {
        return;
    };
    let mut list = rec.spell_list();
    let count = usize::from(list.count).min(list.ids.len());
    if let Some(at) = list.ids[..count]
        .iter()
        .position(|&id| id == battle.queued_action)
        && list.levels[at] > 1
    {
        list.levels[at] -= 1;
        rec.set_spell_list(list);
    }
}

/// Take spell `spell_id` back off a record's list - the inverse of the
/// Done band's prepend (`legaia_engine_core::magic_xp::learn_spell_prepend`):
/// ids, levels and the parallel XP words above it shift down one. A list
/// without the spell is left alone.
fn unlearn_spell(record: &mut legaia_save::CharacterRecord, spell_id: u8) {
    const SPELL_XP_OFFSET: usize = 0x8;
    let mut list = record.spell_list();
    let count = usize::from(list.count).min(list.ids.len());
    let Some(at) = list.ids[..count].iter().position(|&id| id == spell_id) else {
        return;
    };
    for i in at..count - 1 {
        list.ids[i] = list.ids[i + 1];
        list.levels[i] = list.levels[i + 1];
        let src = SPELL_XP_OFFSET + (i + 1) * 4;
        let dst = SPELL_XP_OFFSET + i * 4;
        record.raw.copy_within(src..src + 4, dst);
    }
    list.ids[count - 1] = 0;
    list.levels[count - 1] = 0;
    let last = SPELL_XP_OFFSET + (count - 1) * 4;
    record.raw[last..last + 4].fill(0);
    list.count -= 1;
    record.set_spell_list(list);
}

/// Whether a menu surface belongs to one member (the ring, a submenu, the
/// target cursor) rather than to the party (the round prompt, the commit
/// confirm).
fn menu_seat_matters(flow: BattleFlowState) -> bool {
    !matches!(
        flow,
        BattleFlowState::TurnPrompt | BattleFlowState::CommitBegin
    )
}

impl RetailBattle {
    /// Monster indices that read `0` HP on a capture of a **party** action in
    /// flight: the swing's victims, which [`run_engine_battle`] can seed at
    /// `1` HP so the replay makes the kill rather than ending the fight at
    /// its first wipe gate.
    /// Whether the capture's yaw is an idle-orbit clock reading rather than a
    /// framing: a flow byte the battle tick's orbit store runs on
    /// ([`ORBIT_FLOWS`]), or a pre-seed capture whose far framing has landed
    /// ([`SpanGate::Landed`]) - case 9 passes `_DAT_8007B792` straight
    /// through and nothing in `0x0A` writes it, so it holds wherever the
    /// orbit left it.
    pub fn orbit_owns_yaw(&self) -> bool {
        ORBIT_FLOWS.contains(&self.flow) || self.span_gate == SpanGate::Landed
    }

    pub fn action_victims(&self) -> Vec<usize> {
        if !matches!(self.seed_plan(), SeedPlan::Action { seat, .. } if seat < 3) {
            return Vec::new();
        }
        self.monsters
            .iter()
            .enumerate()
            .filter_map(|(m, c)| c.filter(|c| c.hp == 0 && c.hp_max != 0).map(|_| m))
            .collect()
    }

    /// The pad drive that reaches this capture, when its plan has one.
    pub fn battle_drive(&self) -> Option<BattleDrive> {
        match self.seed_plan() {
            SeedPlan::Menu { flow, seat } => Some(BattleDrive::Menu { flow, seat }),
            SeedPlan::Action { seat, state } => Some(BattleDrive::Action {
                seat,
                state,
                category: self.queued_category,
                queued: self.queued_action,
                spare: !self.action_victims().is_empty(),
                absorbed: if seat < 3 { self.absorbed_seru } else { 0 },
                end: self.span_gate,
                style: Some(self.cam_style),
                steer: ActionSteer {
                    // A party killing blow on its victim, or a monster
                    // action (a cast or a strike) on the party seat it
                    // aimed at: the monster's pick is a draw, and a run on
                    // another member frames another approach.
                    target: ((seat < 3
                        && self.target_code >= 3
                        && self
                            .action_victims()
                            .contains(&usize::from(self.target_code - 3)))
                        || (seat >= 3
                            && matches!(self.queued_category, 2 | 3)
                            && self.target_code < 3)
                        // A monster cast on itself (Cort's Ultra Charge,
                        // `+0x1DD == ctx[+0x13]`): retail's cast-begin skips
                        // the facing store for a self target (`beq v0,t2` at
                        // `0x801E4350`), and the module frames from the
                        // heading the caster keeps.
                        || (seat >= 3 && self.queued_category == 2 && self.target_code == seat))
                        .then_some(self.target_code),
                    yaw: Some(self.walk_yaw_base),
                    coin: Some(self.track_coin),
                    message: self.timed_message,
                    plate_cleared: self.target_plate_cleared && seat < 3,
                    arts: seat < 3 && self.queued_category == 3 && self.arts_queue,
                    gauge: self
                        .acting_gauge
                        .filter(|&(live, base)| {
                            seat < 3 && self.queued_category == 3 && self.arts_queue && live > base
                        })
                        .map(|(live, _)| live),
                    clip: (seat < 3
                        // Gate on the direction swing clips 0x0C..=0x0F only:
                        // retail can sit on the dynamic art slot 0x10 / 0x11
                        // while the engine holds the swing's own clip, which
                        // the gate could then never match
                        // (battle_melee_hit_spark: retail 0x11, engine 0x0E).
                        && ((0x0C..=0x0F).contains(&self.caster_clip)
                            // ...and on the idle clip in the return state
                            // `0x20`, whose first hold waits while the
                            // attacker's `+0x1D9 != 0` (`0x801E54EC`): a
                            // capture reading `0` there is past the last
                            // swing, and the acting actor's idle commit is
                            // what its accumulator counts from. Ungated, the
                            // first tick the engine's accumulator reached the
                            // value was inside the last art clip
                            // (`player_steal_skeleton_banner`: the frame
                            // showed the Somersault still landing, retail the
                            // steal caption over an idle Vahn).
                            || (self.caster_clip == 0 && state == 0x20))
                        && matches!(self.span_gate, SpanGate::Age { .. }))
                    .then_some(self.caster_clip),
                    aim: (seat < 3
                        && self.queued_category == 3
                        && (3..8).contains(&self.target_code))
                    .then(|| self.target_code - 3),
                    queue: (seat < 3 && self.queued_category == 3 && self.arts_queue)
                        .then_some(self.committed_queue),
                    cursor: (seat < 3
                        && self.queued_category == 3
                        && self.arts_queue
                        && matches!(state, 0x1E | 0x1F)
                        // Only on the dynamic art slots, the clips the
                        // `clip` gate leaves open: an idle `0` or a swing is
                        // placed by the age and the clip already.
                        && matches!(self.caster_clip, 0x10 | 0x11)
                        && matches!(self.span_gate, SpanGate::Age { .. }))
                    .then_some(self.strike_cursor),
                },
            }),
            SeedPlan::Opening => Some(BattleDrive::Opening {
                swept: matches!(self.flow, 0x0C | 0x14),
                entry: self.entry_counter,
            }),
            _ => None,
        }
    }
}

/// Whether monster row `row` stands in the engine's pool (party count +
/// `row`), so the target cursor can land on it.
fn aim_alive(world: &legaia_engine_core::world::World, row: u8) -> bool {
    let pc = usize::from(world.party.party_count.clamp(1, 3));
    world
        .actors
        .get(pc + usize::from(row))
        .is_some_and(|a| a.active && a.battle.hp > 0)
}

/// Run `drive` from the round prompt through the pad path. The ticks it took,
/// or `None` when the phase was never reached (or the fight ended first).
fn run_drive(
    session: &mut BootSession,
    director: &mut crate::retail_compare::RecordingDirector,
    drive: BattleDrive,
    age_short: &mut Option<u16>,
) -> Result<Option<u32>> {
    drive.prime(&mut session.host.world);
    let mut reached = None;
    let mut held = 0;
    // The engine accumulator on the last tick an `Age` phase's state held
    // short of its age.
    let mut aged = None;
    for t in 0..drive.budget() {
        let world = &session.host.world;
        if reached.is_none() {
            if drive.in_aged_state(world) {
                aged = world.battle.camera.as_ref().map(|c| c.close_up_accum());
            } else if let Some(a) = aged.take() {
                // The state ended before it was as old as retail's: the
                // re-run samples its last tick.
                *age_short = Some(a.min(u32::from(u16::MAX)) as u16);
                break;
            }
        }
        if drive.reached(world) {
            reached.get_or_insert(t);
            if held >= drive.hold_ticks() {
                break;
            }
            held += 1;
        } else if reached.is_some() {
            // The surface closed under the hold: sample where it stands.
            break;
        }
        if world.mode != SceneMode::Battle {
            break;
        }
        if std::env::var_os("LEGAIA_RC_DRIVE_TRACE").is_some() {
            let hp: Vec<u16> = world.actors.iter().take(8).map(|a| a.battle.hp).collect();
            eprintln!(
                "[rc] t={t} mode={:?} flow={:?} cmd={} act={} st=0x{:02X} hp={hp:?} cam={:?} depth={} acc={:?} plaque_dy={} tint={:?}",
                world.mode,
                world.battle.flow,
                world.battle.command.is_some(),
                world.battle_ctx.active_actor,
                world.battle_ctx.action_state,
                world.battle.camera.as_ref().map(|c| c.phase()),
                world.battle.camera_frame_height as u16,
                world
                    .battle
                    .camera
                    .as_ref()
                    .map(|c| (c.close_up_accum(), c.is_gliding())),
                legaia_engine_core::battle_hud::battle_action_plaque_dy(world),
                world
                    .actors
                    .iter()
                    .take(8)
                    .map(|a| (a.battle.render_flag, a.battle.render_color))
                    .collect::<Vec<_>>()
            );
            let anims: Vec<(u8, u32, bool)> = (0..world.actors.len().min(8))
                .map(|i| {
                    (
                        world.battle_current_anim(i),
                        world.actors[i].battle.damage_accum,
                        world.battle_on_knockdown(i),
                    )
                })
                .collect();
            let pose = world.battle.camera.as_ref().map(|c| {
                let p = c.framing_pose();
                (p.pitch as i32, p.yaw as i32, p.tr.map(|v| v as i32))
            });
            eprintln!(
                "[rc] t={t} anims={anims:?} pose={pose:?} mod={} cd={} tgt={} pos={:?}",
                world.casting.module_phase,
                world.casting.module_cam.countdown.0,
                world
                    .actors
                    .get(usize::from(world.battle_ctx.active_actor))
                    .map_or(0, |a| a.battle.active_target),
                world
                    .actors
                    .iter()
                    .take(8)
                    .map(|a| (a.move_state.world_x, a.move_state.world_z))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "[rc] t={t} yaw_base={:?} style={} y/facing={:?}",
                world.battle.camera.as_ref().map(|c| c.action_yaw_base()),
                world.battle_ctx.camera_variant,
                world
                    .actors
                    .iter()
                    .take(8)
                    .map(|a| (a.move_state.world_y, a.battle.facing_angle & 0xFFF))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "[rc] t={t} clips={:?}",
                world
                    .actors
                    .iter()
                    .take(8)
                    .map(|a| a.battle_animation.as_ref().map(|p| (
                        p.action_id(),
                        p.cursor_sixteenths(),
                        p.frame_count(),
                        p.finished(),
                        a.battle.anim_rate.get()
                    )))
                    .collect::<Vec<_>>()
            );
        }
        let pad = if reached.is_some() {
            0
        } else {
            drive.pad_word_at(world, u64::from(t))
        };
        session.host.world.input.set_pad(pad);
        drive.steer(&mut session.host.world);
        session.tick()?;
        session.fog_render_tick();
        session.host.route_bgm_events(director)?;
    }
    session.host.world.input.set_pad(0);
    Ok(reached)
}

/// Fraction of equal `(hp, hp_max, mp, mp_max)` fields over the retail
/// combatants; `mp_max` is left out where the engine has none (`0`), and so
/// is any field the manifest names as written by the capture probe after
/// battle init (`injected`, e.g. `p0.mp_max`).
fn combatant_score(
    retail: &[Option<Combatant>],
    engine: &[Combatant],
    tag: &str,
    injected: &[String],
) -> (f64, String) {
    let mut total = 0usize;
    let mut skipped = Vec::new();
    let mut equal = 0usize;
    let mut diffs = Vec::new();
    for (i, r) in retail.iter().enumerate() {
        let Some(r) = r else { continue };
        let e = engine.get(i);
        let fields = [
            ("hp", r.hp, e.map(|e| e.hp)),
            ("hp_max", r.hp_max, e.map(|e| e.hp_max)),
            ("mp", r.mp, e.map(|e| e.mp)),
            ("mp_max", r.mp_max, e.map(|e| e.mp_max)),
        ];
        for (name, want, got) in fields {
            if name == "mp_max" && got == Some(0) {
                continue;
            }
            let key = format!("{tag}{i}.{name}");
            if injected.contains(&key) {
                skipped.push(format!("{key} retail={want} engine={got:?}"));
                continue;
            }
            total += 1;
            if got == Some(want) {
                equal += 1;
            } else {
                diffs.push(format!("{tag}{i}.{name} retail={want} engine={got:?}"));
            }
        }
    }
    let score = if total == 0 {
        1.0
    } else {
        equal as f64 / total as f64
    };
    let mut d = format!("{equal}/{total} fields");
    if !diffs.is_empty() {
        d.push_str("; ");
        d.push_str(&diffs.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
    }
    if !skipped.is_empty() {
        d.push_str(&format!(
            "; not scored, written by the capture probe: {}",
            skipped.join(", ")
        ));
    }
    (score, d)
}

/// Score one seeded battle state.
pub fn compare_battle(
    retail: &RetailObs,
    battle: &RetailBattle,
    engine: &EngineBattle,
    injected: &[String],
) -> (BTreeMap<String, f64>, BTreeMap<String, String>) {
    use crate::retail_compare::{camera_score, flags_score, inventory_score, round3};
    let mut ch = BTreeMap::new();
    let mut det = BTreeMap::new();
    let mut put = |name: &str, score: f64, detail: String| {
        ch.insert(name.to_string(), round3(score));
        det.insert(name.to_string(), detail);
    };
    let scene_ok = engine.scene.as_deref() == Some(retail.scene.as_str());
    put(
        "scene",
        f64::from(u8::from(scene_ok)),
        format!("retail={} engine={:?}", retail.scene, engine.scene),
    );
    put(
        "mode",
        f64::from(u8::from(engine.mode == SceneMode::Battle)),
        format!("retail=0x{:02X} engine={:?}", retail.game_mode, engine.mode),
    );
    let n = battle
        .monster_ids
        .len()
        .max(engine.monster_ids.len())
        .max(1);
    let same = battle
        .monster_ids
        .iter()
        .zip(&engine.monster_ids)
        .filter(|(r, e)| **e == Some(u16::from(**r)))
        .count();
    put(
        "enemies",
        same as f64 / n as f64,
        format!(
            "retail={:02X?} engine={:?} ({})",
            battle.monster_ids, engine.monster_ids, engine.formation_source
        ),
    );
    let (s, d) = combatant_score(&battle.monsters, &engine.monsters, "m", injected);
    put("enemy_hp", s, d);
    let (s, mut d) = combatant_score(&battle.party, &engine.party, "p", injected);
    if battle.party.len() != engine.party.len() {
        d.push_str(&format!(
            "; retail seats {:?} vs engine party of {}",
            battle.seat_chars,
            engine.party.len()
        ));
    }
    put("battle_party", s, d);
    let want = BattleFlowState::from_raw(battle.flow);
    // A replayed cast or a driven action is scored on the action-SM state as
    // well (the flow byte reads `Idle` for every in-flight action alike), a
    // driven menu on the member it is open for.
    let plan = battle.seed_plan();
    let replayed = engine.inflight.is_some()
        || matches!(plan, SeedPlan::Action { .. }) && engine.driven.is_some();
    let seat_ok = match plan {
        SeedPlan::Menu { flow, seat } if engine.driven.is_some() => {
            !menu_seat_matters(flow) || engine.active_actor == seat
        }
        SeedPlan::Action { seat, .. } if engine.driven.is_some() => {
            engine.active_actor == engine_seat(seat, engine.party.len() as u8)
        }
        _ => true,
    };
    // Past the end signal the action SM is parked and the drive's own gate
    // ([`SpanGate`]: the sequencer phase on the pose actor) is the phase.
    let end_held = battle.span_gate.is_end() && matches!(engine.driven, Some(Some(_)));
    let phase_ok = end_held
        || engine.flow == want
            && (!replayed || engine.action_state == battle.action_state)
            && seat_ok;
    let inflight = match engine.inflight {
        None => String::new(),
        Some(Some(t)) => format!("; cast replayed, phase reached at +{t}"),
        Some(None) => "; cast replayed, phase never reached".to_string(),
    };
    let driven = match (plan, engine.driven) {
        (_, None) if plan == SeedPlan::Opening => {
            "; sampled at the entry-sweep counter".to_string()
        }
        (_, None) => String::new(),
        (_, Some(Some(t))) if battle.span_gate != SpanGate::None => {
            format!("; driven by pad, {:?} reached at +{t}", battle.span_gate)
        }
        (_, Some(Some(t))) => format!("; driven by pad, reached at +{t}"),
        (_, Some(None)) => "; driven by pad, never reached".to_string(),
    };
    put(
        "phase",
        f64::from(u8::from(phase_ok)),
        format!(
            "retail flow=0x{:02X} ({want:?}) action=0x{:02X} run=0x{:02X} seat={} cat={} queued=0x{:02X} clip=0x{:02X}; engine {:?} action=0x{:02X} seat={} (first prompt at +{:?}){inflight}{driven}",
            battle.flow,
            battle.action_state,
            battle.run_state,
            battle.active_actor,
            battle.queued_category,
            battle.queued_action,
            battle.caster_clip,
            engine.flow,
            engine.action_state,
            engine.active_actor,
            engine.prompt_tick
        ),
    );
    let (s, d) = camera_score(&retail.camera, &engine.camera);
    put("camera", s, d);
    put(
        "bgm",
        f64::from(u8::from(engine.bgm_id == Some(retail.bgm_id))),
        format!(
            "retail word={} engine field track={:?} (engine battle track {:?}; pre-battle word {:?}, current {:?})",
            retail.bgm_id,
            engine.bgm_id,
            engine.battle_bgm,
            engine.field_word,
            engine.field_current
        ),
    );
    if let Some(rs) = &retail.save {
        let (s, d) = flags_score(&rs.ext.story_flag_bits, &engine.save.ext.story_flag_bits);
        put("flags", s, d);
        let (s, d) = inventory_score(rs, &engine.save);
        put("inventory", s, d);
    }
    (ch, det)
}

/// Ticks `play-window --battle` runs before the capture: the intro
/// transition, the opening to the first round prompt, and the same settle
/// the headless side takes.
pub const BATTLE_CAPTURE_TICK: u64 = 320;

/// The `play-window` arguments that reproduce this fight: the MAN row and
/// the retail party composition (`0x8007BD10` ids are 1-based roster ids).
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

pub fn play_window_args(battle: &RetailBattle, row: u16) -> Vec<String> {
    let mut args = vec!["--battle".to_string(), row.to_string()];
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
mod tests {
    use super::*;

    fn put16(ram: &mut [u8], va: u32, v: u16) {
        let o = (va & 0x1F_FFFF) as usize;
        ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(ram: &mut [u8], va: u32, v: u32) {
        let o = (va & 0x1F_FFFF) as usize;
        ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put8(ram: &mut [u8], va: u32, v: u8) {
        ram[(va & 0x1F_FFFF) as usize] = v;
    }

    /// A synthetic two-on-one fight: the reader takes the counts off the
    /// context, the ids off the cell and the combatants off the fixed pool
    /// slots (monster 0 is slot 3 whatever the party size).
    #[test]
    fn reads_a_fight_out_of_ram() {
        let mut ram = vec![0u8; 0x20_0000];
        let ctx = 0x800E_B654;
        put32(&mut ram, BATTLE_CTX, ctx);
        put8(&mut ram, ctx, 2);
        put8(&mut ram, ctx + 1, 1);
        put8(&mut ram, ctx + 6, 0x1E);
        put8(&mut ram, FORMATION_CELL, 0x4F);
        put8(&mut ram, SEAT_CHARS, 1);
        put8(&mut ram, SEAT_CHARS + 1, 2);
        for (slot, base, hp) in [(0u32, 0x800E_C9E8u32, 180u16), (3, 0x800E_D000, 999)] {
            put32(&mut ram, ACTOR_TABLE + slot * 4, base);
            put16(&mut ram, base + 0x14C, hp - 1);
            put16(&mut ram, base + 0x14E, hp);
        }
        let b = RetailBattle::from_ram(&ram).expect("seedable");
        assert_eq!(b.monster_ids, vec![0x4F]);
        assert_eq!(b.seat_chars, vec![1, 2]);
        assert_eq!(b.party.len(), 2);
        assert_eq!(b.party[0].map(|c| (c.hp, c.hp_max)), Some((179, 180)));
        assert_eq!(b.party[1], None, "an empty pool slot reads as no combatant");
        assert_eq!(b.monsters[0].map(|c| c.hp_max), Some(999));
        assert_eq!(
            BattleFlowState::from_raw(b.flow),
            BattleFlowState::TurnPrompt
        );
    }

    /// A live summon flash-in block, as `FUN_80024E80` leaves it in the
    /// actor pool: the reader finds it by tick word, kind, id and delta, and
    /// takes its age off the two countdowns.
    #[test]
    fn reads_the_summon_flash_age_off_the_fade_block() {
        use legaia_engine_vm::battle_action::SUMMON_FLASH_IN;
        let mut ram = vec![0u8; 0x20_0000];
        let actor = 0x8008_2BC4;
        put32(&mut ram, actor + 0x0C, FADE_ACTOR_TICK);
        let b = actor + 0x7C;
        put16(&mut ram, b + 0x10, template_delta(&SUMMON_FLASH_IN) as u16);
        put16(&mut ram, b + 0x18, 1);
        put16(&mut ram, b + 0x22, 1);
        // Still in the start delay: 14 of 20 left -> 6 vsyncs in.
        put16(&mut ram, b + 0x1C, 14);
        put16(&mut ram, b + 0x20, 20);
        assert_eq!(
            summon_fade(&ram),
            Some(RetailFade {
                to_white: true,
                age: 6
            })
        );
        // Landed and 14 into the hold: 20 + 20 + 14 vsyncs, less the one the
        // landing frame counts twice.
        put16(&mut ram, b + 0x1C, 0);
        put16(&mut ram, b + 0x20, (-14i16) as u16);
        assert_eq!(summon_fade(&ram).map(|f| f.age), Some(53));
        // A killed block is not the live flash.
        put32(&mut ram, actor + 0x10, ACTOR_DONE);
        assert_eq!(summon_fade(&ram), None);
    }

    /// The frame step comes off the duration history's longest entry, and
    /// the displayed frame is two steps behind the RAM.
    #[test]
    fn the_frame_step_and_display_lag_come_off_the_history() {
        let mut ram = vec![0u8; 0x20_0000];
        put16(&mut ram, STEP_MODE, 0x10);
        for (i, d) in [296u16, 310, 0x136].into_iter().enumerate() {
            put16(&mut ram, FRAME_HISTORY + i as u32 * 2, d);
        }
        assert_eq!(frame_step(&ram), 2);
        assert_eq!(display_lag_vsyncs(&ram), 4);
        put16(&mut ram, FRAME_HISTORY + 6, 0x210);
        assert_eq!(frame_step(&ram), 3);
        put16(&mut ram, STEP_MODE, 0);
        assert_eq!(frame_step(&ram), 1, "a non-adaptive mode steps one vsync");
        put32(&mut ram, FORCED_STEP, 2);
        assert_eq!(frame_step(&ram), 2, "a forced step skips the history");
    }

    /// The glide table's in-flight records come out with the display lag
    /// taken off `elapsed` (`nivora_duel_mid_blazing_slash`'s plaque and bar
    /// read ten of sixteen, six on screen at step 2), a landed record
    /// (`total == 0`) is skipped, and the list survives its env form.
    #[test]
    fn hud_glides_come_out_lag_corrected_and_round_trip() {
        let mut ram = vec![0u8; 0x20_0000];
        let ctx = 0x800E_B654;
        let rec = |slot: u32| ctx + HUD_GLIDE_TABLE + slot * HUD_GLIDE_STRIDE;
        // slot 0: the readout bar, (16, 234) -> (16, 192), ten in.
        ram[(rec(0) & 0x1F_FFFF) as usize] = 16;
        ram[(rec(0) & 0x1F_FFFF) as usize + 1] = 10;
        put16(&mut ram, rec(0) + 4, 16);
        put16(&mut ram, rec(0) + 6, 192);
        // slot 1: the actor plaque, three in - younger than the lag.
        ram[(rec(1) & 0x1F_FFFF) as usize] = 16;
        ram[(rec(1) & 0x1F_FFFF) as usize + 1] = 3;
        put16(&mut ram, rec(1) + 4, 16);
        put16(&mut ram, rec(1) + 6, 12);
        let seats = hud_glide_seats(&ram, ctx, 4);
        assert_eq!(
            seats,
            vec![
                HudGlideSeat {
                    target: [16, 192],
                    elapsed: 6,
                    total: 16
                },
                HudGlideSeat {
                    target: [16, 12],
                    elapsed: 0,
                    total: 16
                },
            ]
        );
        assert_eq!(
            HudGlideSeat::list_from_env(&HudGlideSeat::to_env(&seats)),
            seats
        );
    }

    #[test]
    fn the_phase_gate_round_trips_through_its_env_form() {
        for g in [
            PhaseGate {
                action_state: 0x33,
                fade: None,
                module_phase: None,
                cam_accum: None,
                walk_yaw: None,
                done_hold: None,
                module_countdown: None,
            },
            PhaseGate {
                action_state: 0x35,
                fade: Some(RetailFade {
                    to_white: false,
                    age: 24,
                }),
                module_phase: None,
                cam_accum: None,
                walk_yaw: None,
                done_hold: None,
                module_countdown: None,
            },
            PhaseGate {
                action_state: 0x36,
                fade: None,
                module_phase: Some(6),
                cam_accum: Some(72),
                walk_yaw: Some(1497),
                done_hold: None,
                module_countdown: Some(-16),
            },
            PhaseGate {
                action_state: 0x51,
                fade: None,
                module_phase: None,
                cam_accum: None,
                walk_yaw: None,
                done_hold: Some(104),
                module_countdown: None,
            },
        ] {
            assert_eq!(PhaseGate::from_env(&g.to_env()), Some(g));
        }
    }

    #[test]
    fn bar_seeds_round_trip_through_the_child_env() {
        let seeds = vec![
            BarSeed {
                slot: 0,
                hp: 412,
                mp: 37,
                ground: None,
                facing: None,
                defeat_lanes: None,
            },
            BarSeed {
                slot: 4,
                hp: 1,
                mp: 0,
                ground: Some([-4, -707]),
                facing: Some(0x9F0),
                defeat_lanes: Some(0),
            },
        ];
        assert_eq!(bar_seeds_from_env(&bar_seeds_to_env(&seeds)), seeds);
        assert!(bar_seeds_from_env("").is_empty());
    }

    #[test]
    fn the_battle_drive_round_trips_through_its_env_form() {
        for d in [
            BattleDrive::Opening {
                swept: true,
                entry: 0xAF,
            },
            BattleDrive::Menu {
                flow: BattleFlowState::ArtsCommandEntry,
                seat: 2,
            },
            BattleDrive::Menu {
                flow: BattleFlowState::CommitBegin,
                seat: 1,
            },
            BattleDrive::Action {
                seat: 3,
                state: 0x6F,
                category: 2,
                queued: 0x7A,
                spare: false,
                absorbed: 0,
                end: SpanGate::Exit { phase: 3 },
                style: None,
                steer: ActionSteer::default(),
            },
            BattleDrive::Action {
                seat: 0,
                state: 0x52,
                category: 3,
                queued: 0x0F,
                spare: true,
                absorbed: 1,
                end: SpanGate::DoneHold { timer: -1 },
                style: None,
                steer: ActionSteer::default(),
            },
            BattleDrive::Action {
                seat: 0,
                state: 0x5A,
                category: 3,
                queued: 0x0D,
                spare: true,
                absorbed: 0,
                end: SpanGate::Results { hold: 80 },
                style: None,
                steer: ActionSteer::default(),
            },
            BattleDrive::Action {
                seat: 3,
                state: 0x6F,
                category: 2,
                queued: 0xAD,
                spare: false,
                absorbed: 0,
                end: SpanGate::CaptureFade {
                    height: 0xFF40,
                    accum: 344,
                    arm: Some((1, 496)),
                    yaw: Some(0x83C),
                },
                style: None,
                steer: ActionSteer::default(),
            },
            BattleDrive::Action {
                seat: 3,
                state: 0x19,
                category: 3,
                queued: 0x08,
                spare: false,
                absorbed: 0,
                end: SpanGate::Age { accum: 552 },
                style: Some(3),
                steer: ActionSteer {
                    target: Some(4),
                    yaw: Some(0xA98),
                    message: Some((0x801C_ED18, 25)),
                    plate_cleared: true,
                    ..ActionSteer::default()
                },
            },
            BattleDrive::Action {
                seat: 0,
                state: 0x20,
                category: 3,
                queued: 0x0F,
                spare: false,
                absorbed: 0,
                end: SpanGate::Age { accum: 176 },
                style: Some(0),
                steer: ActionSteer {
                    yaw: Some(0x280),
                    arts: true,
                    gauge: Some(153),
                    clip: Some(0x11),
                    aim: Some(1),
                    queue: Some([
                        0x0D, 0x0F, 0x0E, 0x19, 0x27, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    ]),
                    cursor: Some(5),
                    ..ActionSteer::default()
                },
            },
        ] {
            assert_eq!(BattleDrive::from_env(&d.to_env()), Some(d));
        }
        assert_eq!(BattleDrive::from_env("menu,40"), None);
    }

    /// A committed arts queue reads back as the arrows that were entered:
    /// swings as they stand, each starter + art pair as its art's last arrow.
    /// `battle_melee_hit_spark`'s `0D 0F 0E 19 27` is Right Up Down Up when
    /// art `0x27` is Up Down Up; a constant with no known combo (a Super
    /// tail) or a stray byte leaves it unrecoverable.
    #[test]
    fn entered_arrows_undo_the_queue_builder() {
        use legaia_art::Command::{Down, Up};
        let combo = |a: u8| (a == 0x27).then(|| vec![Up, Down, Up]);
        let mut q = [0u8; 16];
        q[..5].copy_from_slice(&[0x0D, 0x0F, 0x0E, 0x19, 0x27]);
        let mut want = [0u8; 16];
        want[..4].copy_from_slice(&[0x0D, 0x0F, 0x0E, 0x0F]);
        assert_eq!(entered_arrows(&q, combo), Some(want));
        q[3] = 0x1A; // a newly-learned starter reads the same
        assert_eq!(entered_arrows(&q, combo), Some(want));
        q[4] = 0x2B;
        assert_eq!(entered_arrows(&q, combo), None, "unknown art");
        assert_eq!(entered_arrows(&[0u8; 16], combo), None, "empty queue");
        let mut stray = [0u8; 16];
        stray[0] = 0x2B;
        assert_eq!(entered_arrows(&stray, combo), None, "bare constant");
    }

    /// Taking an absorbed Seru back off a list undoes the Done band's
    /// prepend exactly - ids, levels and the XP words - and leaves a list
    /// without it alone.
    #[test]
    fn unlearning_undoes_the_absorb_grant() {
        use legaia_engine_core::magic_xp::learn_spell_prepend;
        let mut rec = legaia_save::CharacterRecord::zeroed();
        learn_spell_prepend(&mut rec, 0x83);
        learn_spell_prepend(&mut rec, 0x85);
        rec.raw[0x8..0xC].copy_from_slice(&7u32.to_le_bytes());
        rec.raw[0xC..0x10].copy_from_slice(&9u32.to_le_bytes());
        let before = rec.raw.clone();
        learn_spell_prepend(&mut rec, 0x81);
        unlearn_spell(&mut rec, 0x81);
        assert_eq!(rec.raw, before);
        unlearn_spell(&mut rec, 0x8A);
        assert_eq!(rec.raw, before);
    }

    /// The seed plan follows the flow byte: the entry band opens, a cast in
    /// the summon band replays, any other `0xFF` action is driven, the
    /// selection band above the prompt is driven, the prompt parks.
    #[test]
    fn the_seed_plan_follows_the_flow_byte() {
        let mut ram = vec![0u8; 0x20_0000];
        let ctx = 0x800E_B654;
        put32(&mut ram, BATTLE_CTX, ctx);
        put8(&mut ram, ctx, 1);
        put8(&mut ram, ctx + 1, 1);
        put8(&mut ram, FORMATION_CELL, 0x4F);
        let plan = |ram: &mut Vec<u8>, flow: u8, state: u8| {
            put8(ram, ctx + 6, flow);
            put8(ram, ctx + 7, state);
            RetailBattle::from_ram(ram).expect("seedable").seed_plan()
        };
        for flow in OPENING_FLOWS {
            assert_eq!(plan(&mut ram, flow, 0), SeedPlan::Opening);
        }
        assert_eq!(plan(&mut ram, 0x1E, 0), SeedPlan::Prompt);
        assert_eq!(
            plan(&mut ram, 0x50, 0),
            SeedPlan::Menu {
                flow: BattleFlowState::ArtsCommandEntry,
                seat: 0
            }
        );
        assert_eq!(
            plan(&mut ram, 0xFF, 0x1E),
            SeedPlan::Action {
                seat: 0,
                state: 0x1E
            }
        );
    }

    #[test]
    fn a_monster_pool_slot_maps_onto_the_engine_seating() {
        assert_eq!(engine_seat(1, 2), 1);
        assert_eq!(engine_seat(3, 1), 1);
        assert_eq!(engine_seat(4, 2), 3);
        assert_eq!(engine_seat(3, 3), 3);
    }

    #[test]
    fn a_loading_fight_names_its_reason() {
        let ram = vec![0u8; 0x20_0000];
        let err = RetailBattle::from_ram(&ram).unwrap_err();
        assert!(err.contains("not resident"), "{err}");
    }

    #[test]
    fn combatant_score_skips_a_missing_engine_max_mp() {
        let r = Combatant {
            hp: 10,
            hp_max: 20,
            mp: 5,
            mp_max: 9,
        };
        let e = Combatant { mp_max: 0, ..r };
        let (s, _) = combatant_score(&[Some(r)], &[e], "m", &[]);
        assert_eq!(s, 1.0);
        let (s, d) = combatant_score(&[Some(r)], &[], "p", &[]);
        assert_eq!(s, 0.0, "{d}");
    }
}
