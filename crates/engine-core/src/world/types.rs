//! Auxiliary world data types (requests, markers, sprites, actors, configs,
//! camera/return state) extracted verbatim from `world.rs`.

use super::*;

/// One queued fade request. Move-VM ext sub-op 0x3C writes either an
/// immediate fade (`ticks == 0`) or a ramp (`ticks > 0`) - engines drain
/// `pending_fade` each frame to drive the screen overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeRequest {
    pub rgb: [u8; 3],
    pub ticks: u16,
}

/// One placed field prop's collision state - the engine's row for the
/// **actor-collision arms** of retail's movement probe (`FUN_801CFC40`) and
/// the collision candidate list it feeds (`FUN_801CF754`).
///
/// A retail placed object is an actor whose contact box both **blocks** the
/// player's 2-unit step and **posts the touch** that resumes its parked
/// script; the flag word `+0x10` classes it:
///
/// - default (no class bits): static arm - box ±80 around
///   [`Self::center`] (the record-derived footprint centre), contact result
///   bit `4`, auto-posted on body contact (doors);
/// - `+0x10 & 0x40020000` ([`Self::interact`]): result bit `1` - still
///   blocks, but only the button-gated facing probe posts it (cupboards,
///   `31 1E` in the spawn prologue);
/// - `+0x10 & 0x1020000` ([`Self::moving_box`]): the box anchors at
///   [`Self::live`] with the moving-actor extents (±40);
/// - `+0x10 & 3` ([`Self::solid`] = false): exempt - `FUN_801CF754` /
///   `FUN_801CF9F4` skip the actor entirely. The door's touch pass sets bit
///   `0` (`31 00`) as the swing starts, which is when a door stops blocking.
// REF: FUN_801CFC40, FUN_801CF754, FUN_801CF9F4
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldPropCollider {
    /// Footprint-anchor tile of the placement (the [`World::field_prop_bank`]
    /// key), when the placement is bound; `None` for unbound (window-sweep)
    /// placements.
    pub anchor: Option<(u8, u8)>,
    /// Static-arm contact-box centre (`collider_x`/`collider_z`).
    pub center: (i32, i32),
    /// Live actor position (the placement's spawn world position) - the
    /// moving-arm anchor.
    pub live: (i32, i32),
    /// `+0x10 & 0x1020000`: moving-arm box (±40 at [`Self::live`]).
    pub moving_box: bool,
    /// `+0x10 & 0x40020000`: interact-gated class (contact result bit `1`,
    /// never auto-posted).
    pub interact: bool,
    /// Clear once `+0x10 & 3` is set: the prop no longer blocks nor touches.
    pub solid: bool,
}

/// Render-agnostic snapshot of one live effect-pool master slot, produced by
/// [`World::active_effect_markers`] - one entry per effect (effect origin +
/// age). [`World::active_effect_sprites`] is the richer per-child billboard
/// view the textured-quad render path uses; this coarse marker remains for
/// hosts/tests that only need effect positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectMarker {
    /// Effect origin in world units (the pool stores 8.8 fixed-point; this
    /// is the integer-unit position the renderer's MVP consumes).
    pub world_pos: [f32; 3],
    /// 12-bit spawn angle (`master.angle`), passed through for hosts that
    /// orient the effect.
    pub angle: u16,
    /// Lifetime fraction in `0.0..=1.0` (0 = just spawned, 1 = about to
    /// retire). Hosts fade the marker out as this approaches 1.
    pub age01: f32,
}

