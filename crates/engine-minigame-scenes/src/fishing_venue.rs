//! The fishing **venue actors**' per-frame step, shared by every host that
//! runs the fishing minigame over a world.
//!
//! The session ([`legaia_engine_fishing::fishing::PondSession`]) is the minigame's rules: the
//! cast, the band check, the fight, the lure's own walk-grid drift. Around it
//! the overlay runs a handful of venue actors that are presentation rather
//! than rules, and this module is their frame:
//!
//! - the free-swimming fish wander (`FUN_801D2278`,
//!   [`legaia_engine_fishing::fishing_actors::FishWander`]) at the shore while the cast is
//!   idle, steered by the held D-pad, with its **retarget ripple**
//!   (the `FUN_801D2278` spawn at `0x801D23FC`) on every new wander target, seated through the venue camera;
//! - the wander actor settled onto the venue floor through the shared ground
//!   solver ([`legaia_engine_fishing::fishing_chrome::float_actor_tick`]) and its retail
//!   camera publish;
//! - the reeling-line actor (`FUN_801D4948`,
//!   [`legaia_engine_fishing::fishing_actors::LineActorSim`]) across hook, fight and the
//!   catch celebration, whose **bursts** ride the session's lure;
//! - the point-exchange sub-screen's idle sway
//!   ([`legaia_engine_fishing::fishing_chrome::sway_vector`]).
//!
//! It used to live inside the native window's `tick_fishing_actors`, which
//! made the ripples, the bursts, the venue camera and the swaying prize panel
//! native-only. The step is host-agnostic - every input is world state, the
//! held pad or the scene's `.MAP` bytes - so it lives here and each host calls
//! [`tick_fishing_venue`] from its own minigame frame, then applies the
//! returned camera writes to its own [`legaia_engine_field::camera::Camera`].

use crate::minigame_state::MinigameState;
use legaia_engine_fishing::fishing::{PondEvent, PondPhase};
use legaia_engine_fishing::fishing_chrome as fc;

/// Seed of the venue's small `rand()` stand-in (an xorshift the port uses for
/// the wander's retarget rolls). The native window seeded its host-side copy
/// with the same word, so a fixed input stream retargets identically.
pub const VENUE_RNG_SEED: u32 = 0x1234_5678;

/// The party's anchor **tile** per venue, `(x, z)` in 128-unit `.MAP` tiles:
/// venue `0` (Buma) `(0x25, 0x54)`, venue `1` (Vidna) `(0x2E, 0x23)`. The
/// driver's setup state picks the pair on `DAT_801D90D0`
/// (`0x801CF5F8..0x801CF62C`) and shifts both by `7` into the three party
/// actors' `+0x14` / `+0x18`.
pub const PARTY_ANCHOR_TILE: [(i16, i16); 2] = [(0x25, 0x54), (0x2E, 0x23)];

/// GTE `H` the fishing setup installs (`li v0,0x140` / `sh v0,-0x490c(v1)`
/// = `_DAT_8007B6F4` at `0x801CF760..0x801CF764`), in place of the field's
/// `0x200`.
pub const VENUE_GTE_H: i32 = 0x140;

/// One of the three party actors the fishing setup spawns on the shore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingPartyMember {
    /// Global model-pool slot (the spawn record's `+0x04`, copied into the
    /// actor's model word): the party member in that slot.
    pub model: u8,
    /// World `+0x14` / `+0x18`. `+0x16` is `0` at spawn; the floor solver
    /// settles it.
    pub x: i16,
    pub z: i16,
    /// `+0x26` facing (`0x800` faces the water, +Z).
    pub facing: i16,
    /// Bound clip id `+0x5C`.
    pub clip: u16,
    /// Whether the clip resolves against the resident party clip bank
    /// (actor flag `0x01000000`) rather than the venue scene's own ANM bank.
    pub party_bank: bool,
    /// Clip rate `+0x6A` (sixteenths of a frame per tick; `0` keeps the
    /// allocator's default, `0x10`).
    pub rate: u16,
}

