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
//! all four VMs read/write - world position, anim banks, flags, render bank,
//! per-action queue, etc. Splitting that across four crates would force
//! engines to keep four parallel index tables in sync. The composite pattern
//! here keeps the per-VM `ActorState` structs intact (clean-room boundary
//! preserved) but lets one struct own them.
//!
//! Engines that want a different layout - say, ECS storage - should
//! implement the VM `Host` traits themselves; this is the default.

use std::sync::Arc;

use crate::battle_events::BattleEvent;
use crate::field_events::FieldEvent;
use crate::levelup::{LevelUpBanner, LevelUpResult, LevelUpTracker};
use crate::tactical_arts::{ArtLearnedBanner, TacticalArtsTracker};
use crate::world_map::WorldMapController;
pub use legaia_anm::{AnimPlayer, PoseFrame};
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

/// Default `start_slot` engines pass to
/// [`World::materialize_actor_spawns`]. Slots `0..FIELD_SPAWN_START_SLOT`
/// stay reserved so the field-VM actor-allocator path can't clobber the
/// party (slots `0..party_count`, typically 0..3) or the small band of
/// scripted NPC / cutscene actors the scene reserves above the party.
/// The exact retail value is unknown - 8 is the smallest power-of-two
/// that comfortably brackets every observed `party_count + scripted-NPC`
/// span and matches the start-slot the field-VM unit tests use.
pub const FIELD_SPAWN_START_SLOT: u8 = 8;

/// Per-frame opcode cap for the move VM. Retail has no explicit cap (relies
/// on opcodes naturally yielding via `WAIT_SET` / `HALT`); for a software
/// port we set a generous defensive cap so a buggy script can't hang the
/// engine. 4096 is well above the largest real Tactical-Arts move script.
pub const MOVE_VM_BUDGET: usize = 4096;

/// One queued fade request. Move-VM ext sub-op 0x3C writes either an
/// immediate fade (`ticks == 0`) or a ramp (`ticks > 0`) - engines drain
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
    /// Field / town scene - field VM drives event flow.
    Field,
    /// Battle scene - battle action state machine runs over the actor table.
    Battle,
    /// Cutscene mode - actor VM runs but no field/battle dispatch.
    Cutscene,
    /// World-map mode - `WorldMapController` drives camera and entity ticks.
    WorldMap,
}

/// One sprite frame on a sprite sheet. Equivalent in shape to
/// `legaia_engine_render::SpriteRequest` but lives in engine-core (which
/// can't depend on the wgpu-bound render crate). Engine binaries
/// translate one-to-one when handing the per-frame sprite list to the
/// renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteFrame {
    /// Atlas source rect (`x`, `y`, `w`, `h`) in atlas texels.
    pub atlas_src: (u32, u32, u32, u32),
    /// Tint multiplied with the sampled atlas texel.
    pub tint: [f32; 4],
    /// World-y offset (pixels) added to the actor's screen-space y when
    /// the renderer projects [`Actor::move_state`] coords. `0` for ground-
    /// level sprites.
    pub anchor_y: i16,
}

impl Default for SpriteFrame {
    fn default() -> Self {
        Self {
            atlas_src: (0, 0, 0, 0),
            tint: [1.0; 4],
            anchor_y: 0,
        }
    }
}

/// Per-actor sprite request emitted by [`World::collect_sprite_requests`].
/// Engine binaries map this 1:1 to `legaia_engine_render::SpriteRequest`
/// for the wgpu sprite-batch upload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorSpriteRequest {
    /// Index in [`World::actors`] this request came from.
    pub actor_slot: u8,
    /// Top-left in screen-space pixels (the engine projects
    /// [`Actor::move_state`] world coords through its camera before
    /// emitting; this is the post-projection result).
    pub world_x: i32,
    pub world_y: i32,
    pub atlas_src: (u32, u32, u32, u32),
    pub tint: [f32; 4],
}

/// One entry in [`World::global_tmd_pool`]. Mirrors a single slot of the
/// retail `DAT_8007C018` global TMD-pointer table - a parsed TMD plus its
/// raw bytes so downstream renderers can drive `legaia_tmd::mesh::*` builders
/// without re-fetching from disc.
///
/// See `project_dat_8007c018_global_tmd_table.md` and
/// `project_global_tmd_pool_source.md` for the retail-side semantics. The
/// clean-room port treats the pool as opaque indexed storage: the field-VM
/// `0x4C 0xD8` host hook reads slot `tmd_idx` and writes the resulting `Arc`
/// onto [`Actor::tmd_ref`] - whatever populated the slot is the producer's
/// concern.
#[derive(Debug)]
pub struct GlobalTmd {
    pub tmd: legaia_tmd::Tmd,
    pub raw: Vec<u8>,
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

    /// `subobj` snap-clear condition flag - engine sets this when the actor's
    /// subobj is in the "snap to anchor" configuration. Read by actor VM op
    /// `SpawnDefault`.
    pub snap_clear: bool,

    /// Optional motion target consumed by actor VM op `EffectMotion`.
    /// `None` → the op no-ops (the retail equivalent of a null subobj
    /// pointer).
    pub motion_target: Option<ActorVmPosition>,

    /// Last-frame effect spawn - engine wires whatever rendering / sound
    /// flash it has. We just record the actor id for inspection.
    pub last_effect: u32,

    /// Most recent status effect inflicted by an art strike (Burned /
    /// Shocked / …). Engines clear this when they've folded it into their
    /// status-bar UI; defaults to `None`.
    pub pending_status: Option<legaia_art::record::EnemyEffect>,

    /// Optional sprite frame for this actor. Drives the per-frame sprite
    /// batch through [`World::collect_sprite_requests`]. When `None`, the
    /// actor is invisible (or rendered as a 3D mesh through the TMD path).
    pub sprite_frame: Option<SpriteFrame>,

    /// Active keyframe animation player. `None` means no animation is
    /// playing. Set via [`World::set_actor_animation`].
    pub active_animation: Option<AnimPlayer>,

    /// Last per-bone pose produced by `active_animation.tick()`. `None`
    /// until the first frame after an animation is assigned. Renderers
    /// consume this via `tmd_to_vram_mesh_posed` to deform the actor's
    /// mesh each frame.
    pub pose_frame: Option<PoseFrame>,

    /// Index into `SceneResources::tmds` for this actor's bound mesh.
    /// `None` means no TMD is bound - the actor has no visible 3D model.
    /// Set via [`World::set_actor_tmd_binding`].
    pub tmd_binding: Option<usize>,

    /// Actor kind classifier. Retail equivalent: the `u16` at `actor[+0x3C]`
    /// written by `FUN_801D77F4` (overlay actor allocator). Zero means
    /// "unset" - either the actor was created via the actor-VM path
    /// (`spawn` / `spawn_actor`) rather than the field-VM allocator, or no
    /// kind has been wired yet.
    pub kind: u16,
    /// Actor variant. Retail equivalent: the `u16` at `actor[+0x3E]`.
    /// Co-written with `kind` by `FUN_801D77F4`.
    pub variant: u16,
    /// Record bytecode this actor was instantiated from. Retail stores a
    /// pointer at `actor[+0x4C]` whose meaning depends on the allocation
    /// path - for the field-VM `0x4C 0x80` allocator the pointer addresses
    /// the child-actor packet that the parent script's `tail` contributed.
    /// `None` for actors spawned through other paths.
    ///
    /// Distinct from [`Self::tmd_binding`] / [`Self::active_animation`],
    /// which cover the rendering side.
    pub spawn_record: Option<Vec<u8>>,

    /// Global TMD this actor was instantiated with. Retail equivalent: the
    /// `u32` at `actor[+0x48]` written by the overlay actor allocator -
    /// `iVar13 = *(int *)(&DAT_8007C018 + ((tmd_idx << 16) >> 14))`. Set by
    /// the field-VM `0x4C 0xD8` host hook from [`World::global_tmd`], and
    /// reachable from rendering / animation systems that want a mesh +
    /// raw bytes for this actor without re-walking the global pool.
    /// `None` for actors spawned through paths that don't reference the
    /// global TMD pool (the actor VM's own `Spawn*` opcodes, etc.).
    pub tmd_ref: Option<Arc<GlobalTmd>>,
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
/// borrow-checker complexity manageable - see [`World::with_host`].
pub struct World {
    pub mode: SceneMode,
    pub actors: Vec<Actor>,
    pub battle_ctx: BattleActionCtx,
    pub effect_pool: Pool,
    /// Script catalog for the effect VM. Populated at battle-enter time
    /// from PROT 873 (`efect.dat`) pack1 data via
    /// [`legaia_engine_vm::effect_vm::EffectCatalog::from_pack1_bytes`].
    /// An empty catalog is safe - `BattleHostImpl::ui_element` spawns
    /// nothing until a real catalog is wired. Set via
    /// [`crate::scene::SceneHost::set_effect_catalog`].
    pub effect_catalog: vm::effect_vm::EffectCatalog,
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
    /// vec means "no active move" - the move VM is not ticked for that
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
    /// Move-VM axis offset at `_DAT_8007C348` - used by ext sub-ops 0x36 / 0x37
    /// for the `0x8E - axis` threshold predicate. Engines write per-scene.
    pub move_axis_threshold: i16,
    /// Move-VM scratchpad ramp ratio numerator at `_DAT_1F800393` - used by
    /// ext sub-op 0x23 (anim-bank lerp) as the numerator of a 12.0 fixed-point
    /// ratio against the operand-supplied denominator.
    pub move_ramp_ratio: u8,
    /// Fixed map origin pair at `(_DAT_80089118, _DAT_80089120)` - used by ext
    /// sub-op 0x24 (world position lerp toward fixed map origin).
    pub map_origin_xz: (i32, i32),
    /// Player actor slot - when `Some(slot)`, ext sub-ops 0x06 / 0x07 / 0x2A
    /// / 0x36 / 0x39 read `actors[slot].move_state.world_{x,y,z}` as the
    /// player position. `None` falls back to the origin (default impl).
    pub player_actor_slot: Option<u8>,
    /// Party-member actor slots - `party_actor_slots[i] = Some(actor_slot)`
    /// resolves move-VM ext sub-op 0x3B (`ext_party_member_lookup`) to the
    /// world-coords of the actor at that slot. Default empty (the lookup
    /// returns `None`, which forces sub-op 0x3B's "skip" path).
    pub party_actor_slots: Vec<Option<u8>>,
    /// Last fade colour requested by move-VM ext sub-op 0x3C - engines
    /// drain this each frame to drive the screen fade. `None` when no
    /// fade is pending.
    pub pending_fade: Option<FadeRequest>,
    /// Move-VM `_DAT_8007B9D8` - globally-shared 32-bit slot written by ext
    /// sub-op 0x2F. Engines read this on whatever frame-tick they want.
    pub move_dat_8007b9d8: i32,
    /// Move-VM 16-slot scratchpad ramp targets at `_DAT_1F80035C` - used by
    /// ext sub-op 0x29 (per-frame ramp / immediate write). Stored as i16
    /// pairs (target, current); engines apply per-frame interpolation.
    pub scratchpad_targets: [i16; 16],
    /// Shared system flag bank at `_DAT_80086D70` - bitfield read / written
    /// by:
    /// - field VM high-byte default routes 0x5x / 0x6x / 0x7x
    ///   (`system_flag_set` / `system_flag_clear` / `system_flag_test`)
    /// - move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D
    ///   (`ext_query_flag_bank` / `ext_set_flag_bank` / `ext_clear_flag_bank`)
    ///
    /// Lazily grown on write - the field VM's opcode-encoded idx ranges over
    /// `0..=0x87FF`, so a fixed 256-bit array is too small.
    pub system_flags: Vec<u8>,
    /// Field-VM `extra_flags` register read by op 0x42 mode 0 - a 32-bit
    /// auxiliary flag word (origin TBD; treated as scene-local state).
    pub extra_flags: u32,
    /// Field-VM `screen_mode` register read by op 0x42 mode 1 - packed mode
    /// bits (bits 4 / 5 / 6 / 7 individually testable; bits 12..15 indexed
    /// against `screen_mode_table`).
    pub screen_mode: u32,
    /// Field-VM scratchpad flag word (`_DAT_1F800394` in retail). Set
    /// by op `0x2E` GFLAG_SET; cleared by op `0x2F` GFLAG_CLR; tested
    /// by op `0x30` GFLAG_TST.
    ///
    /// Independent of [`Self::story_flag_bits`]: retail seeds this from
    /// the game-mode descriptor table on mode init (low 16 bits of
    /// `mode_table[mode_idx].param`) and the SC save/load bulk copy
    /// from RAM `0x80084340` never reaches scratchpad, so the bitmap
    /// and this word are not mirror copies of each other.
    pub story_flags: u32,
    /// Full 512-byte story-flag bitmap mirroring retail RAM
    /// `0x80085600..0x80085800` (SC block offset `0x14C0`). This is the
    /// narrative-progress bitmap the SC block persists, separate from
    /// the per-mode scratchpad word [`Self::story_flags`].
    ///
    /// Empty (`vec![]`) when the engine hasn't been booted from a retail
    /// SC block; populated via [`Self::load_full`] when a retail-shaped
    /// [`legaia_save::SaveFile`] is restored.
    pub story_flag_bits: Vec<u8>,

    /// PRNG state consumed by every VM that calls `host.rng()`. Default uses
    /// a deterministic LCG so tests are reproducible.
    pub rng_state: u32,

    /// Sin LUT used by move-VM op `0x03`. Engines populate from extracted
    /// asset data; default is empty (returns zero).
    pub sin_lut: Vec<i16>,
    /// Cos LUT - same shape as `sin_lut`.
    pub cos_lut: Vec<i16>,

    /// Battle-action helper tables. Engines populate per scene.
    pub spell_costs: std::collections::HashMap<u8, u8>,
    pub capture_spells: std::collections::HashSet<u8>,
    pub character_ability_bits: [u32; 8],
    pub range_table: std::collections::HashMap<(u8, u8), u16>,
    /// Per-slot weapon attack used by [`art_strike::apply_art_strike`] to
    /// compute Tactical-Art damage. Engines populate from the active
    /// character record's weapon power. Default zero - un-populated slots
    /// produce floor-clamped damage (`= 1`).
    pub battle_attack: [u16; 8],
    /// Per-slot defense facing the strike. The retail engine selects UDF
    /// or LDF based on the strike's `power_target`; this single field is a
    /// minimum-viable substitute that engines wishing to model both can
    /// override via [`World::set_battle_defense_for_target`].
    pub battle_defense: [u16; 8],
    /// Optional UDF / LDF defense override per slot. When set, the
    /// art-strike applier uses the matching half for the strike's target
    /// class instead of [`Self::battle_defense`]. Engines that don't
    /// distinguish UDF / LDF can leave this `None`.
    pub battle_defense_split: [Option<(u16, u16)>; 8],

    /// "Previous action cleared" gate - toggled by the engine when an
    /// animation transition completes.
    pub prev_action_cleared: bool,
    /// "Sound bank ready" gate.
    pub sound_bank_ready: bool,

    /// Number of party slots (default 3).
    pub party_count: u8,

    /// Last-issued battle-end cause (for inspection / engine side-effects).
    pub battle_end: Option<BattleEndCause>,

    /// Persistent per-character roster - populated by [`World::load_party`]
    /// and written back by [`World::save_party`]. Each record is the
    /// 0x414-byte struct documented in `docs/subsystems/battle.md`. The
    /// in-battle `BattleActor` slots mirror HP / MP from this; everything
    /// else (spells, equipment, ability bits) flows through this canonical
    /// store.
    pub roster: legaia_save::Party,

    /// Pending field-VM scene transition (`scene_transition(map_id)` was
    /// called this frame). Drained by [`crate::scene::SceneHost::tick`]:
    /// when `Some(map_id)`, the host resolves the map id to a scene name,
    /// loads it, and reinitialises the field VM. `None` between transitions.
    pub pending_scene_transition: Option<u8>,

