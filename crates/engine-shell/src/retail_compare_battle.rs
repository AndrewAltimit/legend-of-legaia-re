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
/// Eight-slot battle actor pointer table.
const ACTOR_TABLE: u32 = 0x801C_9370;
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
pub const BATTLE_RNG_SEEDS: [u32; 6] = [
    0x1234_5678,
    0x9E37_79B9,
    0x0BAD_F00D,
    0x7F4A_7C15,
    0xC0FF_EE01,
    0x2545_F491,
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
    /// `ctx[+0x13]` - the seat the action SM is running.
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
    /// Each pool slot's live `+0x34` / `+0x38` pair (party `0..=2`,
    /// monsters `3..=7`), `None` for an empty slot.
    pub ground: Vec<Option<[i16; 2]>>,
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
        Some(PhaseGate {
            action_state: self.action_state,
            fade: self.summon_fade,
            module_phase,
            cam_accum,
            walk_yaw,
        })
    }
}

/// Where in the fight a capture sits, as far as the seed has to place the
/// engine to compare the same phase ([`RetailBattle::seed_plan`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedPlan {
    /// The fight is still opening: `ctx[+0x06]` holds one of the entry
    /// values below the round prompt. The engine is sampled at its
    /// battle-mode flip, before its own opening runs.
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
        let active_actor = game_anchors::u8_at(ram, ctx + 0x13);
        let active = Some(game_anchors::u32_at(
            ram,
            ACTOR_TABLE + u32::from(active_actor.min(7)) * 4,
        ))
        .filter(|&p| in_ram(p));
        Ok(Self {
            party_count,
            monster_count,
            flow: game_anchors::u8_at(ram, ctx + 6),
            action_state: game_anchors::u8_at(ram, ctx + 7),
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
        })
    }
}