/// The three shore actors the fishing setup spawns (`FUN_801CF3BC`,
/// `0x801CF66C..0x801CF79C`, three `FUN_80020DE0` calls):
///
/// | spawn | record | model `+0x04` | `+0x14` | `+0x18` | `+0x26` | `+0x5C` | `+0x6A` | flag `0x01000000` |
/// |---|---|---|---|---|---|---|---|---|
/// | lead | `0x801D8FAC` (tick `FUN_801D2050`) | `0` | `tx << 7` | `tz << 7` | `0x800` | `2` | - | set |
/// | 2nd | `0x801D8FC4` (tick `FUN_801D70EC`) | `1` | `(tx << 7) + 0x60` | `tz << 7` | `0x680` | `0xB` | `8` | cleared |
/// | 3rd | `0x801D8FC4` | `2` | `(tx << 7) - 0x60` | `(tz << 7) - 0x40` | `0x600` | `0xC` | `8` | cleared |
///
/// The lead plays its party-bank idle; the other two play clips `0xB` / `0xC`
/// of the venue scene's own bank. The library state `minigame_fishing` holds
/// exactly these three actors at `(4736, 10752)`, `(4832, 10752)` and
/// `(4640, 10688)` on venue `0`.
pub fn party_placements(venue: usize) -> [FishingPartyMember; 3] {
    let (tx, tz) = PARTY_ANCHOR_TILE[venue & 1];
    let (x, z) = (tx << 7, tz << 7);
    [
        FishingPartyMember {
            model: 0,
            x,
            z,
            facing: 0x800,
            clip: 2,
            party_bank: true,
            rate: 0,
        },
        FishingPartyMember {
            model: 1,
            x: x + 0x60,
            z,
            facing: 0x680,
            clip: 0xB,
            party_bank: false,
            rate: 8,
        },
        FishingPartyMember {
            model: 2,
            x: x - 0x60,
            z: z - 0x40,
            facing: 0x600,
            clip: 0xC,
            party_bank: false,
            rate: 8,
        },
    ]
}

/// The fishing venue's camera for the lead at `(x, y, z)` facing `facing`:
/// the setup's reset (angles `0`, `TR.x = 0`, `TR.z = 0x974`), the lead
/// tick's publish ([`legaia_engine_fishing::fishing_actors::fish_camera`]: yaw off the facing, `TR.y = 0x400 -
/// 6 y`, focus on the lead with a zero `Y`) and the setup's `H`
/// ([`VENUE_GTE_H`]). The eye trio reduces by the 6x world scale retail folds
/// into its rotation, as the field follow camera's does.
///
/// The library state `minigame_fishing` reads exactly these globals: angles
/// `(0, 0, 0)`, eye `(0, 0x700, 0x974)`, focus `(-4736, 0, -10752)`, `H`
/// `0x140`.
pub fn venue_camera_view(
    x: i16,
    y: i16,
    z: i16,
    facing: i16,
) -> legaia_engine_camera::psx_camera::FieldCameraView {
    let reset = fc::venue_camera_reset();
    let publish = legaia_engine_fishing::fishing_actors::fish_camera(x, y, z, facing);
    let s = legaia_engine_field::camera::CUTSCENE_WORLD_SCALE;
    let to_rad = |u: i32| u as f32 * (std::f32::consts::TAU / 4096.0);
    legaia_engine_camera::psx_camera::FieldCameraView {
        focus: [
            -publish.translation.0 as f32,
            publish.translation.1 as f32,
            -publish.translation.2 as f32,
        ],
        pitch: to_rad(i32::from(reset.rot[0])),
        yaw: to_rad(i32::from(publish.yaw)),
        roll: to_rad(i32::from(reset.rot[2])),
        h: VENUE_GTE_H as f32,
        tr_eye: [
            reset.tr_x as f32 / s,
            publish.pitch_term as f32 / s,
            reset.tr_z as f32 / s,
        ],
    }
}

/// The venue camera over the live lead actor (his position, floor and D-pad
/// aim), while a fishing session is up: the view both play hosts draw the
/// pond with and project the fishing line through. `None` before the venue
/// has armed.
pub fn venue_view(mg: &MinigameState) -> Option<legaia_engine_camera::psx_camera::FieldCameraView> {
    mg.fishing.as_ref()?;
    let w = mg.fishing_venue.wander.as_ref()?;
    Some(venue_camera_view(w.x, w.y, w.z, w.facing))
}

/// Where the lead's actor spawns for `venue`, `(x, y, z)` world units - the
/// actor [`legaia_engine_fishing::fishing_actors::FishWander`] models (the lead's tick `FUN_801D2050` /
/// `FUN_801D2278`, whose actor pointer `DAT_801D928C` is the lead's).
pub fn lead_spawn(venue: usize) -> (i16, i16, i16) {
    let lead = party_placements(venue)[0];
    (lead.x, 0, lead.z)
}