/// Render-agnostic snapshot of one live effect **child sprite**, produced by
/// [`World::active_effect_sprites`] straight off the pool's live child slots
/// (`Pool::child_billboards`, the `FUN_801E0088` pass-2 port): a camera-facing
/// quad at the child's integrated world position, sized by the sprite-atlas
/// entry × the pool sprite scale, sampling VRAM at the current animation
/// frame's atlas `(u, v)` / `tpage` / `clut`, with the retail brightness
/// envelope and random UV-mirror corner order.
///
/// The texel-source VRAM upload for battle effects is not yet pinned (see
/// `docs/formats/effect.md`), so a host that samples VRAM here will draw the
/// faithful geometry/animation with whatever is resident; the `page`/`clut`/
/// `uv` carry the real coordinates so textures appear once that upload lands.
///
/// REF: FUN_801E0088
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectSprite {
    /// Child world position in world units (the pool's 16.8 coordinates
    /// `>> 8`, exactly as the retail projection input truncates).
    pub world_pos: [f32; 3],
    /// Billboard size in world units - the pass-2 sizing `atlas w/h *
    /// sprite_scale >> 8` (x10 the texel size at the retail `0xA00` scale).
    pub size: [f32; 2],
    /// Top-left source texel within the texture page (atlas `u`, `v`).
    pub uv: [u16; 2],
    /// Source rectangle size in texels (atlas `w`, `h`).
    pub uv_size: [u16; 2],
    /// PSX `tpage` descriptor (texture-page base + colour mode).
    pub page: u16,
    /// CLUT (CBA) id.
    pub clut: u16,
    /// Brightness modulation `0..=0x80` from the retail ramp-in / ramp-out
    /// envelope (`0x80` = neutral). Hosts write it as `r = g = b`.
    pub brightness: u8,
    /// Horizontal texel-corner swap (the random UV mirror, bit 0 clear).
    pub flip_h: bool,
    /// Vertical texel-corner swap (mirror bit 1 clear).
    pub flip_v: bool,
    /// Animation fraction `0.0..=1.0` (`frame_cursor / frame_count`) - a
    /// render aid for outline fades; the faithful fade is `brightness`.
    pub age01: f32,
}

/// One dev-spawned synthetic effect ([`World::spawn_debug_effect`] /
/// [`World::spawn_debug_effect_model`]) - an engine-side visualization aid,
/// **not** a retail pool slot. Lives outside the effect pool so the faithful
/// walker never sees it; [`World::tick_effects`] ages it over a fixed budget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebugEffect {
    /// Effect origin in world units.
    pub world_pos: [f32; 3],
    /// Index into [`World::global_tmd_pool`] for the model-driven debug
    /// effects (`None` for the plain marker spawn).
    pub model_index: Option<usize>,
    /// Frames elapsed since the spawn.
    pub age_frames: u32,
}

/// Render-agnostic snapshot of one live effect's **3D model** (`etmd.dat`),
/// produced by [`World::active_effect_models`]. This is the other effect
/// render path alongside [`EffectSprite`]'s 2D billboards: spell effects like
/// *Tail Fire* are small Gouraud-shaded `etmd` meshes textured by `etim`
/// (pinned pixel-exact against a live battle VRAM capture - see
/// `docs/formats/effect.md`), not billboards.
///
/// The host resolves `tmd_index` through its global TMD pool
/// ([`World::global_tmd`]), builds a VRAM mesh, and draws it at `world_pos`
/// (the `etim` texels are already resident in scene VRAM). The retail
/// effect-id -> model selection is driven by the move/art VM and not yet
/// decoded, so the only producer today is the model-spawn helper
/// ([`World::spawn_debug_effect_model`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectModel {
    /// Index into [`World::global_tmd_pool`] (the `etmd.dat` head) for this
    /// effect's mesh.
    pub tmd_index: usize,
    /// Effect origin in world units (the pool stores 8.8 fixed-point).
    pub world_pos: [f32; 3],
    /// 12-bit spawn angle (`master.angle`).
    pub angle: u16,
    /// Lifetime fraction `0.0..=1.0` (0 = just spawned, 1 = about to retire).
    pub age01: f32,
}

/// Coarse role of a placed overworld entity, carried on
/// [`WorldMapEntityMarker`] so a host can colour-code its render marker
/// without inspecting the full [`WorldMapEntityConfig`] (which carries
/// per-kind payload the renderer doesn't need).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMapEntityKind {
    /// A roaming-encounter zone ([`WorldMapEntityConfig::EncounterZone`]).
    EncounterZone,
    /// A town / dungeon portal ([`WorldMapEntityConfig::Portal`]).
    Portal,
    /// A plain interactable NPC / signpost ([`WorldMapEntityConfig::Npc`]),
    /// also the fallback for an entity with no config row.
    Npc,
}

