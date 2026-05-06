//! Composite actor + scene runtime that wires the per-VM hosts together.
//!
//! `legaia-engine-vm` ships each script VM (actor / sprite, move-table,
//! effect, field, battle action) as a small clean-room port + a `Host` trait
//! that lets engines plug in their own state. This module is the engine-side
//! glue: a single [`World`] that owns the per-actor data and implements every
//! VM `Host` trait by routing into that data.
//!
//! ## Why a single composite
//!
//! In the retail runtime, "an actor" is a 0xCB-byte record holding everything
//! all four VMs read/write — world position, anim banks, flags, render bank,
//! per-action queue, etc. Splitting that across four crates would force
//! engines to keep four parallel index tables in sync. The composite pattern
//! here keeps the per-VM `ActorState` structs intact (clean-room boundary
//! preserved) but lets one struct own them.
//!
//! Engines that want a different layout — say, ECS storage — should
//! implement the VM `Host` traits themselves; this is the default.

use crate::field_events::FieldEvent;
use legaia_engine_vm as vm;
use legaia_save;
use vm::battle_action::{
    BattleActionCtx, BattleActionHost, BattleActor, BattleEndCause, Pose, StepOutcome,
};
use vm::effect_vm::{EffectHost, MasterSlot, Pool, StateOutcome};
use vm::field::{CameraParam, FieldCtx, FieldHost, SceneFadeResult, StepResult as FieldStepResult};
use vm::move_vm::{ActorState as MoveActorState, MoveHost};
use vm::{Host as ActorVmHost, Position as ActorVmPosition};

/// Maximum simultaneous actors in the world. Mirrors the battle-side cap of
/// 8 + 32 spare slots for field-side NPCs / cutscene actors.
pub const MAX_ACTORS: usize = 64;

/// One queued fade request. Move-VM ext sub-op 0x3C writes either an
/// immediate fade (`ticks == 0`) or a ramp (`ticks > 0`) — engines drain
/// `pending_fade` each frame to drive the screen overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeRequest {
    pub rgb: [u8; 3],
    pub ticks: u16,
}

/// Scene mode the world is running. Drives which top-level VMs tick and
/// which auxiliary state lives in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneMode {
    /// Title / no scene. Only the actor VM and effect pool are live.
    #[default]
    Title,
    /// Field / town scene — field VM drives event flow.
    Field,
    /// Battle scene — battle action state machine runs over the actor table.
    Battle,
    /// Cutscene mode — actor VM runs but no field/battle dispatch.
    Cutscene,
}

/// Per-actor record held by the world. Composes the per-VM state structs.
///
/// Each VM's `Host` trait reads/writes only the slice it owns, so the per-VM
/// state structs don't need to know about each other. The world coalesces
/// world-XYZ to keep rendering / collision queries on one source of truth.
#[derive(Debug, Clone, Default)]
pub struct Actor {
    /// `true` if this slot is in use. Empty slots are skipped by every VM
    /// dispatcher.
    pub active: bool,

    /// Default-position lookup result for the actor VM. Retail reads this
    /// from a 16-byte-stride table at `0x801E473C`; engines populate per
    /// scene from extracted assets.
    pub default_pos: ActorVmPosition,

    /// Move-VM per-actor state.
    pub move_state: MoveActorState,

    /// Battle-action per-actor state. Populated only when the world is in
    /// [`SceneMode::Battle`].
    pub battle: BattleActor,

    /// Sprite / actor-VM scratch fields:
    /// - `field_1d`: opaque per-actor flag (set by actor VM op `SetField1d`).
    /// - `field_20`: opaque per-actor 16-bit slot (cleared by `ClearField20`).
    pub field_1d: u8,
    pub field_20: u16,

    /// `subobj` snap-clear condition flag — engine sets this when the actor's
    /// subobj is in the "snap to anchor" configuration. Read by actor VM op
    /// `SpawnDefault`.
    pub snap_clear: bool,

    /// Optional motion target consumed by actor VM op `EffectMotion`.
    /// `None` → the op no-ops (the retail equivalent of a null subobj
    /// pointer).
    pub motion_target: Option<ActorVmPosition>,

    /// Last-frame effect spawn — engine wires whatever rendering / sound
    /// flash it has. We just record the actor id for inspection.
    pub last_effect: u32,
}

impl Actor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark this slot as active. Returns `&mut Self` for chaining.
    pub fn activate(&mut self) -> &mut Self {
        self.active = true;
        self
    }
}

/// Singleton world / scene held by an engine integration.
///
/// Holds the actor table, the active battle-action ctx (when the scene mode
/// is [`SceneMode::Battle`]), the shared effect-VM pool, and the rotation
/// LUTs / RNG state used by the move-VM ports.
///
/// The `Host` trait impls live on a thin `WorldHost<'_>` borrow to keep
/// borrow-checker complexity manageable — see [`World::with_host`].
pub struct World {
    pub mode: SceneMode,
    pub actors: Vec<Actor>,
    pub battle_ctx: BattleActionCtx,
    pub effect_pool: Pool,
    /// Field VM execution context. Live in `SceneMode::Field` and
    /// `SceneMode::Cutscene` (cutscenes are field scenes that suppress
    /// player input via context flags).
    pub field_ctx: FieldCtx,
    /// Field VM bytecode buffer. Engines load this from a scene's PROT
    /// asset bundle when entering a field scene; `field_pc` indexes it.
    pub field_bytecode: Vec<u8>,
    /// Current field-VM PC. Updated by `tick()` based on the StepResult.
    pub field_pc: usize,
    /// Per-actor move-VM bytecode buffers. Indexed by actor slot. Empty
    /// vec means "no active move" — the move VM is not ticked for that
    /// actor. Set via [`World::set_move_bytecode`].
    pub move_bytecode: Vec<Vec<u16>>,
    /// Move-VM global predicate at `_DAT_801F22F4` (set by ext sub-op 0x08,
    /// cleared by 0x09; sub-ops 0x0A / 0x0B branch on it).
    pub move_predicate: u32,
    /// Move-VM global counter at `_DAT_801F22F6` (cleared by ext sub-op 0x0F,
    /// cycled mod 16 by sub-op 0x10).
    pub move_counter: u16,
    /// Move-VM 16-slot 8-byte-stride scratch table at `&DAT_801F3498`. Used
    /// by ext sub-ops 0x11 / 0x12 / 0x25 / 0x27 / 0x28 / 0x31 / 0x32 / 0x34
    /// / 0x35 to checkpoint world coords + tween state per actor / animation.
    pub move_slot_table: [[u8; 8]; 16],
    /// Move-VM axis offset at `_DAT_8007C348` — used by ext sub-ops 0x36 / 0x37
    /// for the `0x8E - axis` threshold predicate. Engines write per-scene.
    pub move_axis_threshold: i16,
    /// Move-VM scratchpad ramp ratio numerator at `_DAT_1F800393` — used by
    /// ext sub-op 0x23 (anim-bank lerp) as the numerator of a 12.0 fixed-point
    /// ratio against the operand-supplied denominator.
    pub move_ramp_ratio: u8,
    /// Fixed map origin pair at `(_DAT_80089118, _DAT_80089120)` — used by ext
    /// sub-op 0x24 (world position lerp toward fixed map origin).
    pub map_origin_xz: (i32, i32),
    /// Player actor slot — when `Some(slot)`, ext sub-ops 0x06 / 0x07 / 0x2A
    /// / 0x36 / 0x39 read `actors[slot].move_state.world_{x,y,z}` as the
    /// player position. `None` falls back to the origin (default impl).
    pub player_actor_slot: Option<u8>,
    /// Party-member actor slots — `party_actor_slots[i] = Some(actor_slot)`
    /// resolves move-VM ext sub-op 0x3B (`ext_party_member_lookup`) to the
    /// world-coords of the actor at that slot. Default empty (the lookup
    /// returns `None`, which forces sub-op 0x3B's "skip" path).
    pub party_actor_slots: Vec<Option<u8>>,
    /// Last fade colour requested by move-VM ext sub-op 0x3C — engines
    /// drain this each frame to drive the screen fade. `None` when no
    /// fade is pending.
    pub pending_fade: Option<FadeRequest>,
    /// Move-VM `_DAT_8007B9D8` — globally-shared 32-bit slot written by ext
    /// sub-op 0x2F. Engines read this on whatever frame-tick they want.
    pub move_dat_8007b9d8: i32,
    /// Move-VM 16-slot scratchpad ramp targets at `_DAT_1F80035C` — used by
    /// ext sub-op 0x29 (per-frame ramp / immediate write). Stored as i16
    /// pairs (target, current); engines apply per-frame interpolation.
    pub scratchpad_targets: [i16; 16],
    /// Shared system flag bank at `_DAT_80086D70` — bitfield read / written
    /// by:
    /// - field VM high-byte default routes 0x5x / 0x6x / 0x7x
    ///   (`system_flag_set` / `system_flag_clear` / `system_flag_test`)
    /// - move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D
    ///   (`ext_query_flag_bank` / `ext_set_flag_bank` / `ext_clear_flag_bank`)
    ///
    /// Lazily grown on write — the field VM's opcode-encoded idx ranges over
    /// `0..=0x87FF`, so a fixed 256-bit array is too small.
    pub system_flags: Vec<u8>,
    /// Field-VM `extra_flags` register read by op 0x42 mode 0 — a 32-bit
    /// auxiliary flag word (origin TBD; treated as scene-local state).
    pub extra_flags: u32,
    /// Field-VM `screen_mode` register read by op 0x42 mode 1 — packed mode
    /// bits (bits 4 / 5 / 6 / 7 individually testable; bits 12..15 indexed
    /// against `screen_mode_table`).
    pub screen_mode: u32,
    /// Story-flag word (`_DAT_1F800394` in retail). Read by field-VM
    /// op 0x30 GFLAG_TST and friends.
    pub story_flags: u32,