/// The venue actors' state across frames. Lives on
/// [`MinigameState::fishing_venue`] so every host reads the same actors.
#[derive(Debug, Clone)]
pub struct FishingVenue {
    /// The free-swimming fish, armed on the first fishing frame.
    pub wander: Option<legaia_engine_fishing::fishing_actors::FishWander>,
    /// The reeling-line actor, armed on the hook event.
    pub line: Option<legaia_engine_fishing::fishing_actors::LineActorSim>,
    /// The venue scene's `.MAP` extended footprint (the engine's
    /// `_DAT_1F8003EC` floor buffer), loaded once at arm.
    floor: Option<Vec<u8>>,
    /// The sub-screen sway phase (`0x801D9118`).
    sway_angle: i32,
    /// This frame's sway offset, applied to the point-exchange panel.
    pub sway_offset: (i16, i16),
    rng: u32,
    /// The two ambient-ripple part records off the fishing overlay
    /// ([`RippleRecords`]), when the host installed them.
    pub ripple_records: Option<RippleRecords>,
    /// The live ripple parts, run through the move VM as `FUN_80021B04`
    /// seats them and `FUN_80021DF4` ticks them.
    pub ripple_fx: Option<legaia_engine_effects::summon::SummonScene>,
}

/// VA of the ripple record the lead's roll spawns on `rand() & 1 != 0`
/// (`lui a2,0x801e` / `addiu a2,a2,-0x7128` at `0x801D23E4..0x801D23EC`).
pub const RIPPLE_RECORD_A_VA: u32 = 0x801D_8ED8;
/// VA of the ripple record the roll spawns on `rand() & 1 == 0`
/// (`addiu a2,a2,-0x70bc` at `0x801D23F8`).
pub const RIPPLE_RECORD_B_VA: u32 = 0x801D_8F44;

/// The ambient ripple's two part records, as the fishing overlay holds them:
/// the move-buffer format `[i16 model_sel][u16 reserved][bytecode]`. Both
/// select library mesh `0`, but their bytecode turns the part into a
/// render-mode-4 default-arm node (move-VM op `0x13`), whose geometry is the
/// procedural ring `FUN_80028158` builds - so what draws is the ring
/// ([`legaia_engine_effects::effect_default_arm`]), not a mesh.
#[derive(Debug, Clone)]
pub struct RippleRecords {
    /// The two records' bytes, record A at `0`.
    bytes: Vec<u8>,
    /// Record A then record B.
    parts: Vec<legaia_overlay_images::summon_overlay::SummonPart>,
}

impl RippleRecords {
    /// Bytes kept past record B's start - its bytecode ends at op `0x08`
    /// well inside this.
    const TAIL: usize = 0x100;

    /// Cut both records out of the fishing overlay's as-loaded image (PROT
    /// 0972 at `base_va`). `None` when the image does not reach them.
    pub fn from_overlay(loaded: &[u8], base_va: u32) -> Option<Self> {
        let a = RIPPLE_RECORD_A_VA.checked_sub(base_va)? as usize;
        let b = (RIPPLE_RECORD_B_VA - RIPPLE_RECORD_A_VA) as usize;
        let bytes = loaded
            .get(a..(a + b + Self::TAIL).min(loaded.len()))?
            .to_vec();
        if bytes.len() <= b + 4 {
            return None;
        }
        let parts = legaia_overlay_images::summon_overlay::parse_records_at(&bytes, &[0, b]);
        (parts.len() == 2).then_some(Self { bytes, parts })
    }
}

/// One ripple-ring triangle on the retail 320x240 stage
/// ([`FishingVenue::ripple_screen_tris`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RippleTri {
    /// The three corners, stage pixels.
    pub xy: [(i16, i16); 3],
    /// The three corners' packet colours (`texel * colour / 128` modulation).
    pub rgb: [[u8; 3]; 3],
    /// The packet's semi-transparency bit.
    pub semi: bool,
}

/// The ripple parts' move-VM host: their programs touch no world state.
struct RippleHost;
impl legaia_engine_vm::move_vm::MoveHost for RippleHost {}

impl Default for FishingVenue {
    fn default() -> Self {
        Self {
            wander: None,
            line: None,
            floor: None,
            sway_angle: 0,
            sway_offset: (0, 0),
            rng: VENUE_RNG_SEED,
            ripple_records: None,
            ripple_fx: None,
        }
    }
}

impl FishingVenue {
    /// Whether the venue has been armed (its wander actor exists).
    pub fn armed(&self) -> bool {
        self.wander.is_some()
    }