/// Render-agnostic snapshot of one placed overworld entity, produced by
/// [`World::world_map_entity_markers`]. The disc-sourced placement seeding
/// ([`crate::scene::SceneHost::enter_world_map_scene`]) installs each entity
/// with a world position and a [`WorldMapEntityConfig`]; this pairs the
/// position with its coarse [`WorldMapEntityKind`] so a host can draw a marker
/// at each on-map portal / NPC / encounter zone.
///
/// This is the seam for the still-open per-entity mesh-resolution thread: the
/// retail engine binds each placement to its own actor model, which is not yet
/// decoded, so the host draws a kind-coded marker at the position rather than
/// the entity's real mesh. The position shares the player's coordinate frame
/// (both come from the scene MAN), so a marker reads correctly relative to the
/// player even while the kingdom terrain mesh renders at its own pack-local
/// coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldMapEntityMarker {
    /// Entity world position in world units. `x` / `z` are the MAN placement
    /// coordinates; `y` is the player actor's current plane (the entities are
    /// 2D placements, so they sit on the player's walking plane).
    pub world_pos: [f32; 3],
    /// Coarse role, for colour-coding the marker.
    pub kind: WorldMapEntityKind,
}

/// Render-agnostic snapshot of the player's overworld position, produced by
/// [`World::world_map_player_marker`]. In `SceneMode::WorldMap` the kingdom
/// terrain mesh renders at its pack-local coordinates and the player actor's
/// own mesh is not drawn, so a host draws a marker at this position to show the
/// player on the map. `facing` is the player's heading (`render_26`), so the
/// host can draw a direction tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldMapPlayerMarker {
    /// Player world position in world units (the player actor's
    /// `move_state.world_x / y / z`).
    pub world_pos: [f32; 3],
    /// Player heading as a PSX 12-bit angle (`4096` = full turn), measured the
    /// same way the field path stores it (`render_26`). `0` faces `+Z`.
    pub facing: i16,
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
    /// Noa dance (rhythm) minigame - the beat clock + hit judge own the frame
    /// ([`crate::dance::DanceGame`]); field / battle dispatch is suspended. The
    /// hosting session preserves the suspended scene state and restores its
    /// mode on exit, like the pause menu. Retail `game_mode 0x18` (`OTHER MODE`)
    /// overlay pair for the dance overlay (PROT 0980).
    Dance,
    /// Fishing minigame - the cast / fight / score loop owns the frame
    /// ([`crate::fishing::FishingSession`]); field / battle dispatch is
    /// suspended and the interrupted mode restored on exit. Retail
    /// `game_mode 0x18` (`OTHER MODE`) overlay pair for the fishing overlay
    /// (PROT 0972).
    Fishing,
    /// Casino slot-machine minigame - the reel state machine owns the frame
    /// ([`crate::slot_machine::SlotMachine`]); field / battle dispatch is
    /// suspended and the interrupted mode restored on exit. Retail
    /// `game_mode 0x18` (`OTHER MODE`, the mode-24 minigame door-warp) for
    /// the slot overlay (PROT 0975).
    SlotMachine,
    /// Baka Fighter duel minigame - the exchange / round / match state
    /// machine owns the frame ([`crate::baka_fighter::BakaFight`]); field /
    /// battle dispatch is suspended and the interrupted mode restored on
    /// exit. Retail `game_mode 0x18` (`OTHER MODE`, the mode-24 minigame
    /// door-warp) for the Baka Fighter overlay (PROT 0976).
    BakaFighter,
    /// Muscle Dome card-battle contest - the hand-select / commit / resolve
    /// loop owns the frame ([`crate::muscle_dome::MuscleDomeSession`]); field
    /// / battle dispatch is suspended and the interrupted mode restored on
    /// exit. Retail: the arena runs *inside* the battle overlay (PROT 0898)
    /// on the `_DAT_8007bd24` context, entered through the mode-24 sub-id-5
    /// door (PROT 0977).
    MuscleDome,
    /// In-field pause menu - the retail CARD mode pair (`game_mode 0x17`,
    /// `CARD MODE`, which hosts both the memory-card UI and the pause menu;
    /// every menu-open capture holds `_DAT_8007B83C = 0x17`). Field/battle
    /// dispatch is suspended while the menu owns the frame; only the actor
    /// VM and effect pool run, like `Title`. The hosting session preserves
    /// the suspended scene state and restores its mode on close.
    Menu,
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

    /// Per-actor physics tick state - port of `FUN_80021DF4`. Driven
    /// each frame by [`World::tick_actor_physics`]. The dispatch byte
    /// at [`ActorPhysics::dispatch_byte`] (`+0x5A`) selects the per-arm
    /// behaviour; engines set it via [`Actor::set_physics_dispatch`].
    pub physics: ActorPhysics,

    /// Per-actor move-buffer cursor + envelope state, mirroring the
    /// retail layout at `actor[+0x5C..+0xCC]`. Lives in its own struct
    /// because retail aliases `+0xA0` / `+0xB8` / `+0xC8` with the
    /// physics view; see [`vm::move_buffer::MoveBufferState`].
    /// Updated by [`World::tick_actor_physics`] in response to the
    /// per-arm [`TickEvent::MoveVmKick`] signal.
    pub move_buffer: MoveBufferState,

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

    /// Most recent status effect inflicted by an art strike (Toxic /
    /// Numb / …). Engines clear this when they've folded it into their
    /// status-bar UI; defaults to `None`.
    pub pending_status: Option<legaia_art::record::EnemyEffect>,

    /// Optional sprite frame for this actor. Drives the per-frame sprite
    /// batch through [`World::collect_sprite_requests`]. When `None`, the
    /// actor is invisible (or rendered as a 3D mesh through the TMD path).
    pub sprite_frame: Option<SpriteFrame>,

    /// Active keyframe animation player. `None` means no animation is
    /// playing. Set via [`World::set_actor_animation`].
    pub active_animation: Option<AnimPlayer>,

    /// Last per-bone pose produced by `active_animation.tick()` (field) or
    /// `battle_animation.tick()` (battle). `None` until the first frame after
    /// an animation is assigned. Field renderers consume this via
    /// `tmd_to_vram_mesh_posed` (translation only); battle renderers use
    /// `tmd_to_vram_mesh_posed_rot` (full per-object rigid transform).
    pub pose_frame: Option<PoseFrame>,

    /// Battle per-object rigid-transform animation player. Set at battle init
    /// for monster (and player-summon) actors from their archive idle clip; the
    /// battle tick advances it into `pose_frame`. Mutually exclusive with
    /// `active_animation` (field ANM) on a given actor.
    pub battle_animation: Option<crate::battle_anim::MonsterAnimPlayer>,

    /// Per-slot battle action clips for this actor (the player files'
    /// `record[0]` action streams, expanded so channel `i` drives TMD object
    /// `i`), indexed by action-stream slot; index 0 = the idle loop. Set by
    /// the host at battle init (see `World::set_actor_battle_action_clips`);
    /// consulted by the battle-action SM's `pose()` host hook to switch
    /// `battle_animation` between idle and action clips. `None` keeps the
    /// idle-only behavior.
    pub battle_action_clips: Option<std::sync::Arc<Vec<Option<MonsterAnimation>>>>,

    /// Pose id the current `battle_animation` was selected for, in the pose-id
    /// space the battle-action SM emits (`6` idle / `7` ready / `8` recover /
    /// `9` defeat). Lets a repeated request for the same pose keep the playing
    /// clip instead of restarting it. (Retail mechanism note: the SM's
    /// `FUN_801D5854(actor, 6..9)` calls are camera/presentation programs - the
    /// anim ids retail stages into `actor+0x1DA` are entry indices, with idle
    /// = id `0`; ids `7`/`8`/`9` are real entries staged by the SM and the
    /// anim commit `FUN_8004AD80`. The engine folds both spaces into this one
    /// hook; pose `6` plays entry 0's idle loop, which matches the frames
    /// retail shows.)
    pub battle_pose: Option<u8>,

    /// Hit-reaction action tag currently playing on `battle_animation`
    /// (the retail `actor+0x1EF..+0x1F3` key space: `2`/`3` light flinch,
    /// `4` knockdown, `5` get-up, `0x0B` block). Set by
    /// [`World::queue_battle_reaction`]; cleared when the reaction chain
    /// finishes and idle resumes. Takes precedence over `battle_pose` while
    /// active.
    pub battle_reaction: Option<u8>,

    /// Per-character **art-animation bank** clips (the player file's
    /// record[0] `+0x58` bank, each record's keyframe stream resolved
    /// through its `readef.DAT` `"ME"` archive and expanded so channel `i`
    /// drives TMD object `i`), indexed by bank record. The staged-anim
    /// commit ([`World::commit_staged_battle_anims`]) materializes record
    /// `q - 0x10` from here for a staged id `q >= 0x10`, exactly like
    /// retail `FUN_8004AD80`. `None` (monsters, or hosts that didn't decode
    /// the bank) treats staged ids as plain action-table entry indices.
    pub battle_art_bank: Option<std::sync::Arc<Vec<Option<MonsterAnimation>>>>,

    /// Committed anim id (post `FUN_8004AD80` rewrite) of the **staged
    /// one-shot clip** currently owning `battle_animation` - a weapon swing
    /// (`0xC..0xF`) or a materialized art record (dynamic slot `0x10`/
    /// `0x11`). While `Some`, the SM's per-frame `pose()` requests don't
    /// steal the player (same precedence rule as `battle_reaction`).
    /// Cleared by [`World::tick_battle_animations`] when the clip finishes:
    /// that's the engine's anim-end signal - `ADVANCE_DONE` is cleared so
    /// the attack chain reads its next strike byte, and the id pair
    /// converges back to idle `0`.
    pub battle_staged_anim: Option<u8>,

    /// Battle monster texture slot (`0..=4`). The monster TMD's on-disc CBA/TSB
    /// are nominal defaults the battle loader relocates per slot
    /// (`FUN_80055468` → `legaia_asset::monster_archive::relocate_cba/relocate_tsb`).
    /// Renderers that rebuild this actor's mesh from the raw TMD each frame (the
    /// posed-animation path) must re-apply that relocation, since a fresh build
    /// carries the nominal addresses again. `None` for non-monster actors.
    pub battle_tex_slot: Option<u8>,

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

    /// Monster id (1-based, into the PROT 867 monster archive) when this
    /// actor is an enemy spawned for a battle formation. `None` for party
    /// members and for actors outside a formation-driven battle. Set by
    /// `World::enter_battle_from_formation`; lets a renderer fetch the
    /// monster's battle mesh + texture pool (the on-disc CBA/TSB are
    /// relocated per battle slot - see
    /// `legaia_asset::monster_archive::MonsterMesh::battle_render_mesh`).
    pub battle_monster_id: Option<u16>,
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

    /// Engine-side hook: set the dispatch byte the per-actor physics
    /// tick reads at `actor[+0x5A]`. See [`vm::actor_tick`] for the
    /// dispatch ladder. Most actors run dispatch byte `0x06` (the
    /// keyframe arm); engines set per-actor as scene assets are bound.
    pub fn set_physics_dispatch(&mut self, b: u16) {
        self.physics.set_dispatch(b);
    }
}