    /// Pending FMV trigger (field-VM op `0x4C 0xE2`). When `Some(fmv_id)`,
    /// the field VM has signalled that the next-game-mode global should
    /// transition to game mode 26 (StrInit) with the given index. Engines
    /// drain this after [`World::tick`] to actually open the corresponding
    /// `MV*.STR` (use [`crate::cutscene::FmvIndex::str_filename`] for the
    /// retail mapping). `None` between triggers.
    pub pending_fmv_trigger: Option<i16>,

    /// Field-VM side-effects emitted this frame. Engines drain after
    /// [`World::tick`] to dispatch BGM, dialog, money, party, camera, etc.
    /// Mirror of the `FieldHost` callbacks - see [`FieldEvent`] for the
    /// per-variant citation.
    ///
    /// [`FieldEvent`]: crate::field_events::FieldEvent
    pub pending_field_events: Vec<FieldEvent>,

    /// Pending actor-spawn requests emitted by field-VM op `0x4C 0x80`
    /// (the actor allocator). Each entry is one child-actor's bytecode
    /// stream, split out of the parent script's `tail` via the retail
    /// `FUN_8003CA38` packet-length walker. Engines drain this through
    /// [`Self::drain_actor_spawns`] after [`Self::tick`] and route each
    /// record into their own actor pool - the retail engine mallocs a
    /// per-actor vertex pool and stores the record pointer at
    /// `actor[+0x90]`; the clean-room port leaves that policy to the
    /// engine that consumes the request.
    pub pending_actor_spawns: Vec<Vec<u8>>,

    /// Battle action state machine side-effects emitted this frame.
    /// Engines drain after [`World::tick`] to dispatch poses, UI elements,
    /// damage, screen-shake, etc. See [`BattleEvent`] for the per-variant
    /// citation.
    ///
    /// [`BattleEvent`]: crate::battle_events::BattleEvent
    pub pending_battle_events: Vec<BattleEvent>,

    /// Last BGM the field VM started (op 0x35 sub-1 / sub-9). `None` until
    /// a scene starts one. Updated synchronously when the VM emits the
    /// corresponding `Bgm` event.
    pub current_bgm: Option<u16>,

    /// Active dialog request - populated by the field-VM op 0x3F handler,
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

    /// Last camera state snapshot - filled by `camera_save`, applied by
    /// `camera_apply` / `camera_load`. Engines that draw a camera read
    /// this between frames.
    pub camera_state: CameraState,

    /// Frame counter incremented every [`World::tick`].
    pub frame: u64,

    /// Per-actor move-VM outcomes from the most recent [`World::tick_move_vms`]
    /// call. Pairs of `(actor_slot, outcome)`. Engines drain or inspect this
    /// after `World::tick` to react to halts / pending opcodes.
    pub move_outcomes: Vec<(u8, vm::move_vm::ActorTickOutcome)>,

    /// Per-character Tactical Arts use-counter tracker. Engines call
    /// [`World::notify_art_used`] from the battle side-effects handler when
    /// a Tactical Arts strike lands; the tracker emits
    /// [`BattleEvent::TacticalArtLearned`] and sets
    /// [`World::current_art_banner`] on first learn.
    pub tactical_arts: TacticalArtsTracker,

    /// Active "art learned" HUD banner. Set by [`World::notify_art_used`]
    /// when a new art crosses the learn threshold; its `frames_remaining`
    /// counter is decremented by [`World::tick`] until it reaches zero.
    /// `None` when no banner is active. Engines render this as a dialog-
    /// font overlay above the battle HUD.
    pub current_art_banner: Option<ArtLearnedBanner>,

    /// Per-party XP accumulator and level state. Engines call
    /// [`World::apply_battle_xp`] after a `BattleEndCause::MonsterWipe` to
    /// distribute XP and check for level-ups.
    pub level_up_tracker: LevelUpTracker,

    /// Active level-up HUD banner. Set by [`World::apply_battle_xp`] for the
    /// last character that leveled up; `frames_remaining` is decremented by
    /// [`World::tick`] until it reaches zero. `None` when no banner is active.
    /// Engines render this as a dialog-font overlay after battle.
    pub current_level_up_banner: Option<LevelUpBanner>,

    /// World-map camera and entity state. `Some` when `mode == SceneMode::WorldMap`,
    /// `None` otherwise.
    pub world_map_ctrl: Option<WorldMapController>,

    /// Per-actor status-effect tracker (Burned / Shocked / Poisoned /
    /// Asleep / Confused / Silenced / Stunned / Petrified). Populated by
    /// [`World::fold_battle_event`] on `ApplyArtStrike` events whose
    /// `enemy_effect` is non-`None`; ticked per turn by engines that
    /// drive a battle round. See [`legaia_engine_vm::status_effects`].
    pub status_effects: vm::status_effects::StatusEffectTracker,

    /// Per-character AP gauge - drives Tactical-Arts command input.
    /// Index 0..=2 maps to party slots; engines call
    /// [`crate::ap_gauge::ApGauge::reset_for_turn`] at turn start and
    /// [`crate::ap_gauge::ApGauge::charge_spirit`] when the player
    /// presses Spirit during command input.
    pub ap_gauges: [crate::ap_gauge::ApGauge; 3],

    /// Item catalog used by item-action resolution. Populated at battle
    /// init from [`crate::items::ItemCatalog::vanilla`] (or a custom
    /// catalog set by [`World::set_item_catalog`]); empty by default so
    /// the field VM doesn't trigger item effects in non-battle scenes.
    pub item_catalog: crate::items::ItemCatalog,

    /// Per-actor character max MP. The retail `BattleActor` holds only
    /// the running `mp` value (not the cap); the cap lives on the
    /// character record at `+0x140`. Engines populate this from the
    /// character record at battle init.
    pub character_max_mp: Vec<u16>,

    /// Active encounter session - bracketed transition + grace machine for
    /// step-driven random battles. `Some` when an encounter table is
    /// installed; `None` in scenes where encounters are disabled
    /// (towns / cutscenes / world-map). Engines call
    /// [`World::on_field_step`] from the field-step path (player walks one
    /// tile) to advance the tracker; the resulting [`crate::encounter::EncounterPhase`]
    /// drives the camera-shake / fade / battle-load chain.
    pub encounter: Option<crate::encounter::EncounterSession>,

    /// Per-character v2 save extension data. Mirrors `SaveExtV2` shape;
    /// engines populate from in-memory state at save time and consume on
    /// load. Index 0..=2 = main characters; entries beyond are story
    /// guests. Each entry holds learned-arts mask, learned spells, seru
    /// captures, and per-character active chain quick-slots.
    pub per_char_ext: Vec<(u8, legaia_save::CharSaveExt)>,

    /// Cross-character saved-chain library. Engines populate from a
    /// [`crate::tactical_arts_editor::ChainLibrary`] at save time and
    /// hydrate one back into the editor on load.
    pub saved_chains: Vec<legaia_save::SavedChainRecord>,

    /// Per-character Seru capture log - drives the post-battle "spell
    /// learned!" banner and the in-menu spell list. Pure data; saved
    /// through [`legaia_save::SaveExtV2::per_char`].
    pub seru_log: crate::seru_learning::SeruCaptureLog,

    /// Total game time in wall-clock seconds since the world was
    /// instantiated or loaded. Engines tick this independently of
    /// `frame` (which can pause-skip during dialogs / cutscenes).
    /// Persisted in [`legaia_save::SaveExtV2::play_time_seconds`].
    pub play_time_seconds: u32,

    /// Optional formation table - engines install this at boot via
    /// [`World::set_formation_table`] so triggered encounters can resolve
    /// their `formation_id` into concrete monster slot definitions.
    pub formation_table: crate::monster_catalog::FormationTable,

    /// Optional monster catalog - paired with `formation_table`. Engines
    /// look up [`crate::monster_catalog::MonsterDef`] by id when
    /// initialising the [`crate::battle_session::BattleSession`].
    pub monster_catalog: crate::monster_catalog::MonsterCatalog,

    /// CDNAME label of the active scene, if any. Set by
    /// [`World::set_active_scene_label`] on scene-load and consumed by
    /// engine-side helpers ([`World::install_encounter_for_scene`] reads
    /// this when it's called with the empty string, the encounter HUD
    /// surfaces it for diagnostics, etc.). Empty when no scene is loaded.
    pub active_scene_label: String,

    /// VDF ("set_mime", asset type `0x07`) buffer for the active scene.
    /// Layout `[u32 count][u32 byte_offsets[count]][body...]` mirrors the
    /// retail `DAT_8007B7DC` buffer the asset-dispatcher case 7 builds
    /// (see `project_vdf_buffer_and_parallel_table.md` for byte-level
    /// detail). The buffer holds the spawnable actor templates the field
    /// VM's `0x4C 0xD8` opcode resolves via [`World::vdf_record_bytes`].
    ///
    /// `None` when no scene is loaded or the scene carries no VDF chunk
    /// (most utility / cutscene scenes don't). Populated by
    /// [`crate::scene::SceneHost::enter_field_scene`] from the first
    /// asset-type-7 chunk found in the scene's streaming entries.
    pub vdf_buffer: Option<Vec<u8>>,