    /// Spawn ripple record `variant` (`1` = A, `0` = B) at world `pos` with
    /// the roll's rotation trio - one `FUN_80021B04(sp+0x10, sp+0x18,
    /// record, 0x1000)` call (`0x801D23FC`), which seats the part and runs
    /// its first move-VM slice.
    fn spawn_ripple(&mut self, variant: u32, pos: [i16; 3], rot: [i16; 3]) {
        let Some(rec) = self.ripple_records.as_ref() else {
            return;
        };
        let part = &rec.parts[if variant != 0 { 0 } else { 1 }];
        let scene = self.ripple_fx.get_or_insert_with(|| {
            legaia_engine_effects::summon::SummonScene::spawn_parts(&[], &rec.bytes, 0, pos)
        });
        scene.push_parts(std::slice::from_ref(part), &rec.bytes, pos, rot);
        // The seater's own first move-VM run (`FUN_80021B04` calls
        // `FUN_80023070` before it returns).
        scene.seat_run(&mut RippleHost);
    }

    /// One frame of the ripple parts' tick `FUN_80021DF4` (the move VM and
    /// the channel / motion blocks), retiring the parts whose program halted.
    fn tick_ripples(&mut self) {
        let Some(scene) = self.ripple_fx.as_mut() else {
            return;
        };
        scene.tick(
            &mut RippleHost,
            legaia_engine_effects::summon::RETAIL_CHANNEL_DELTA,
        );
        scene.parts.retain(|p| !p.finished);
    }

    /// [`Self::ripple_world_mesh`] projected onto the stage through the
    /// venue camera over the lead ([`venue_camera_view`],
    /// [`project_to_retail_screen`]). For a host that draws on a 2D canvas
    /// rather than through an effect-mesh pass - the minigames page. A
    /// triangle with a corner behind the eye is dropped.
    pub fn ripple_screen_tris(&self) -> Vec<RippleTri> {
        let Some(w) = self.wander.as_ref() else {
            return Vec::new();
        };
        let view = venue_camera_view(w.x, w.y, w.z, w.facing);
        let m = self.ripple_world_mesh();
        let mut out = Vec::new();
        for tri in m.indices.as_chunks::<3>().0 {
            let idx = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
            let Some(pts) = idx
                .iter()
                .map(|&i| {
                    m.positions
                        .get(i)
                        .and_then(|v| project_to_retail_screen(&view, v.map(|c| c.round() as i32)))
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let rgb = idx.map(|i| m.colors.get(i).copied().unwrap_or([0x80; 3]));
            let semi = m
                .cba_tsb
                .get(idx[0])
                .is_some_and(|ct| ct[1] & legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT != 0);
            out.push(RippleTri {
                xy: [pts[0], pts[1], pts[2]],
                rgb,
                semi,
            });
        }
        out
    }

    /// Every live ripple ring of [`Self::ripple_draws`] as one mesh in raw
    /// retail world space (Y down): each ring vertex placed by its node's
    /// rotation banks (`FUN_80026988`, `Rx * Ry * Rz`) and position, rounded
    /// to the integer world units the GTE works in. The geometry a host draws
    /// in the same pass as the pond, under the pond's own view-projection
    /// (`FishingScene::vp_raw`) - the browser play page's venue frame.
    pub fn ripple_world_mesh(&self) -> legaia_tmd::mesh::VramMesh {
        let mut out = legaia_tmd::mesh::VramMesh {
            positions: Vec::new(),
            uvs: Vec::new(),
            cba_tsb: Vec::new(),
            indices: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
        };
        for d in self.ripple_draws() {
            let r = legaia_engine_camera::psx_camera::camera_rotation(d.rot[0], d.rot[1], d.rot[2]);
            let place = |v: [f32; 3]| -> [f32; 3] {
                let mut p = [0f32; 3];
                for (row, o) in p.iter_mut().enumerate() {
                    *o = d.world_pos[row] + (0..3).map(|c| r[c * 4 + row] * v[c]).sum::<f32>();
                }
                p.map(f32::round)
            };
            let m = &d.mesh;
            let base = out.positions.len() as u32;
            out.positions.extend(m.positions.iter().map(|&v| place(v)));
            for i in 0..m.positions.len() {
                out.uvs.push(m.uvs.get(i).copied().unwrap_or_default());
                out.cba_tsb
                    .push(m.cba_tsb.get(i).copied().unwrap_or_default());
                out.colors
                    .push(m.colors.get(i).copied().unwrap_or([0x80; 3]));
                out.normals.push([0.0; 3]);
            }
            out.indices.extend(m.indices.iter().map(|&i| base + i));
        }
        out
    }

    /// This frame's ripple geometry: each live part's default-arm ring
    /// ([`legaia_engine_effects::effect_default_arm::default_arm_draw`]) at its
    /// world position and rotation banks, in raw retail world units - the
    /// list every host draws through its effect-mesh pass.
    pub fn ripple_draws(&self) -> Vec<legaia_engine_effects::effect_ribbon::RibbonDraw> {
        self.ripple_fx
            .as_ref()
            .map(|s| s.default_arm_draws())
            .unwrap_or_default()
    }
}

/// The camera writes one venue frame asks of its host, in the order the
/// native window used to perform them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VenueCameraWrites {
    /// The one-time venue reset (`rot` trio + `TR.x` / `TR.z`), on the arm
    /// frame. Axis 4 (`TR.y`) is deliberately untouched, as retail leaves
    /// `_DAT_800840BC` alone.
    pub reset: Option<fc::VenueCameraReset>,
    /// The wander actor's camera publish, every frame it exists.
    pub publish: Option<legaia_engine_fishing::fishing_actors::FishCamera>,
}

impl VenueCameraWrites {
    /// Write these into the engine camera's retail globals.
    pub fn apply(&self, camera: &mut legaia_engine_field::camera::Camera) {
        let g = &mut camera.globals.0;
        if let Some(reset) = self.reset {
            g[0] = reset.rot[0] as i32;
            g[1] = reset.rot[1] as i32;
            g[2] = reset.rot[2] as i32;
            g[3] = reset.tr_x;
            g[5] = reset.tr_z;
        }
        if let Some(cam) = self.publish {
            g[1] = cam.yaw as i32;
            g[4] = cam.pitch_term;
            g[6] = cam.translation.0;
            g[7] = cam.translation.1;
            g[8] = cam.translation.2;
        }
    }
}

/// The 4096-step sine table the sub-screen sway samples. Retail reads the
/// shared table through `*_DAT_8007B81C` (runtime data the port does not
/// stage); this synthesizes an equivalent once.
pub fn sway_sine_table() -> &'static [i16] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<i16>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..fc::SINE_TURN)
            .map(|i| {
                let f = (i as f64) * std::f64::consts::TAU / fc::SINE_TURN as f64;
                (f.sin() * 4096.0).round() as i16
            })
            .collect()
    })
}