/// One active stat buff / debuff produced by a battle Magic cast.
///
/// The applied delta is the exact change written into the per-slot scalar
/// ([`World::battle_attack`] / [`World::battle_defense`] / [`World::battle_magic`]),
/// so `World::finish_battle` (or natural expiry) can revert it precisely.
/// Stats with no live-loop scalar (Accuracy / Evasion / Speed) are tracked
/// with a zero delta so the timer still expires cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleBuff {
    /// Actor-table slot the buff applies to.
    pub slot: u8,
    /// Buffed stat.
    pub stat: crate::spells::BuffStat,
    /// Signed delta actually written into the per-slot scalar (`0` for stats
    /// that have no live-loop scalar).
    pub applied_delta: i16,
    /// Turns remaining before the buff expires.
    pub turns: u8,
}

/// One monster's chosen action for its turn, produced by the action picker
/// [`World::pick_monster_action`] (the port of `FUN_801E9FD4`'s decision core).
///
/// REF: FUN_801E9FD4
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MonsterAction {
    /// Physical strike against party slot `target`.
    Physical { target: u8 },
    /// Cast `spell_id` against the resolved absolute `targets` slots.
    Cast { spell_id: u8, targets: Vec<u8> },
}

/// A live scripted-encounter carrier menu: the 4-option picker its dialogue
/// presents (the Rim Elm spar's "hear about Biron / secrets of fighting /
/// **practice with you** / nothing"). Navigated with Up/Down; confirming the
/// `fight_option` index engages the carrier (-> Battle), any other option just
/// closes (its talk branch). Mirrors the retail inline-picker cursor
/// `*(0x801C6EA4)+0x0C`; the engine can't run the option's field-VM branch, so it
/// gates the engage on `fight_option` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarrierMenu {
    /// Index into [`World::field_carriers`] this menu engages.
    pub carrier_idx: usize,
    /// Number of options in the picker (4 for the spar).
    pub n: usize,
    /// The picker option whose confirm engages the carrier (the fight choice).
    pub fight_option: usize,
    /// Highlighted option `0..n`.
    pub cursor: usize,
}

