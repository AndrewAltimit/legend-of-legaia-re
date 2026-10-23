//! Per-part render types: render mode, special render nodes, the part runtime, the draw record and its morph.

use super::*;

/// Upper bound on a `model_sel` that names a real mesh (`DAT_8007C018[model_sel
/// + base]`). The model library is small (~30 entries), so a part whose
/// `model_sel` is `-1` (transform node) or a large sentinel (`0x1000`, `0x4000`,
/// `0x4001` - special render-mode markers, per the summon-overlay decode) binds
/// no mesh. Mesh parts have `0 <= model_sel < MAX_MESH_SEL`.
pub const MAX_MESH_SEL: i16 = 0x100;

/// `true` when `model_sel` names a real library mesh (vs. a transform/pivot node
/// or a special-mode sentinel).
pub(super) fn is_mesh_sel(model_sel: i16) -> bool {
    (0..MAX_MESH_SEL).contains(&model_sel)
}

/// A move-VM scene-graph part's **render mode** - the `actor[+0x5A]` discriminant
/// the per-frame part tick `FUN_80021DF4` dispatches on. Each value integrates a
/// different channel set and selects a different draw/emit path; see
/// [`docs/subsystems/move-vm.md`](../../../docs/subsystems/move-vm.md) §
/// "Part render-tail" for the full table.
///
/// Only the two values the spawner `FUN_80021B04` seeds from a record's
/// `model_sel` sentinel ([`legaia_overlay_images::summon_overlay::RENDER_NODE_MODE_A`] `0x4000`, [`legaia_overlay_images::summon_overlay::RENDER_NODE_MODE_B`]
/// `0x4001`) are classified statically here - the rest of the `+0x5A` space is
/// rebound at runtime by move-VM anim ops, which the engine abstracts
/// away (it carries no `+0x5A` cell), so this is the faithful static surface.
///
/// The important fact this captures: **`SoundEmitter` (`0x4001` → `+0x5A = 5`)
/// is not a visual node** - it is a 3D positional sound-effect emitter (range /
/// volume attenuation + SE trigger), so it must be excluded from the mesh draw
/// list and routed to the audio host instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// `0x4000` → `+0x5A = 3`: moving particle (position channels integrated by
    /// frame-delta; retail draw `FUN_80019D50`).
    Particle,
    /// `0x4001` → `+0x5A = 5`: 3D positional **sound** emitter - no mesh draw.
    SoundEmitter,
}

impl RenderMode {
    /// Classify the `+0x5A` render mode `FUN_80021B04` seeds from a part record's
    /// `model_sel`. Returns `None` for ordinary mesh parts (`0 <= sel < 0x100`)
    /// and transform nodes (`-1`), whose render mode is the move-VM-driven
    /// default the engine models through its normal part transform.
    // PORT: FUN_80021B04 (the +0x5A seeding: 0x4000 -> 3, 0x4001 -> 5);
    // REF: FUN_80021DF4 (the per-mode draw/emit dispatch, host-delegated).
    ///
    /// The classification is the spawn's own: `SpawnSubmode::classify` (the
    /// init-word test at `0x80021cc0`) is the one decoder, and the two
    /// render-mode-node arms it names are the two values here.
    pub fn from_model_sel(model_sel: i16) -> Option<Self> {
        use legaia_engine_vm::move_vm::SpawnSubmode;
        match SpawnSubmode::classify(model_sel as u16) {
            SpawnSubmode::Keyframe => Some(RenderMode::Particle),
            SpawnSubmode::Tween => Some(RenderMode::SoundEmitter),
            SpawnSubmode::Negative | SpawnSubmode::Default => None,
        }
    }

    /// The raw retail `actor[+0x5A]` value (`3` for [`RenderMode::Particle`],
    /// `5` for [`RenderMode::SoundEmitter`]).
    pub fn raw(self) -> i16 {
        match self {
            RenderMode::Particle => 3,
            RenderMode::SoundEmitter => 5,
        }
    }
}

/// A live special render-mode node (a `0x4000`/`0x4001` part), surfaced for the
/// renderer (particle) / audio (positional SE) host. These carry no library mesh
/// so they never appear in [`SummonScene::part_draws`]; this is the channel a
/// host consumes them through instead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpecialRenderNode {
    /// Which non-visual / particle behaviour the node drives.
    pub mode: RenderMode,
    /// The node's current world position (move-VM `world_x/y/z`).
    pub world_pos: [f32; 3],
}