    /// PRNG state consumed by every VM that calls `host.rng()`. Default uses
    /// a deterministic LCG so tests are reproducible.
    pub rng_state: u32,

    /// Sin LUT used by move-VM op `0x03`. Engines populate from extracted
    /// asset data; default is empty (returns zero).
    pub sin_lut: Vec<i16>,
    /// Cos LUT — same shape as `sin_lut`.
    pub cos_lut: Vec<i16>,

    /// Battle-action helper tables. Engines populate per scene.
    pub spell_costs: std::collections::HashMap<u8, u8>,
    pub capture_spells: std::collections::HashSet<u8>,
    pub character_ability_bits: [u32; 8],
    pub range_table: std::collections::HashMap<(u8, u8), u16>,

    /// "Previous action cleared" gate — toggled by the engine when an
    /// animation transition completes.
    pub prev_action_cleared: bool,
    /// "Sound bank ready" gate.
    pub sound_bank_ready: bool,

    /// Number of party slots (default 3).
    pub party_count: u8,

    /// Last-issued battle-end cause (for inspection / engine side-effects).
    pub battle_end: Option<BattleEndCause>,

    /// Persistent per-character roster — populated by [`World::load_party`]
    /// and written back by [`World::save_party`]. Each record is the
    /// 0x414-byte struct documented in `docs/subsystems/battle.md`. The
    /// in-battle `BattleActor` slots mirror HP / MP from this; everything
    /// else (spells, equipment, ability bits) flows through this canonical
    /// store.
    pub roster: legaia_save::Party,

    /// Pending field-VM scene transition (`scene_transition(map_id)` was
    /// called this frame). Drained by [`crate::scene::SceneHost::tick`]
    /// — when `Some(map_id)`, the host resolves the map id to a scene
    /// name, loads it, and reinitialises the field VM. `None` between
    /// transitions.
    pub pending_scene_transition: Option<u8>,

    /// Field-VM side-effects emitted this frame. Engines drain after
    /// [`World::tick`] to dispatch BGM, dialog, money, party, camera, etc.
    /// Mirror of the `FieldHost` callbacks — see [`FieldEvent`] for the
    /// per-variant citation.
    ///
    /// [`FieldEvent`]: crate::field_events::FieldEvent
    pub pending_field_events: Vec<FieldEvent>,

    /// Last BGM the field VM started (op 0x35 sub-1 / sub-9). `None` until
    /// a scene starts one. Updated synchronously when the VM emits the
    /// corresponding `Bgm` event.
    pub current_bgm: Option<u16>,

    /// Active dialog request — populated by the field-VM op 0x3F handler,
    /// cleared by the engine after the user dismisses the box. The MES
    /// renderer reads `text_id` + `inline`; the world-coords + depth feed
    /// the box placement.
    pub current_dialog: Option<DialogRequest>,

    /// Last `field_interact` request. Cleared by the engine when handled
    /// (set to `None`).
    pub last_field_interact: Option<(u8, u8)>,

    /// Active party slot for the leader (op 0x4C sub-0 writes here, plus
    /// `party_add` populates it on the first member).
    pub party_leader_slot: Option<u8>,

    /// Running money total (gold). Modified by op 0x3A `add_money`,
    /// clamped to `[0, 9_999_999]` per the original retail formula.
    pub money: i32,

    /// Per-slot inventory counts. Indexed by raw `slot_byte` operand of
    /// op 0x3B (`(slot >> 4) * 0x414 + (slot & 0xF)` in retail). Engines
    /// can re-key this to their own inventory model.
    pub inventory: std::collections::HashMap<u8, u8>,

    /// Last camera state snapshot — filled by `camera_save`, applied by
    /// `camera_apply` / `camera_load`. Engines that draw a camera read
    /// this between frames.
    pub camera_state: CameraState,

    /// Frame counter incremented every [`World::tick`].
    pub frame: u64,
}

/// Pending dialog request for the field-VM op 0x3F handler. The engine
/// renders + advances; clearing `World::current_dialog` signals the script
/// to resume.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogRequest {
    pub text_id: u16,
    pub inline: Vec<u8>,
    pub world_x: u16,
    pub world_z: u16,
    pub depth_id: u8,
}