/// The field-VM scripted-battle install prefix (`3E FF <formation-row>`): the
/// op `FUN_801DA51C` uses to arm a formation-row fight (the Rim Elm spar's
/// `3E FF 04` = row 4 lone Tetsu; garmel's Zeto `3E FF 09`; rikuroa's Caruban
/// `3E FF 11` - see `rim_elm_sparring_carrier`). Matching the two-byte prefix
/// is row-agnostic, so it recognises any scripted-battle branch.
const SCRIPTED_BATTLE_INSTALL: [u8; 2] = [0x3E, 0xFF];

/// Bytes to scan from an option's branch target when hunting the install, if no
/// nearer branch bounds the window first. A spar reply branch is short; a span
/// this size can't reach into an unrelated later region.
const SPAR_BRANCH_SCAN_SPAN: usize = 256;

/// If `dialogue` presents the spar's 4-option picker, return `(n_options,
/// fight_option_index)`. The fight option is disc-derived: the choice whose
/// branch (at its [`legaia_mes::Picker::jump_target`]) installs a scripted
/// battle (`3E FF _`, the exact op the RE pinned for the Rim Elm training
/// fight). Because that ties the choice to a disc op rather than an English
/// label, it holds under translation packs / PAL discs. The `"practice"` label
/// match is kept only as a last-resort fallback for a buffer whose branch
/// bytes were truncated away (so no install is present to key on). `None` when
/// the dialogue carries no such picker, so a carrier without a menu keeps the
/// any-accept engage path.
pub fn spar_menu_of(dialogue: &[u8]) -> Option<(usize, usize)> {
    for p in legaia_mes::scan_pickers(dialogue) {
        if p.n != 4 {
            continue;
        }
        // Resolve each option's branch entry offset into this same buffer.
        // `jump_target` shares `dialogue`'s coordinate space (both `open` and
        // the returned offset index it), so an out-of-range target is dropped.
        let targets: Vec<usize> = (0..p.n)
            .map(|i| p.jump_target(i).unwrap_or(usize::MAX))
            .collect();
        // Disc-derived fight option: its branch carries the scripted-battle
        // install. Bound each option's scan to the start of the nearest later
        // branch (so a lower option's window can't run into a higher option's
        // install) capped by a fixed span and the buffer end.
        let fight = (0..p.n).find(|&i| {
            let t = targets[i];
            if t >= dialogue.len() {
                return false;
            }
            let next = targets
                .iter()
                .copied()
                .filter(|&o| o > t && o <= dialogue.len())
                .min()
                .unwrap_or(dialogue.len());
            let end = next
                .min(t.saturating_add(SPAR_BRANCH_SCAN_SPAN))
                .min(dialogue.len());
            dialogue[t..end]
                .windows(2)
                .any(|w| w == SCRIPTED_BATTLE_INSTALL)
        });
        if let Some(f) = fight {
            return Some((p.n, f));
        }
        // Last-resort fallback: the English "practice" label.
        if let Some(f) = p.options.iter().position(|o| {
            o.label
                .windows(8)
                .any(|w| w.eq_ignore_ascii_case(b"practice"))
        }) {
            return Some((p.n, f));
        }
    }
    None
}