/// Runtime state of one staged summon part.
#[derive(Debug, Clone)]
pub struct SummonPartRuntime {
    /// `record[+0]` mesh selector (`-1` = transform/pivot node).
    pub model_sel: i16,
    /// `record[+2]`, the reserved halfword.
    pub reserved: u16,
    /// The part's move buffer (its record bytes as a u16 array; PC indexes this).
    pub buf: Vec<u16>,
    /// Live move-VM actor state.
    pub state: ActorState,
    /// `true` once the part's program halted or ran off its buffer.
    pub finished: bool,
    /// The point this part's translation glide is anchored on, when it is
    /// not the scene's [`SummonScene::origin`] - a part seated later than
    /// the scene's first, on its own spawn position ([`SummonScene::push_parts`]).
    pub origin: Option<[i16; 3]>,
    /// A label the code that seated the part keeps to find it again - a
    /// module that writes into its own spawned parts every frame (PROT 0954
    /// places its wheel icons through their `+0x14` / `+0x16`). `None` for
    /// a part nobody addresses.
    pub tag: Option<u32>,
    /// Halfword stores the code that seated the part makes into it right
    /// after the seater's own move-VM run, as `(actor offset, value)`: a
    /// stager arm writing into the part `FUN_80021B04` handed back
    /// ([`SummonScene::seat_run`] applies them once, after that run).
    pub seat_pokes: Vec<(u16, u16)>,
    /// Halve the part's speed `+0x98` after the seat run (PROT 0908's
    /// stager arm 2, `0x801F84E0..0x801F8500`).
    pub seat_halve_speed: bool,
}

impl SummonPartRuntime {
    /// Store halfword `value` at actor offset `off` - the offsets a stager
    /// arm writes into a part it seated: `+0x14..+0x18` (position), `+0x28`
    /// (roll), `+0x3C..+0x40` (velocity), `+0x42`, `+0x54` (wait), `+0x72`
    /// (scale), `+0x84`, `+0x96` (heading), `+0x98` (speed) and the
    /// `+0x9C..` block.
    pub fn poke(&mut self, off: u16, value: u16) {
        let s = &mut self.state;
        match off {
            0x14 => s.world_x = value as i16,
            0x16 => s.world_y = value as i16,
            0x18 => s.world_z = value as i16,
            0x24 => s.render_24 = value as i16,
            0x26 => s.render_26 = value as i16,
            0x28 => s.render_28 = value as i16,
            0x3C => s.anim_3c = value as i16,
            0x3E => s.anim_3e = value as i16,
            0x40 => s.anim_40 = value as i16,
            0x42 => s.field_42 = value,
            0x54 => s.wait_timer = value as i16,
            0x72 => s.field_72 = value,
            0x80 => s.anim_80 = value as i16,
            0x82 => s.anim_82 = value as i16,
            0x84 => s.anim_84 = value as i16,
            0x96 => s.tween_scale_x = value as i16,
            0x98 => s.tween_scale_y = value as i16,
            0x9A => s.tween_scale_z = value as i16,
            o if o >= 0x9C => s.set_actor_u16(usize::from(o), value),
            _ => {}
        }
    }
}

/// One live cast part as a retail state holds it: the record it runs
/// (`+0x48`, the slot-B VA the engine keeps as [`SummonPartRuntime::tag`])
/// and the halfwords [`SummonPartRuntime::poke`] seats - position, rotation
/// banks, wait and the `+0x9C..+0xCC` channel block. Installed by
/// [`crate::summon::SummonScene::install_part_snapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastPartSeed {
    pub tag: u32,
    pub pokes: Vec<(u16, u16)>,
}

/// The actor offsets a [`CastPartSeed`] carries.
pub const CAST_PART_SEED_OFFSETS: [u16; 31] = [
    0x14, 0x16, 0x18, 0x24, 0x26, 0x28, 0x54, 0x9C, 0x9E, 0xA0, 0xA2, 0xA4, 0xA6, 0xA8, 0xAA, 0xAC,
    0xAE, 0xB0, 0xB2, 0xB4, 0xB6, 0xB8, 0xBA, 0xBC, 0xBE, 0xC0, 0xC2, 0xC4, 0xC6, 0xC8, 0xCA,
];

/// `DAT_1F800393 * DAT_1F80037D` at one vsync per frame and the rate byte's
/// boot value `8` (planted at `0x80055FB4` / `0x80055FBC`).
pub const RETAIL_CHANNEL_DELTA: u16 = 8;