/// Camera state populated by `camera_save` / `camera_load` and read by
/// `camera_apply`. Engines render the configured camera each frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraState {
    /// Last `CameraConfigure` params applied (param-mask + values).
    pub params: Vec<CameraParam>,
    /// Last apply-trigger value.
    pub apply_trigger: u16,
    /// Last apply-mode nibble.
    pub mode: u8,
    /// Snapshot bytes from `camera_save`.
    pub saved: Vec<u8>,
    /// Last `camera_load` payload.
    pub loaded_payload: Vec<u8>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// Build a fresh world with `MAX_ACTORS` empty slots.
    pub fn new() -> Self {
        Self {
            mode: SceneMode::default(),
            actors: (0..MAX_ACTORS).map(|_| Actor::new()).collect(),
            battle_ctx: BattleActionCtx::new(),
            effect_pool: Pool::new(),
            field_ctx: FieldCtx::default(),
            field_bytecode: Vec::new(),
            field_pc: 0,
            move_bytecode: vec![Vec::new(); MAX_ACTORS],
            move_predicate: 0,
            move_counter: 0,
            move_slot_table: [[0u8; 8]; 16],
            move_axis_threshold: 0,
            move_ramp_ratio: 0,
            map_origin_xz: (0, 0),
            player_actor_slot: None,
            party_actor_slots: Vec::new(),
            pending_fade: None,
            move_dat_8007b9d8: 0,
            scratchpad_targets: [0; 16],
            system_flags: Vec::new(),
            extra_flags: 0,
            screen_mode: 0,
            story_flags: 0,
            rng_state: 0x1234_5678,
            sin_lut: Vec::new(),
            cos_lut: Vec::new(),
            spell_costs: Default::default(),
            capture_spells: Default::default(),
            character_ability_bits: [0; 8],
            range_table: Default::default(),
            prev_action_cleared: true,
            sound_bank_ready: true,
            party_count: 3,
            battle_end: None,
            roster: legaia_save::Party::zeroed(0),
            pending_scene_transition: None,
            pending_field_events: Vec::new(),
            current_bgm: None,
            current_dialog: None,
            last_field_interact: None,
            party_leader_slot: None,
            money: 0,
            inventory: std::collections::HashMap::new(),
            camera_state: CameraState::default(),
            frame: 0,
        }
    }

    /// Drain emitted field-VM events. Engines call once per frame after
    /// [`World::tick`] to dispatch BGM, dialog, money, etc. Returns events
    /// in emission order.
    pub fn drain_field_events(&mut self) -> Vec<FieldEvent> {
        std::mem::take(&mut self.pending_field_events)
    }

    /// Set / clear the move-VM bytecode for `slot`. `None` clears the
    /// buffer; subsequent ticks won't run the move VM on this actor.
    pub fn set_move_bytecode(&mut self, slot: usize, bytecode: Option<Vec<u16>>) {
        if slot < self.move_bytecode.len() {
            self.move_bytecode[slot] = bytecode.unwrap_or_default();
        }
    }

    /// Set bit `idx` in the shared system flag bank. `idx >> 3` is the byte
    /// offset; the bit mask is `0x80 >> (idx & 7)` (MSB-first, mirroring the
    /// SCUS helper at `FUN_8003CE08`). The bank grows lazily as needed.
    pub fn system_flag_set(&mut self, idx: u16) {
        let byte = (idx >> 3) as usize;
        if byte >= self.system_flags.len() {
            self.system_flags.resize(byte + 1, 0);
        }
        self.system_flags[byte] |= 0x80u8 >> (idx & 7);
    }

    /// Clear bit `idx` in the shared system flag bank. See [`system_flag_set`].
    /// Out-of-bounds clears are no-ops (the bit is already zero).
    ///
    /// [`system_flag_set`]: World::system_flag_set
    pub fn system_flag_clear(&mut self, idx: u16) {
        let byte = (idx >> 3) as usize;
        if byte < self.system_flags.len() {
            self.system_flags[byte] &= !(0x80u8 >> (idx & 7));
        }
    }

    /// Test bit `idx` in the shared system flag bank. Returns `false` for
    /// indices past the currently-grown size.
    pub fn system_flag_test(&self, idx: u16) -> bool {
        let byte = (idx >> 3) as usize;
        if byte < self.system_flags.len() {
            self.system_flags[byte] & (0x80u8 >> (idx & 7)) != 0
        } else {
            false
        }
    }

    /// Replace the field-VM bytecode buffer + reset PC. Engines call this
    /// when entering a new field scene (loading the scene's per-event
    /// script) to start interpretation from the beginning.
    pub fn load_field_script(&mut self, bytecode: Vec<u8>) {
        self.field_bytecode = bytecode;
        self.field_pc = 0;
        self.field_ctx = FieldCtx::default();
    }

    /// Load one event-script record into the field VM, skipping the leading
    /// `0xFFFF 0x0000` frame-divider sentinel when present.
    ///
    /// Records pulled from `scene_event_scripts` / `scene_scripted_asset_table`
    /// containers commonly open with the 4-byte sentinel; the field VM's
    /// dispatcher in retail consumes the sentinel as a record-start marker
    /// rather than an opcode (the high bit + low-7-bits 0x7F would otherwise
    /// hit the "UNFIND INDICATION" default arm). The exact dispatcher prelude
    /// hasn't been fully traced, so this skip is heuristic — revise once
    /// `FUN_801DE840`'s outer loop is captured.
    pub fn load_field_record(&mut self, record_bytes: &[u8]) {
        const FRAME_DIVIDER: [u8; 4] = [0xFF, 0xFF, 0x00, 0x00];
        let pc = if record_bytes.starts_with(&FRAME_DIVIDER) {
            4
        } else {
            0
        };
        self.field_bytecode = record_bytes.to_vec();
        self.field_pc = pc;
        self.field_ctx = FieldCtx::default();
    }

    /// Load a `Party` (per-character roster) into the world's actor table.
    ///
    /// Per-character record 0 maps to actor slot 0, record 1 to slot 1, …
    /// up to `party.len()` (capped by `MAX_ACTORS`). For each loaded slot
    /// the world:
    ///
    /// - activates the actor,
    /// - copies HP / MP from the record's [`HpMpSp`] block into the
    ///   `BattleActor` mirrors,
    /// - stows the full record bytes via [`World::roster`] for later
    ///   round-trip via [`World::save_party`].
    ///
    /// The `legaia-save` crate's [`legaia_save::CharacterRecord::parse`] is
    /// the lossless deserializer; this method is the runtime-side glue that
    /// projects the persistent record into the per-VM actor state.
    ///
    /// [`HpMpSp`]: legaia_save::HpMpSp
    pub fn load_party(&mut self, party: legaia_save::Party) {
        let n = party.members.len().min(self.actors.len());
        for (slot, rec) in party.members.iter().take(n).enumerate() {
            let hms = rec.hp_mp_sp();
            let a = &mut self.actors[slot];
            a.active = true;
            a.battle.hp = hms.hp_cur;
            a.battle.max_hp = hms.hp_max;
            a.battle.mp = hms.mp_cur;
            a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
        }
        self.party_count = n as u8;
        self.roster = party;
    }

    /// Capture the world's current actor state back into a `Party`. The
    /// roster bytes are returned verbatim except for the HP / MP / max-HP
    /// fields, which are resynced from the live `BattleActor` mirrors so
    /// in-battle damage / heals end up in the saved record.
    ///
    /// Round-trip: `world.load_party(p); world.save_party() == p` modulo
    /// the HP/MP resync (which is a no-op when no battle has run yet).
    pub fn save_party(&mut self) -> legaia_save::Party {
        for (slot, rec) in self
            .roster
            .members
            .iter_mut()
            .enumerate()
            .take(self.actors.len())
        {
            let mut hms = rec.hp_mp_sp();
            let a = &self.actors[slot];
            hms.hp_cur = a.battle.hp;
            hms.hp_max = a.battle.max_hp;
            hms.mp_cur = a.battle.mp;
            rec.set_hp_mp_sp(hms);
        }
        self.roster.clone()
    }

    /// Activate a slot and return a mutable reference to the actor.
    pub fn spawn_actor(&mut self, slot: usize) -> &mut Actor {
        let a = &mut self.actors[slot];
        a.active = true;
        a
    }

    /// Ensure the slot at `id` is initialized with the supplied default
    /// position and active. Idempotent.
    pub fn ensure_actor(&mut self, id: u8, default_pos: ActorVmPosition) -> &mut Actor {
        let a = &mut self.actors[id as usize];
        if !a.active {
            *a = Actor::new();
            a.active = true;
        }
        a.default_pos = default_pos;
        a
    }

    /// Run the actor VM bytecode against this world.
    ///
    /// Convenience wrapper around [`vm::run`] that constructs a host borrow.
    pub fn run_actor_bytecode(&mut self, bytecode: &[u8]) -> Result<usize, vm::VmError> {
        let mut host = ActorVmHostImpl { world: self };
        vm::run(&mut host, bytecode)
    }

    /// Step the move VM once for the actor at `slot`, using `bytecode` as
    /// the move buffer. Returns the [`vm::move_vm::StepResult`].
    ///
    /// Engines typically call this in a loop on each per-frame actor tick
    /// until the inner step returns `Halt` or `Wait`.
    ///
    /// Writes the host's `move_bytecode_write_u16` calls (issued by ext
    /// sub-ops 0x04 / 0x1B / 0x1E / 0x36) back to `world.move_bytecode[slot]`
    /// after step completes — see the `MoveVmHostImpl` deferred-writes map.
    pub fn step_move_vm(&mut self, slot: usize, bytecode: &[u16]) -> vm::move_vm::StepResult {
        let mut host = MoveVmHostImpl {
            world: self,
            current_slot: Some(slot),
            deferred_writes: std::collections::BTreeMap::new(),
        };
        let actor_state = unsafe {
            // SAFETY: the host borrows `world.actors[slot]` only through
            // queries that don't read this slot's `move_state`. The host
            // implementation never touches `actors[slot].move_state`; it
            // only reads sin/cos LUTs and other engine-side data.
            &mut *(&mut host.world.actors[slot].move_state as *mut MoveActorState)
        };
        let result = vm::move_vm::step(&mut host, actor_state, bytecode);
        let writes = std::mem::take(&mut host.deferred_writes);
        if !writes.is_empty()
            && let Some(buf) = self.move_bytecode.get_mut(slot)
        {
            for (off, value) in writes {
                if off >= buf.len() {
                    buf.resize(off + 1, 0);
                }
                buf[off] = value;
            }
        }
        result
    }

    /// Run one battle-action state-machine step.
    pub fn step_battle(&mut self) -> StepOutcome {
        let ctx_ptr: *mut BattleActionCtx = &mut self.battle_ctx;
        let mut host = BattleHostImpl { world: self };
        // SAFETY: BattleHostImpl never reads or writes `world.battle_ctx`
        // through the borrow; it only touches `actors`, helper tables, and
        // call-records.
        let ctx = unsafe { &mut *ctx_ptr };
        vm::battle_action::step(&mut host, ctx)
    }

    /// Tick the effect pool.
    pub fn tick_effects(&mut self) {
        let pool_ptr: *mut Pool = &mut self.effect_pool;
        let mut host = EffectHostImpl { world: self };
        // SAFETY: EffectHostImpl never reads `world.effect_pool` through
        // the borrow.
        let pool = unsafe { &mut *pool_ptr };
        pool.tick(&mut host);
    }

    /// Increment the deterministic LCG and return the new value.
    pub fn next_rng(&mut self) -> u32 {
        // Numerical Recipes LCG. Cheap, deterministic.
        self.rng_state = self
            .rng_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.rng_state
    }

    /// Per-frame world tick. Drives whichever scene-mode VMs are live.
    /// Returns the battle-step outcome when in [`SceneMode::Battle`], else
    /// `None`.
    ///
    /// Order of operations:
    ///  1. Effect pool tick — runs every frame regardless of mode.
    ///  2. Per-actor move-VM tick — only for actors with bytecode loaded.
    ///  3. Mode-specific VM:
    ///     - `Battle`     → battle-action state machine step.
    ///     - `Field`      → field-VM step (or no-op if no bytecode loaded).
    ///     - `Cutscene`   → field-VM step (cutscenes use the same script VM).
    ///     - `Title`      → no further VM.
    pub fn tick(&mut self) -> Option<StepOutcome> {
        self.frame += 1;
        self.tick_effects();
        self.tick_move_vms();
        match self.mode {
            SceneMode::Battle => Some(self.step_battle()),
            SceneMode::Field | SceneMode::Cutscene => {
                self.step_field();
                None
            }
            SceneMode::Title => None,
        }
    }

    /// Per-actor move-VM tick. Calls `step` once per active actor that has
    /// bytecode loaded. The retail equivalent runs in `FUN_80021DF4`
    /// (per-frame actor tick) and yields when `wait_timer >= 0`.
    pub fn tick_move_vms(&mut self) {
        for slot in 0..self.actors.len() {
            if !self.actors[slot].active {
                continue;
            }
            let bc = self.move_bytecode.get(slot).cloned().unwrap_or_default();
            if bc.is_empty() {
                continue;
            }
            // Decrement wait timer; if non-negative, skip.
            if self.actors[slot].move_state.wait_timer > 0 {
                self.actors[slot].move_state.wait_timer -= 1;
                continue;
            }
            let _ = self.step_move_vm(slot, &bc);
        }
    }

    /// One field-VM step. Drives `field_ctx` + `field_pc` from the loaded
    /// `field_bytecode`. No-op when no bytecode is loaded.
    pub fn step_field(&mut self) -> Option<FieldStepResult> {
        if self.field_bytecode.is_empty() {
            return None;
        }
        let ctx_ptr: *mut FieldCtx = &mut self.field_ctx;
        let bc_ptr: *const Vec<u8> = &self.field_bytecode;
        let pc = self.field_pc;
        let mut host = FieldHostImpl { world: self };
        // SAFETY: FieldHostImpl never borrows `world.field_ctx` or
        // `world.field_bytecode` through the borrow.
        let ctx = unsafe { &mut *ctx_ptr };
        let bc: &[u8] = unsafe { (*bc_ptr).as_slice() };
        let res = vm::field::step(&mut host, ctx, bc, pc);
        match &res {
            FieldStepResult::Advance { next_pc } => self.field_pc = *next_pc,
            FieldStepResult::Yield { resume_pc } => self.field_pc = *resume_pc,
            FieldStepResult::Halt { final_pc } => self.field_pc = *final_pc,
            FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                self.field_pc = *pc;
            }
        }
        Some(res)
    }
}

// --- actor VM host ---------------------------------------------------------

struct ActorVmHostImpl<'a> {
    world: &'a mut World,
}