/// Per-field-carrier role. The retail engine builds one record per MAN-placed
/// scene entity; this is the clean-room slice the field entity SM acts on.
/// Paired by index with [`World::field_carriers`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldCarrierConfig {
    /// A **scripted-encounter carrier**: engaging it (the dialogue-accept)
    /// advances its `FUN_801DA51C` SM to its scene-transition, which selects
    /// MAN formation `formation_id` (by index, so the scene's merged monster
    /// stats stand) and launches the battle. The Rim Elm Tetsu tutorial fight
    /// is `formation_id` [`crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID`].
    ScriptedEncounter { formation_id: u16 },
    /// A plain interactable NPC. Surfaces a
    /// [`crate::field_events::FieldEvent::FieldInteract`] with `interact_id`.
    Npc { interact_id: u8 },
}

/// Per-overworld-entity role. The retail engine builds one record per on-map
/// entity from the scene's entity table; this is the clean-room slice the
/// gameplay SM acts on - an encounter zone spawns its own formation, a portal
/// targets a scene, an NPC just surfaces an interaction. Paired by index with
/// [`World::world_map_entities`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldMapEntityConfig {
    /// A roaming-encounter zone: when the shared countdown drains while this
    /// entity is the one that fires, it spawns `formation_id` (instead of the
    /// map-wide [`WorldMapEncounterState::formation_id`]).
    EncounterZone { formation_id: u16 },
    /// A town / dungeon portal: engaging it (via
    /// [`World::engage_world_map_entity`]) transitions to `target_map`,
    /// surfaced as [`crate::field_events::FieldEvent::WorldMapTransition`].
    ///
    /// This is the **door-warp** `map_id` (the `0x3E`, `op0 >= 100` selector,
    /// `0..=6`) the placement classifier reads off a partition-1 actor's
    /// script. Its name resolution goes through a
    /// [`crate::scene::MapIdResolver`] (a 7-id scene-*type* space). The
    /// overworld's town/dungeon entrances are **not** this - they are
    /// [`Self::OverworldPortal`], sourced from the `0x3F` named-scene-change
    /// bridge below.
    Portal { target_map: u16 },
    /// An overworld town / dungeon entrance sourced from the disc's `.MAP`
    /// walk-on tile-trigger → MAN partition-2 record → `0x3F`
    /// named-scene-change bridge (pinned on the `map01` hub: each gate-1
    /// kind-1 tile trigger references a partition-2 record whose script's
    /// `0x3F` op carries the destination scene name + arrival entry tile).
    ///
    /// Unlike the door-warp [`Self::Portal`] (a 7-id scene-*type* selector
    /// whose name resolution lives in an uncaptured overlay), this carries
    /// the **exact CDNAME destination** + arrival tile straight from the
    /// controller's bytecode, so engaging it warps to a real scene. Engaging
    /// it surfaces a [`crate::field_events::FieldEvent::WorldMapTransition`]
    /// (whose `slot` points back at this config); the host's transition drain
    /// reads the destination from here.
    OverworldPortal {
        /// Destination CDNAME scene label (e.g. `"keikoku"`, `"rikuroa"`).
        scene_name: String,
        /// The `0x3F` op's `i16` destination index (story/entry id; the
        /// wider [`crate::scene::SceneDestinationResolver`] key, not the
        /// door-warp `map_id`).
        index: i16,
        /// Arrival entry-tile X byte at the destination (`& 0x7F` tile).
        entry_x: u8,
        /// Arrival entry-tile Z byte at the destination.
        entry_z: u8,
        /// Arrival facing/depth selector (`& 7` indexes the entry-direction
        /// table).
        dir: u8,
    },
    /// A plain interactable (NPC / signpost). Surfaces a
    /// [`crate::field_events::FieldEvent::FieldInteract`] with `interact_id`.
    ///
    /// `inline` is the entity's own placement-script dialog text (the `0x3F`
    /// op's inline buffer the placement walker captured); `World::tick_world_map`
    /// opens it when the player presses confirm next to the entity. `text_id`
    /// is the box-config id carried alongside it (not an MES index). Both are
    /// empty/`None` when the placement has an interaction but no inline dialog.
    Npc {
        interact_id: u8,
        text_id: Option<u16>,
        inline: Vec<u8>,
    },
}