/// Run one venue frame.
///
/// `in_fishing` is whether the world is in [`legaia_engine_mode::mode::SceneMode::Fishing`];
/// leaving the mode drops every actor. `held_packed` is the held pad in the
/// packed retail layout (`_DAT_8007B850`; the engine's PSX-layout mask
/// rotated right by eight). `floor` is asked for the venue's `.MAP` bytes
/// once, on the arm frame - a host with no scene supplies `None` and the
/// wander keeps its spawn height.
///
/// Reads this tick's `fishing_events` (so it must run after the world tick
/// that raised them) and spawns the retarget ripples and catch bursts into
/// [`MinigameState::fx`], where every host's effect-pool draw picks them up.
pub fn tick_fishing_venue(
    mg: &mut MinigameState,
    in_fishing: bool,
    held_packed: u16,
    floor: impl FnOnce() -> Option<Vec<u8>>,
) -> VenueCameraWrites {
    if !in_fishing {
        mg.fishing_venue = FishingVenue::default();
        return VenueCameraWrites::default();
    }
    let Some(session) = mg.fishing.as_ref() else {
        return VenueCameraWrites::default();
    };
    tick_venue_actors(
        &mut mg.fishing_venue,
        session,
        &mg.fishing_events,
        mg.fishing_exchange.is_some(),
        held_packed,
        &mut mg.fx,
        floor,
    )
}