impl<'a> ActorVmHost for ActorVmHostImpl<'a> {
    fn actor_exists(&self, actor_id: u8) -> bool {
        self.world
            .actors
            .get(actor_id as usize)
            .is_some_and(|a| a.active)
    }
    fn default_position(&self, actor_id: u8) -> ActorVmPosition {
        self.world
            .actors
            .get(actor_id as usize)
            .map(|a| a.default_pos)
            .unwrap_or_default()
    }
    fn spawn(&mut self, actor_id: u8, default_position: ActorVmPosition) {
        let a = &mut self.world.actors[actor_id as usize];
        if !a.active {
            *a = Actor::new();
            a.active = true;
        }
        a.default_pos = default_position;
        a.move_state.world_x = default_position.x;
        a.move_state.world_y = default_position.y;
    }
    fn set_position(&mut self, actor_id: u8, p: ActorVmPosition) {
        let a = &mut self.world.actors[actor_id as usize];
        a.move_state.world_x = p.x;
        a.move_state.world_y = p.y;
    }
    fn start_motion(&mut self, _actor_id: u8, _target: ActorVmPosition) {
        // Engines typically schedule a tween here; the world records nothing
        // by default.
    }
    fn delete_sprite(&mut self, actor_id: u8) {
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.active = false;
        }
    }
    fn global_update(&mut self) {
        // Tick whatever per-frame sprite-system state advances. The default
        // world has no global sprite ticker, but engines override this.
    }
    fn actor_effect(&mut self, actor_id: u8) {
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.last_effect = a.last_effect.wrapping_add(1);
        }
    }
    fn set_field_1d(&mut self, actor_id: u8, value: u8) {
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.field_1d = value;
        }
    }
    fn clear_field_20(&mut self, actor_id: u8) {
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.field_20 = 0;
        }
    }
    fn snap_clear_condition(&self, actor_id: u8) -> bool {
        self.world
            .actors
            .get(actor_id as usize)
            .map(|a| a.snap_clear)
            .unwrap_or(false)
    }
    fn motion_target(&self, actor_id: u8) -> Option<ActorVmPosition> {
        self.world
            .actors
            .get(actor_id as usize)
            .and_then(|a| a.motion_target)
    }
}

// --- move VM host ----------------------------------------------------------

struct MoveVmHostImpl<'a> {
    world: &'a mut World,
    /// Actor slot currently being stepped. Routes `move_bytecode_*` callbacks
    /// to the right `world.move_bytecode[slot]` buffer and the `*_slot_*`
    /// table reads to per-slot scratch (the shared 16-slot table is global,
    /// not per actor; this is unused there).
    current_slot: Option<usize>,
    /// Deferred bytecode writes accumulated during one `step` call. The VM
    /// borrows `world.move_bytecode[slot]` immutably as the bytecode slice;
    /// we can't write back through the same borrow, so the host buffers
    /// writes and `step_move_vm` flushes them after step returns.
    ///
    /// Reads consult this map first so an in-flight write within the same
    /// step (e.g. 0x1B copy loop reading from a freshly-mutated word) sees
    /// the latest value.
    deferred_writes: std::collections::BTreeMap<usize, u16>,
}

