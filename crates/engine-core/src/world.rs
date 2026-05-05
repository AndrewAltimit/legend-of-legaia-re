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

use legaia_engine_vm as vm;
use vm::battle_action::{
    BattleActionCtx, BattleActionHost, BattleActor, BattleEndCause, Pose, StepOutcome,
};
use vm::effect_vm::{EffectHost, MasterSlot, Pool, StateOutcome};
use vm::field::{FieldCtx, FieldHost, StepResult as FieldStepResult};
use vm::move_vm::{ActorState as MoveActorState, MoveExtResult, MoveHost};
use vm::{Host as ActorVmHost, Position as ActorVmPosition};

/// Maximum simultaneous actors in the world. Mirrors the battle-side cap of
/// 8 + 32 spare slots for field-side NPCs / cutscene actors.
pub const MAX_ACTORS: usize = 64;

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

    /// Frame counter incremented every [`World::tick`].
    pub frame: u64,
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
            frame: 0,
        }
    }

    /// Set / clear the move-VM bytecode for `slot`. `None` clears the
    /// buffer; subsequent ticks won't run the move VM on this actor.
    pub fn set_move_bytecode(&mut self, slot: usize, bytecode: Option<Vec<u16>>) {
        if slot < self.move_bytecode.len() {
            self.move_bytecode[slot] = bytecode.unwrap_or_default();
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
    pub fn step_move_vm(&mut self, slot: usize, bytecode: &[u16]) -> vm::move_vm::StepResult {
        let mut host = MoveVmHostImpl { world: self };
        let actor_state = unsafe {
            // SAFETY: the host borrows `world.actors[slot]` only through
            // queries that don't read this slot's `move_state`. The host
            // implementation never touches `actors[slot].move_state`; it
            // only reads sin/cos LUTs and other engine-side data.
            &mut *(&mut host.world.actors[slot].move_state as *mut MoveActorState)
        };
        vm::move_vm::step(&mut host, actor_state, bytecode)
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
    fn ext_dispatch(
        &mut self,
        state: &mut MoveActorState,
        sub_opcode: u16,
        operand: &[u16],
    ) -> MoveExtResult {
        // Defer to the in-VM default dispatcher, which itself returns
        // `default_arm()` for sub-ops we haven't ported.
        vm::move_vm::MoveHost::ext_dispatch(&mut PassthroughMoveHost, state, sub_opcode, operand)
    }
}

/// Empty MoveHost used to forward to the in-VM default dispatcher when the
/// world doesn't override any sub-op behaviour. Kept zero-sized so it
/// optimises to nothing.
struct PassthroughMoveHost;
impl MoveHost for PassthroughMoveHost {}

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
}