/// What the engine shows after the battle seed.
pub struct EngineBattle {
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
        session.tick()?;
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
    {
        let world = &mut session.host.world;
        let pc = world.party.party_count.clamp(1, 3) as usize;
        let seeds = battle
            .party
            .iter()
            .enumerate()
            .chain(battle.monsters.iter().enumerate().map(|(m, c)| (pc + m, c)));
        for (slot, c) in seeds {
            let (Some(c), Some(a)) = (c, world.actors.get_mut(slot)) else {
                continue;
            };
            a.battle.hp = if slot >= pc && victims.contains(&(slot - pc)) {
                1
            } else {
                c.hp
            };
            a.battle.liveness = a.battle.hp;
            if a.battle.hp_display.is_some() {
                a.battle.hp_display = Some(a.battle.hp);
            }
            a.battle.mp = c.mp;
        }
    }
    // Place the engine at the capture's phase ([`SeedPlan`]).
    //
    // An opening capture is sampled at the battle-mode flip. Everything else
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
    session.host.world.battle.inflight_seed = seed;
    let mut prompt_tick = None;
    if plan != SeedPlan::Opening {
        for t in 0..OPENING_TICKS {
            let w = &session.host.world;
            let reached = match seed {
                Some(_) => w.battle.inflight_seed.is_none(),
                None => w.battle.flow != BattleFlowState::Idle,
            };
            if reached {
                prompt_tick = Some(t);
                break;
            }
            session.tick()?;
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
    match (plan, battle.phase_gate()) {
        (SeedPlan::Opening, _) => {}
        (SeedPlan::Cast, Some(gate)) if prompt_tick.is_some() => {
            for t in 0..INFLIGHT_TICKS {
                if gate.met(&session.host.world) {
                    phase_tick = Some(t);
                    break;
                }
                session.tick()?;
                session.host.route_bgm_events(&mut director)?;
            }
        }
        (SeedPlan::Menu { .. } | SeedPlan::Action { .. }, _) if prompt_tick.is_some() => {
            if let Some(drive) = battle.battle_drive() {
                let track = session.host.bgm_track_word.or(director.last);
                pre_drive = Some(combat_snapshot(&mut session.host.world, track));
                driven = Some(run_drive(&mut session, &mut director, drive)?);
            }
        }
        _ => {
            for _ in 0..BATTLE_SETTLE_TICKS {
                session.tick()?;
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
    if ORBIT_FLOWS.contains(&battle.flow)
        && let Some(cam) = session.host.world.battle.camera.as_mut()
    {
        cam.align_orbit_yaw(f32::from(retail.camera.yaw));
    }
    let world = &session.host.world;
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
        rng_seed,
        scene: session.host.scene.as_ref().map(|s| s.name.clone()),
        mode: snap.mode,
        formation_source: source,
        man_row,
        prompt_tick,
        inflight: seed.map(|_| phase_tick),
        driven,
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
    /// A command-selection surface on party seat `seat`: members ahead of
    /// it commit a plain Attack, the seat itself takes the arm that leads
    /// to `flow`.
    Menu { flow: BattleFlowState, seat: u8 },
    /// An action in flight: rounds are committed (a plain Attack each, the
    /// capture's own seat Spirit when that is what it had committed) until
    /// the action SM holds `state` on `seat`. A monster seat that was
    /// casting (`category == 2`) casts the capture's spell `queued` on its
    /// next turn ([`legaia_engine_core::world::BattleState::forced_monster_cast`]).
    Action {
        seat: u8,
        state: u8,
        category: u8,
        queued: u8,
    },
}

impl BattleDrive {
    /// `menu,<flow>,<seat>` or `action,<seat>,<state>,<category>,<queued>`.
    pub fn to_env(&self) -> String {
        match *self {
            Self::Menu { flow, seat } => format!("menu,{},{seat}", flow.raw()),
            Self::Action {
                seat,
                state,
                category,
                queued,
            } => format!("action,{seat},{state},{category},{queued}"),
        }
    }

    pub fn from_env(s: &str) -> Option<Self> {
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
            }),
            _ => None,
        }
    }

    /// The tick budget the drive gets past the first prompt.
    pub fn budget(&self) -> u32 {
        match self {
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
            Self::Action { .. } => 0,
        }
    }

    /// Arm the drive's one-shot world seed (a monster cast to replay).
    /// Idempotent while the seed is unconsumed.
    pub fn prime(&self, world: &mut legaia_engine_core::world::World) {
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
            Self::Menu { flow, seat } => {
                world.battle.flow == flow
                    && (!menu_seat_matters(flow) || world.battle_ctx.active_actor == seat)
            }
            Self::Action { seat, state, .. } => {
                world.battle.flow == BattleFlowState::Idle
                    && world.battle.command.is_none()
                    && world.battle_ctx.active_actor == engine_seat(seat, pc)
                    && world.battle_ctx.action_state == state
            }
        }
    }

    /// The press that walks the engine one step toward the phase, or `None`
    /// when nothing on screen wants one (the action SM owns the frame).
    pub fn press(
        &self,
        world: &legaia_engine_core::world::World,
    ) -> Option<legaia_engine_core::input::PadButton> {
        use legaia_engine_core::battle_input::CommandPhase;
        use legaia_engine_core::input::PadButton;
        if world.mode != SceneMode::Battle {
            return None;
        }
        // A battle message box parks the whole battle until it is dismissed.
        if !world.battle.tutorial_boxes.is_empty() {
            return Some(PadButton::Cross);
        }
        let cmd = world.battle.command.as_ref()?;
        match *self {
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
            Self::Action { seat, category, .. } => Some(match cmd.phase {
                CommandPhase::Menu { .. } if cmd.actor == seat && category == 4 => PadButton::Down,
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
            }),
            _ => None,
        }
    }
}

/// Run `drive` from the round prompt through the pad path. The ticks it took,
/// or `None` when the phase was never reached (or the fight ended first).
fn run_drive(
    session: &mut BootSession,
    director: &mut crate::retail_compare::RecordingDirector,
    drive: BattleDrive,
) -> Result<Option<u32>> {
    drive.prime(&mut session.host.world);
    let mut reached = None;
    let mut held = 0;
    for t in 0..drive.budget() {
        let world = &session.host.world;
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
        let pad = if reached.is_some() {
            0
        } else {
            drive.pad_word_at(world, u64::from(t))
        };
        session.host.world.input.set_pad(pad);
        session.tick()?;
        session.host.route_bgm_events(director)?;
    }
    session.host.world.input.set_pad(0);
    Ok(reached)
}

/// Fraction of equal `(hp, hp_max, mp, mp_max)` fields over the retail
/// combatants; `mp_max` is left out where the engine has none (`0`).
fn combatant_score(retail: &[Option<Combatant>], engine: &[Combatant], tag: &str) -> (f64, String) {
    let mut total = 0usize;
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
    (score, d)
}

/// Score one seeded battle state.
pub fn compare_battle(
    retail: &RetailObs,
    battle: &RetailBattle,
    engine: &EngineBattle,
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
    let (s, d) = combatant_score(&battle.monsters, &engine.monsters, "m");
    put("enemy_hp", s, d);
    let (s, mut d) = combatant_score(&battle.party, &engine.party, "p");
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
    let phase_ok =
        engine.flow == want && (!replayed || engine.action_state == battle.action_state) && seat_ok;
    let inflight = match engine.inflight {
        None => String::new(),
        Some(Some(t)) => format!("; cast replayed, phase reached at +{t}"),
        Some(None) => "; cast replayed, phase never reached".to_string(),
    };
    let driven = match (plan, engine.driven) {
        (_, None) if plan == SeedPlan::Opening => "; sampled at the battle-mode flip".to_string(),
        (_, None) => String::new(),
        (_, Some(Some(t))) => format!("; driven by pad, reached at +{t}"),
        (_, Some(None)) => "; driven by pad, never reached".to_string(),
    };
    put(
        "phase",
        f64::from(u8::from(phase_ok)),
        format!(
            "retail flow=0x{:02X} ({want:?}) action=0x{:02X} run=0x{:02X} seat={} cat={} queued=0x{:02X}; engine {:?} action=0x{:02X} seat={} (first prompt at +{:?}){inflight}{driven}",
            battle.flow,
            battle.action_state,
            battle.run_state,
            battle.active_actor,
            battle.queued_category,
            battle.queued_action,
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

    #[test]
    fn the_phase_gate_round_trips_through_its_env_form() {
        for g in [
            PhaseGate {
                action_state: 0x33,
                fade: None,
                module_phase: None,
                cam_accum: None,
                walk_yaw: None,
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
            },
            PhaseGate {
                action_state: 0x36,
                fade: None,
                module_phase: Some(6),
                cam_accum: Some(72),
                walk_yaw: Some(1497),
            },
        ] {
            assert_eq!(PhaseGate::from_env(&g.to_env()), Some(g));
        }
    }

    #[test]
    fn the_battle_drive_round_trips_through_its_env_form() {
        for d in [
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
            },
        ] {
            assert_eq!(BattleDrive::from_env(&d.to_env()), Some(d));
        }
        assert_eq!(BattleDrive::from_env("menu,40"), None);
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
        let (s, _) = combatant_score(&[Some(r)], &[e], "m");
        assert_eq!(s, 1.0);
        let (s, d) = combatant_score(&[Some(r)], &[], "p");
        assert_eq!(s, 0.0, "{d}");
    }
}