impl<'a> MoveHost for MoveVmHostImpl<'a> {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let idx = index as usize % self.world.sin_lut.len().max(1);
        let s = self.world.sin_lut.get(idx).copied().unwrap_or(0);
        let c = self.world.cos_lut.get(idx).copied().unwrap_or(0);
        (s, c)
    }
    fn keyframe_curve_multiplier(&self) -> u8 {
        // Default mirrors retail's startup-time write of `DAT_1F80037D`.
        0x10
    }

    // --- ext-VM globals -----------------------------------------------

    fn move_global_predicate_get(&self) -> u32 {
        self.world.move_predicate
    }
    fn move_global_predicate_set(&mut self, value: u32) {
        self.world.move_predicate = value;
    }
    fn move_global_counter_get(&self) -> u16 {
        self.world.move_counter
    }
    fn move_global_counter_set(&mut self, value: u16) {
        self.world.move_counter = value;
    }

    // --- ext-VM 16-slot scratch table ---------------------------------

    fn move_slot_load_u32(&self, slot: u16, dword_off: u8) -> u32 {
        let i = (slot & 0x0F) as usize;
        let off = (dword_off & 0x4) as usize; // 0 or 4
        let bytes = &self.world.move_slot_table[i][off..off + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
    fn move_slot_save_u32(&mut self, slot: u16, dword_off: u8, value: u32) {
        let i = (slot & 0x0F) as usize;
        let off = (dword_off & 0x4) as usize;
        self.world.move_slot_table[i][off..off + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn move_slot_load_u16(&self, slot: u16, byte_off: u8) -> u16 {
        let i = (slot & 0x0F) as usize;
        let off = (byte_off & 0x6) as usize; // even, 0..6
        let bytes = &self.world.move_slot_table[i][off..off + 2];
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
    fn move_slot_save_u16(&mut self, slot: u16, byte_off: u8, value: u16) {
        let i = (slot & 0x0F) as usize;
        let off = (byte_off & 0x6) as usize;
        self.world.move_slot_table[i][off..off + 2].copy_from_slice(&value.to_le_bytes());
    }

    // --- bytecode self-modify (0x04 / 0x1B / 0x1E) --------------------

    fn move_bytecode_read_u16(&self, word_off: usize) -> u16 {
        if let Some(&v) = self.deferred_writes.get(&word_off) {
            return v;
        }
        let Some(slot) = self.current_slot else {
            return 0;
        };
        self.world
            .move_bytecode
            .get(slot)
            .and_then(|bc| bc.get(word_off))
            .copied()
            .unwrap_or(0)
    }
    fn move_bytecode_write_u16(&mut self, word_off: usize, value: u16) {
        self.deferred_writes.insert(word_off, value);
    }

    // --- player / map-origin queries ----------------------------------

    fn move_player_world_xyz(&self) -> [i16; 3] {
        match self.world.player_actor_slot {
            Some(slot) => {
                let s = &self.world.actors[slot as usize].move_state;
                [s.world_x, s.world_y, s.world_z]
            }
            None => [0, 0, 0],
        }
    }
    fn move_fixed_origin_xz(&self) -> (i32, i32) {
        self.world.map_origin_xz
    }
    fn move_axis_threshold(&self) -> i16 {
        self.world.move_axis_threshold
    }
    fn move_dat_1f800393(&self) -> u8 {
        self.world.move_ramp_ratio
    }

    // --- shared system flag bank --------------------------------------

    fn ext_query_flag_bank(&self, flag_index: i16) -> u32 {
        if self.world.system_flag_test(flag_index as u16) {
            1
        } else {
            0
        }
    }
    fn ext_set_flag_bank(&mut self, flag_index: i16) {
        self.world.system_flag_set(flag_index as u16);
    }
    fn ext_clear_flag_bank(&mut self, flag_index: i16) {
        self.world.system_flag_clear(flag_index as u16);
    }

    // --- ext sub-op 0x29 scratchpad ramp ------------------------------

    fn ext_scratchpad_write(&mut self, slot_index: i16, value: i16) {
        let i = (slot_index as u16 & 0x0F) as usize;
        self.world.scratchpad_targets[i] = value;
    }
    fn ext_scratchpad_ramp(&mut self, slot_index: i16, target: i16, _ticks: i16) {
        // Default world has no per-frame ramp scheduler; record the target
        // immediately so reads see the final state. Engines override to
        // model the per-frame interpolation.
        let i = (slot_index as u16 & 0x0F) as usize;
        self.world.scratchpad_targets[i] = target;
    }

    // --- ext sub-op 0x2F global slot ---------------------------------

    fn ext_set_8007b9d8(&mut self, value: i32) {
        self.world.move_dat_8007b9d8 = value;
    }

    // --- ext sub-op 0x3A angle-to-player ------------------------------

    fn ext_compute_angle(&self, state: &MoveActorState) -> u16 {
        // Per the original: `func_0x80019B28(actor.world_z, actor.world_x,
        // player.world_z, player.world_x)`. Engines that don't model a
        // player slot get angle 0 (matching the no-player default).
        let Some(player_slot) = self.world.player_actor_slot else {
            return 0;
        };
        let player = &self.world.actors[player_slot as usize].move_state;
        // Atan2-style angle in PSX 12-bit units (4096 = full circle). The
        // original used a libgte angle helper; we use a portable
        // f32::atan2 then quantise. Direction convention matches the
        // original (Z first arg, X second).
        let dz = (player.world_z as i32 - state.world_z as i32) as f32;
        let dx = (player.world_x as i32 - state.world_x as i32) as f32;
        if dx == 0.0 && dz == 0.0 {
            return 0;
        }
        let theta = dz.atan2(dx);
        let units = (theta / std::f32::consts::TAU * 4096.0).round() as i32;
        (units & 0x0FFF) as u16
    }

    // --- ext sub-op 0x3B party-member position lookup ------------------

    fn ext_party_member_lookup(&self, slot: i16) -> Option<[i16; 3]> {
        let actor_slot = *self.world.party_actor_slots.get(slot as usize)?;
        let actor_slot = actor_slot? as usize;
        let st = &self.world.actors[actor_slot].move_state;
        Some([st.world_x, st.world_y, st.world_z])
    }

    // --- ext sub-op 0x3C fade colour -----------------------------------

    fn ext_fade_color(&mut self, rgb: [u8; 3], ticks: u16) {
        self.world.pending_fade = Some(FadeRequest { rgb, ticks });
    }

    // `ext_dispatch` uses the default trait impl, which routes through
    // `self` — so sub-op handlers see the world-backed callbacks above.
}

// --- effect VM host --------------------------------------------------------

struct EffectHostImpl<'a> {
    world: &'a mut World,
}

impl<'a> EffectHost for EffectHostImpl<'a> {
    fn next_random(&mut self) -> i32 {
        self.world.next_rng() as i32
    }
    fn advance_state(&mut self, _slot: usize, _master: &mut MasterSlot) -> StateOutcome {
        // Default world has no state-transition wiring; let the slot terminate
        // so the pool doesn't leak. Engines that wire sprites override this.
        StateOutcome::Terminate
    }
}

// --- field VM host ---------------------------------------------------------

struct FieldHostImpl<'a> {
    world: &'a mut World,
}

impl<'a> FieldHost for FieldHostImpl<'a> {
    fn global_flags(&self) -> u32 {
        self.world.story_flags
    }
    fn set_global_flags(&mut self, value: u32) {
        self.world.story_flags = value;
    }
    fn frame_delta(&self) -> u16 {
        // Default world ticks one logical frame per `tick()`. Engines that
        // run faster-than-frame can override this on a custom host wrapper.
        1
    }
    fn extra_flags(&self) -> u32 {
        self.world.extra_flags
    }
    fn screen_mode(&self) -> u32 {
        self.world.screen_mode
    }

    // Shared system flag bank — same fourth-flag-bank at `_DAT_80086D70`
    // that move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D query, plus the
    // 0x5x / 0x6x / 0x7x default-route opcodes.
    fn system_flag_set(&mut self, idx: u16) {
        self.world.system_flag_set(idx);
    }
    fn system_flag_clear(&mut self, idx: u16) {
        self.world.system_flag_clear(idx);
    }
    fn system_flag_test(&self, idx: u16) -> bool {
        self.world.system_flag_test(idx)
    }
    fn scene_transition(&mut self, map_id: u8) {
        // Record the request; SceneHost::tick drains it after the field
        // step returns so the bytecode swap doesn't invalidate the
        // borrow we're stepping through.
        self.world.pending_scene_transition = Some(map_id);
    }

    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        // Sub-ops 1 (start field BGM) and 9 (queue) are the cases that
        // pin a "currently playing" id. Other sub-ops are control words
        // (pause / stop / volume / etc.) — we still surface the event so
        // the engine can route them, just without overwriting current_bgm.
        if sub_op == 1 || sub_op == 9 {
            self.world.current_bgm = Some(text_id);
        } else if sub_op == 4 {
            // 4 = stop.
            self.world.current_bgm = None;
        }
        self.world
            .pending_field_events
            .push(FieldEvent::Bgm { text_id, sub_op });
    }

    fn play_sfx(&mut self, sfx_id: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::PlaySfx { sfx_id });
    }

    fn open_dialog(
        &mut self,
        text_id: u16,
        inline: &[u8],
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    ) {
        let inline_vec = inline.to_vec();
        self.world.current_dialog = Some(DialogRequest {
            text_id,
            inline: inline_vec.clone(),
            world_x,
            world_z,
            depth_id,
        });
        self.world
            .pending_field_events
            .push(FieldEvent::OpenDialog {
                text_id,
                inline: inline_vec,
                world_x,
                world_z,
                depth_id,
            });
    }

    fn add_money(&mut self, delta: i32) {
        let new_total = (self.world.money as i64 + delta as i64).clamp(0, 9_999_999) as i32;
        self.world.money = new_total;
        self.world
            .pending_field_events
            .push(FieldEvent::AddMoney { delta });
    }

    fn set_item_count(&mut self, slot_byte: u8, count: u8) {
        if count == 0 {
            self.world.inventory.remove(&slot_byte);
        } else {
            self.world.inventory.insert(slot_byte, count);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SetItemCount { slot_byte, count });
    }

    fn party_add(&mut self, char_id: u8) -> bool {
        // The retail engine maintains a sorted insertion in
        // `_DAT_80084598..` (cap 4) and writes the leader slot when the
        // party transitions from empty. We mirror that with
        // `party_actor_slots` + `party_leader_slot`.
        let already_present = self
            .world
            .party_actor_slots
            .iter()
            .any(|s| matches!(s, Some(id) if *id == char_id));
        let accepted = if already_present {
            false
        } else if self.world.party_actor_slots.len() < 4 {
            self.world.party_actor_slots.push(Some(char_id));
            // First member also becomes the leader (matches retail's
            // `count == 0` arm).
            if self.world.party_leader_slot.is_none() {
                self.world.party_leader_slot = Some(char_id);
            }
            true
        } else {
            false
        };
        self.world
            .pending_field_events
            .push(FieldEvent::PartyAdd { char_id, accepted });
        accepted
    }

    fn party_remove(&mut self, char_id: u8) {
        self.world
            .party_actor_slots
            .retain(|s| !matches!(s, Some(id) if *id == char_id));
        if matches!(self.world.party_leader_slot, Some(id) if id == char_id) {
            // Promote next member or clear.
            self.world.party_leader_slot = self.world.party_actor_slots.first().copied().flatten();
        }
        self.world
            .pending_field_events
            .push(FieldEvent::PartyRemove { char_id });
    }

    fn field_interact(&mut self, interact_id: u8, slot: u8) {
        self.world.last_field_interact = Some((interact_id, slot));
        self.world
            .pending_field_events
            .push(FieldEvent::FieldInteract { interact_id, slot });
    }

    fn render_cfg_long(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::RenderCfgLong { b1, b2, b3, b4 });
    }

    fn render_cfg_short(&mut self, r: u8, g: u8, b: u8, packed: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::RenderCfgShort { r, g, b, packed });
    }

    fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::SceneRegisterWrite {
                slot_10,
                slot_12,
                slot_14,
            });
    }

    fn counter_update(&mut self, op0: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::CounterUpdate { op0 });
    }

    fn setup_animation(&mut self, _ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
        self.world
            .pending_field_events
            .push(FieldEvent::SetupAnimation {
                count,
                base_id,
                frames: frames.to_vec(),
            });
    }

    fn set_party_leader(&mut self, leader_id: u8) {
        self.world.party_leader_slot = Some(leader_id);
        self.world
            .pending_field_events
            .push(FieldEvent::SetPartyLeader { leader_id });
    }

    fn camera_configure(&mut self, params: &[CameraParam], apply_trigger: u16, mode: u8) {
        self.world.camera_state.params = params.to_vec();
        self.world.camera_state.apply_trigger = apply_trigger;
        self.world.camera_state.mode = mode;
        self.world
            .pending_field_events
            .push(FieldEvent::CameraConfigure {
                params: params.to_vec(),
                apply_trigger,
                mode,
            });
    }

    fn camera_load(&mut self, payload: &[u8]) {
        self.world.camera_state.loaded_payload = payload.to_vec();
        self.world
            .pending_field_events
            .push(FieldEvent::CameraLoad {
                payload: payload.to_vec(),
            });
    }

    fn camera_save(&mut self) {
        // Snapshot what we have currently — engines that model real camera
        // matrices can override this on a custom host wrapper. For now we
        // write a placeholder so save/load round-trip behaves.
        self.world.camera_state.saved = self.world.camera_state.loaded_payload.clone();
        self.world.pending_field_events.push(FieldEvent::CameraSave);
    }

    fn camera_apply(&mut self) {
        self.world
            .pending_field_events
            .push(FieldEvent::CameraApply);
    }

    fn scene_fade(&mut self, op0_word: u16, op1_word: u16) -> SceneFadeResult {
        self.world
            .pending_field_events
            .push(FieldEvent::SceneFade { op0_word, op1_word });
        SceneFadeResult::Done
    }

    fn effect_anim_trigger(&mut self, _ctx: &mut FieldCtx, arg: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::EffectAnimTrigger { arg });
    }

    fn menu_ctrl_sub1(&mut self, op0: u8, payload: &[u8; 5]) {
        self.world.pending_field_events.push(FieldEvent::MenuCtrl {
            op0,
            payload: *payload,
        });
    }

    fn menu_refresh(&mut self) {
        self.world
            .pending_field_events
            .push(FieldEvent::MenuRefresh);
    }

    fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
        // Player path: also propagate to the active actor slot's
        // move_state so the renderer / collision layer sees the teleport.
        if is_player
            && let Some(slot) = self.world.player_actor_slot
            && let Some(actor) = self.world.actors.get_mut(slot as usize)
        {
            actor.move_state.world_x = world_x as i16;
            actor.move_state.world_z = world_z as i16;
        }
        let _ = ctx;
        self.world.pending_field_events.push(FieldEvent::MoveTo {
            world_x,
            world_z,
            is_player,
        });
    }

    fn exec_move(&mut self, _ctx: &mut FieldCtx, move_id: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::ExecMove { move_id });
    }
}

// --- battle action host ----------------------------------------------------

struct BattleHostImpl<'a> {
    world: &'a mut World,
}

impl<'a> BattleActionHost for BattleHostImpl<'a> {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.world.actors.get(slot as usize).map(|a| &a.battle)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.world
            .actors
            .get_mut(slot as usize)
            .map(|a| &mut a.battle)
    }
    fn rng(&mut self) -> u32 {
        self.world.next_rng()
    }
    fn previous_action_cleared(&self, _: u8) -> bool {
        self.world.prev_action_cleared
    }
    fn sound_bank_ready(&self, _: u8) -> bool {
        self.world.sound_bank_ready
    }
    fn is_capture_spell(&self, id: u8) -> bool {
        self.world.capture_spells.contains(&id)
    }
    fn spell_mp_cost(&self, id: u8) -> u8 {
        self.world.spell_costs.get(&id).copied().unwrap_or(0)
    }
    fn character_ability_bits(&self, slot: u8) -> u32 {
        let i = slot as usize;
        self.world
            .character_ability_bits
            .get(i)
            .copied()
            .unwrap_or(0)
    }
    fn range_check(&self, attacker: u8, target: u8) -> u16 {
        self.world
            .range_table
            .get(&(attacker, target))
            .copied()
            .unwrap_or(0)
    }
    fn battle_end(&mut self, cause: BattleEndCause) {
        self.world.battle_end = Some(cause);
    }
    fn party_count(&self) -> u8 {
        self.world.party_count
    }
    fn pose(&mut self, _actor_id: u8, _pose: Pose) {
        // Engines hook pose changes; default world records nothing.
    }
}