/// One mesh-bearing part's render draw. The transform is the engine's
/// interpretation of the move-VM state (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SummonPartDraw {
    /// Index into [`crate::world::World::global_tmd_pool`].
    pub model_index: usize,
    /// World position (move-VM `world_x/y/z`).
    pub world_pos: [f32; 3],
    /// Euler XYZ rotation in radians (from the move-VM rotation banks).
    pub rot: [f32; 3],
    /// The part's `+0x52` word (move-VM op `0x15`,
    /// `move_vm::ActorState::field_52`). Its `0x780` bits send the draw
    /// through retail's camera-relative rotation `FUN_8001CF50`; a host
    /// resolves them with `legaia_render_kernels::gte::camera_relative_model_prefix`.
    pub flags_52: u16,
    /// The part's armed VDF morph lanes - `(sub_entry_index, weight)` per
    /// lane op `0x0A` set up, while the envelope bit `0x1000` is raised.
    /// Empty (`count == 0`) for a part that draws its rest mesh. A host
    /// resolves the morphed mesh with
    /// [`crate::world::World::morphed_part_tmd`].
    pub morph: PartMorph,
    /// The part's render scale `+0x72` (`0x1000` = unit). The model draw at
    /// `0x8001B160` scales the mesh by `+0x72 / 0x1000` when it is not
    /// `0x1000` (`0x8001B240..0x8001B2C4`).
    pub scale: u16,
    /// The colour word `+0x74` and depth-cue level `+0x78` the model draw
    /// hands the prim dispatcher `FUN_80043390`: ABE / ABR ORed into every
    /// packet, each packet colour cued toward the word's far colour by the
    /// level. A black far colour under an additive ABR is how a part fades:
    /// the Spirit aura's op `0x0D` ramps the level from `0x1000` (black) to
    /// `0` and back.
    pub colour: legaia_render_kernels::sprite_arm::ColourWord,
    /// The part's shatter word `+0x7A`: non-zero breaks its mesh apart
    /// through the shatter renderer ([`crate::part_shatter`]).
    pub shatter: i16,
}

impl SummonPartDraw {
    /// Whether the draw renders its pool mesh untouched: no morph lane, unit
    /// scale and a colour word that changes no packet. A host that keeps the
    /// pool meshes resident can draw such a part by index; any other part
    /// needs [`crate::world::World::part_draw_vram_mesh`].
    pub fn draws_rest_mesh(&self) -> bool {
        self.morph.count == 0
            && self.scale == SPAWN_RENDER_SCALE
            && !self.colour.semi
            && self.colour.abr == 0
            && self.colour.ir0 == 0
            && self.shatter == 0
    }

    /// Apply the model draw's per-part render state to a local-space mesh:
    /// the shatter word, the render scale, then each packet's colour
    /// through the colour word's depth cue with its ABE / ABR ORed into the
    /// TSB (the dispatcher ORs both into the packets it writes,
    /// `0x80043504..0x80043510`).
    pub fn apply_render_state(&self, mesh: &mut legaia_tmd::mesh::VramMesh) {
        // The shatter works on the object-local vertices, ahead of the
        // object matrix that carries the scale.
        crate::part_shatter::apply_part_shatter(mesh, self.shatter);
        if self.scale != SPAWN_RENDER_SCALE {
            let k = f32::from(self.scale) / 4096.0;
            for p in &mut mesh.positions {
                *p = p.map(|v| v * k);
            }
        }
        if self.colour.ir0 != 0 {
            for c in &mut mesh.colors {
                *c = self.colour.cue(*c);
            }
        }
        let abr = u16::from(self.colour.abr) << 5;
        let semi = if self.colour.semi {
            legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT
        } else {
            0
        };
        if abr | semi != 0 {
            for ct in &mut mesh.cba_tsb {
                ct[1] |= abr | semi;
            }
        }
    }
}

/// A part's live morph lanes, carried by value on [`SummonPartDraw`] (the
/// retail arrays op `0x0A` writes: `+0x6C` count, `+0xB0 + lane` index byte,
/// `+0xA0 + lane*2` weight - [`legaia_engine_vm::vdf_morph::actor_morph_lanes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartMorph {
    pub lanes: [(u8, u16); legaia_engine_vm::vdf_morph::ACTOR_MORPH_LANES],
    pub count: u8,
}

impl PartMorph {
    /// The armed lanes of a part state, or the empty morph when its envelope
    /// bit is clear.
    pub fn of(state: &ActorState) -> Self {
        let mut out = Self::default();
        if state.flags & legaia_engine_vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE == 0 {
            return out;
        }
        for (i, lane) in legaia_engine_vm::vdf_morph::actor_morph_lanes(state)
            .into_iter()
            .take(out.lanes.len())
            .enumerate()
        {
            out.lanes[i] = lane;
            out.count = i as u8 + 1;
        }
        out
    }

    /// The armed lanes.
    pub fn lanes(&self) -> &[(u8, u16)] {
        &self.lanes[..usize::from(self.count)]
    }
}