/// Shared overworld encounter-rate state read by the world-map entity SM.
#[derive(Debug, Clone)]
pub struct WorldMapEncounterState {
    /// `DAT_8007b604` - signed per-step encounter countdown shared across all
    /// overworld entities. Decremented in the entity SM's Idle state; an
    /// encounter fires when it reaches zero (and [`Self::enabled`]).
    pub countdown: i8,
    /// `DAT_8007b5f8 != 0` - master encounter-enable flag. When `false`,
    /// overworld encounters never fire regardless of the countdown.
    pub enabled: bool,
    /// Formation id an overworld encounter spawns (resolved against
    /// [`World::formation_table`]). The retail per-region resolution
    /// (`FUN_800243F0` → the MAN region table) is a separate thread; v0.1
    /// uses one configured formation for the whole map.
    pub formation_id: u16,
    /// Frames to reset the countdown to after an encounter fires (so the next
    /// encounter is paced rather than re-firing every frame at zero).
    pub reset_to: i8,
}

impl Default for WorldMapEncounterState {
    fn default() -> Self {
        Self {
            countdown: 0,
            enabled: false,
            formation_id: 0,
            reset_to: 64,
        }
    }
}

/// Field state snapshot taken at the `Field -> Battle` transition and
/// restored when the live loop returns from battle. See
/// [`World::live_gameplay_loop`].
#[derive(Debug, Clone, Default)]
pub struct FieldReturnState {
    pub actors: Vec<Actor>,
    pub player_actor_slot: Option<u8>,
    pub party_count: u8,
}