/// One venue frame over the pieces it touches, for a host that holds a
/// [`PondSession`](legaia_engine_fishing::fishing::PondSession) and an effect
/// pool but no [`MinigameState`] - the minigames page. [`tick_fishing_venue`]
/// is this over the world's own fields, so the two run one step.
///
/// `events` is this tick's session events and `exchange_open` whether the
/// point-exchange list is up. The retarget ripples and the catch bursts are
/// world points; each is seated in `fx` at its projection through the venue
/// camera over the lead ([`venue_camera_view`], the camera this frame
/// publishes) by [`project_to_retail_screen`]. A point that does not project
/// spawns nothing.
pub fn tick_venue_actors(
    v: &mut FishingVenue,
    session: &legaia_engine_fishing::fishing::PondSession,
    events: &[PondEvent],
    exchange_open: bool,
    held_packed: u16,
    fx: &mut legaia_engine_minigames::minigame_fx::MinigameFxPool,
    floor: impl FnOnce() -> Option<Vec<u8>>,
) -> VenueCameraWrites {
    let mut out = VenueCameraWrites::default();
    let phase = session.phase();
    // One-time venue arm: the wander actor, the floor buffer, and the venue
    // camera reset.
    if v.wander.is_none() {
        let (x, y, z) = lead_spawn(session.venue);
        v.wander = Some(legaia_engine_fishing::fishing_actors::FishWander::new(
            x, y, z,
        ));
        v.floor = floor();
        out.reset = Some(fc::venue_camera_reset());
    }
    // The wander runs while the cast is idle (retail's shore states `0xc` /
    // `0xd` / `0x14`, before the lure flies); the D-pad steers the lead.
    let mut ripple = None;
    if matches!(
        phase,
        PondPhase::Idle | PondPhase::WindUp | PondPhase::Power
    ) {
        let mut rng = v.rng;
        let rolled = v.wander.as_mut().and_then(|w| {
            w.tick(held_packed, || {
                let mut x = rng;
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                rng = x;
                x
            })
        });
        v.rng = rng;
        // Retail spawns the ripple at the rolled point: an on-stack copy of
        // the lead's `+0x14..+0x18` trio with `x` / `z` offset and `y` kept
        // (`0x801D22A4..0x801D23C8`), handed to `FUN_80021B04(sp+0x10, ..)`
        // at `0x801D23FC`; the actor itself never moves.
        let y = v.wander.as_ref().map_or(0, |w| i32::from(w.y));
        ripple = rolled.map(|t| {
            (
                t.ripple_variant,
                [t.x as i16, y as i16, t.z as i16],
                [0, t.rotation as i16, 0],
            )
        });
    }
    // Settle the actor onto the venue floor (the `.MAP` height grid through
    // the shared ground solver) and publish its camera.
    if let (Some(w), Some(buf)) = (v.wander.as_mut(), v.floor.as_ref()) {
        let ramp = legaia_engine_field::minigame_floor::height_ramp();
        let grid = legaia_engine_field::minigame_floor::FloorGrid::new(buf);
        let t = fc::float_actor_tick(grid, w.x, w.z, 0, &ramp);
        w.y = t.y;
    }
    if let Some(w) = v.wander.as_ref() {
        out.publish = Some(w.camera());
    }
    // The camera the frame draws with seats this frame's world-space spawns.
    let view = v
        .wander
        .as_ref()
        .map(|w| venue_camera_view(w.x, w.y, w.z, w.facing));
    let seat = |p: [i32; 3]| {
        view.as_ref()
            .and_then(|view| project_to_retail_screen(view, p))
    };
    // The ripple parts tick first (the actor list runs the parts before the
    // lead's tick seats a new one), then the roll's spawn runs its first
    // slice.
    v.tick_ripples();
    if let Some((variant, pos, rot)) = ripple {
        v.spawn_ripple(variant, pos, rot);
    }
    // The line actor: armed on the hook event, landed on the catch event,
    // dropped on a snap.
    for e in events {
        match *e {
            PondEvent::Hooked(_) => {
                v.line = Some(legaia_engine_fishing::fishing_actors::LineActorSim::hooked())
            }
            PondEvent::Landed(points) => match v.line.as_mut() {
                Some(line) => line.land(points),
                None => v.line = None,
            },
            PondEvent::Snapped => v.line = None,
            PondEvent::Splash | PondEvent::Recast => {}
        }
    }
    if let Some(mut line) = v.line.take() {
        let f = line.tick(1);
        // Retail's celebration bursts ride the line actor, which sits on the
        // lure - not on the free-swimming fish the venue also draws.
        let origin = session
            .lure_actor()
            .map(|l| (l.x(), l.z))
            .or_else(|| v.wander.as_ref().map(|w| (w.x, w.z)))
            .unwrap_or((0, 0));
        // The bursts' *visuals* are this actor's. Their **cues** are not - the
        // hook cue and the celebration tiers are queued by
        // `World::tick_fishing` off the session's own events, where every
        // host drains them. Firing them here as well would play each twice.
        for b in &f.bursts {
            if let Some((x, y)) = seat(b.world_point(origin)) {
                fx.spawn(x, y, legaia_engine_minigames::minigame_fx::BURST_SPRITE_ID);
            }
        }
        if !f.done {
            v.line = Some(line);
        }
    }
    // Sub-screen idle sway while the point-exchange list is up.
    if exchange_open {
        let (sv, next) = fc::sway_vector(sway_sine_table(), v.sway_angle, 1);
        v.sway_angle = next;
        v.sway_offset = (sv.x, sv.y);
    } else {
        v.sway_offset = (0, 0);
    }
    out
}