// --- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use vm::Insn;

    #[test]
    fn world_starts_with_inactive_actors() {
        let world = World::new();
        assert_eq!(world.actors.len(), MAX_ACTORS);
        assert!(world.actors.iter().all(|a| !a.active));
    }

    #[test]
    fn actor_vm_spawn_default_runs_through_world() {
        let mut world = World::new();
        // Pre-set default position for actor 7.
        world.actors[7].default_pos = ActorVmPosition::new(100, 50);
        // Bytecode: SpawnDefault actor 7, then End.
        let bc = {
            let mut v = vec![];
            v.extend_from_slice(
                &Insn {
                    opcode: 0x01,
                    operand_b: 7,
                    operand_w: 0,
                }
                .encode(),
            );
            v.extend_from_slice(&[0u8; 4]);
            v
        };
        let pc = world.run_actor_bytecode(&bc).unwrap();
        assert_eq!(pc, 4);
        assert!(world.actors[7].active);
        assert_eq!(world.actors[7].move_state.world_x, 100);
    }

    #[test]
    fn actor_vm_set_field_1d_writes_when_actor_exists() {
        let mut world = World::new();
        world.actors[3].active = true;
        let bc = {
            let mut v = vec![];
            v.extend_from_slice(
                &Insn {
                    opcode: 0x03,
                    operand_b: 3,
                    operand_w: 0xFF42,
                }
                .encode(),
            );
            v.extend_from_slice(&[0u8; 4]);
            v
        };
        world.run_actor_bytecode(&bc).unwrap();
        assert_eq!(world.actors[3].field_1d, 0x42);
    }

    #[test]
    fn move_vm_step_writes_world_state() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Bytecode: WORLD_SET (op 0x07) x=100, y=50, z=10, then HALT.
        let bc: Vec<u16> = vec![0x0007, 100, 50, 10, 0x0008];
        let res = world.step_move_vm(0, &bc);
        // First step is WORLD_SET (Advance), then we'd need to call again for HALT.
        assert!(matches!(res, vm::move_vm::StepResult::Advance));
        assert_eq!(world.actors[0].move_state.world_x, 100);
        assert_eq!(world.actors[0].move_state.world_y, 50);
    }

    #[test]
    fn world_tick_in_battle_mode_runs_state_machine() {
        let mut world = World::new();
        world.mode = SceneMode::Battle;
        // Mark all actors alive so end-of-action doesn't immediately wipe.
        for a in &mut world.actors {
            a.battle.liveness = 1;
        }
        world.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        world.battle_ctx.queued_action = 5;
        let out = world.tick();
        assert!(matches!(out, Some(StepOutcome::Transition { .. })));
        assert_eq!(
            world.battle_ctx.action_state,
            vm::battle_action::ActionState::PreActionWait.as_byte()
        );
    }

    #[test]
    fn world_tick_in_title_mode_returns_none() {
        let mut world = World::new();
        world.mode = SceneMode::Title;
        let out = world.tick();
        assert!(out.is_none());
        assert_eq!(world.frame, 1);
    }

    #[test]
    fn next_rng_is_deterministic() {
        let mut a = World::new();
        let mut b = World::new();
        let seq_a: Vec<_> = (0..10).map(|_| a.next_rng()).collect();
        let seq_b: Vec<_> = (0..10).map(|_| b.next_rng()).collect();
        assert_eq!(seq_a, seq_b);
        // And not all zero.
        assert!(seq_a.iter().any(|&x| x != 0));
    }

    #[test]
    fn battle_party_wipe_signals_end_via_world() {
        let mut world = World::new();
        world.mode = SceneMode::Battle;
        // Kill all party.
        for i in 0..3 {
            world.actors[i].battle.liveness = 0;
        }
        // Mark monsters alive.
        for i in 3..8 {
            world.actors[i].battle.liveness = 1;
        }
        world.battle_ctx.action_state = vm::battle_action::ActionState::EndOfAction.as_byte();
        let out = world.tick();
        assert_eq!(out, Some(StepOutcome::BattleComplete));
        assert_eq!(world.battle_end, Some(BattleEndCause::PartyWipe));
    }

    #[test]
    fn ensure_actor_is_idempotent_and_writes_default_pos() {
        let mut world = World::new();
        world.ensure_actor(2, ActorVmPosition::new(7, 11));
        assert!(world.actors[2].active);
        assert_eq!(world.actors[2].default_pos, ActorVmPosition::new(7, 11));
        // Calling again with new pos updates it but doesn't reset the actor.
        world.actors[2].field_1d = 0xAB;
        world.ensure_actor(2, ActorVmPosition::new(13, 17));
        assert_eq!(world.actors[2].default_pos, ActorVmPosition::new(13, 17));
        assert_eq!(world.actors[2].field_1d, 0xAB);
    }

    #[test]
    fn effect_pool_tick_terminates_default_slots() {
        let mut world = World::new();
        // Mark slot 0 active by setting child_count > 0 so the tick walker
        // visits it.
        world.effect_pool.master_slots[0].child_count = 4;
        world.tick_effects();
        // Default advance_state returns Terminate → slot zeroes out.
        assert_eq!(world.effect_pool.master_slots[0].child_count, 0);
    }

    #[test]
    fn world_tick_in_field_mode_steps_field_vm() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // Bytecode: 0x37 YIELD. Should set ctx.flags |= 0x400 + advance PC
        // past the yield.
        world.load_field_script(vec![0x37, 0x00]);
        let _ = world.tick();
        assert_eq!(world.field_ctx.flags & 0x400, 0x400, "halt bit set");
        assert!(
            world.field_pc > 0,
            "field_pc should advance after yield, got {}",
            world.field_pc
        );
    }

    #[test]
    fn world_tick_field_mode_no_bytecode_is_noop() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // No bytecode loaded. Tick should not panic and should not advance
        // field_pc.
        let _ = world.tick();
        assert_eq!(world.field_pc, 0);
    }

    #[test]
    fn world_tick_drives_per_actor_move_vm() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.actors[0].active = true;
        // Move-VM bytecode: WORLD_SET (op 0x07) x=42, y=10, z=5, then HALT.
        world.set_move_bytecode(0, Some(vec![0x0007, 42, 10, 5, 0x0008]));
        let _ = world.tick();
        // First step is WORLD_SET; should write the position.
        assert_eq!(world.actors[0].move_state.world_x, 42);
        assert_eq!(world.actors[0].move_state.world_y, 10);
    }

    #[test]
    fn world_tick_skips_move_vm_when_wait_timer_set() {
        let mut world = World::new();
        world.actors[0].active = true;
        world.actors[0].move_state.wait_timer = 5;
        world.set_move_bytecode(0, Some(vec![0x0007, 99, 99, 99, 0x0008]));
        let _ = world.tick();
        // Wait timer decremented, but move VM didn't run -> position unchanged.
        assert_eq!(world.actors[0].move_state.wait_timer, 4);
        assert_eq!(world.actors[0].move_state.world_x, 0);
    }

    #[test]
    fn load_field_script_resets_pc_and_ctx() {
        let mut world = World::new();
        world.field_pc = 42;
        world.field_ctx.flags = 0xFFFF;
        world.load_field_script(vec![0xFF; 8]);
        assert_eq!(world.field_pc, 0);
        assert_eq!(world.field_ctx.flags, 0);
        assert_eq!(world.field_bytecode.len(), 8);
    }

    #[test]
    fn load_field_record_skips_frame_divider_sentinel() {
        let mut world = World::new();
        // Record opens with FFFF 0000 frame divider.
        let record = vec![0xFF, 0xFF, 0x00, 0x00, 0x37, 0x00];
        world.load_field_record(&record);
        assert_eq!(world.field_pc, 4, "frame divider should bump pc to 4");
        assert_eq!(world.field_bytecode.len(), 6);
    }

    #[test]
    fn load_field_record_without_sentinel_starts_at_zero() {
        let mut world = World::new();
        let record = vec![0x37, 0x00];
        world.load_field_record(&record);
        assert_eq!(world.field_pc, 0);
    }

    /// Field VM op 0x3E with `op0 >= 100` is the scene-transition arm
    /// (`map_id = op0 - 100`). The world's `FieldHostImpl` records the
    /// request in `pending_scene_transition` for `SceneHost::tick` to
    /// drain on the next frame boundary.
    #[test]
    fn field_scene_transition_writes_pending_map_id() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // Bytecode: opcode 0x3E, op0 = 105 (map_id 5), then 4 padding
        // bytes (op0 + 4 trailing operand bytes per the dispatcher math).
        let bytecode = vec![0x3E, 105, 0, 0, 0, 0];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.pending_scene_transition, Some(5));
    }

    /// `op0 < 100` is the field_interact arm — should NOT trigger a
    /// scene transition.
    #[test]
    fn field_op_3e_low_op0_does_not_request_scene_transition() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let bytecode = vec![0x3E, 50, 7];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.pending_scene_transition, None);
    }

    // --- Save / load round-trip ----------------------------------------

    #[test]
    fn load_party_populates_battle_actor_hp_mp() {
        let mut party = legaia_save::Party::zeroed(3);
        let mut hms = party.members[0].hp_mp_sp();
        hms.hp_cur = 137;
        hms.hp_max = 150;
        hms.mp_cur = 42;
        party.members[0].set_hp_mp_sp(hms);
        let mut hms1 = party.members[1].hp_mp_sp();
        hms1.hp_cur = 0; // dead member
        hms1.hp_max = 100;
        party.members[1].set_hp_mp_sp(hms1);

        let mut world = World::new();
        world.load_party(party);

        assert!(world.actors[0].active);
        assert_eq!(world.actors[0].battle.hp, 137);
        assert_eq!(world.actors[0].battle.max_hp, 150);
        assert_eq!(world.actors[0].battle.mp, 42);
        assert_eq!(world.actors[0].battle.liveness, 1);
        // Dead member: liveness flipped to 0.
        assert_eq!(world.actors[1].battle.liveness, 0);
        assert_eq!(world.party_count, 3);
    }

    #[test]
    fn save_party_round_trips_after_load() {
        let mut party = legaia_save::Party::zeroed(3);
        let mut hms = party.members[0].hp_mp_sp();
        hms.hp_cur = 200;
        hms.hp_max = 250;
        hms.mp_cur = 100;
        party.members[0].set_hp_mp_sp(hms);

        let original_bytes = party.write();

        let mut world = World::new();
        world.load_party(party);
        let saved = world.save_party();

        assert_eq!(saved.write(), original_bytes);
    }

    #[test]
    fn save_party_picks_up_in_battle_hp_changes() {
        let mut party = legaia_save::Party::zeroed(2);
        let mut hms = party.members[0].hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        party.members[0].set_hp_mp_sp(hms);

        let mut world = World::new();
        world.load_party(party);
        // Simulate damage during battle.
        world.actors[0].battle.hp = 25;

        let saved = world.save_party();
        assert_eq!(saved.members[0].hp_mp_sp().hp_cur, 25);
        // Max HP unchanged.
        assert_eq!(saved.members[0].hp_mp_sp().hp_max, 100);
    }

    #[test]
    fn load_party_caps_at_max_actors() {
        let many = legaia_save::Party::zeroed(MAX_ACTORS + 10);
        let mut world = World::new();
        world.load_party(many);
        assert_eq!(world.party_count, MAX_ACTORS as u8);
    }

    #[test]
    fn effect_pool_tick_decrements_state_byte() {
        let mut world = World::new();
        world.effect_pool.master_slots[0].child_count = 4;
        // state >= 8 → write back state - 8 and skip.
        world.effect_pool.master_slots[0].state = 12;
        world.tick_effects();
        assert_eq!(world.effect_pool.master_slots[0].state, 4);
        // Slot still active.
        assert_eq!(world.effect_pool.master_slots[0].child_count, 4);
    }

    // --- move-VM host wiring (round 5) ------------------------------------

    #[test]
    fn move_vm_global_predicate_round_trips_through_world() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Move bytecode: 0x2F sub-op 0x08 (set predicate to 1), then HALT.
        world.set_move_bytecode(0, Some(vec![0x002F, 0x0008, 0x0008]));
        let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
        assert_eq!(
            world.move_predicate, 1,
            "ext sub-op 0x08 should set move_predicate to 1"
        );
    }

    #[test]
    fn move_vm_global_counter_set_and_get() {
        let mut world = World::new();
        world.actors[0].active = true;
        // 0x2F sub-op 0x0F clears counter, then HALT.
        world.move_counter = 5;
        world.set_move_bytecode(0, Some(vec![0x002F, 0x000F, 0x0008]));
        let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
        assert_eq!(world.move_counter, 0);
    }

    #[test]
    fn move_vm_slot_table_save_and_load_round_trip() {
        let mut world = World::new();
        world.actors[0].active = true;
        world.actors[0].move_state.world_x = 0x1234u16 as i16;
        world.actors[0].move_state.world_y = 0x5678u16 as i16;
        world.actors[0].move_state.world_z = 0x9ABCu16 as i16;
        world.actors[0].move_state.world_y_mirror = 0xDEF0u16 as i16;
        world.actors[0].move_state.field_86 = 0x0003; // slot index = 3
        // 0x2F sub-op 0x11 — save world coords into slot 3, then HALT.
        world.set_move_bytecode(0, Some(vec![0x002F, 0x0011, 0x0008]));
        let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
        // Verify the bytes landed in slot 3.
        let lo = u32::from_le_bytes(world.move_slot_table[3][0..4].try_into().unwrap());
        let hi = u32::from_le_bytes(world.move_slot_table[3][4..8].try_into().unwrap());
        assert_eq!(lo & 0xFFFF, 0x1234);
        assert_eq!((lo >> 16) & 0xFFFF, 0x5678);
        assert_eq!(hi & 0xFFFF, 0x9ABC);
        assert_eq!((hi >> 16) & 0xFFFF, 0xDEF0);
    }

    #[test]
    fn move_vm_bytecode_write_persists_after_step() {
        let mut world = World::new();
        world.actors[0].active = true;
        world.actors[0].move_state.world_x = 100;
        world.actors[0].move_state.world_y = 200;
        world.actors[0].move_state.world_z = 50;
        // 0x2F sub-op 0x04 — write actor world XYZ to bytecode at
        // pc + op[2] + 3. With pc=0 and op[2]=2, target indices are 5/6/7.
        let bc = vec![
            0x002F, 0x0004, 0x0002, 0xCAFE, 0xCAFE, 0x0000, 0x0000, 0x0000,
        ];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // After step, the world's stored bytecode should reflect the writes.
        assert_eq!(world.move_bytecode[0][5], 100u16);
        assert_eq!(world.move_bytecode[0][6], 200u16);
        assert_eq!(world.move_bytecode[0][7], 50u16);
    }

    #[test]
    fn move_vm_bytecode_inplace_add_sees_prior_step_writes() {
        // 0x2F sub-op 0x1E does buffer[pc + op[2] + 4] += op[3].
        // After two consecutive steps each adding 5, the slot should hold 10
        // (proving the world flushes deferred writes between steps).
        let mut world = World::new();
        world.actors[0].active = true;
        // Two 0x1E ops back-to-back, each pointing at the same operand slot.
        // Each op is size 1 (default_arm), so we step it twice.
        // Slot 4 from instruction at pc=0 lands at index 4.
        let bc = vec![0x002F, 0x001E, 0, 5, 0]; // op[2]=0, op[3]=5
        world.set_move_bytecode(0, Some(bc.clone()));
        // First step: bytecode[0 + 0 + 4] (= 0) += 5 → 5.
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.move_bytecode[0][4], 5);
        // Step again with a fresh-cloned bytecode read of the world's buffer.
        let bc2 = world.move_bytecode[0].clone();
        // PC has advanced; reset for the same op to fire again.
        world.actors[0].move_state.pc = 0;
        let _ = world.step_move_vm(0, &bc2);
        assert_eq!(
            world.move_bytecode[0][4], 10,
            "second 0x1E should see flushed write from first step"
        );
    }

    // --- system flag bank (round 6) -------------------------------------

    #[test]
    fn system_flag_set_and_test_round_trips_through_world() {
        let mut world = World::new();
        world.system_flag_set(0);
        world.system_flag_set(7);
        world.system_flag_set(15);
        world.system_flag_set(255);
        assert!(world.system_flag_test(0));
        assert!(world.system_flag_test(7));
        assert!(world.system_flag_test(15));
        assert!(world.system_flag_test(255));
        assert!(!world.system_flag_test(1));
        assert!(!world.system_flag_test(254));
        // Out-of-bounds idx returns false.
        assert!(!world.system_flag_test(256));
        assert!(!world.system_flag_test(0xFFFF));
    }

    #[test]
    fn system_flag_clear_only_touches_target_bit() {
        let mut world = World::new();
        world.system_flag_set(3);
        world.system_flag_set(4);
        world.system_flag_clear(3);
        assert!(!world.system_flag_test(3));
        assert!(world.system_flag_test(4));
    }

    #[test]
    fn move_vm_ext_query_flag_bank_reads_world_system_flags() {
        let mut world = World::new();
        world.actors[0].active = true;
        world.system_flag_set(42);
        // Bytecode: 0x2F sub-op 0x13 — predicate-true → default_arm (size 1),
        // predicate-false → size 4.
        let bc = vec![0x002F, 0x0013, 42];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // Predicate true → PC advanced by 1.
        assert_eq!(world.actors[0].move_state.pc, 1);
        // Now clear and re-run — predicate false → PC += 4.
        world.system_flag_clear(42);
        world.actors[0].move_state.pc = 0;
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.actors[0].move_state.pc, 4);
    }

    #[test]
    fn move_vm_ext_set_flag_bank_writes_world_system_flags() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Bytecode: 0x2F sub-op 0x1C — set flag bank (idx = op_w(2)).
        let bc = vec![0x002F, 0x001C, 100];
        world.set_move_bytecode(0, Some(bc.clone()));
        assert!(!world.system_flag_test(100));
        let _ = world.step_move_vm(0, &bc);
        assert!(world.system_flag_test(100));
    }

    #[test]
    fn field_vm_system_flag_set_routes_to_world() {
        // Field-VM 0x5x default-route SET — `[0x50 | nibble, idx_byte]`.
        // idx encoding: `((opcode_byte & 0x8F) << 8) | idx_byte`. For raw
        // opcode 0x50, top bit clear, low nibble 0 → idx = idx_byte.
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.load_field_script(vec![0x50, 42]);
        let _ = world.tick();
        assert!(
            world.system_flag_test(42),
            "0x50 default-route should set system flag 42"
        );
    }

    #[test]
    fn field_vm_system_flag_set_with_low_nibble_includes_high_byte() {
        // 0x52 with low-nibble 2 → idx = (0x02 << 8) | idx_byte.
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.load_field_script(vec![0x52, 7]);
        let _ = world.tick();
        assert!(
            world.system_flag_test(0x0207),
            "0x52 default-route should set system flag 0x0207"
        );
    }

    #[test]
    fn field_vm_system_flag_clear_routes_to_world() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.system_flag_set(99);
        // 0x60 CLEAR with operand 99.
        world.load_field_script(vec![0x60, 99]);
        let _ = world.tick();
        assert!(!world.system_flag_test(99));
    }

    #[test]
    fn field_vm_system_flag_test_takes_jump_when_bit_set() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.system_flag_set(33);
        // 0x70 TEST with idx=33, jump delta = 10.
        world.load_field_script(vec![0x70, 33, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let _ = world.tick();
        // pc was 0; header_size = 1; +1 (idx byte) + delta(10) = 12.
        assert_eq!(world.field_pc, 12);
    }

    #[test]
    fn field_vm_extra_flags_op42_reads_world() {
        // Op 0x42 mode=0 — host.extra_flags() & (1 << (op1 & 0x1F)) test.
        // Set bit 5 in extra_flags; op_42 with op1=5 should take the jump.
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.extra_flags = 1 << 5;
        // [0x42, mode=0, op1=5, lo=4, hi=0] — header_size + 4 = 5 byte total
        // for skip path; jump path = pc + header_size + 2 + delta.
        world.load_field_script(vec![0x42, 0, 5, 4, 0]);
        let _ = world.tick();
        // With extra_flags bit 5 set, predicate is true → jump.
        // Jump target = 0 + 1 (header) + 2 + 4 = 7.
        assert_eq!(world.field_pc, 7, "extra_flags-true 0x42 should take jump");
    }

    #[test]
    fn move_vm_ext_set_8007b9d8_writes_world_field() {
        let mut world = World::new();
        world.actors[0].active = true;
        // 0x2F sub-op 0x2F — `_DAT_8007B9D8 = (i32) op[1]`. Note: op[1] in
        // sub-op space = sub-op selector 0x2F itself, op[2] = the value.
        // Per the move_vm port, ext sub-op 0x2F passes op[1] (the sub-op
        // word's "next slot" in the operand stream).
        let bc = vec![0x002F, 0x002F, 0xCAFE];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // Whatever the sub-op handler writes, world.move_dat_8007b9d8 should
        // pick up a non-zero value.
        assert_ne!(world.move_dat_8007b9d8, 0);
    }

    #[test]
    fn ext_compute_angle_matches_quadrant_when_player_set() {
        // Place actor at origin, player due-east; angle should be ~0 mod 4096
        // (positive X direction = angle 0 in the dz.atan2(dx) convention).
        let mut world = World::new();
        world.actors[0].active = true;
        world.actors[0].move_state.world_x = 0;
        world.actors[0].move_state.world_z = 0;
        world.actors[1].active = true;
        world.actors[1].move_state.world_x = 100;
        world.actors[1].move_state.world_z = 0;
        world.player_actor_slot = Some(1);
        // Drive ext sub-op 0x3A: VM writes the angle into bytecode at
        // `state.pc + op_w(2) + 3`. With pc=0 and op_w(2)=0, dst = u16[3].
        let bc = vec![0x002F, 0x003A, 0, 0xFFFF, 0xFFFF];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // angle 0 (player due-east) should produce ~0 in the dst slot.
        assert_eq!(
            world.move_bytecode[0][3], 0,
            "angle to due-east player should be 0"
        );
    }

    #[test]
    fn ext_compute_angle_returns_zero_when_no_player() {
        // No player slot designated → ext_compute_angle returns 0.
        let mut world = World::new();
        world.actors[0].active = true;
        let bc = vec![0x002F, 0x003A, 0, 0xFFFF];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.move_bytecode[0][3], 0);
    }

    #[test]
    fn ext_party_member_lookup_returns_table_position() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Party member at index 1 = world actor slot 5 with a known position.
        world.actors[5].active = true;
        world.actors[5].move_state.world_x = 100;
        world.actors[5].move_state.world_y = 50;
        world.actors[5].move_state.world_z = 200;
        world.party_actor_slots = vec![None, Some(5), None];
        // Sub-op 0x3B: dst = pc + op_w(3) + 4. We use op_w(2)=1 (party slot 1)
        // and op_w(3)=0 so dst = u16[4..7].
        let bc = vec![
            0x002F, 0x003B, 0x0001, 0x0000, 0xAAAA, 0xAAAA, 0xAAAA, 0xAAAA,
        ];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.move_bytecode[0][4], 100u16);
        assert_eq!(world.move_bytecode[0][5], 50u16);
        assert_eq!(world.move_bytecode[0][6], 200u16);
    }

    #[test]
    fn ext_party_member_lookup_skips_when_none() {
        // No party table entry → 0x3B returns size-4 (skip), pre-clears dst.
        let mut world = World::new();
        world.actors[0].active = true;
        let bc = vec![0x002F, 0x003B, 0x0000, 0x0000, 0xAAAA, 0xAAAA, 0xAAAA];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // Dst slots pre-cleared even when lookup returns None.
        assert_eq!(world.move_bytecode[0][4], 0);
        assert_eq!(world.move_bytecode[0][5], 0);
        assert_eq!(world.move_bytecode[0][6], 0);
    }

    #[test]
    fn ext_fade_color_records_pending_request() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Sub-op 0x3C: r=0xAB, g=0xCD, b=0xEF, ticks=4 (ramp).
        let bc = vec![0x002F, 0x003C, 0x00AB, 0x00CD, 0x00EF, 0x0004];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(
            world.pending_fade,
            Some(FadeRequest {
                rgb: [0xAB, 0xCD, 0xEF],
                ticks: 4
            })
        );
    }

    #[test]
    fn move_player_world_xyz_reads_designated_player_slot() {
        let mut world = World::new();
        world.actors[2].active = true;
        world.actors[2].move_state.world_x = 100;
        world.actors[2].move_state.world_y = 200;
        world.actors[2].move_state.world_z = 300;
        world.player_actor_slot = Some(2);
        // No direct API to read move_player_world_xyz; verify by stepping
        // sub-op 0x39 (squared-distance "inside radius" predicate). With
        // actor 0 at origin and player at (100, _, 300), dist_sq = 100²+300² =
        // 100000 — predicate fails for r=10 (r² = 100), passes for r=400
        // (r² = 160000).
        world.actors[0].active = true;
        // Predicate fail → PC += 4.
        let bc = vec![0x002F, 0x0039, 10, 0, 0, 0];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(
            world.actors[0].move_state.pc, 4,
            "small-radius 0x39 should fail"
        );
        // Predicate pass → PC += 1.
        world.actors[0].move_state.pc = 0;
        let bc2 = vec![0x002F, 0x0039, 400, 0, 0, 0];
        world.set_move_bytecode(0, Some(bc2.clone()));
        let _ = world.step_move_vm(0, &bc2);
        assert_eq!(
            world.actors[0].move_state.pc, 1,
            "large-radius 0x39 should pass"
        );
    }

    // --- Field-event emission ---------------------------------------------

    /// Op 0x35 sub-1 (start BGM) emits `FieldEvent::Bgm` and pins
    /// `current_bgm`. Encoding: `[0x35, lo, hi, sub_op]`.
    #[test]
    fn field_op_35_sub1_emits_bgm_event_and_pins_current() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // text_id = 0x42 (LE), sub_op = 1 (start field BGM).
        let bytecode = vec![0x35, 0x42, 0x00, 0x01];
        world.load_field_script(bytecode);
        let _ = world.tick();
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                FieldEvent::Bgm {
                    sub_op: 1,
                    text_id: 0x42
                }
            )),
            "expected Bgm event, got {evs:?}"
        );
        assert_eq!(world.current_bgm, Some(0x42));
    }

    /// Op 0x3F (open dialog) populates `current_dialog` and emits an
    /// `OpenDialog` event. Encoding: `[0x3F, lo, hi, len, ...inline, xb, zb, depth]`.
    #[test]
    fn field_op_3f_emits_open_dialog() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // text_id = 0xAB, len = 0, then xb / zb / depth_id (3 bytes).
        let bytecode = vec![0x3F, 0xAB, 0x00, 0x00, 0x01, 0x02, 0x03];
        world.load_field_script(bytecode);
        let _ = world.tick();
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(
                |e| matches!(e, FieldEvent::OpenDialog { text_id: 0xAB, inline, .. } if inline.is_empty())
            ),
            "expected OpenDialog event, got {evs:?}"
        );
        assert_eq!(world.current_dialog.as_ref().map(|d| d.text_id), Some(0xAB));
    }

    /// Op 0x3A (add_money) clamps to `[0, 9_999_999]` and emits `AddMoney`.
    #[test]
    fn field_op_3a_clamps_and_emits_add_money() {
        let mut world = World::new();
        world.money = 100;
        world.mode = SceneMode::Field;
        // 0x3A op0=0xFF op1=0xFF op2=0xFF (24-bit -1) → delta = -1.
        // The op handler reads the 3-byte payload; sign-extend to i32.
        let bytecode = vec![0x3A, 0xFF, 0xFF, 0xFF];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert!(world.money >= 0, "money clamps to non-negative");
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(e, FieldEvent::AddMoney { .. })),
            "expected AddMoney event, got {evs:?}"
        );
    }

    /// Op 0x3C (party_add) appends to `party_actor_slots` and seeds the
    /// leader on the empty-party path.
    #[test]
    fn field_op_3c_party_add_first_member_becomes_leader() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // 0x3C + char_id (op0).
        let bytecode = vec![0x3C, 0x07];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.party_actor_slots, vec![Some(7)]);
        assert_eq!(world.party_leader_slot, Some(7));
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                FieldEvent::PartyAdd {
                    char_id: 7,
                    accepted: true
                }
            )),
            "expected PartyAdd event, got {evs:?}"
        );
    }

    /// Drain helper empties the queue.
    #[test]
    fn drain_field_events_empties_queue() {
        let mut world = World::new();
        world
            .pending_field_events
            .push(FieldEvent::PlaySfx { sfx_id: 1 });
        let drained = world.drain_field_events();
        assert_eq!(drained.len(), 1);
        assert!(world.pending_field_events.is_empty());
    }
}