/// Active 3-actor talk session - the engine mirror of the talk-controller
/// actor `FUN_801D2D38` spawns from pool `0x801F22C4` for field-VM op
/// `0x43` sub-2 (three-way cutscene conversations).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThreeActorTalk {
    /// The three participant ids from the instruction (retail resolves them
    /// through the actor-list walk `FUN_8003C83C` and stores the pointers at
    /// controller `+0x80/+0x84/+0x88`).
    pub actor_ids: [u8; 3],
    /// Controller script id (retail `+0x50` from the instruction's u16).
    pub script_id: u16,
    /// Raw duration byte from the instruction. Retail stores
    /// `arg - a - b` at controller `+0x72`, where `(a, b)` is the scene-MAN
    /// header pair read via `FUN_8003D064(_DAT_8007B898 + 0x22)`; the engine
    /// keeps the raw operand (its MAN header staging lives elsewhere).
    pub duration: u8,
    /// Positions + headings of the three participants captured when the
    /// talk armed. Retail: the controller SM's state 0 (`FUN_801D27E0`)
    /// writes the 3-record table at `0x800845E4`; a re-arm while the talk
    /// flag is up restores from it (`FUN_801D2D38`'s else-branch loop).
    pub saved: [Option<((i16, i16), i16)>; 3],
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