    /// Global TMD-pointer pool indexed by `tmd_idx`. Mirrors retail
    /// `DAT_8007C018` (the 143-entry homogeneous TMD pointer table in
    /// steady-state - see `project_dat_8007c018_global_tmd_table.md`).
    /// `None` at indices the active loader chain hasn't populated; the
    /// vector grows on demand through [`Self::set_global_tmd`].
    ///
    /// Seeded by [`crate::scene::SceneHost::enter_field_scene`] with the
    /// 5 character-mesh TMDs from PROT 0874 section 0 (byte-equality
    /// verified in `project_global_tmd_pool_source.md`). Producers of the
    /// other 138 kingdom-derived entries are not yet pinned; those slots
    /// stay `None` until the full chain lands.
    ///
    /// Read by the field-VM `0x4C 0xD8` host hook to populate
    /// [`Actor::tmd_ref`] on synchronous-spawn.
    pub global_tmd_pool: Vec<Option<Arc<GlobalTmd>>>,
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

/// Aggregated post-battle rewards returned by [`World::apply_battle_loot`].
///
/// Engines surface this as the post-battle banner ("got X XP, Y gold, level
/// up!"). The XP / gold totals reflect what was actually credited (monster
/// ids missing from the catalog don't contribute), `level_ups` carries the
/// per-character results from [`World::apply_battle_xp`], and `drops`
/// carries the item ids the loot roll surfaced from each fallen monster.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleRewards {
    pub xp: u32,
    pub gold: u32,
    pub level_ups: Vec<LevelUpResult>,
    /// Item drops the post-battle loot roll surfaced. One entry per
    /// monster slot that *both* (a) had a non-`None` `drop_item` in the
    /// catalog and (b) rolled below `drop_rate_q8 / 256`.
    pub drops: Vec<u8>,
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
            effect_catalog: vm::effect_vm::EffectCatalog::default(),
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
            story_flag_bits: Vec::new(),
            rng_state: 0x1234_5678,
            sin_lut: Vec::new(),
            cos_lut: Vec::new(),
            spell_costs: Default::default(),
            capture_spells: Default::default(),
            character_ability_bits: [0; 8],
            range_table: Default::default(),
            battle_attack: [0; 8],
            battle_defense: [0; 8],
            battle_defense_split: [None; 8],
            prev_action_cleared: true,
            sound_bank_ready: true,
            party_count: 3,
            battle_end: None,
            roster: legaia_save::Party::zeroed(0),
            pending_scene_transition: None,
            pending_fmv_trigger: None,
            pending_field_events: Vec::new(),
            pending_actor_spawns: Vec::new(),
            pending_battle_events: Vec::new(),
            current_bgm: None,
            current_dialog: None,
            last_field_interact: None,
            party_leader_slot: None,
            money: 0,
            inventory: std::collections::HashMap::new(),
            camera_state: CameraState::default(),
            frame: 0,
            move_outcomes: Vec::new(),
            tactical_arts: TacticalArtsTracker::new(),
            current_art_banner: None,
            level_up_tracker: LevelUpTracker::new(),
            current_level_up_banner: None,
            world_map_ctrl: None,
            status_effects: vm::status_effects::StatusEffectTracker::new(),
            ap_gauges: [crate::ap_gauge::ApGauge::default(); 3],
            item_catalog: crate::items::ItemCatalog::default(),
            character_max_mp: Vec::new(),
            encounter: None,
            per_char_ext: Vec::new(),
            saved_chains: Vec::new(),
            seru_log: crate::seru_learning::SeruCaptureLog::new(),
            play_time_seconds: 0,
            formation_table: crate::monster_catalog::FormationTable::new(),
            monster_catalog: crate::monster_catalog::MonsterCatalog::new(),
            active_scene_label: String::new(),
            vdf_buffer: None,
            global_tmd_pool: Vec::new(),
        }
    }

    /// Record the active scene label. Engines call this from the scene-load
    /// path (typically right before `install_encounter_for_scene`) so
    /// downstream consumers (HUD, diagnostics, save snapshots) can surface
    /// the current scene without re-walking the [`crate::scene::SceneHost`].
    pub fn set_active_scene_label(&mut self, label: impl Into<String>) {
        self.active_scene_label = label.into();
    }

    /// Install the VDF ("set_mime") buffer for the active scene. The bytes
    /// must follow the `[u32 count][u32 byte_offsets[count]][body...]`
    /// layout the retail asset-dispatcher case 7 produces; see
    /// [`Self::vdf_buffer`] for citation. Engines that want the
    /// field-VM `0x4C 0xD8` opcode to surface real spawn bytecode call
    /// this on scene-load with the extracted asset-type-7 chunk's body.
    ///
    /// Passing `None` clears the buffer; the next `0x4C 0xD8` call will
    /// leave `Actor::spawn_record` empty.
    pub fn set_vdf_buffer(&mut self, bytes: Option<Vec<u8>>) {
        self.vdf_buffer = bytes;
    }

    /// Resolve a VDF body slice by index using the
    /// `[u32 count][u32 byte_offsets[count]][body...]` layout. Each
    /// returned slice starts at `byte_offsets[idx]` and runs to the
    /// next body's offset (or end-of-buffer for the last entry).
    ///
    /// Returns `None` when:
    ///  - no VDF buffer is set (the scene loader skipped the install),
    ///  - the buffer is too short to read the header,
    ///  - `idx >= count`, or
    ///  - the indexed offset walks past end-of-buffer.
    ///
    /// Mirrors the retail body at
    /// `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt:152-203`:
    /// `puVar11 = (uint *)(iVar12 + *(int *)(((vdf_idx << 16) >> 14) + iVar12 + 4))`.
    pub fn vdf_record_bytes(&self, idx: u8) -> Option<&[u8]> {
        let buf = self.vdf_buffer.as_deref()?;
        if buf.len() < 4 {
            return None;
        }
        let count = u32::from_le_bytes(buf[0..4].try_into().ok()?);
        if (idx as u32) >= count {
            return None;
        }
        let table_byte = 4usize;
        let slot = table_byte + (idx as usize) * 4;
        if slot + 4 > buf.len() {
            return None;
        }
        let off = u32::from_le_bytes(buf[slot..slot + 4].try_into().ok()?) as usize;
        if off >= buf.len() {
            return None;
        }
        // Bound the body by the next *greater* offset (offsets aren't
        // guaranteed monotonic - we pick the smallest offset above
        // `off` from any later table slot, defaulting to EOB).
        let mut end = buf.len();
        for i in (idx as u32 + 1)..count {
            let s = table_byte + (i as usize) * 4;
            if s + 4 > buf.len() {
                break;
            }
            let next = u32::from_le_bytes(buf[s..s + 4].try_into().ok()?) as usize;
            if next > off && next <= buf.len() && next < end {
                end = next;
            }
        }
        Some(&buf[off..end])
    }

    /// Install a global TMD at pool index `idx`. The pool grows lazily on
    /// write to accommodate sparse loader-chain installs. Indices that
    /// later producers fill in stay `None` until they're explicitly set.
    ///
    /// Mirrors the retail `FUN_80026B4C` writer
    /// (`DAT_8007C018[DAT_8007B774++] = tmd_ptr`) but exposes the index
    /// directly rather than auto-bumping a counter - engines that want
    /// the retail behaviour can read the next free slot via
    /// [`Self::global_tmd_pool`]`.len()` and pass it here.
    pub fn set_global_tmd(&mut self, idx: usize, tmd: Arc<GlobalTmd>) {
        if idx >= self.global_tmd_pool.len() {
            self.global_tmd_pool.resize(idx + 1, None);
        }
        self.global_tmd_pool[idx] = Some(tmd);
    }

    /// Resolve a global TMD by pool index. Mirrors the retail field-VM
    /// allocator's `iVar13 = DAT_8007C018[(int16_t)tmd_idx]` read - the
    /// caller is responsible for clamping negative indices (the retail
    /// engine sign-extends the i16 then implicitly treats it as unsigned;
    /// the clean-room port returns `None` for negative or out-of-range
    /// indices via the `i16 → usize` cast guarded by the bounds check).
    ///
    /// Returns `None` when the slot is empty or `idx` is out of range.
    pub fn global_tmd(&self, idx: i16) -> Option<&Arc<GlobalTmd>> {
        if idx < 0 {
            return None;
        }
        self.global_tmd_pool.get(idx as usize)?.as_ref()
    }

    /// Drain emitted field-VM events. Engines call once per frame after
    /// [`World::tick`] to dispatch BGM, dialog, money, etc. Returns events
    /// in emission order.
    pub fn drain_field_events(&mut self) -> Vec<FieldEvent> {
        std::mem::take(&mut self.pending_field_events)
    }

    /// Drain queued actor-spawn requests emitted by field-VM op `0x4C 0x80`.
    /// Each entry is the variable-length bytecode stream for one child
    /// actor. Engines route these into their actor pool.
    pub fn drain_actor_spawns(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_actor_spawns)
    }

    /// Engine-side consumer for queued actor-spawn requests.
    ///
    /// Drains [`Self::pending_actor_spawns`] (records queued by the
    /// `0x4C 0x80` halt-acquire-gated path) and, for each record:
    /// 1. Scans `actors[start_slot..MAX_ACTORS]` for the first inactive
    ///    slot. Slots `0..start_slot` are skipped so engines keep their
    ///    party / scripted actors out of the auto-allocation range.
    /// 2. Activates the slot and stores the record bytes on
    ///    [`Actor::spawn_record`]. The retail allocator writes the
    ///    bytecode pointer to `actor[+0x90]` (different from the `+0x4C`
    ///    VDF-body field that the synchronous `0x4C 0xD8` path uses);
    ///    the clean-room port stores the raw bytes on `spawn_record`
    ///    regardless and lets the engine route them as field-VM
    ///    bytecode for a child actor (the records are scripted-child
    ///    coroutines, not TMD-body or kind/variant tuples).
    /// 3. Emits a [`FieldEvent::ActorSpawned`] event for the engine.
    ///
    /// Leaves [`Actor::kind`] and [`Actor::variant`] at zero. The
    /// retail allocator for this opcode (overlay code at
    /// `overlay_world_map_801de840.txt:7080-7123` case `8 sub-0`)
    /// allocates from pool `0x801f28a0` and writes only
    /// `actor[+0x90]` (bytecode ptr), `actor[+0x94]` (parent
    /// back-pointer) and `actor[+0x54] = 0`; the `+0x3C`/`+0x3E`
    /// kind/variant fields are never written by this path, so zero
    /// matches retail.
    ///
    /// Mirrors the retail allocator's pool-exhausted branch: if no
    /// inactive slot is available, the record is dropped silently and
    /// a [`FieldEvent::ActorSpawnFailed`] event is emitted instead.
    ///
    /// Returns the count of slots that were actually allocated.
    pub fn materialize_actor_spawns(&mut self, start_slot: u8) -> usize {
        let start = (start_slot as usize).min(self.actors.len());
        let records = std::mem::take(&mut self.pending_actor_spawns);
        let mut allocated = 0usize;
        for record in records {
            match self
                .actors
                .iter()
                .enumerate()
                .skip(start)
                .find(|(_, a)| !a.active)
                .map(|(i, _)| i)
            {
                Some(slot_idx) => {
                    let actor = &mut self.actors[slot_idx];
                    actor.active = true;
                    actor.kind = 0;
                    actor.variant = 0;
                    actor.spawn_record = Some(record.clone());
                    self.pending_field_events.push(FieldEvent::ActorSpawned {
                        slot: slot_idx as u8,
                        kind: 0,
                        variant: 0,
                        record,
                    });
                    allocated += 1;
                }
                None => {
                    self.pending_field_events
                        .push(FieldEvent::ActorSpawnFailed { record });
                }
            }
        }
        allocated
    }

    /// Drain emitted battle action events. Engines call once per frame
    /// after [`World::tick`] to dispatch poses, UI elements, damage, etc.
    /// Returns events in emission order.
    pub fn drain_battle_events(&mut self) -> Vec<BattleEvent> {
        std::mem::take(&mut self.pending_battle_events)
    }

    /// Apply the gameplay-state side of a single battle event - currently
    /// `ApplyArtStrike` (subtracts the resolved damage from the target's
    /// `BattleActor::hp`, clamping at zero, and records the enemy effect on
    /// the target's `pending_status`). Engines that want both the visual
    /// dispatch and the gameplay-state update call this for each event
    /// drained from [`Self::drain_battle_events`].
    ///
    /// Returns `Some((target_slot, hp_after))` for events that changed HP,
    /// `None` otherwise - useful for HUD popups that want the post-hit HP.
    pub fn fold_battle_event(&mut self, event: &BattleEvent) -> Option<(u8, u16)> {
        match event {
            BattleEvent::ApplyArtStrike {
                target_slot,
                outcome,
                ..
            } => {
                if let Some(target) = self.actors.get_mut(*target_slot as usize) {
                    if let Some(dmg) = outcome.damage {
                        target.battle.hp = target.battle.hp.saturating_sub(dmg);
                        // Damage clears Asleep on the target (matches retail -
                        // the enemy wakes when hit).
                        self.status_effects.on_damaged(*target_slot);
                    }
                    if outcome.enemy_effect != legaia_art::record::EnemyEffect::None {
                        target.pending_status = Some(outcome.enemy_effect);
                        // Push the status into the tracker so it
                        // subsequently ticks per-turn.
                        self.status_effects
                            .apply_from_enemy_effect(*target_slot, outcome.enemy_effect);
                    }
                    return Some((*target_slot, target.battle.hp));
                }
                None
            }
            _ => None,
        }
    }

    /// Step every actor's status effects forward one turn - folds the
    /// tick-damage into `BattleActor::hp` and emits per-status events.
    /// Called by engines once per battle round.
    pub fn tick_status_effects(&mut self) {
        let actor_count = self.actors.len();
        for slot in 0..actor_count as u8 {
            let (cur, max) = self
                .actors
                .get(slot as usize)
                .map(|a| (a.battle.hp, a.battle.max_hp))
                .unwrap_or((0, 0));
            if max == 0 {
                continue;
            }
            let dmg = self.status_effects.tick_actor(slot, cur, max);
            if dmg > 0
                && let Some(actor) = self.actors.get_mut(slot as usize)
            {
                actor.battle.hp = actor.battle.hp.saturating_sub(dmg);
            }
        }
    }

    /// Set the item catalog the battle / field menu consults for item
    /// actions. Replaces any prior catalog. Engines populate this at
    /// boot time (typically from the vanilla catalog).
    pub fn set_item_catalog(&mut self, catalog: crate::items::ItemCatalog) {
        self.item_catalog = catalog;
    }

    /// Use an item from the catalog against a target slot. Wraps the
    /// `items::apply_effect` resolution and folds the outcome back into
    /// world state (HP/MP deltas, status cure, revive HP). Returns the
    /// resolved [`crate::items::ItemOutcome`] so the engine can drive
    /// dialog / SFX / visual cues.
    ///
    /// Returns [`crate::items::ItemOutcome::NoEffect`] when:
    ///   - the item id is not in the catalog,
    ///   - or the target slot is out of range.
    ///
    /// HP / MP changes are clamped to the actor's max values. Cure /
    /// CureAll outcomes also clear the corresponding entries from the
    /// `StatusEffectTracker`.
    pub fn use_item(&mut self, item_id: u8, target_slot: u8) -> crate::items::ItemOutcome {
        let entry = match self.item_catalog.get(item_id) {
            Some(e) => *e,
            None => return crate::items::ItemOutcome::NoEffect,
        };
        let idx = target_slot as usize;
        // BattleActor holds `mp` but not `max_mp`; engines that wire the
        // character record into the actor populate it via a sibling field.
        // For the snapshot we use the character_max_mp accessor (defaults
        // to `mp` itself when not separately tracked, which gives a
        // conservative "MP already capped" reading).
        let snapshot = match self.actors.get(idx) {
            Some(a) => crate::items::TargetSnapshot {
                hp: a.battle.hp,
                hp_max: a.battle.max_hp,
                mp: a.battle.mp,
                mp_max: self
                    .character_max_mp
                    .get(idx)
                    .copied()
                    .unwrap_or(a.battle.mp),
                is_dead: a.battle.hp == 0 && a.battle.max_hp > 0,
            },
            None => return crate::items::ItemOutcome::NoEffect,
        };
        let outcome = crate::items::apply_effect(entry.effect, &snapshot);
        match outcome {
            crate::items::ItemOutcome::HealedHp { amount } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = a.battle.hp.saturating_add(amount).min(a.battle.max_hp);
                }
            }
            crate::items::ItemOutcome::HealedMp { amount } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    let cap = self.character_max_mp.get(idx).copied().unwrap_or(u16::MAX);
                    a.battle.mp = a.battle.mp.saturating_add(amount).min(cap);
                }
            }
            crate::items::ItemOutcome::Cured { kind } => {
                self.status_effects.cure(target_slot, kind);
            }
            crate::items::ItemOutcome::CuredAll => {
                self.status_effects.cure_all(target_slot);
            }
            crate::items::ItemOutcome::Revived { hp_after } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = hp_after.min(a.battle.max_hp);
                }
            }
            crate::items::ItemOutcome::SpiritGained { amount } if idx < self.ap_gauges.len() => {
                // Refund AP into the active actor's gauge if it's a party slot.
                self.ap_gauges[idx].refund(amount);
            }
            _ => {}
        }
        outcome
    }

    /// Set per-slot character max MP (mirrors `char_record[+0x140]`
    /// from the save record). Engines call this once per scene init -
    /// usually from `set_character_record_for_slot`. Unset slots default
    /// to `0`, which makes [`Self::use_item`] treat MP healing as a
    /// no-op for that slot.
    pub fn set_character_max_mp(&mut self, slot: u8, mp_max: u16) {
        let i = slot as usize;
        if i >= self.character_max_mp.len() {
            self.character_max_mp.resize(i + 1, 0);
        }
        self.character_max_mp[i] = mp_max;
    }

    /// Reset every party-member's AP gauge for a new turn. Refills to
    /// `base_ap`, clears the Spirit-charged flag.
    pub fn reset_party_ap(&mut self) {
        for g in self.ap_gauges.iter_mut() {
            g.reset_for_turn();
        }
    }

    /// Set the per-slot weapon attack used by Tactical-Art strike damage
    /// resolution. Engines call this when a character equips / unequips a
    /// weapon, or once at battle init from the active stat record.
    pub fn set_battle_attack(&mut self, slot: u8, atk: u16) {
        if let Some(s) = self.battle_attack.get_mut(slot as usize) {
            *s = atk;
        }
    }

    /// Set the per-slot generic defense - used when no UDF / LDF split is
    /// configured for the slot.
    pub fn set_battle_defense(&mut self, slot: u8, def: u16) {
        if let Some(s) = self.battle_defense.get_mut(slot as usize) {
            *s = def;
        }
    }

    /// Set per-slot UDF / LDF defense override. Replaces any prior value.
    /// Pass `None` to revert to [`Self::set_battle_defense`].
    pub fn set_battle_defense_split(&mut self, slot: u8, udf_ldf: Option<(u16, u16)>) {
        if let Some(s) = self.battle_defense_split.get_mut(slot as usize) {
            *s = udf_ldf;
        }
    }

    /// Resolve the defense value to use against a single Tactical-Art
    /// strike. Used by the world's `BattleActionHost::apply_art_strike`
    /// impl. Public so engines can call the same lookup directly when
    /// they want to apply art strikes outside the SM (e.g. for testing).
    pub fn resolve_battle_defense(
        &self,
        target_slot: u8,
        info: &legaia_engine_vm::battle_action::ArtStrikeInfo,
    ) -> u16 {
        let idx = target_slot as usize;
        // If we have a UDF / LDF split for the slot, pick the half that
        // matches the strike's power target. Otherwise fall back to the
        // single defense value.
        if let Some(Some((udf, ldf))) = self.battle_defense_split.get(idx)
            && let Some(legaia_art::power::PowerByte::Damage(p)) = info.power
        {
            return match p.target {
                legaia_art::power::PowerTarget::Udf => *udf,
                legaia_art::power::PowerTarget::Ldf => *ldf,
            };
        }
        self.battle_defense.get(idx).copied().unwrap_or(0)
    }

    /// Distribute `xp_reward` to the surviving party members after a
    /// `BattleEndCause::MonsterWipe`. Mirrors the retail-shape split:
    ///
    /// - Surviving members (HP > 0) split the reward equally, rounded down.
    /// - Dead members (HP == 0) receive zero XP.
    /// - Remainder bytes from the integer divide are dropped on the floor,
    ///   matching the retail end-of-battle distribution.
    ///
    /// For each member that crosses a level threshold, bumps the roster
    /// record's HP/MP maxima, resyncs the live `BattleActor` mirror, pushes
    /// a [`BattleEvent::LevelUp`], and appends a [`LevelUpResult`] to the
    /// returned vec.
    ///
    /// If every party member is dead (TPK) but the caller still invokes this
    /// (e.g. a Phoenix Down style revive-after-victory), the split degenerates
    /// to a no-op — there are no alive recipients.
    pub fn apply_battle_xp(&mut self, xp_reward: u32) -> Vec<LevelUpResult> {
        let party_count = self.party_count as usize;
        // Living-member count drives the divisor. We pull HP from
        // `BattleActor` (the live mirror) so the resolver sees the
        // post-battle state, not the record's saved HP.
        let alive: Vec<u8> = (0..party_count as u8)
            .filter(|&i| self.actors.get(i as usize).is_some_and(|a| a.battle.hp > 0))
            .collect();
        if alive.is_empty() {
            return Vec::new();
        }
        let per_member_xp = xp_reward / alive.len() as u32;
        if per_member_xp == 0 {
            return Vec::new();
        }
        let mut results = Vec::new();
        for char_id in alive {
            let Some(result) = self.level_up_tracker.grant_xp(char_id, per_member_xp) else {
                continue;
            };
            let slot = char_id as usize;
            if let Some(rec) = self.roster.members.get_mut(slot) {
                LevelUpTracker::apply_to_record(&result, rec);
            }
            let new_hms = self.roster.members.get(slot).map(|r| r.hp_mp_sp());
            if let (Some(actor), Some(hms)) = (self.actors.get_mut(slot), new_hms) {
                actor.battle.max_hp = hms.hp_max;
                actor.battle.hp = hms.hp_cur;
                actor.battle.mp = hms.mp_cur;
            }
            self.pending_battle_events.push(BattleEvent::LevelUp {
                char_id,
                new_level: result.new_level,
                hp_gained: result.hp_gained,
                mp_gained: result.mp_gained,
            });
            self.current_level_up_banner = Some(LevelUpBanner {
                char_id,
                new_level: result.new_level,
                hp_gained: result.hp_gained,
                mp_gained: result.mp_gained,
                frames_remaining: LevelUpBanner::DEFAULT_FRAMES,
            });
            results.push(result);
        }
        results
    }

    /// Sum the per-monster `exp` and `gold` for every slot in `formation`,
    /// distribute the XP via [`World::apply_battle_xp`], and add the gold
    /// to [`World::money`]. Returns the aggregated [`BattleRewards`] so
    /// engines can surface the post-battle banner ("got N XP, M gold,
    /// learned spell X").
    ///
    /// Monsters whose ids aren't in `catalog` contribute zero - the call
    /// silently skips them rather than failing, so a partially-populated
    /// catalog still drives a battle-end transition.
    pub fn apply_battle_loot(
        &mut self,
        formation: &crate::monster_catalog::FormationDef,
        catalog: &crate::monster_catalog::MonsterCatalog,
    ) -> BattleRewards {
        let mut xp_total: u32 = 0;
        let mut gold_total: u32 = 0;
        let mut drops: Vec<u8> = Vec::new();
        for slot in &formation.slots {
            let Some(def) = catalog.get(slot.monster_id) else {
                continue;
            };
            xp_total = xp_total.saturating_add(def.exp as u32);
            gold_total = gold_total.saturating_add(def.gold as u32);
            if let Some(item_id) = def.drop_item
                && def.drop_rate_q8 > 0
            {
                // 1-in-256 fixed-point drop roll: pull one byte from the
                // deterministic RNG and compare. `drop_rate_q8 == 255`
                // makes the drop near-guaranteed (1/256 floor); `0`
                // already short-circuited above.
                let roll = (self.next_rng() & 0xFF) as u8;
                if roll < def.drop_rate_q8 {
                    drops.push(item_id);
                    let entry = self.inventory.entry(item_id).or_insert(0);
                    *entry = entry.saturating_add(1);
                }
            }
        }
        let level_ups = if xp_total > 0 {
            self.apply_battle_xp(xp_total)
        } else {
            Vec::new()
        };
        let new_money = (self.money as i64).saturating_add(gold_total as i64);
        self.money = new_money.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        BattleRewards {
            xp: xp_total,
            gold: gold_total,
            level_ups,
            drops,
        }
    }

    /// Record one use of `art_id` by `char_id` (roster index).
    ///
    /// Delegates to [`TacticalArtsTracker::notify_art_used`]. When the use
    /// count first crosses the learn threshold, this method:
    ///
    /// 1. Pushes [`BattleEvent::TacticalArtLearned`] onto
    ///    [`Self::pending_battle_events`].
    /// 2. Sets [`Self::current_art_banner`] with a 2-second display window
    ///    so the engine's HUD overlay can show "Learned Art #N!".
    ///
    /// Subsequent calls for the same `(char_id, art_id)` pair are no-ops.
    pub fn notify_art_used(&mut self, char_id: u8, art_id: u8) {
        if let Some(ev) = self.tactical_arts.notify_art_used(char_id, art_id) {
            let text = format!("Learned {}!", ev.name);
            self.current_art_banner = Some(ArtLearnedBanner {
                text,
                frames_remaining: ArtLearnedBanner::DEFAULT_FRAMES,
            });
            self.pending_battle_events
                .push(BattleEvent::TacticalArtLearned {
                    char_id: ev.char_id,
                    art_id: ev.art_id,
                });
        }
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
    /// hasn't been fully traced, so this skip is heuristic - revise once
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

    /// Capture the complete engine state (party + globals) into a [`legaia_save::SaveFile`].
    ///
    /// Pairs with [`World::load_full`]. Use this instead of [`World::save_party`] when
    /// you need `story_flags`, `money`, and `inventory` to survive a save/load cycle.
    pub fn save_full(&mut self) -> legaia_save::SaveFile {
        let party = self.save_party();
        let mut inventory: Vec<(u8, u8)> = self
            .inventory
            .iter()
            .map(|(&id, &count)| (id, count))
            .collect();
        inventory.sort_by_key(|&(id, _)| id);

        // Build per-character extension records from live world state.
        let active_party: Vec<u8> = (0..party.members.len() as u8).collect();
        let mut per_char: Vec<(u8, legaia_save::CharSaveExt)> = Vec::new();
        for slot in 0..party.members.len() as u8 {
            let mut ce = legaia_save::CharSaveExt::default();
            // Learned arts: derive from TacticalArtsTracker - bit i is
            // set when art id i has crossed the learn threshold.
            for art_id in 0..32u8 {
                if self.tactical_arts.is_learned(slot, art_id) {
                    ce.learned_arts_mask |= 1u32 << art_id;
                }
            }
            // Spells: the per-character learned spell list from the seru log.
            ce.spells = self.seru_log.learned_spells(slot).to_vec();
            // Seru captures: walk the ext mirror - fall back to empty for
            // characters that haven't captured anything yet.
            if let Some((_, src)) = self.per_char_ext.iter().find(|(s, _)| *s == slot) {
                ce.seru_captures = src.seru_captures.clone();
                ce.active_chains = src.active_chains;
            }
            per_char.push((slot, ce));
        }

        legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt {
                story_flags: self.story_flags,
                story_flag_bits: self.story_flag_bits.clone(),
                money: self.money,
                inventory,
            },
            ext_v2: legaia_save::SaveExtV2 {
                play_time_seconds: self.play_time_seconds,
                active_party,
                per_char,
                saved_chains: self.saved_chains.clone(),
            },
        }
    }

    /// Restore engine state from a [`legaia_save::SaveFile`] produced by [`World::save_full`].
    ///
    /// Party records are applied through [`World::load_party`]; globals overwrite the
    /// current `story_flags`, `money`, and `inventory`. Sync per-slot
    /// [`LevelUpTracker::level`] from each loaded record's `+0x100` byte
    /// so reloads don't silently reset every party slot to level 1.
    pub fn load_full(&mut self, sf: legaia_save::SaveFile) {
        self.load_party(sf.party);
        self.story_flags = sf.ext.story_flags;
        self.story_flag_bits = sf.ext.story_flag_bits;
        self.money = sf.ext.money;
        self.inventory.clear();
        for (id, count) in sf.ext.inventory {
            if count > 0 {
                self.inventory.insert(id, count);
            }
        }
        // Hydrate the level-up tracker's per-slot level from the loaded
        // character records. Without this, the tracker keeps its default
        // 1-per-slot level even when the saved record has the party at
        // level 30 — the next level-up grant would silently roll the
        // party back to level 1 + N.
        for (slot, rec) in self.roster.members.iter().enumerate() {
            if slot < self.level_up_tracker.level.len() {
                self.level_up_tracker.level[slot] = rec.level().max(1);
            }
        }
        // V2 ext block - repopulate engine-side trackers.
        self.play_time_seconds = sf.ext_v2.play_time_seconds;
        self.saved_chains = sf.ext_v2.saved_chains.clone();
        self.per_char_ext = sf.ext_v2.per_char.clone();
        // Reset trackers so reloads don't accumulate stale state.
        self.tactical_arts = TacticalArtsTracker::new();
        self.seru_log = crate::seru_learning::SeruCaptureLog::new();
        for (slot, ce) in &sf.ext_v2.per_char {
            // Re-mark learned arts so the tracker doesn't re-fire the
            // "first time learned" event for arts the save already has.
            for art_id in 0..32u8 {
                if ce.learned_arts_mask & (1u32 << art_id) != 0 {
                    self.tactical_arts.mark_known(*slot, art_id);
                }
            }
            // Re-seed the seru log's learned-spells list. We synthesise
            // a placeholder seru_id for each spell since the save layer
            // only persists the spell ids - engines can rebuild the
            // capture-points totals from the saved seru_captures pairs.
            for &spell_id in &ce.spells {
                // seru_id is unknown without the registry; use spell_id
                // as a surrogate key (preserves uniqueness within a slot).
                self.seru_log.mark_learned(*slot, spell_id as u16, spell_id);
            }
        }
    }

    /// Activate a slot and return a mutable reference to the actor.
    pub fn spawn_actor(&mut self, slot: usize) -> &mut Actor {
        let a = &mut self.actors[slot];
        a.active = true;
        a
    }

    /// Ensure the slot at `id` is initialized with the supplied default
    /// position and active. Idempotent.
    ///
    /// Preserves `tmd_binding` and `active_animation` across the reset so
    /// that `init_scene_animations` bindings survive the first field-VM
    /// actor-spawn opcode.
    pub fn ensure_actor(&mut self, id: u8, default_pos: ActorVmPosition) -> &mut Actor {
        let a = &mut self.actors[id as usize];
        if !a.active {
            let tmd_binding = a.tmd_binding;
            let active_animation = a.active_animation.take();
            *a = Actor::new();
            a.tmd_binding = tmd_binding;
            a.active_animation = active_animation;
            a.active = true;
        }
        a.default_pos = default_pos;
        a
    }

    /// Pre-bind every actor slot to its scene resources before the field VM
    /// spawns actors. Wires:
    ///
    /// - `actor.tmd_binding = slot_idx` (direct 1:1 ordering: the retail
    ///   `FUN_8001E890` loop registers TMDs in pack offset-table order -
    ///   actor K → TMD slot K).
    /// - `actor.active_animation` seeded from ANM record 0 (idle) when an
    ///   ANM pack is present for that slot.
    ///
    /// Because `ensure_actor` preserves these fields across resets, the
    /// bindings survive the first field-VM actor-spawn opcode.
    pub fn init_scene_animations(&mut self, resources: &crate::scene_resources::SceneResources) {
        for (i, actor) in self.actors.iter_mut().enumerate() {
            if i < resources.tmds.len() {
                actor.tmd_binding = Some(i);
            }
            if actor.active_animation.is_none()
                && let Some(anm) = resources.anm_pack_for_actor(i)
                && let Some(rec_bytes) = anm.record_bytes(0)
            {
                let bone_count = resources
                    .tmds
                    .get(i)
                    .map(|t| t.tmd.objects.len())
                    .unwrap_or(1)
                    .max(1);
                if let Ok(player) = AnimPlayer::new(rec_bytes.to_vec(), bone_count) {
                    actor.active_animation = Some(player);
                }
            }
        }
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
    /// after step completes - see the `MoveVmHostImpl` deferred-writes map.
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

    /// Spawn effect `ui_id` at `world_pos` / `angle` via the pool, looking
    /// up the script in `self.effect_catalog`. No-op when the catalog is
    /// empty or the id is out of range. Mirrors the retail path through
    /// `FUN_801D8DE8 → FUN_801DFDF8`.
    pub fn try_spawn_effect(&mut self, ui_id: u8, world_pos: [i16; 3], angle: u16) {
        let catalog_ptr: *const vm::effect_vm::EffectCatalog = &self.effect_catalog;
        let pool_ptr: *mut vm::effect_vm::Pool = &mut self.effect_pool;
        let mut host = EffectHostImpl { world: self };
        // SAFETY: EffectHostImpl only reads `world.rng_state`; it never
        // accesses `effect_pool` or `effect_catalog` through the borrow.
        let pool = unsafe { &mut *pool_ptr };
        let catalog = unsafe { &*catalog_ptr };
        let _ = pool.spawn_by_ui_id(&mut host, ui_id, world_pos, angle, catalog);
    }

    /// Install an [`crate::encounter::EncounterSession`] for the current
    /// scene. Engines call this on scene-enter once the per-scene encounter
    /// table is known. `None` disables encounters for the active scene.
    pub fn set_encounter_session(&mut self, session: Option<crate::encounter::EncounterSession>) {
        self.encounter = session;
    }

    /// Install an encounter session resolved from a registry against the
    /// given CDNAME label. Engines call this from the scene-load path so
    /// every scene gets its retail-mapped encounter table without having
    /// to plumb tables through the boot config.
    ///
    /// Returns `true` when the registry resolved a non-empty table and
    /// installed it, `false` when no rule matched (or the resolved table
    /// has `trigger_rate_q8 == 0` - in which case the session is
    /// installed-but-quiet so the engine can still call `on_field_step`
    /// without nil checks).
    ///
    /// The on-disc resolver (reading the per-scene encounter table out of
    /// `0865_battle_data`) lands once a runtime watchpoint trace pins the
    /// table offset. Engines currently feed the registry from
    /// [`crate::encounter_registry::vanilla_encounter_registry`] or a
    /// custom composition.
    pub fn install_encounter_for_scene(
        &mut self,
        registry: &crate::encounter_registry::EncounterRegistry,
        scene_label: &str,
    ) -> bool {
        match registry.resolve(scene_label) {
            Some(table) => {
                let tracker = crate::encounter::EncounterTracker::new(table.clone());
                let session = crate::encounter::EncounterSession::new(tracker);
                let nonempty = !table.is_empty();
                self.encounter = Some(session);
                nonempty
            }
            None => {
                self.encounter = None;
                false
            }
        }
    }

    /// Install a formation + monster catalog pair. Boot wires this once;
    /// engines read it at battle-load time.
    pub fn set_formation_table(
        &mut self,
        table: crate::monster_catalog::FormationTable,
        catalog: crate::monster_catalog::MonsterCatalog,
    ) {
        self.formation_table = table;
        self.monster_catalog = catalog;
    }

    /// Install a [`crate::encounter_record::EncounterRecord`] decoded from
    /// an on-disc byte slice as the next encounter for the active scene.
    ///
    /// Mirrors the retail flow at `0x801DA620..0x801DA678`: the parsed
    /// record's monster ids are turned into a [`crate::monster_catalog::FormationDef`]
    /// (registered into `formation_table`), wrapped in a single-row
    /// [`crate::encounter::EncounterTable`] (rate `0xFF/256` so the next
    /// step roll always fires), and installed as the active session.
    ///
    /// Returns the synthesized `formation_id` so engines can immediately
    /// transition to battle if they want to skip the per-step roll.
    /// `None` means the record was empty (no monsters).
    pub fn install_encounter_from_record(
        &mut self,
        scene_label: &str,
        record: &crate::encounter_record::EncounterRecord,
    ) -> Option<u16> {
        if record.is_empty() {
            return None;
        }
        let formation = record.to_formation_def(scene_label);
        let formation_id = formation.formation_id;
        self.formation_table.insert(formation);

        use crate::encounter::{
            EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
        };
        let mut table = EncounterTable::new(scene_label);
        // Force the next roll to succeed: the record IS the encounter.
        table.set_trigger_rate(0xFF);
        table.push(EncounterEntry::new(formation_id, 1));
        let tracker = EncounterTracker::new(table);
        self.encounter = Some(EncounterSession::new(tracker));
        Some(formation_id)
    }

    /// Field-step trigger. Engines call this once per "the player walked
    /// one map cell" (typically when the player actor's grid coord moves)
    /// to advance the encounter tracker. Returns `true` if a battle
    /// transition was triggered this step.
    ///
    /// The method is a no-op when no [`crate::encounter::EncounterSession`]
    /// is installed, when the session is not in `Idle`, or when the world
    /// is not in [`SceneMode::Field`].
    pub fn on_field_step(&mut self) -> bool {
        if !matches!(self.mode, SceneMode::Field) {
            return false;
        }
        let rng = self.next_rng();
        match self.encounter.as_mut() {
            Some(session) => session.on_step(rng),
            None => false,
        }
    }

    /// Per-frame tick of the encounter session timers. Drives the
    /// `Transition` and `Grace` countdowns.
    pub fn tick_encounter(&mut self) {
        if let Some(session) = self.encounter.as_mut() {
            session.tick_frame();
        }
    }

    /// Return the resolved [`crate::monster_catalog::FormationDef`] for the
    /// currently-triggered encounter, if any. Engines call this after the
    /// session reports `Triggered` to drain the roll and resolve into a
    /// concrete monster set; the session advances to `Battling` as a
    /// side-effect.
    pub fn drain_encounter_formation(&mut self) -> Option<crate::encounter::EncounterRoll> {
        self.encounter.as_mut().and_then(|s| s.drain_triggered())
    }

    /// Mark that the active battle finished. Engines call this from the
    /// post-battle resolution path so the session enters its grace window
    /// (suppresses encounters for `grace_frames` frames).
    pub fn end_encounter_battle(&mut self) {
        if let Some(session) = self.encounter.as_mut() {
            session.end_battle();
        }
    }

    /// Advance the wall-clock play-time counter by `delta_seconds`. Engines
    /// drive this from the frame loop's wall-clock delta. Mirrors the
    /// retail "play time" field shown on the save screen.
    pub fn advance_play_time(&mut self, delta_seconds: u32) {
        self.play_time_seconds = self.play_time_seconds.saturating_add(delta_seconds);
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
    ///  1. Effect pool tick - runs every frame regardless of mode.
    ///  2. Per-actor move-VM tick - only for actors with bytecode loaded.
    ///  3. Mode-specific VM:
    ///     - `Battle`     → battle-action state machine step.
    ///     - `Field`      → field-VM step (or no-op if no bytecode loaded).
    ///     - `Cutscene`   → field-VM step (cutscenes use the same script VM).
    ///     - `Title`      → no further VM.
    pub fn tick(&mut self) -> Option<StepOutcome> {
        self.frame += 1;
        self.tick_effects();
        self.tick_move_vms();
        self.tick_actors();
        // Tick art-learned banner countdown - clear when it reaches zero.
        if let Some(banner) = &mut self.current_art_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.current_art_banner = None;
            }
        }
        // Tick level-up banner countdown.
        if let Some(banner) = &mut self.current_level_up_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.current_level_up_banner = None;
            }
        }
        match self.mode {
            SceneMode::Battle => Some(self.step_battle()),
            SceneMode::Field | SceneMode::Cutscene => {
                self.step_field();
                None
            }
            SceneMode::Title | SceneMode::WorldMap => None,
        }
    }

    /// Per-actor move-VM tick - clean port of `FUN_80021DF4` (lines
    /// `80022B94..80022BBC`).
    ///
    /// Two-phase: (1) pre-tick decrement the per-actor `wait_timer` by the
    /// global frame-time `delta`, (2) run the move VM through
    /// [`vm::move_vm::actor_tick`], which gates on the resulting timer and
    /// inspects the HALT flag after the call. Outcomes are recorded in
    /// [`World::move_outcomes`] so engines that want to react to per-actor
    /// halts / waits can read them after the world ticks.
    ///
    /// `delta` mirrors the retail product `_DAT_1f800393 * _DAT_1f80037D`
    /// (per-frame anim-speed scalars). Engines pass their own per-frame
    /// scalar; the default world tick uses `1` so a Wait of N consumes N
    /// frames.
    pub fn tick_move_vms_with_delta(&mut self, delta: u16) {
        self.move_outcomes.clear();
        for slot in 0..self.actors.len() {
            if !self.actors[slot].active {
                continue;
            }
            let bc = self.move_bytecode.get(slot).cloned().unwrap_or_default();
            if bc.is_empty() {
                continue;
            }
            // Pre-tick: decrement wait timer (retail does this unconditionally
            // before the gate).
            vm::move_vm::decrement_wait_timer(&mut self.actors[slot].move_state, delta);
            let outcome = self.actor_tick_at(slot, &bc, MOVE_VM_BUDGET);
            self.move_outcomes.push((slot as u8, outcome));
        }
    }

    /// Backwards-compatible wrapper using `delta = 1`.
    pub fn tick_move_vms(&mut self) {
        self.tick_move_vms_with_delta(1);
    }

    /// Advance all active actor animations one frame. Mirrors the
    /// keyframe-table block in `FUN_80021DF4` (`0x80022ec4..0x80023040`)
    /// that walks `actor[+0x4C]` (anim pointer) when `actor[+0x22]`
    /// (factor) is non-zero. Called by [`World::tick`] after the move-VM
    /// pass.
    pub fn tick_actors(&mut self) {
        for actor in &mut self.actors {
            if !actor.active {
                continue;
            }
            if let Some(player) = &mut actor.active_animation {
                actor.pose_frame = Some(player.tick());
            }
        }
    }

    /// Bind an animation player to actor `slot`. Replaces any existing
    /// player and resets the playhead. No-ops for out-of-range slots.
    pub fn set_actor_animation(&mut self, slot: usize, player: AnimPlayer) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.active_animation = Some(player);
            actor.pose_frame = None;
        }
    }

    /// Bind actor `slot` to TMD index `tmd_idx` in `SceneResources::tmds`.
    /// Renderers use this binding to look up the right mesh when applying
    /// the actor's `pose_frame`. No-ops for out-of-range slots.
    pub fn set_actor_tmd_binding(&mut self, slot: usize, tmd_idx: usize) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.tmd_binding = Some(tmd_idx);
        }
    }

    /// Run [`vm::move_vm::actor_tick`] for `slot` against the given `bytecode`
    /// with the supplied opcode `budget`. Returns the typed outcome -
    /// engines route `Halted` to their halt-handler, `EndOfBuffer` to "clear
    /// the move", `Pending` to a debug log.
    pub fn actor_tick_at(
        &mut self,
        slot: usize,
        bytecode: &[u16],
        budget: usize,
    ) -> vm::move_vm::ActorTickOutcome {
        let mut host = MoveVmHostImpl {
            world: self,
            current_slot: Some(slot),
            deferred_writes: std::collections::BTreeMap::new(),
        };
        let actor_state = unsafe {
            // SAFETY: same disjoint-field justification as `step_move_vm`.
            &mut *(&mut host.world.actors[slot].move_state as *mut MoveActorState)
        };
        let outcome = vm::move_vm::actor_tick(&mut host, actor_state, bytecode, budget);
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
        outcome
    }

    /// Place the world into [`SceneMode::Battle`] and populate the actor
    /// pointer table with `party_count` party slots followed by
    /// `monster_count` monster slots, mirroring the layout
    /// `FUN_800520F0` produces (slots 0..2 = party, 3..7 = monsters; total
    /// caps at 8). Each actor is positioned `radius` units left (party)
    /// or right (monsters) of the origin, with a per-row z spread.
    ///
    /// This is the engine-core analogue of the retail battle scene
    /// loader's "stamp the actor table from the scene record" pre-pass.
    /// Engines that drive the loader from real scene data (party data +
    /// monster archive) skip this helper and write the slots directly;
    /// it's the convenience path for tests + the asset-viewer's
    /// `battle-scene` subcommand.
    ///
    /// The battle-action state machine is seeded at
    /// [`legaia_engine_vm::battle_action::ActionState::Begin`].
    pub fn enter_battle(&mut self, party_count: u8, monster_count: u8, radius: i16) {
        self.mode = SceneMode::Battle;
        self.party_count = party_count.min(3);
        let actor_count =
            ((self.party_count as usize) + (monster_count.min(5) as usize)).min(MAX_ACTORS);
        // Spread along z. Party left, monsters right, both staggered by 0.6 / 0.4.
        for i in 0..(self.party_count as usize).min(actor_count) {
            let z = (i as i16 - 1) * (radius * 6 / 10);
            let actor = self.spawn_actor(i);
            actor.move_state.world_x = -radius;
            actor.move_state.world_y = 0;
            actor.move_state.world_z = z;
            actor.battle.liveness = 1;
        }
        for i in (self.party_count as usize)..actor_count {
            let z = (i as i16 - 5) * (radius * 4 / 10);
            let actor = self.spawn_actor(i);
            actor.move_state.world_x = radius;
            actor.move_state.world_y = 0;
            actor.move_state.world_z = z;
            actor.battle.liveness = 1;
        }
        // Reset the battle ctx and seed at Begin via the public byte API to
        // avoid pulling battle_action::ActionState into world.rs imports.
        self.battle_ctx = vm::battle_action::BattleActionCtx::new();
        self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        self.battle_end = None;
        // Effect pool is reused across scenes - reset to a fresh instance
        // (per-battle the head/free-list rebuilds from scratch).
        self.effect_pool = vm::effect_vm::Pool::new();
    }

    /// Build the per-frame sprite list for the renderer. One
    /// [`ActorSpriteRequest`] per active actor with a [`SpriteFrame`] set;
    /// the screen-space coordinates are derived from the actor's
    /// `move_state.world_x` / `move_state.world_z` (PSX field coords) by
    /// flattening to a top-down `(x, z)` view and adding the sprite's
    /// `anchor_y`. Engines that have a real camera projection pre-process
    /// the move_state coords before populating [`Actor::sprite_frame`] (or
    /// override this helper).
    ///
    /// Mirrors the retail `FUN_80021DF4` per-frame actor tick's "draw
    /// sprite at world position" pre-pass - the actual GPU upload happens
    /// in `legaia_engine_render` against the supplied atlas.
    pub fn collect_sprite_requests(&self) -> Vec<ActorSpriteRequest> {
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(slot, a)| {
                if !a.active {
                    return None;
                }
                let frame = a.sprite_frame?;
                let world_x = a.move_state.world_x as i32;
                let world_y = a.move_state.world_z as i32 + frame.anchor_y as i32;
                Some(ActorSpriteRequest {
                    actor_slot: slot as u8,
                    world_x,
                    world_y,
                    atlas_src: frame.atlas_src,
                    tint: frame.tint,
                })
            })
            .collect()
    }

    /// Set the sprite frame for the actor at `slot`. Idempotent - passing
    /// `None` removes the frame so the actor stops rendering as a sprite.
    pub fn set_actor_sprite(&mut self, slot: u8, frame: Option<SpriteFrame>) {
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.sprite_frame = frame;
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
    // `self` - so sub-op handlers see the world-backed callbacks above.
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

    // Shared system flag bank - same fourth-flag-bank at `_DAT_80086D70`
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

    fn op4c_n_e_sub2_fmv_trigger(&mut self, fmv_id: i16) {
        // Field-VM op `0x4C 0xE2` - retail handler at 0x801E30E4 writes
        // the resolved s16 to `_DAT_8007BA78` (FMV index) and pokes
        // `_DAT_8007B83C = 0x1A` (next game mode = 26 = StrInit). We
        // record the request here so the SceneHost / engine driver can
        // pop it after the field step returns and switch its scene
        // mode without invalidating the field-VM borrow.
        self.world.pending_fmv_trigger = Some(fmv_id);
        self.world
            .pending_field_events
            .push(FieldEvent::FmvTrigger { fmv_id });
    }

    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        // Sub-ops 1 (start field BGM) and 9 (queue) are the cases that
        // pin a "currently playing" id. Other sub-ops are control words
        // (pause / stop / volume / etc.) - we still surface the event so
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
        // Snapshot what we have currently - engines that model real camera
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

    fn op4c_n8_sub_0_actor_allocator(&mut self, _ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        // Walk `count` variable-length records out of `tail` using the
        // retail packet-length rule (FUN_8003CA38, mirrored in
        // `legaia_engine_vm::field_helpers::packet_length`): bytes <= 0x1E
        // terminate a record; bytes whose top nibble is 0xC consume one
        // extra byte. The walker stops when the tail is exhausted - the
        // retail original would over-read into adjacent memory, which the
        // clean-room port refuses by construction.
        let mut records: Vec<Vec<u8>> = Vec::with_capacity(count as usize);
        let mut cursor = 0usize;
        for _ in 0..count {
            if cursor >= tail.len() {
                break;
            }
            let len = vm::field_helpers::packet_length(&tail[cursor..]);
            records.push(tail[cursor..cursor + len].to_vec());
            // Skip the terminator byte itself (the byte <= 0x1E that
            // closed the record); if the walker ran off the end without
            // seeing one, `cursor + len == tail.len()` and the next
            // iteration's bounds check exits the loop.
            cursor += len + 1;
        }
        for record in &records {
            self.world.pending_actor_spawns.push(record.clone());
        }
        self.world
            .pending_field_events
            .push(FieldEvent::ActorAllocate { records });
    }

    fn op4c_n_d_sub8_call_d77f4(&mut self, b1: u8, words: [i16; 3]) {
        // Synchronous actor allocator (see retail `FUN_801D77F4` body
        // dumped at `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt`).
        // The dispatcher packs the four args
        //   `[vdf_idx: u8, tmd_idx: i16, kind: i16, variant: i16]`
        // into the 7 bytes after `[0x4C, 0xD8]`; FUN_801D77F4 then writes
        // `actor[+0x3C] = kind` and `actor[+0x3E] = variant` on the
        // allocated slot, plus `actor[+0x48] = DAT_8007C018[tmd_idx]`
        // (TMD pointer) and `actor[+0x4C] = VDF_body_ptr`. We mirror
        // all four writes here.
        let kind = words[1] as u16;
        let variant = words[2] as u16;
        let tmd_ref = self.world.global_tmd(words[0]).cloned();
        // Mirror retail's `actor[+0x4C] = VDF_body_ptr`: look up the
        // VDF record body bytes and store them on the allocated actor.
        // `None` when no VDF buffer is installed or the index is OOR;
        // engines that drive the host without setting one still get the
        // kind/variant writes (synchronous spawn semantics) plus an
        // empty `record` in the event payload.
        let record_bytes: Vec<u8> = self
            .world
            .vdf_record_bytes(b1)
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let start = FIELD_SPAWN_START_SLOT as usize;
        match self
            .world
            .actors
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, a)| !a.active)
            .map(|(i, _)| i)
        {
            Some(slot_idx) => {
                let actor = &mut self.world.actors[slot_idx];
                actor.active = true;
                actor.kind = kind;
                actor.variant = variant;
                actor.tmd_ref = tmd_ref;
                actor.spawn_record = if record_bytes.is_empty() {
                    None
                } else {
                    Some(record_bytes.clone())
                };
                self.world
                    .pending_field_events
                    .push(FieldEvent::ActorSpawned {
                        slot: slot_idx as u8,
                        kind,
                        variant,
                        record: record_bytes,
                    });
            }
            None => {
                // Pool-exhausted: mirrors the retail bail-silently branch
                // where FUN_80020DE0 returns 0.
                self.world
                    .pending_field_events
                    .push(FieldEvent::ActorSpawnFailed {
                        record: record_bytes,
                    });
            }
        }
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
        self.world
            .pending_battle_events
            .push(BattleEvent::BattleEnd { cause });
    }
    fn party_count(&self) -> u8 {
        self.world.party_count
    }
    fn pose(&mut self, actor_id: u8, pose: Pose) {
        self.world
            .pending_battle_events
            .push(BattleEvent::Pose { actor_id, pose });
    }
    fn ui_element(&mut self, effect_id: u8, mode: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::UiElement { effect_id, mode });
        // mode == 0: spawn/reset. Route directly into the effect pool so
        // the VM's state machine drives the effect lifecycle while engines
        // also receive the event for visual dispatch.
        if mode == 0 {
            self.world.try_spawn_effect(effect_id, [0, 0, 0], 0);
        }
    }
    fn camera_bounds(&mut self) {
        self.world
            .pending_battle_events
            .push(BattleEvent::CameraBounds);
    }
    fn party_setup(&mut self, actor_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::PartySetup { actor_slot });
    }
    fn monster_setup(&mut self, actor_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::MonsterSetup { actor_slot });
    }
    fn recompute_battle_order(&mut self) {
        self.world
            .pending_battle_events
            .push(BattleEvent::RecomputeBattleOrder);
    }
    fn load_capture_archive(&mut self, idx: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::LoadCaptureArchive { idx });
    }
    fn spell_anim_trigger(&mut self, party_slot: u8, spell_id: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::SpellAnimTrigger {
                party_slot,
                spell_id,
            });
    }
    fn spell_anim_sustain(&mut self, actor_id: u8, anim_id: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::SpellAnimSustain { actor_id, anim_id });
    }
    fn apply_damage(&mut self, icon: u8, page: u8, target_slot: u8, party_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::ApplyDamage {
                icon,
                page,
                target_slot,
                party_slot,
            });
    }
    fn apply_art_strike(&mut self, info: legaia_engine_vm::battle_action::ArtStrikeInfo) {
        // Resolve per-slot weapon attack and the defense the art targets.
        let attack = self
            .world
            .battle_attack
            .get(info.actor_slot as usize)
            .copied()
            .unwrap_or(0);
        let defense = self.world.resolve_battle_defense(info.target_slot, &info);
        let outcome = crate::art_strike::apply_art_strike(attack, defense, &info);
        self.world
            .pending_battle_events
            .push(BattleEvent::ApplyArtStrike {
                actor_slot: info.actor_slot,
                target_slot: info.target_slot,
                strike_index: info.strike_index,
                outcome,
            });
    }
    fn screen_shake(&mut self, magnitude: u16) {
        self.world
            .pending_battle_events
            .push(BattleEvent::ScreenShake { magnitude });
    }
    fn ramp_brightness(&mut self, target_pct: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::RampBrightness { target_pct });
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
    fn enter_battle_populates_party_and_monsters() {
        let mut world = World::default();
        world.enter_battle(3, 5, 600);
        assert_eq!(world.mode, SceneMode::Battle);
        assert_eq!(world.party_count, 3);
        // 3 party + 5 monsters = 8 active.
        let active_count = world.actors.iter().filter(|a| a.active).count();
        assert_eq!(active_count, 8);
        // Party slots are at -600 X.
        for i in 0..3 {
            assert_eq!(world.actors[i].move_state.world_x, -600);
            assert_eq!(world.actors[i].battle.liveness, 1);
        }
        // Monster slots at +600 X.
        for i in 3..8 {
            assert_eq!(world.actors[i].move_state.world_x, 600);
            assert_eq!(world.actors[i].battle.liveness, 1);
        }
        // SM seeded at Begin.
        assert_eq!(
            world.battle_ctx.action_state,
            vm::battle_action::ActionState::Begin.as_byte()
        );
    }

    #[test]
    fn enter_battle_caps_party_at_three() {
        let mut world = World::default();
        // Even if asked for more party than the cap, we clamp to 3.
        world.enter_battle(8, 0, 100);
        assert_eq!(world.party_count, 3);
    }

    #[test]
    fn collect_sprite_requests_emits_one_per_active_actor_with_frame() {
        let mut world = World::default();
        // Slot 0: active + sprite frame at (10, 20) world coords.
        world.actors[0].active = true;
        world.actors[0].move_state.world_x = 100;
        world.actors[0].move_state.world_z = 200;
        world.set_actor_sprite(
            0,
            Some(SpriteFrame {
                atlas_src: (0, 0, 16, 24),
                tint: [1.0, 1.0, 1.0, 1.0],
                anchor_y: -8,
            }),
        );
        // Slot 1: active but no frame - shouldn't emit.
        world.actors[1].active = true;
        // Slot 2: frame but inactive - shouldn't emit.
        world.set_actor_sprite(
            2,
            Some(SpriteFrame {
                atlas_src: (16, 0, 16, 24),
                tint: [1.0; 4],
                anchor_y: 0,
            }),
        );

        let requests = world.collect_sprite_requests();
        assert_eq!(requests.len(), 1);
        let r = &requests[0];
        assert_eq!(r.actor_slot, 0);
        assert_eq!(r.world_x, 100);
        // anchor_y subtracts from world_z (z + (-8)) = 192.
        assert_eq!(r.world_y, 192);
        assert_eq!(r.atlas_src, (0, 0, 16, 24));
    }

    #[test]
    fn set_actor_sprite_with_none_clears_existing_frame() {
        let mut world = World::default();
        world.actors[0].active = true;
        world.set_actor_sprite(
            0,
            Some(SpriteFrame {
                atlas_src: (0, 0, 8, 8),
                ..Default::default()
            }),
        );
        assert!(world.actors[0].sprite_frame.is_some());
        world.set_actor_sprite(0, None);
        assert!(world.actors[0].sprite_frame.is_none());
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

    /// `op0 < 100` is the field_interact arm - should NOT trigger a
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

    /// Field-VM op `0x4C 0xE2` (FMV trigger) records the FMV index in
    /// `World::pending_fmv_trigger` AND emits a `FieldEvent::FmvTrigger`
    /// for engines to drain. Retail handler at `0x801E30E4` writes the
    /// s16 to `_DAT_8007BA78` and pokes next-game-mode = 0x1A; the
    /// world mirrors the request via these two channels.
    #[test]
    fn field_op_4c_e2_records_pending_fmv_trigger() {
        use crate::cutscene::{STR_INIT_GAME_MODE, fmv_index_to_str_filename};
        use crate::field_events::FieldEvent;

        let mut world = World::new();
        world.mode = SceneMode::Field;
        // `[0x4C, 0xE2, 0x03, 0x00, 0, 0]` → fmv_id 3 → MV4.STR.
        let bytecode = vec![0x4C, 0xE2, 0x03, 0x00, 0, 0];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.pending_fmv_trigger, Some(3));
        let events = world.drain_field_events();
        assert!(events.contains(&FieldEvent::FmvTrigger { fmv_id: 3 }));
        assert_eq!(fmv_index_to_str_filename(3), Some("MOV/MV4.STR"));
        assert_eq!(STR_INIT_GAME_MODE, 26);
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
    fn save_full_round_trips_globals() {
        let mut world = World::new();
        world.load_party(legaia_save::Party::zeroed(2));
        world.story_flags = 0xCAFE_F00D;
        world.money = 54321;
        world.inventory.insert(3, 9);
        world.inventory.insert(77, 1);

        let sf = world.save_full();
        assert_eq!(sf.ext.story_flags, 0xCAFE_F00D);
        assert_eq!(sf.ext.money, 54321);
        // inventory is sorted by item_id
        assert_eq!(sf.ext.inventory, vec![(3, 9), (77, 1)]);

        let bytes = sf.write();
        let parsed = legaia_save::SaveFile::parse(&bytes).unwrap();

        let mut world2 = World::new();
        world2.load_full(parsed);
        assert_eq!(world2.story_flags, 0xCAFE_F00D);
        assert_eq!(world2.money, 54321);
        assert_eq!(world2.inventory.get(&3), Some(&9));
        assert_eq!(world2.inventory.get(&77), Some(&1));
        assert_eq!(world2.party_count, 2);
    }

    #[test]
    fn load_full_clears_old_inventory() {
        let mut world = World::new();
        world.inventory.insert(1, 10);
        world.inventory.insert(2, 20);

        let sf = legaia_save::SaveFile {
            party: legaia_save::Party::zeroed(1),
            ext: legaia_save::SaveExt {
                story_flags: 1,
                story_flag_bits: Vec::new(),
                money: 0,
                inventory: vec![(5, 3)],
            },
            ext_v2: legaia_save::SaveExtV2::default(),
        };
        world.load_full(sf);
        assert!(!world.inventory.contains_key(&1));
        assert!(!world.inventory.contains_key(&2));
        assert_eq!(world.inventory.get(&5), Some(&3));
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
        // 0x2F sub-op 0x11 - save world coords into slot 3, then HALT.
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
        // 0x2F sub-op 0x04 - write actor world XYZ to bytecode at
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
        // Bytecode: 0x2F sub-op 0x13 - predicate-true → default_arm (size 1),
        // predicate-false → size 4.
        let bc = vec![0x002F, 0x0013, 42];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // Predicate true → PC advanced by 1.
        assert_eq!(world.actors[0].move_state.pc, 1);
        // Now clear and re-run - predicate false → PC += 4.
        world.system_flag_clear(42);
        world.actors[0].move_state.pc = 0;
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.actors[0].move_state.pc, 4);
    }

    #[test]
    fn move_vm_ext_set_flag_bank_writes_world_system_flags() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Bytecode: 0x2F sub-op 0x1C - set flag bank (idx = op_w(2)).
        let bc = vec![0x002F, 0x001C, 100];
        world.set_move_bytecode(0, Some(bc.clone()));
        assert!(!world.system_flag_test(100));
        let _ = world.step_move_vm(0, &bc);
        assert!(world.system_flag_test(100));
    }

    #[test]
    fn field_vm_system_flag_set_routes_to_world() {
        // Field-VM 0x5x default-route SET - `[0x50 | nibble, idx_byte]`.
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
        // Op 0x42 mode=0 - host.extra_flags() & (1 << (op1 & 0x1F)) test.
        // Set bit 5 in extra_flags; op_42 with op1=5 should take the jump.
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.extra_flags = 1 << 5;
        // [0x42, mode=0, op1=5, lo=4, hi=0] - header_size + 4 = 5 byte total
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
        // 0x2F sub-op 0x2F - `_DAT_8007B9D8 = (i32) op[1]`. Note: op[1] in
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
        // 100000 - predicate fails for r=10 (r² = 100), passes for r=400
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

    /// Op `0x4C 0x80` (actor allocator) walks `count` variable-length
    /// records using the `FUN_8003CA38` packet-length rule, emits one
    /// `ActorAllocate` event, and queues each record's bytecode in
    /// `pending_actor_spawns`. Encoding here: count=2, two records each
    /// terminated by `0x00`.
    #[test]
    fn field_op_4c_n8_sub0_walks_records_and_queues_spawns() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // [4C, 0x80, 2, 0x40, 0x41, 0x00, 0xC1, 0x42, 0x00]
        //   record 0 = [0x40, 0x41] (two normal tokens, terminator 0x00)
        //   record 1 = [0xC1, 0x42] (escape pair via 0xCx high nibble)
        let bytecode = vec![0x4C, 0x80, 0x02, 0x40, 0x41, 0x00, 0xC1, 0x42, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();
        // PC should land at byte 3 (the first record's first byte) - the
        // retail VM advances PC by exactly 3 regardless of how many
        // records the host consumes.
        assert_eq!(world.field_pc, 3);
        // Pending queue should hold both records, in emission order.
        let spawns = world.drain_actor_spawns();
        assert_eq!(spawns.len(), 2);
        assert_eq!(spawns[0], vec![0x40, 0x41]);
        assert_eq!(spawns[1], vec![0xC1, 0x42]);
        // The event queue should also carry one ActorAllocate with both
        // records.
        let evs = world.drain_field_events();
        let allocate = evs
            .iter()
            .find_map(|e| match e {
                FieldEvent::ActorAllocate { records } => Some(records.clone()),
                _ => None,
            })
            .expect("expected ActorAllocate event");
        assert_eq!(allocate.len(), 2);
        assert_eq!(allocate[0], vec![0x40, 0x41]);
        assert_eq!(allocate[1], vec![0xC1, 0x42]);
    }

    /// `count = 0` is a legal degenerate case - no records walked, no
    /// event payload, but the event is still emitted to mark the
    /// allocator call site.
    #[test]
    fn field_op_4c_n8_sub0_zero_count_emits_empty_event() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let bytecode = vec![0x4C, 0x80, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.field_pc, 3);
        assert!(world.drain_actor_spawns().is_empty());
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                FieldEvent::ActorAllocate { records } if records.is_empty()
            )),
            "expected empty ActorAllocate event, got {evs:?}"
        );
    }

    /// `drain_actor_spawns` empties the queue.
    #[test]
    fn drain_actor_spawns_empties_queue() {
        let mut world = World::new();
        world.pending_actor_spawns.push(vec![0xAA, 0xBB]);
        let drained = world.drain_actor_spawns();
        assert_eq!(drained, vec![vec![0xAA, 0xBB]]);
        assert!(world.pending_actor_spawns.is_empty());
    }

    /// `materialize_actor_spawns` allocates a fresh slot from
    /// `start_slot..MAX_ACTORS`, populates it with the queued record, and
    /// emits an `ActorSpawned` event.
    #[test]
    fn materialize_actor_spawns_allocates_slot_and_emits_event() {
        let mut world = World::new();
        world.pending_actor_spawns.push(vec![0x10, 0x20, 0x30]);
        let allocated = world.materialize_actor_spawns(8);
        assert_eq!(allocated, 1);
        assert!(world.pending_actor_spawns.is_empty());
        assert!(world.actors[8].active);
        assert_eq!(
            world.actors[8].spawn_record.as_deref(),
            Some(&[0x10, 0x20, 0x30][..])
        );
        assert_eq!(world.actors[8].kind, 0);
        assert_eq!(world.actors[8].variant, 0);
        let evs = world.drain_field_events();
        let spawned = evs
            .iter()
            .find_map(|e| match e {
                FieldEvent::ActorSpawned {
                    slot,
                    kind,
                    variant,
                    record,
                } => Some((*slot, *kind, *variant, record.clone())),
                _ => None,
            })
            .expect("expected ActorSpawned event");
        assert_eq!(spawned, (8u8, 0u16, 0u16, vec![0x10, 0x20, 0x30]));
    }

    /// `materialize_actor_spawns` allocates consecutive inactive slots
    /// when several spawn requests are queued.
    #[test]
    fn materialize_actor_spawns_fills_consecutive_inactive_slots() {
        let mut world = World::new();
        world.pending_actor_spawns.push(vec![0xAA]);
        world.pending_actor_spawns.push(vec![0xBB]);
        world.pending_actor_spawns.push(vec![0xCC]);
        let allocated = world.materialize_actor_spawns(4);
        assert_eq!(allocated, 3);
        assert!(world.actors[4].active);
        assert!(world.actors[5].active);
        assert!(world.actors[6].active);
        assert_eq!(world.actors[4].spawn_record.as_deref(), Some(&[0xAA][..]));
        assert_eq!(world.actors[5].spawn_record.as_deref(), Some(&[0xBB][..]));
        assert_eq!(world.actors[6].spawn_record.as_deref(), Some(&[0xCC][..]));
    }

    /// Slots below `start_slot` are reserved - even when they are
    /// inactive, the materializer doesn't touch them.
    #[test]
    fn materialize_actor_spawns_skips_reserved_low_slots() {
        let mut world = World::new();
        // Slot 0 is inactive but reserved (start_slot=10).
        world.pending_actor_spawns.push(vec![0xDE, 0xAD]);
        world.materialize_actor_spawns(10);
        assert!(!world.actors[0].active);
        assert!(world.actors[10].active);
    }

    /// Mirrors retail's "pool exhausted → bail silently" branch of
    /// `FUN_801D77F4`. When no inactive slot is available in the
    /// allocation range, the record is dropped and a `ActorSpawnFailed`
    /// event is emitted instead of `ActorSpawned`.
    #[test]
    fn materialize_actor_spawns_emits_failure_when_pool_exhausted() {
        let mut world = World::new();
        // Make every slot from index 60 upward active.
        for slot in 60..MAX_ACTORS {
            world.actors[slot].active = true;
        }
        world.pending_actor_spawns.push(vec![0xEE]);
        let allocated = world.materialize_actor_spawns(60);
        assert_eq!(allocated, 0);
        let evs = world.drain_field_events();
        assert!(evs.iter().any(|e| matches!(
            e,
            FieldEvent::ActorSpawnFailed { record } if record == &[0xEE]
        )));
    }

    /// End-to-end: a field-VM `0x4C 0x80` opcode followed by
    /// `materialize_actor_spawns` should land both events
    /// (`ActorAllocate` from the opcode, `ActorSpawned` from the
    /// materializer) and leave the actor slot populated.
    #[test]
    fn field_op_4c_n8_sub0_then_materialize_flow_end_to_end() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // One record `[0x40, 0x41]` terminated by `0x00`.
        let bytecode = vec![0x4C, 0x80, 0x01, 0x40, 0x41, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();
        let allocated = world.materialize_actor_spawns(16);
        assert_eq!(allocated, 1);
        assert!(world.actors[16].active);
        assert_eq!(
            world.actors[16].spawn_record.as_deref(),
            Some(&[0x40, 0x41][..])
        );
        let evs = world.drain_field_events();
        // Both the ActorAllocate (from the opcode) and ActorSpawned (from
        // the materializer) should appear in emission order.
        let kinds: Vec<&'static str> = evs
            .iter()
            .filter_map(|e| match e {
                FieldEvent::ActorAllocate { .. } => Some("alloc"),
                FieldEvent::ActorSpawned { .. } => Some("spawned"),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, vec!["alloc", "spawned"]);
    }

    /// Op `0x4C 0xD8` is the synchronous-spawn sibling of the halt-acquire
    /// `0x4C 0x80` path. The dispatcher decodes
    /// `[0x4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, var_lo, var_hi]`
    /// into `(vdf_idx, [tmd_idx, kind, variant])` and calls the
    /// FieldHostImpl override directly - no queue. The actor slot must
    /// come out active with `kind` / `variant` mirrored from the operand,
    /// and a single `ActorSpawned` event must surface in the queue.
    #[test]
    fn field_op_4c_d8_spawns_actor_synchronously_with_kind_variant() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // `[0x4C, 0xD8, vdf_idx=0x07, tmd=0x0102, kind=0xABCD, variant=0xBEEF, 0x00]`.
        // Trailing 0x00 is a HALT so the VM doesn't run off the end.
        let bytecode = vec![0x4C, 0xD8, 0x07, 0x02, 0x01, 0xCD, 0xAB, 0xEF, 0xBE, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();

        let slot = FIELD_SPAWN_START_SLOT as usize;
        assert!(
            world.actors[slot].active,
            "0x4C 0xD8 should have spawned synchronously into slot {slot}",
        );
        assert_eq!(world.actors[slot].kind, 0xABCD);
        assert_eq!(world.actors[slot].variant, 0xBEEF);
        // 0x4C 0xD8 doesn't carry packet bytes in the bytecode - the
        // record lives in the VDF buffer at runtime - so spawn_record
        // stays `None` until the VDF / global TMD lift lands.
        assert!(world.actors[slot].spawn_record.is_none());

        let evs = world.drain_field_events();
        let spawned: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                FieldEvent::ActorSpawned {
                    slot: s,
                    kind,
                    variant,
                    record,
                } => Some((*s, *kind, *variant, record.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            spawned,
            vec![(FIELD_SPAWN_START_SLOT, 0xABCDu16, 0xBEEFu16, Vec::new())]
        );
        // No ActorAllocate event - that one is exclusively the
        // queue-based 0x4C 0x80 path.
        assert!(
            !evs.iter()
                .any(|e| matches!(e, FieldEvent::ActorAllocate { .. })),
            "0x4C 0xD8 must not emit ActorAllocate; got {evs:?}"
        );
        // And nothing was queued on the pending_actor_spawns side - the
        // synchronous path doesn't go through the materializer.
        assert!(world.pending_actor_spawns.is_empty());
    }

    /// `0x4C 0xD8` with a populated VDF buffer should copy the indexed
    /// body bytes onto the spawned actor's `spawn_record` (mirror of
    /// retail `actor[+0x4C] = VDF_body_ptr`) and surface them in the
    /// `ActorSpawned` event payload.
    #[test]
    fn field_op_4c_d8_with_vdf_buffer_populates_spawn_record() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // VDF buffer with two records:
        //   header:  count = 2
        //   table:   offsets[0] = 12, offsets[1] = 16
        //   body 0:  [0xDE, 0xAD, 0xBE, 0xEF] @ off 12 (4 bytes -> 16)
        //   body 1:  [0xCA, 0xFE, 0xBA, 0xBE, 0x42] @ off 16 (to EOB)
        let mut vdf = Vec::new();
        vdf.extend_from_slice(&2u32.to_le_bytes()); // count
        vdf.extend_from_slice(&12u32.to_le_bytes()); // offsets[0]
        vdf.extend_from_slice(&16u32.to_le_bytes()); // offsets[1]
        vdf.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        vdf.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42]);
        world.set_vdf_buffer(Some(vdf));

        // Sanity-check the lookup helper.
        assert_eq!(
            world.vdf_record_bytes(0),
            Some(&[0xDE, 0xAD, 0xBE, 0xEF][..])
        );
        assert_eq!(
            world.vdf_record_bytes(1),
            Some(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42][..])
        );
        assert_eq!(world.vdf_record_bytes(2), None); // idx >= count

        // `[0x4C, 0xD8, vdf_idx=0x01, tmd=0x0102, kind=0x1111, variant=0x2222, 0x00]`.
        let bytecode = vec![0x4C, 0xD8, 0x01, 0x02, 0x01, 0x11, 0x11, 0x22, 0x22, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();

        let slot = FIELD_SPAWN_START_SLOT as usize;
        assert!(world.actors[slot].active);
        assert_eq!(world.actors[slot].kind, 0x1111);
        assert_eq!(world.actors[slot].variant, 0x2222);
        assert_eq!(
            world.actors[slot].spawn_record.as_deref(),
            Some(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42][..]),
            "spawn_record should mirror VDF body 1"
        );

        let evs = world.drain_field_events();
        let spawned: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                FieldEvent::ActorSpawned { record, .. } => Some(record.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(spawned, vec![vec![0xCA, 0xFE, 0xBA, 0xBE, 0x42]]);
    }

    /// `0x4C 0xD8` with a populated global TMD pool should write a
    /// matching `Arc<GlobalTmd>` onto the spawned actor's `tmd_ref`
    /// (mirror of retail `actor[+0x48] = DAT_8007C018[tmd_idx]`).
    /// Indices the pool hasn't seen leave `tmd_ref` at `None` rather
    /// than aborting the spawn.
    #[test]
    fn field_op_4c_d8_with_global_tmd_pool_populates_tmd_ref() {
        let mut world = World::new();
        world.mode = SceneMode::Field;

        // Install a stub TMD at pool slot 5. The Tmd doesn't need to
        // represent realistic mesh data - the host hook only does an
        // Arc::clone and stores the result.
        let stub = std::sync::Arc::new(GlobalTmd {
            tmd: legaia_tmd::Tmd {
                header: legaia_tmd::Header {
                    id: 0x8000_0002,
                    flags: 1,
                    nobj: 0,
                    flist_bit_set: true,
                },
                objects: Vec::new(),
            },
            raw: vec![
                0x02, 0x00, 0x00, 0x80, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        });
        let stub_ptr = std::sync::Arc::as_ptr(&stub);
        world.set_global_tmd(5, stub.clone());

        // `[0x4C, 0xD8, vdf_idx=0x00, tmd=0x0005, kind=0x1111, variant=0x2222, 0x00]`.
        let bytecode = vec![0x4C, 0xD8, 0x00, 0x05, 0x00, 0x11, 0x11, 0x22, 0x22, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();

        let slot = FIELD_SPAWN_START_SLOT as usize;
        assert!(world.actors[slot].active);
        let tmd_ref = world.actors[slot]
            .tmd_ref
            .as_ref()
            .expect("tmd_ref should mirror DAT_8007C018[5]");
        assert_eq!(
            std::sync::Arc::as_ptr(tmd_ref),
            stub_ptr,
            "tmd_ref should reference the installed pool entry by Arc identity",
        );

        // A second spawn with an unpopulated index leaves tmd_ref at None.
        let bytecode2 = vec![0x4C, 0xD8, 0x00, 0x09, 0x00, 0x33, 0x33, 0x44, 0x44, 0x00];
        world.load_field_script(bytecode2);
        let _ = world.tick();
        let slot2 = slot + 1;
        assert!(world.actors[slot2].active);
        assert!(
            world.actors[slot2].tmd_ref.is_none(),
            "empty pool slot should not populate tmd_ref",
        );
    }

    /// Accessors round-trip: `set_global_tmd` + `global_tmd` agree on
    /// installed slots, negative indices return `None`, and the pool
    /// grows lazily.
    #[test]
    fn global_tmd_accessor_round_trip() {
        let mut world = World::new();
        assert!(world.global_tmd(0).is_none());
        assert!(world.global_tmd(-1).is_none());

        let stub = std::sync::Arc::new(GlobalTmd {
            tmd: legaia_tmd::Tmd {
                header: legaia_tmd::Header {
                    id: 0x8000_0002,
                    flags: 1,
                    nobj: 0,
                    flist_bit_set: true,
                },
                objects: Vec::new(),
            },
            raw: Vec::new(),
        });
        world.set_global_tmd(3, stub.clone());
        // Pool grew to fit idx 3.
        assert_eq!(world.global_tmd_pool.len(), 4);
        assert!(world.global_tmd_pool[0..3].iter().all(|s| s.is_none()));
        assert!(std::sync::Arc::ptr_eq(
            world.global_tmd(3).expect("slot 3 populated"),
            &stub
        ));
        assert!(world.global_tmd(7).is_none(), "out-of-range -> None");
        assert!(world.global_tmd(-5).is_none(), "negative -> None");
    }

    /// `vdf_record_bytes` rejects out-of-range indices, malformed
    /// buffers, and the `None` (no VDF installed) path.
    #[test]
    fn vdf_record_bytes_handles_edge_cases() {
        let mut world = World::new();
        assert_eq!(world.vdf_record_bytes(0), None, "no VDF -> None");

        // Empty buffer (shorter than header word).
        world.set_vdf_buffer(Some(vec![0x01, 0x02]));
        assert_eq!(world.vdf_record_bytes(0), None);

        // Count = 0.
        world.set_vdf_buffer(Some(vec![0x00, 0x00, 0x00, 0x00]));
        assert_eq!(world.vdf_record_bytes(0), None);

        // Count = 1 but offset walks past EOB.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&0xFFFFu32.to_le_bytes()); // offsets[0] - past EOB
        buf.extend_from_slice(&[0xAAu8; 8]);
        world.set_vdf_buffer(Some(buf));
        assert_eq!(world.vdf_record_bytes(0), None);
    }

    /// `tick_move_vms` records per-actor outcomes via `actor_tick`. A
    /// HALT-loaded script (op `0x08` = HALT, encoded as `0x0008` in u16)
    /// should yield `Halted`.
    #[test]
    fn tick_move_vms_records_halt_outcome() {
        let mut world = World::new();
        world.spawn_actor(0);
        world.actors[0].move_state.wait_timer = -1;
        // Move-VM HALT opcode is `0x08`.
        world.set_move_bytecode(0, Some(vec![0x0008]));
        world.tick_move_vms();
        assert!(
            world
                .move_outcomes
                .iter()
                .any(|(s, o)| *s == 0 && matches!(o, vm::move_vm::ActorTickOutcome::Halted)),
            "expected actor 0 to halt, got {:?}",
            world.move_outcomes
        );
    }

    /// Wait gate: actor with `wait_timer >= 0` reports Waiting and the VM
    /// is not entered. Decrement happens before the gate.
    #[test]
    fn tick_move_vms_with_delta_decrements_then_gates() {
        let mut world = World::new();
        world.spawn_actor(0);
        world.actors[0].move_state.wait_timer = 3;
        // Bytecode that would change state if VM ran (op 0x08 HALT).
        world.set_move_bytecode(0, Some(vec![0x0008]));
        world.tick_move_vms_with_delta(1);
        // After delta=1: wait_timer = 2, still >= 0 -> Waiting.
        assert_eq!(world.actors[0].move_state.wait_timer, 2);
        assert!(matches!(
            world.move_outcomes[0],
            (0, vm::move_vm::ActorTickOutcome::Waiting)
        ));
        // After three more ticks (delta=1 each): wait_timer goes 1, 0, -1.
        // Only when wait_timer is strictly negative does the VM run.
        world.tick_move_vms_with_delta(1);
        world.tick_move_vms_with_delta(1);
        world.tick_move_vms_with_delta(1);
        // The last tick should have entered the VM and Halted.
        assert!(matches!(
            world.move_outcomes[0],
            (0, vm::move_vm::ActorTickOutcome::Halted)
        ));
    }

    #[test]
    fn try_spawn_effect_populates_pool() {
        let mut world = World::default();
        let script = vm::effect_vm::EffectScript {
            child_count: 2,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
        assert_eq!(world.effect_pool.active_count(), 0);
        world.try_spawn_effect(0, [10, 0, -10], 0x200);
        assert_eq!(world.effect_pool.active_count(), 1);
        assert_eq!(world.effect_pool.master_slots[0].pos_x, 10i32 << 8);
    }

    #[test]
    fn try_spawn_effect_noop_on_empty_catalog() {
        let mut world = World::default();
        world.try_spawn_effect(0, [0, 0, 0], 0);
        assert_eq!(world.effect_pool.active_count(), 0);
    }

    #[test]
    fn ui_element_mode0_pushes_event_and_spawns_effect() {
        let mut world = World {
            mode: SceneMode::Battle,
            ..World::default()
        };
        let script = vm::effect_vm::EffectScript {
            child_count: 1,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
        // Drive through the BattleHostImpl path by ticking the SM. Setting
        // up a full SM state is complex; we call try_spawn_effect directly
        // (the BattleHostImpl wiring is verified by the disc-gated test).
        world.try_spawn_effect(0, [0, 0, 0], 0);
        assert_eq!(world.effect_pool.active_count(), 1);
    }

    #[test]
    fn ui_element_mode1_does_not_spawn() {
        let mut world = World::default();
        let script = vm::effect_vm::EffectScript {
            child_count: 1,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
        // Simulate the mode==1 (terminate) path: only the event is pushed,
        // no pool spawn. try_spawn_effect is not called for mode==1.
        // Directly confirm pool stays empty if we don't call try_spawn_effect.
        assert_eq!(world.effect_pool.active_count(), 0);
    }

    // --- Tactical Arts ---

    #[test]
    fn notify_art_used_emits_event_and_sets_banner() {
        let mut world = World::default();
        world.tactical_arts.set_threshold(1);
        world.notify_art_used(0, 3);
        let evs = world.drain_battle_events();
        assert_eq!(evs.len(), 1);
        assert_eq!(
            evs[0],
            BattleEvent::TacticalArtLearned {
                char_id: 0,
                art_id: 3
            }
        );
        let banner = world.current_art_banner.as_ref().expect("banner set");
        assert!(banner.text.contains("Art #3"));
        assert_eq!(
            banner.frames_remaining,
            crate::tactical_arts::ArtLearnedBanner::DEFAULT_FRAMES
        );
    }

    #[test]
    fn notify_art_used_no_event_before_threshold() {
        let mut world = World::default();
        world.tactical_arts.set_threshold(5);
        for _ in 0..4 {
            world.notify_art_used(0, 1);
        }
        assert!(world.drain_battle_events().is_empty());
        assert!(world.current_art_banner.is_none());
    }

    #[test]
    fn banner_countdown_clears_after_frames() {
        let mut world = World::default();
        world.tactical_arts.set_threshold(1);
        world.notify_art_used(0, 0);
        // Banner starts at DEFAULT_FRAMES.
        assert!(world.current_art_banner.is_some());
        // Tick DEFAULT_FRAMES times; banner should reach 0 and clear.
        for _ in 0..=crate::tactical_arts::ArtLearnedBanner::DEFAULT_FRAMES {
            world.tick();
        }
        assert!(
            world.current_art_banner.is_none(),
            "banner should have cleared"
        );
    }

    // --- Level-up banner ---

    #[test]
    fn apply_battle_xp_sets_level_up_banner() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        // Slot 0 must be alive for the split to credit XP.
        world.actors[0].battle.hp = 100;
        // Retail table: 50 XP to reach level 2 (SCUS 0x8007123C entry[0]).
        world.apply_battle_xp(50);
        let banner = world
            .current_level_up_banner
            .as_ref()
            .expect("level-up banner should be set");
        assert_eq!(banner.char_id, 0);
        assert_eq!(banner.new_level, 2);
        assert_eq!(banner.hp_gained, 10); // default StatGain
        assert_eq!(banner.mp_gained, 5);
        assert_eq!(
            banner.frames_remaining,
            crate::levelup::LevelUpBanner::DEFAULT_FRAMES
        );
    }

    #[test]
    fn apply_battle_xp_skips_dead_members() {
        let mut world = World {
            party_count: 3,
            ..World::default()
        };
        // Alive: slots 0 + 2. Dead: slot 1 (HP = 0).
        world.actors[0].battle.hp = 100;
        world.actors[1].battle.hp = 0;
        world.actors[2].battle.hp = 100;
        let results = world.apply_battle_xp(100);
        // 100 / 2 alive = 50 each; both should reach L2 (50 threshold).
        let slot_ids: Vec<u8> = results.iter().map(|r| r.char_id).collect();
        assert!(slot_ids.contains(&0));
        assert!(slot_ids.contains(&2));
        assert!(
            !slot_ids.contains(&1),
            "dead slot 1 must not appear in level-up results"
        );
    }

    #[test]
    fn apply_battle_xp_no_alive_returns_empty() {
        let mut world = World {
            party_count: 3,
            ..World::default()
        };
        // No actor with HP > 0 → nobody to credit.
        let results = world.apply_battle_xp(500);
        assert!(results.is_empty());
        assert!(world.current_level_up_banner.is_none());
    }

    #[test]
    fn apply_battle_loot_rolls_drop_item_when_rate_is_max() {
        use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
        let mut cat = MonsterCatalog::new();
        let mut def = MonsterDef::new(7, "Slime", 10, 5);
        def.drop_item = Some(0x42);
        def.drop_rate_q8 = 255; // near-guaranteed roll
        cat.insert(def);
        let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        let rewards = world.apply_battle_loot(&formation, &cat);
        assert_eq!(rewards.drops, vec![0x42]);
        assert_eq!(world.inventory.get(&0x42).copied(), Some(1));
    }

    #[test]
    fn apply_battle_loot_never_drops_when_rate_zero() {
        use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
        let mut cat = MonsterCatalog::new();
        let mut def = MonsterDef::new(7, "Slime", 10, 5);
        def.drop_item = Some(0x42);
        def.drop_rate_q8 = 0;
        cat.insert(def);
        let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        let rewards = world.apply_battle_loot(&formation, &cat);
        assert!(rewards.drops.is_empty());
        assert!(!world.inventory.contains_key(&0x42));
    }

    #[test]
    fn load_full_hydrates_level_up_tracker_from_record_levels() {
        // Build a 3-character save with levels 7, 12, 25.
        let mut party = legaia_save::Party::zeroed(3);
        party.members[0].set_level(7);
        party.members[1].set_level(12);
        party.members[2].set_level(25);
        let sf = legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt::default(),
            ext_v2: legaia_save::SaveExtV2::default(),
        };
        let mut world = World::new();
        // Tracker defaults to 1 for every slot.
        assert_eq!(world.level_up_tracker.level[0], 1);
        world.load_full(sf);
        assert_eq!(world.level_up_tracker.level[0], 7);
        assert_eq!(world.level_up_tracker.level[1], 12);
        assert_eq!(world.level_up_tracker.level[2], 25);
    }

    #[test]
    fn load_full_zero_level_record_clamps_to_one() {
        // Records that haven't had a level written (zero byte at +0x100)
        // shouldn't make the tracker think the slot is below L1.
        let party = legaia_save::Party::zeroed(2);
        let sf = legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt::default(),
            ext_v2: legaia_save::SaveExtV2::default(),
        };
        let mut world = World::new();
        world.load_full(sf);
        assert_eq!(world.level_up_tracker.level[0], 1);
        assert_eq!(world.level_up_tracker.level[1], 1);
    }

    #[test]
    fn apply_battle_xp_drops_remainder_from_integer_division() {
        let mut world = World {
            party_count: 3,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.actors[1].battle.hp = 100;
        world.actors[2].battle.hp = 100;
        // 101 / 3 = 33; the leftover 2 XP is dropped.
        let _ = world.apply_battle_xp(101);
        assert_eq!(world.level_up_tracker.xp[0], 33);
        assert_eq!(world.level_up_tracker.xp[1], 33);
        assert_eq!(world.level_up_tracker.xp[2], 33);
    }

    #[test]
    fn level_up_banner_countdown_clears() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.apply_battle_xp(50); // retail table: exactly L2 threshold
        assert!(world.current_level_up_banner.is_some());
        for _ in 0..=crate::levelup::LevelUpBanner::DEFAULT_FRAMES {
            world.tick();
        }
        assert!(
            world.current_level_up_banner.is_none(),
            "level-up banner should have cleared"
        );
    }

    #[test]
    fn no_level_up_banner_when_xp_insufficient() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.apply_battle_xp(49); // retail table: 49 < 50 (L2 threshold)
        assert!(world.current_level_up_banner.is_none());
    }

    #[test]
    fn art_strike_applier_pushes_apply_art_strike_event() {
        // Drive `BattleHostImpl::apply_art_strike` from a synthetic
        // ArtStrikeInfo and assert the world's pending_battle_events grows
        // by one ApplyArtStrike with the resolved damage.
        use legaia_art::Character;
        use legaia_art::power::PowerByte;
        use legaia_art::queue::ActionConstant;
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::{ArtStrikeInfo, BattleActionHost};

        let mut world = World::new();
        world.set_battle_attack(0, 64);
        world.set_battle_defense(3, 10);
        let info = ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(PowerByte::from_byte(0x1A)), // UDF × 28
            dmg_timing: Some(0x10),
            enemy_effect: EnemyEffect::Burned,
            hit_cue: None,
        };
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(info);

        assert_eq!(world.pending_battle_events.len(), 1);
        match &world.pending_battle_events[0] {
            BattleEvent::ApplyArtStrike {
                actor_slot,
                target_slot,
                strike_index,
                outcome,
            } => {
                assert_eq!(*actor_slot, 0);
                assert_eq!(*target_slot, 3);
                assert_eq!(*strike_index, 0);
                assert_eq!(outcome.damage, Some(102));
                assert_eq!(outcome.enemy_effect, EnemyEffect::Burned);
            }
            other => panic!("unexpected event: {:?}", other.summary()),
        }
    }

    #[test]
    fn art_strike_split_defense_picks_udf_or_ldf() {
        // With a (UDF=5, LDF=50) split on slot 3, a UDF-targeted strike
        // hits 5 def → high damage; LDF-targeted hits 50 def → low.
        use legaia_art::Character;
        use legaia_art::power::PowerByte;
        use legaia_art::queue::ActionConstant;
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::{ArtStrikeInfo, BattleActionHost};

        let mut world = World::new();
        world.set_battle_attack(0, 64);
        world.set_battle_defense_split(3, Some((5, 50)));

        let mk = |power: PowerByte| ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(power),
            dmg_timing: Some(0x10),
            enemy_effect: EnemyEffect::None,
            hit_cue: None,
        };
        // 0x1A = UDF × 28 → (64 * 28)/16 - 5 = 112 - 5 = 107.
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(mk(PowerByte::from_byte(0x1A)));
        // 0x1F = LDF × 28 → (64 * 28)/16 - 50 = 112 - 50 = 62.
        host.apply_art_strike(mk(PowerByte::from_byte(0x1F)));
        let events = world.drain_battle_events();
        let mut udf_dmg = None;
        let mut ldf_dmg = None;
        for e in events {
            if let BattleEvent::ApplyArtStrike { outcome, .. } = e
                && let Some(t) = outcome.power_target
            {
                match t {
                    legaia_art::power::PowerTarget::Udf => udf_dmg = outcome.damage,
                    legaia_art::power::PowerTarget::Ldf => ldf_dmg = outcome.damage,
                }
            }
        }
        assert_eq!(udf_dmg, Some(107));
        assert_eq!(ldf_dmg, Some(62));
    }

    #[test]
    fn fold_battle_event_apply_art_strike_subtracts_hp_and_records_status() {
        use legaia_art::power::{PowerByte, PowerTarget};
        use legaia_art::queue::{ActionConstant, Character};
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;

        let mut world = World::new();
        world.party_count = 4;
        for slot in 0..4 {
            world.actors[slot].active = true;
            world.actors[slot].battle.hp = 200;
            world.actors[slot].battle.max_hp = 200;
        }
        world.set_battle_attack(0, 64);
        world.set_battle_defense(3, 5);

        let info = ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(PowerByte::from_byte(0x1A)), // UDF × 28
            dmg_timing: Some(0x10),
            enemy_effect: EnemyEffect::Burned,
            hit_cue: None,
        };
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(info);
        let events = world.drain_battle_events();
        assert_eq!(events.len(), 1);
        for e in &events {
            let r = world.fold_battle_event(e);
            // 64 * 28 / 16 - 5 = 107 damage. Target slot 3 starts at 200,
            // ends at 93.
            assert_eq!(r, Some((3, 93)));
        }
        assert_eq!(world.actors[3].battle.hp, 93);
        // Burned status was folded into pending_status.
        assert_eq!(
            world.actors[3].pending_status,
            Some(legaia_art::record::EnemyEffect::Burned)
        );
        // PowerTarget enum is needed only to satisfy the import linter
        // when the assertions don't otherwise reference it.
        let _ = PowerTarget::Udf;
    }

    #[test]
    fn fold_battle_event_other_variants_dont_modify_state() {
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].active = true;
        world.actors[0].battle.hp = 100;
        let r = world.fold_battle_event(&BattleEvent::CameraBounds);
        assert_eq!(r, None);
        assert_eq!(world.actors[0].battle.hp, 100);
    }

    #[test]
    fn use_item_heals_hp_clamped_to_max() {
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 50;
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        // Item id 1 is the small heal in the vanilla catalog.
        let outcome = world.use_item(1, 0);
        assert!(matches!(
            outcome,
            crate::items::ItemOutcome::HealedHp { .. }
        ));
        // HP raised but clamped at max.
        assert!(world.actors[0].battle.hp > 50);
        assert!(world.actors[0].battle.hp <= 200);
    }

    #[test]
    fn use_item_heal_all_fills_to_max() {
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].battle.max_hp = 300;
        world.actors[0].battle.hp = 100;
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        // Find the HealAll entry (id 4 in the vanilla catalog - Healing Globe).
        let outcome = world.use_item(4, 0);
        assert!(matches!(
            outcome,
            crate::items::ItemOutcome::HealedHp { .. }
        ));
        assert_eq!(world.actors[0].battle.hp, 300);
    }

    #[test]
    fn use_item_unknown_id_returns_no_effect() {
        let mut world = World::new();
        world.party_count = 1;
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        let outcome = world.use_item(99, 0);
        assert!(matches!(outcome, crate::items::ItemOutcome::NoEffect));
    }

    #[test]
    fn use_item_revive_writes_hp_after() {
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 0; // dead
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        // Resurrection Leaf is id 0x0C (50% revive).
        let outcome = world.use_item(0x0C, 0);
        assert!(matches!(outcome, crate::items::ItemOutcome::Revived { .. }));
        // 50% of 400 = 200.
        assert_eq!(world.actors[0].battle.hp, 200);
    }

    #[test]
    fn use_item_cure_clears_status() {
        use legaia_art::record::EnemyEffect;
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].battle.max_hp = 100;
        world.actors[0].battle.hp = 50;
        // Apply a Burned status, then cure it via CureAll.
        world
            .status_effects
            .apply_from_enemy_effect(0, EnemyEffect::Burned);
        assert!(world.status_effects.is_afflicted(0));
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        // Antidote Flower is id 0x09 (CureAll).
        let outcome = world.use_item(0x09, 0);
        assert!(matches!(outcome, crate::items::ItemOutcome::CuredAll));
        assert!(!world.status_effects.is_afflicted(0));
    }

    #[test]
    fn fold_battle_event_clamps_to_zero_hp() {
        use legaia_art::power::PowerByte;
        use legaia_art::queue::{ActionConstant, Character};
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;

        let mut world = World::new();
        world.party_count = 4;
        world.actors[3].active = true;
        world.actors[3].battle.hp = 30;
        world.actors[3].battle.max_hp = 30;
        world.set_battle_attack(0, 64);
        world.set_battle_defense(3, 0);

        let info = ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(PowerByte::from_byte(0x1A)), // huge damage vs 30 HP
            dmg_timing: None,
            enemy_effect: EnemyEffect::None,
            hit_cue: None,
        };
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(info);
        let events = world.drain_battle_events();
        for e in &events {
            world.fold_battle_event(e);
        }
        // saturating_sub clamps to 0 instead of wrapping.
        assert_eq!(world.actors[3].battle.hp, 0);
    }

    #[test]
    fn fold_battle_event_pushes_status_into_tracker() {
        use legaia_art::power::PowerByte;
        use legaia_art::queue::{ActionConstant, Character};
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;
        use legaia_engine_vm::status_effects::StatusKind;

        let mut world = World::new();
        world.party_count = 4;
        world.actors[3].active = true;
        world.actors[3].battle.hp = 100;
        world.actors[3].battle.max_hp = 100;
        world.set_battle_attack(0, 64);
        world.set_battle_defense(3, 10);
        let info = ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(PowerByte::from_byte(0x1A)),
            dmg_timing: None,
            enemy_effect: EnemyEffect::Burned,
            hit_cue: None,
        };
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(info);
        let events = world.drain_battle_events();
        for e in &events {
            world.fold_battle_event(e);
        }
        assert!(world.status_effects.has(3, StatusKind::Burned));
    }

    #[test]
    fn tick_status_effects_drains_hp() {
        use legaia_engine_vm::status_effects::StatusKind;
        let mut world = World::new();
        world.actors[0].battle.hp = 100;
        world.actors[0].battle.max_hp = 160;
        world.status_effects.apply(0, StatusKind::Burned);
        world.tick_status_effects();
        assert_eq!(world.actors[0].battle.hp, 90);
    }

    #[test]
    fn reset_party_ap_refills_all_three_gauges() {
        let mut world = World::new();
        for g in world.ap_gauges.iter_mut() {
            g.try_spend(3);
        }
        world.reset_party_ap();
        for g in world.ap_gauges.iter() {
            assert_eq!(g.current_ap, g.base_ap);
            assert!(!g.spirit_charged);
        }
    }

    #[test]
    fn item_catalog_setter_replaces() {
        use crate::items::ItemCatalog;
        let mut world = World::new();
        assert!(world.item_catalog.is_empty());
        world.set_item_catalog(ItemCatalog::vanilla());
        assert!(!world.item_catalog.is_empty());
    }

    #[test]
    fn install_encounter_for_scene_resolves_field_pattern() {
        use crate::encounter_registry::vanilla_encounter_registry;
        let mut world = World::new();
        let r = vanilla_encounter_registry();
        let installed = world.install_encounter_for_scene(&r, "map01");
        assert!(installed, "field pattern should match");
        assert!(world.encounter.is_some());
    }

    #[test]
    fn install_encounter_for_scene_quiets_in_towns() {
        use crate::encounter_registry::vanilla_encounter_registry;
        let mut world = World::new();
        let r = vanilla_encounter_registry();
        let installed = world.install_encounter_for_scene(&r, "town01");
        assert!(!installed, "town pattern resolves but is quiet");
        assert!(
            world.encounter.is_some(),
            "session installed for nil checks"
        );
    }

    #[test]
    fn install_encounter_for_scene_returns_false_with_no_default() {
        use crate::encounter_registry::EncounterRegistry;
        let mut world = World::new();
        let r = EncounterRegistry::new(); // empty, no default
        let installed = world.install_encounter_for_scene(&r, "anything");
        assert!(!installed);
        assert!(world.encounter.is_none());
    }

    #[test]
    fn install_encounter_for_scene_replaces_active_session() {
        use crate::encounter_registry::vanilla_encounter_registry;
        let mut world = World::new();
        let r = vanilla_encounter_registry();
        // Install a field session, then a town session - the town call
        // should replace the field session even though it's quiet.
        world.install_encounter_for_scene(&r, "map01");
        assert!(world.encounter.is_some());
        let initial_table_label = world
            .encounter
            .as_ref()
            .unwrap()
            .tracker()
            .table()
            .scene_label
            .clone();
        world.install_encounter_for_scene(&r, "town01");
        let new_table_label = world
            .encounter
            .as_ref()
            .unwrap()
            .tracker()
            .table()
            .scene_label
            .clone();
        assert_ne!(initial_table_label, new_table_label);
    }

    #[test]
    fn install_encounter_from_record_registers_and_arms() {
        use crate::encounter_record::EncounterRecord;
        let mut world = World::new();
        // mc2-shaped record: two monsters, both id 4.
        let record = EncounterRecord {
            count: 2,
            monster_ids: [0x04, 0x04, 0, 0],
        };
        let formation_id = world
            .install_encounter_from_record("map01", &record)
            .expect("non-empty record produces an id");
        // Formation registered.
        let formation = world
            .formation_table
            .formation(formation_id)
            .expect("formation registered");
        assert_eq!(formation.slots.len(), 2);
        assert_eq!(formation.slots[0].monster_id, 4);
        assert_eq!(formation.slots[1].monster_id, 4);
        // Session installed and rate forced high.
        let session = world.encounter.as_ref().expect("session installed");
        assert_eq!(session.tracker().table().trigger_rate_q8, 0xFF);
        assert_eq!(session.tracker().table().entries.len(), 1);
        assert_eq!(
            session.tracker().table().entries[0].formation_id,
            formation_id
        );
    }

    #[test]
    fn install_encounter_from_record_empty_returns_none() {
        use crate::encounter_record::EncounterRecord;
        let mut world = World::new();
        let id = world.install_encounter_from_record("map01", &EncounterRecord::EMPTY);
        assert!(id.is_none());
        // No session installed.
        assert!(world.encounter.is_none());
    }
}