/// This frame's fishing line on a world host: [`legaia_engine_fishing::fishing::PondSession::line_frame`]
/// with the fish end projected through `view`, the scene camera the host
/// resolved for the frame it draws (`camera_view::resolve_field_camera(..)
/// .field_view()`), as retail's lure tick projects it through the scene
/// view the field matrix build leaves in the GTE.
///
/// The native window and the browser play page both call this and wrap the
/// result with the one builder `legaia_engine_ui::fishing::line`. `None`
/// with no session, no camera or no line out.
pub fn fishing_line_frame(
    mg: &mut MinigameState,
    view: Option<&legaia_engine_camera::psx_camera::FieldCameraView>,
) -> Option<legaia_engine_fishing::fishing_actors::FishingLine> {
    let view = view?;
    mg.fishing
        .as_mut()?
        .line_frame(|p| project_to_retail_screen(view, p))
}

/// Project a raw retail-world point through a host camera pose into retail
/// 320x240 screen space - `RTPS`'s `OFX/OFY + H * xy / z`, saturated to the
/// `SXY` range, floored as the GTE's `>> 16` floors. `None` behind the eye.
pub fn project_to_retail_screen(
    view: &legaia_engine_camera::psx_camera::FieldCameraView,
    p: [i32; 3],
) -> Option<(i16, i16)> {
    let e = view.eye_space([p[0] as f32, p[1] as f32, p[2] as f32]);
    if e[2] <= 0.0 {
        return None;
    }
    let axis = |c: f32, of: i32| {
        (of as f32 + view.h * c / e[2])
            .floor()
            .clamp(-1024.0, 1023.0) as i16
    };
    Some((
        axis(
            e[0],
            legaia_engine_fishing::fishing_actors::GTE_SCREEN_CENTRE.0,
        ),
        axis(
            e[1],
            legaia_engine_fishing::fishing_actors::GTE_SCREEN_CENTRE.1,
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_fishing::fishing::PondSession;

    fn session() -> PondSession {
        PondSession::new(
            Vec::new(),
            vec![[0u32; 8]; 8],
            Vec::new(),
            0,
            1,
            2,
            60,
            legaia_engine_fishing::fishing::FishingRecord::default(),
            0,
            0x0BAD_F00D,
        )
    }

    #[test]
    fn leaving_the_mode_drops_the_actors() {
        let mut mg = MinigameState::new();
        mg.fishing = Some(session());
        let w = tick_fishing_venue(&mut mg, true, 0, || None);
        assert!(mg.fishing_venue.armed());
        assert!(w.reset.is_some(), "the arm frame resets the venue camera");
        assert!(w.publish.is_some());
        let w = tick_fishing_venue(&mut mg, true, 0, || None);
        assert!(w.reset.is_none(), "the reset is one-time");
        let w = tick_fishing_venue(&mut mg, false, 0, || None);
        assert_eq!(w, VenueCameraWrites::default());
        assert!(!mg.fishing_venue.armed());
    }

    #[test]
    fn the_wander_retargets_into_its_own_ripple_parts() {
        // Held D-pad right steers the lead; the retarget roll seats one of
        // the overlay's ripple records as a move-VM part (here a record that
        // halts on its first op), not a pool placeholder.
        let base = legaia_minigame_formats::fishing_species::FISHING_OVERLAY_BASE_VA;
        let mut image = vec![0u8; (RIPPLE_RECORD_B_VA - base) as usize + 0x200];
        for va in [RIPPLE_RECORD_A_VA, RIPPLE_RECORD_B_VA] {
            let o = (va - base) as usize;
            // model_sel 0, reserved 0, then op 0x08 (halt).
            image[o + 4] = 0x08;
        }
        let mut mg = MinigameState::new();
        mg.fishing = Some(session());
        mg.fishing_venue.ripple_records = RippleRecords::from_overlay(&image, base);
        assert!(mg.fishing_venue.ripple_records.is_some());
        let held = legaia_engine_system::pad::PACK_RIGHT;
        let mut spawned = false;
        for _ in 0..2000 {
            tick_fishing_venue(&mut mg, true, held, || None);
            if mg.fishing_venue.ripple_fx.is_some() {
                spawned = true;
                break;
            }
        }
        assert!(spawned, "no retarget ripple was seated");
        assert!(mg.fx.is_empty(), "the ripple is not a pool part");
        tick_fishing_venue(&mut mg, true, held, || None);
        assert!(
            mg.fishing_venue
                .ripple_fx
                .as_ref()
                .unwrap()
                .parts
                .is_empty(),
            "a halted ripple part retires"
        );
        let facing = mg.fishing_venue.wander.as_ref().unwrap().facing;
        assert_ne!(facing, 0, "the held D-pad did not steer the wander");
    }

    #[test]
    fn the_hook_arms_the_line_and_the_catch_bursts_off_it() {
        let mut mg = MinigameState::new();
        mg.fishing = Some(session());
        tick_fishing_venue(&mut mg, true, 0, || None);
        mg.fishing_events = vec![PondEvent::Hooked(3)];
        tick_fishing_venue(&mut mg, true, 0, || None);
        assert!(mg.fishing_venue.line.is_some());
        mg.fishing_events = vec![PondEvent::Landed(9000)];
        let before = mg.fx.len();
        let mut burst = false;
        for _ in 0..600 {
            tick_fishing_venue(&mut mg, true, 0, || None);
            mg.fishing_events.clear();
            if mg.fx.len() > before {
                burst = true;
            }
            if mg.fishing_venue.line.is_none() {
                break;
            }
        }
        assert!(burst, "the catch celebration spawned no burst");
    }

    #[test]
    fn the_party_stands_where_the_capture_has_it() {
        // `minigame_fishing` (venue 0): lead (4736, 10752) facing 0x800,
        // second (4832, 10752) 0x680, third (4640, 10688) 0x600.
        let p = party_placements(0);
        assert_eq!(
            p.map(|m| (m.model, m.x, m.z, m.facing, m.clip)),
            [
                (0, 4736, 10752, 0x800, 2),
                (1, 4832, 10752, 0x680, 0xB),
                (2, 4640, 10688, 0x600, 0xC),
            ]
        );
        assert!(p[0].party_bank && !p[1].party_bank && !p[2].party_bank);
        let v = party_placements(1);
        assert_eq!((v[0].x, v[0].z), (0x2E << 7, 0x23 << 7));
        assert_eq!(lead_spawn(0), (4736, 0, 10752));
    }

    #[test]
    fn the_venue_camera_frames_the_party_from_behind() {
        // The captured globals: angles 0, eye (0, 0x700, 0x974), H 0x140,
        // with the lead on the floor at y = -128.
        let view = venue_camera_view(4736, -128, 10752, 0x800);
        assert_eq!(view.h, 320.0);
        assert_eq!(view.tr_eye[1] * 6.0, 1792.0);
        assert_eq!(view.tr_eye[2] * 6.0, 2420.0);
        let screen = |p: [f32; 3]| {
            let e = view.eye_space(p);
            (160.0 + view.h * e[0] / e[2], 120.0 + view.h * e[1] / e[2])
        };
        // The lead's head (~130 units above his feet) sits on the centre
        // column in the lower half, his feet below the frame.
        let (hx, hy) = screen([4736.0, -128.0 - 130.0, 10752.0]);
        assert!(
            (hx - 160.0).abs() < 0.5 && (130.0..200.0).contains(&hy),
            "{hx} {hy}"
        );
        let (_, fy) = screen([4736.0, -128.0, 10752.0]);
        assert!(fy > 240.0, "feet should fall below the frame: {fy}");
        // The second member stands right of him, the third left (and nearer).
        let (nx, _) = screen([4832.0, -258.0, 10752.0]);
        let (gx, _) = screen([4640.0, -258.0, 10688.0]);
        assert!(nx > 200.0 && gx < 120.0, "{nx} {gx}");
    }

    #[test]
    fn the_lead_spawns_on_the_shore_and_holds_his_place() {
        let mut mg = MinigameState::new();
        mg.fishing = Some(session());
        tick_fishing_venue(&mut mg, true, 0, || None);
        let w = mg.fishing_venue.wander.as_ref().unwrap();
        assert_eq!((w.x, w.z), (4736, 10752));
        for _ in 0..600 {
            tick_fishing_venue(&mut mg, true, 0, || None);
        }
        let w = mg.fishing_venue.wander.as_ref().unwrap();
        assert_eq!((w.x, w.z), (4736, 10752));
    }

    #[test]
    fn camera_writes_land_in_the_retail_globals() {
        let mut cam = legaia_engine_field::camera::Camera::default();
        let reset = fc::venue_camera_reset();
        let w = VenueCameraWrites {
            reset: Some(reset),
            publish: None,
        };
        let before_y = cam.globals.0[4];
        w.apply(&mut cam);
        assert_eq!(cam.globals.0[3], reset.tr_x);
        assert_eq!(cam.globals.0[5], reset.tr_z);
        assert_eq!(cam.globals.0[4], before_y, "the reset leaves TR.y alone");
    }
}
