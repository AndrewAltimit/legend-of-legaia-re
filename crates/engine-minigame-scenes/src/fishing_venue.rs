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
//! - the pond swell ([`legaia_engine_fishing::fishing_chrome::water_swell`]):
//!   the head of the venue's floor-height ladder
//!   ([`FishingVenue::floor_ladder`]), which heaves the water cells of the
//!   drawn ground and moves whatever the floor solver settles on them.
//!
//! It used to live inside the native window's `tick_fishing_actors`, which
//! made the ripples, the bursts and the venue camera native-only. The step is host-agnostic - every input is world state, the
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
    let mut cam = legaia_engine_fishing::fishing_actors::VenueCamera::default();
    cam.lead_publish(legaia_engine_fishing::fishing_actors::fish_camera(
        x, y, z, facing,
    ));
    venue_camera_globals_view(&cam)
}

/// The view the fishing camera's retail globals describe
/// ([`legaia_engine_fishing::fishing_actors::VenueCamera`]): the rotation
/// trio as pitch / yaw / roll, the negated focus translation as the focus,
/// the setup's `H` ([`VENUE_GTE_H`]), and `TR` reduced by the 6x world scale
/// retail folds into its rotation, as the field follow camera's is.
pub fn venue_camera_globals_view(
    cam: &legaia_engine_fishing::fishing_actors::VenueCamera,
) -> legaia_engine_camera::psx_camera::FieldCameraView {
    let s = legaia_engine_field::camera::CUTSCENE_WORLD_SCALE;
    let to_rad = |u: i32| u as f32 * (std::f32::consts::TAU / 4096.0);
    legaia_engine_camera::psx_camera::FieldCameraView {
        focus: [
            -cam.focus[0] as f32,
            cam.focus[1] as f32,
            -cam.focus[2] as f32,
        ],
        pitch: to_rad(i32::from(cam.rot[0])),
        yaw: to_rad(i32::from(cam.rot[1])),
        roll: to_rad(i32::from(cam.rot[2])),
        h: VENUE_GTE_H as f32,
        tr_eye: [
            cam.tr[0] as f32 / s,
            cam.tr[1] as f32 / s,
            cam.tr[2] as f32 / s,
        ],
    }
}

/// The sky strip's vertex positions for a frame drawn under `view`, its
/// screen rects projected from `sky` - the globals the lead's tick saw
/// (`VenueCameraDriver::sky_camera`), whose stored focus and yaw words
/// scroll it.
pub fn venue_sky_positions(
    view: &legaia_engine_camera::psx_camera::FieldCameraView,
    sky: &legaia_engine_fishing::fishing_actors::VenueCamera,
) -> Vec<[f32; 3]> {
    crate::fishing_scene::sky_positions_split(
        view,
        &venue_camera_globals_view(sky),
        sky.focus[0],
        sky.rot[1],
    )
}

/// The fishing camera this frame, while a fishing session is up: the
/// venue's scripted camera ([`FishingVenue::camera`] - the lead's view at the
/// shore, the dolly over the water on the cast, the in-water view, the catch
/// close-up and showcase), the view every host draws the pond with and
/// projects the fishing line through. `None` before the venue has armed.
pub fn venue_view(mg: &MinigameState) -> Option<legaia_engine_camera::psx_camera::FieldCameraView> {
    mg.fishing.as_ref()?;
    mg.fishing_venue.wander.as_ref()?;
    Some(venue_camera_globals_view(mg.fishing_venue.camera.camera()))
}

/// Where the lead's actor spawns for `venue`, `(x, y, z)` world units - the
/// actor [`legaia_engine_fishing::fishing_actors::FishWander`] models (the lead's tick `FUN_801D2050` /
/// `FUN_801D2278`, whose actor pointer `DAT_801D928C` is the lead's).
pub fn lead_spawn(venue: usize) -> (i16, i16, i16) {
    let lead = party_placements(venue)[0];
    (lead.x, 0, lead.z)
}

/// The cast lure's anchor for `venue`: the lead's seat with his footing
/// `+0x16`, which the cast arm reads less `0x80` as the lure's height
/// (`sh` at `0x801CFCEC`). The footing is the float solver's
/// (`FUN_801D6028` through `FUN_801D70EC`) over the venue `.MAP` at the
/// seat - `-128` on the first shore, the step layer `minigame_fishing` holds
/// the three actors on, and `-64` on the second - and `0` without a map.
pub fn lead_anchor(venue: usize, map: Option<&[u8]>) -> (i16, i16, i16) {
    let (x, _, z) = lead_spawn(venue);
    let y = map.map_or(0, |buf| {
        let ramp = legaia_engine_field::minigame_floor::height_ramp();
        let grid = legaia_engine_field::minigame_floor::FloorGrid::new(buf);
        legaia_engine_fishing::fishing_chrome::float_actor_tick(grid, x, z, 0, &ramp).y
    });
    (x, y, z)
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
    /// The pond swell's phase (`0x801D9118`).
    swell_angle: i32,
    /// The live floor-height ladder (`0x1F80035C`), in the scratchpad frame
    /// the ground pass and the floor solver read it in: rungs `0..4` are this
    /// frame's [`fc::water_swell`], rungs `4..16` [`venue_floor_ladder`]'s.
    pub floor_ladder: [i16; 16],
    rng: u32,
    /// The two ambient-ripple part records off the fishing overlay
    /// ([`RippleRecords`]), when the host installed them.
    pub ripple_records: Option<RippleRecords>,
    /// The live ripple parts, run through the move VM as `FUN_80021B04`
    /// seats them and `FUN_80021DF4` ticks them.
    pub ripple_fx: Option<legaia_engine_effects::summon::SummonScene>,
    /// The scripted camera ([`legaia_engine_fishing::fishing_actors::VenueCameraDriver`]),
    /// stepped once a venue frame.
    pub camera: legaia_engine_fishing::fishing_actors::VenueCameraDriver,
    /// The catch's white-out: the two screen fades the line actor's
    /// celebration spawns ([`legaia_engine_fishing::fishing_actors::CATCH_FLASH_FADES`]),
    /// stepped once a venue frame until each runs out. Every host draws them
    /// through [`Self::catch_flash_prims`].
    pub catch_flash: Vec<legaia_engine_system::fade::FadeState>,
    /// Close-up frames run, the venue's stand-in for the word
    /// `_DAT_80083838` whose bit `0` sizes the trail's spark (inference: a
    /// frame counter's parity).
    catch_frames: u32,
}

/// VA of the ripple record the lead's roll spawns on `rand() & 1 != 0`
/// (`lui a2,0x801e` / `addiu a2,a2,-0x7128` at `0x801D23E4..0x801D23EC`).
pub const RIPPLE_RECORD_A_VA: u32 = 0x801D_8ED8;
/// VA of the ripple record the roll spawns on `rand() & 1 == 0`
/// (`addiu a2,a2,-0x70bc` at `0x801D23F8`).
pub const RIPPLE_RECORD_B_VA: u32 = 0x801D_8F44;
/// VA of the ring the lure seats where it lands and behind a reeled float
/// (`addiu a2,a2,-0x76D4` at `0x801D2934` and `0x801D39A0`).
pub const LANDING_RING_RECORD_VA: u32 = 0x801D_892C;

/// The catch close-up's eight sprite-arm parts (the halo, the lens flare
/// and its rings), seated on the close-up's first frame
/// (`0x801D4C40..0x801D4D3C`): each record with the height step its `y`
/// takes before the call. The first stands `(-0x3E8, -0x1388, -0x4B0)` off
/// the fish (whose height that frame is `0`, `sh zero,0x16` at `0x801D4BDC`);
/// every later one keeps `x` / `z` and climbs by its step.
pub const CATCH_SPRITE_RECORDS: [(u32, i16); 8] = [
    (0x801D_7E68, -0x1388),
    (0x801D_7EAC, -0x7D0),
    (0x801D_7EF0, -0x5DC),
    (0x801D_7F34, -0x4B0),
    (0x801D_7F78, -0x578),
    (0x801D_8044, -0x640),
    (0x801D_8000, -0x708),
    (0x801D_7FBC, -0x7D0),
];
/// The close-up's per-frame trail at the fish (`0x801D4E38..0x801D4F0C`):
/// a spark `0x64` above it on a random heading, a second part there that
/// is then thrown up to `0xFF` units aside on `x` and `z`, and a third where
/// the second landed.
pub const CATCH_TRAIL_RECORDS: [u32; 3] = [0x801D_8D30, 0x801D_8D9C, 0x801D_8E0C];
/// The camera-locked ring the celebration's first stage bursts three at a
/// time through `FUN_801D7A5C` (`0x801D4F34..0x801D50C4`): one ring per
/// colour word, offset by the spread - the rainbow.
pub const CATCH_RING_RECORD: u32 = 0x801D_8CDC;

/// The fishing overlay's move-VM part records the venue seats: the lure's
/// ring, the ambient ripple's two records and the catch close-up's parts,
/// in the move-buffer format `[i16 model_sel][u16 reserved][bytecode]`.
/// The ripples and the catch's rings select library mesh `0`, but their
/// bytecode turns the part into a render-mode-4 default-arm node (move-VM op
/// `0x13`), whose geometry is the procedural ring `FUN_80028158` builds; the
/// catch's halo and flare parts are `0x4000` sprite-arm nodes (op `0x23`).
// REF: FUN_80021B04 (the seater), FUN_80028158 (the default arm's ring)
#[derive(Debug, Clone)]
pub struct RippleRecords {
    /// The records' bytes, the lowest record at `0`.
    bytes: Vec<u8>,
    /// The parsed records, lowest VA first.
    parts: Vec<legaia_overlay_images::summon_overlay::SummonPart>,
    /// Each part's VA, aligned with [`Self::parts`].
    vas: Vec<u32>,
}

impl RippleRecords {
    /// Bytes kept past record B's start - its bytecode ends at op `0x08`
    /// well inside this.
    const TAIL: usize = 0x100;

    /// Every record VA the venue seats, ascending.
    fn record_vas() -> Vec<u32> {
        let mut v: Vec<u32> = CATCH_SPRITE_RECORDS.iter().map(|r| r.0).collect();
        v.extend(CATCH_TRAIL_RECORDS);
        v.extend([
            CATCH_RING_RECORD,
            LANDING_RING_RECORD_VA,
            RIPPLE_RECORD_A_VA,
            RIPPLE_RECORD_B_VA,
        ]);
        v.sort_unstable();
        v
    }

    /// Cut the records out of the fishing overlay's as-loaded image (PROT
    /// 0972 at `base_va`). `None` when the image does not reach them.
    pub fn from_overlay(loaded: &[u8], base_va: u32) -> Option<Self> {
        let vas = Self::record_vas();
        let lo = vas[0];
        let first = lo.checked_sub(base_va)? as usize;
        let b = (RIPPLE_RECORD_B_VA - lo) as usize;
        let bytes = loaded
            .get(first..(first + b + Self::TAIL).min(loaded.len()))?
            .to_vec();
        if bytes.len() <= b + 4 {
            return None;
        }
        let offs: Vec<usize> = vas.iter().map(|&v| (v - lo) as usize).collect();
        let parts = legaia_overlay_images::summon_overlay::parse_records_at(&bytes, &offs);
        (parts.len() == vas.len()).then_some(Self { bytes, parts, vas })
    }

    /// The part at record `va`.
    fn part(&self, va: u32) -> Option<&legaia_overlay_images::summon_overlay::SummonPart> {
        self.vas
            .iter()
            .position(|&v| v == va)
            .and_then(|i| self.parts.get(i))
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
            swell_angle: 0,
            floor_ladder: venue_floor_ladder(),
            rng: VENUE_RNG_SEED,
            ripple_records: None,
            ripple_fx: None,
            camera: Default::default(),
            catch_flash: Vec::new(),
            catch_frames: 0,
        }
    }
}

impl FishingVenue {
    /// The catch white-out's full-screen quads this frame: one additive
    /// quad per live fade past its start delay, at the fade's ordering-table
    /// bucket (`FUN_80025000` -> `FUN_80024EE4(id, kind, rgb)`). Hosts draw
    /// them after the HUD sprites and before the font text.
    // REF: FUN_80025000
    pub fn catch_flash_prims(&self) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        self.catch_flash
            .iter()
            .filter(|f| f.visible())
            .map(|f| {
                legaia_render_kernels::screen_prim::screen_fade_prim(f.rgb(), f.abr(), f.ot_layer())
            })
            .collect()
    }

    /// Whether the venue has been armed (its wander actor exists).
    pub fn armed(&self) -> bool {
        self.wander.is_some()
    }

    /// Spawn ripple record `variant` (`1` = A, `0` = B) at world `pos` with
    /// the roll's rotation trio - one `FUN_80021B04(sp+0x10, sp+0x18,
    /// record, 0x1000)` call (`0x801D23FC`), which seats the part and runs
    /// its first move-VM slice.
    fn spawn_ripple(&mut self, variant: u32, pos: [i16; 3], rot: [i16; 3]) {
        self.seat_ring(
            if variant != 0 {
                RIPPLE_RECORD_A_VA
            } else {
                RIPPLE_RECORD_B_VA
            },
            pos,
            rot,
        );
    }

    /// Seat the part at record `va` as `FUN_80021B04` does, and hand back
    /// its runtime for the caller's after-seat stores.
    fn seat_ring(
        &mut self,
        va: u32,
        pos: [i16; 3],
        rot: [i16; 3],
    ) -> Option<&mut legaia_engine_effects::summon::SummonPartRuntime> {
        let rec = self.ripple_records.as_ref()?;
        let part = rec.part(va)?;
        let scene = self.ripple_fx.get_or_insert_with(|| {
            legaia_engine_effects::summon::SummonScene::spawn_parts(&[], &rec.bytes, 0, pos)
        });
        scene.push_parts(std::slice::from_ref(part), &rec.bytes, pos, rot);
        // The seater's own first move-VM run (`FUN_80021B04` calls
        // `FUN_80023070` before it returns).
        scene.seat_run(&mut RippleHost);
        scene.parts.last_mut()
    }

    /// The close-up's first frame: the eight sprite parts over the fish at
    /// world `(x, z)` ([`CATCH_SPRITE_RECORDS`]).
    ///
    /// PORT: FUN_801d4948 (sub-state 4's first frame, `0x801D4BF4..0x801D4D40`)
    fn seat_catch_sprites(&mut self, x: i16, z: i16) {
        let (x, z) = (x.wrapping_sub(0x3E8), z.wrapping_sub(0x4B0));
        let mut y = 0i16;
        for &(va, step) in &CATCH_SPRITE_RECORDS {
            y = y.wrapping_add(step);
            self.seat_ring(va, [x, y, z], [0; 3]);
        }
    }

    /// One close-up frame's trail at the fish's `pos` ([`CATCH_TRAIL_RECORDS`]):
    /// three `rand()` draws, the venue's stream standing in for the BIOS one.
    /// `odd` is bit `0` of the word `_DAT_80083838` the spark's size reads.
    ///
    /// PORT: FUN_801d4948 (sub-state 4's trail, `0x801D4E38..0x801D4F0C`)
    fn seat_catch_trail(&mut self, pos: [i16; 3], odd: bool) {
        let mut rand = || {
            let mut x = self.rng;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.rng = x;
            (x >> 16 & 0x7FFF) as u16
        };
        let r = [rand(), rand(), rand()];
        let at = [pos[0], pos[1].wrapping_sub(0x64), pos[2]];
        let size = if odd { 0x800 } else { 0x400 };
        if let Some(p) = self.seat_ring(CATCH_TRAIL_RECORDS[0], at, [0; 3]) {
            p.poke(0xBC, size);
            p.poke(0xBE, size);
            p.poke(0x26, r[0] & 0xFFF);
        }
        let Some(p) = self.seat_ring(CATCH_TRAIL_RECORDS[1], at, [0; 3]) else {
            return;
        };
        let s = &mut p.state;
        s.world_x = s.world_x.wrapping_add((r[1] & 0x1FF) as i16 - 0xFF);
        s.world_z = s.world_z.wrapping_add((r[2] & 0x1FF) as i16 - 0xFF);
        let (thrown, rot) = (
            [s.world_x, s.world_y, s.world_z],
            [s.render_24, s.render_26, s.render_28],
        );
        self.seat_ring(CATCH_TRAIL_RECORDS[2], thrown, rot);
    }

    /// One celebration tier's two ring bursts (`FUN_801D7A5C` with the forms
    /// `0x1028` and `0x1037`) at the eye-space point `at`: three
    /// camera-locked rings each, coloured and fanned by
    /// [`legaia_engine_fishing::fishing_chrome::splash_burst`]'s words.
    ///
    /// REF: FUN_801d7a5c (the part stores after each seat; the words are `splash_burst`'s)
    fn seat_catch_rings(&mut self, at: [i16; 3]) {
        for packed in [0x1028, 0x1037] {
            for b in legaia_engine_fishing::fishing_chrome::splash_burst(0, 0, 0, packed) {
                let Some(p) = self.seat_ring(CATCH_RING_RECORD, at, [0; 3]) else {
                    return;
                };
                let s = &mut p.state;
                s.set_actor_u32(0xA0, b.rot.0);
                s.set_actor_u32(0xA4, b.rot.1);
                if b.sub_block {
                    for (off, d) in [(0xB4, b.nudge.0), (0xB6, b.nudge.1)] {
                        let v = s.actor_u16(off).wrapping_add(d as u16);
                        s.set_actor_u16(off, v);
                    }
                } else {
                    s.world_x = s.world_x.wrapping_add(b.nudge.0 as i16);
                    s.world_y = s.world_y.wrapping_add(b.nudge.1 as i16);
                }
            }
        }
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
    /// venue's scripted camera ([`venue_camera_globals_view`],
    /// [`project_to_retail_screen`]). For a host that draws on a 2D canvas
    /// rather than through an effect-mesh pass - the minigames page. A
    /// triangle with a corner behind the eye is dropped.
    pub fn ripple_screen_tris(&self) -> Vec<RippleTri> {
        if self.wander.is_none() {
            return Vec::new();
        }
        let view = venue_camera_globals_view(self.camera.camera());
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
    // REF: FUN_8001CF50 (the camera-relative arm a part's `+0x52` selects)
    pub fn ripple_world_mesh(&self) -> legaia_tmd::mesh::VramMesh {
        let mut out = legaia_tmd::mesh::VramMesh {
            positions: Vec::new(),
            uvs: Vec::new(),
            cba_tsb: Vec::new(),
            indices: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
        };
        // A part whose `+0x52` routes it through retail's camera-relative
        // arm (`FUN_8001CF50`: the catch's camera-locked rings, its
        // camera-facing halo) is placed by the venue camera's pose.
        let pose = self.wander.as_ref().map(|_| {
            legaia_render_kernels::gte::PartCameraPose::from_field_view(&venue_camera_globals_view(
                self.camera.camera(),
            ))
        });
        for d in self.ripple_draws() {
            let r = legaia_engine_camera::psx_camera::camera_rotation(d.rot[0], d.rot[1], d.rot[2]);
            let prefix = legaia_render_kernels::gte::camera_relative_model_prefix(
                d.flags_52,
                d.world_pos,
                pose.as_ref(),
                false,
            );
            let place = |v: [f32; 3]| -> [f32; 3] {
                let mut p = [0f32; 3];
                for (row, o) in p.iter_mut().enumerate() {
                    *o = (0..3).map(|c| r[c * 4 + row] * v[c]).sum::<f32>();
                }
                let q = match &prefix {
                    Some(m) => std::array::from_fn(|row| {
                        m[12 + row] + (0..3).map(|c| m[c * 4 + row] * p[c]).sum::<f32>()
                    }),
                    None => std::array::from_fn(|row| d.world_pos[row] + p[row]),
                };
                q.map(f32::round)
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

    /// [`Self::ripple_world_mesh`] rasterised onto a `w x h` RGBA8 stage
    /// over black, textured out of `vram` (`1024 x 512` BGR555 words): the
    /// parts as one additive layer, for a host that composites it over its
    /// own frame with an add (the minigames page's 2D canvas). A
    /// semi-transparent `B + F` texel adds as retail adds it; the half rate
    /// (`ABR 0`) adds half its texel, exact over black and an approximation
    /// over the pond. Empty when no part draws.
    pub fn ripple_rgba(&self, vram: &[u16], w: usize, h: usize) -> Vec<u8> {
        let m = self.ripple_world_mesh();
        if m.indices.is_empty() || self.wander.is_none() || vram.len() < 1024 * 512 {
            return Vec::new();
        }
        let view = venue_camera_globals_view(self.camera.camera());
        let (sx, sy) = (w as f32 / 320.0, h as f32 / 240.0);
        let px = |x: usize, y: usize| vram[(y & 511) * 1024 + (x & 1023)];
        let texel = |cba: u16, tsb: u16, u: u8, v: u8| -> u16 {
            let (tx, ty) = (
                usize::from(tsb & 0xF) * 64,
                usize::from((tsb >> 4) & 1) * 256,
            );
            let (cx, cy) = (
                usize::from(cba & 0x3F) * 16,
                usize::from((cba >> 6) & 0x1FF),
            );
            let (u, v) = (usize::from(u), usize::from(v));
            match (tsb >> 7) & 3 {
                0 => px(
                    cx + usize::from((px(tx + u / 4, ty + v) >> ((u & 3) * 4)) & 0xF),
                    cy,
                ),
                1 => px(
                    cx + usize::from((px(tx + u / 2, ty + v) >> ((u & 1) * 8)) & 0xFF),
                    cy,
                ),
                _ => px(tx + u, ty + v),
            }
        };
        let mut acc = vec![[0f32; 3]; w * h];
        let mut hit = vec![false; w * h];
        for tri in m.indices.as_chunks::<3>().0 {
            let idx = tri.map(|t| t as usize);
            let Some(p) = idx
                .iter()
                .map(|&k| {
                    m.positions.get(k).and_then(|v| {
                        project_to_retail_screen(&view, v.map(|c| c.round() as i32))
                            .map(|(x, y)| (f32::from(x) * sx, f32::from(y) * sy))
                    })
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let area =
                (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
            if area.abs() < 1e-3 {
                continue;
            }
            let ct = m.cba_tsb[idx[0]];
            let semi = ct[1] & legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT != 0;
            let rate = match (ct[1] >> 5) & 3 {
                0 => 0.5,
                1 => 1.0,
                2 => 0.0,
                _ => 0.25,
            };
            let lo = |f: fn(f32, f32) -> f32, k: usize, init: f32| {
                p.iter()
                    .map(|q| if k == 0 { q.0 } else { q.1 })
                    .fold(init, f)
            };
            let x0 = lo(f32::min, 0, f32::MAX).floor().max(0.0) as usize;
            let x1 = lo(f32::max, 0, f32::MIN).ceil().min(w as f32 - 1.0);
            let y0 = lo(f32::min, 1, f32::MAX).floor().max(0.0) as usize;
            let y1 = lo(f32::max, 1, f32::MIN).ceil().min(h as f32 - 1.0);
            if x1 < 0.0 || y1 < 0.0 {
                continue;
            }
            for y in y0..=y1 as usize {
                for x in x0..=x1 as usize {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let b0 = ((p[1].0 - fx) * (p[2].1 - fy) - (p[2].0 - fx) * (p[1].1 - fy)) / area;
                    let b1 = ((p[2].0 - fx) * (p[0].1 - fy) - (p[0].0 - fx) * (p[2].1 - fy)) / area;
                    let b2 = 1.0 - b0 - b1;
                    if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                        continue;
                    }
                    let lerp = |a: f32, b: f32, c: f32| a * b0 + b * b1 + c * b2;
                    let uv = |c: usize| {
                        lerp(
                            f32::from(m.uvs[idx[0]][c]),
                            f32::from(m.uvs[idx[1]][c]),
                            f32::from(m.uvs[idx[2]][c]),
                        ) as u8
                    };
                    let t = texel(ct[0] & 0x7FFF, ct[1], uv(0), uv(1));
                    if t == 0 {
                        continue;
                    }
                    let k = if semi && t & 0x8000 != 0 { rate } else { 1.0 };
                    let o = &mut acc[y * w + x];
                    for (c, ch) in o.iter_mut().enumerate() {
                        let tc = f32::from((t >> (5 * c)) & 31) * 8.0;
                        let col = lerp(
                            f32::from(m.colors[idx[0]][c]),
                            f32::from(m.colors[idx[1]][c]),
                            f32::from(m.colors[idx[2]][c]),
                        );
                        *ch += k * tc * col / 128.0;
                    }
                    hit[y * w + x] = true;
                }
            }
        }
        if !hit.iter().any(|&b| b) {
            return Vec::new();
        }
        acc.iter()
            .zip(&hit)
            .flat_map(|(c, &on)| {
                let [r, g, b] = c.map(|v| v.clamp(0.0, 255.0) as u8);
                [r, g, b, if on { 255 } else { 0 }]
            })
            .collect()
    }

    /// This frame's ripple geometry: each live part's default-arm ring
    /// ([`legaia_engine_effects::effect_default_arm::default_arm_draw`]) at its
    /// world position and rotation banks, in raw retail world units - the
    /// list every host draws through its effect-mesh pass.
    pub fn ripple_draws(&self) -> Vec<legaia_engine_effects::effect_ribbon::RibbonDraw> {
        self.ripple_fx
            .as_ref()
            .map(|s| {
                let mut d = s.sprite_arm_draws();
                d.extend(s.default_arm_draws());
                d
            })
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
    /// The scripted camera's globals after the frame, every frame the venue
    /// is armed; written last, so they are what the host's camera holds.
    pub camera: Option<legaia_engine_fishing::fishing_actors::VenueCamera>,
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
        if let Some(c) = self.camera {
            g[0] = c.rot[0] as i32;
            g[1] = c.rot[1] as i32;
            g[2] = c.rot[2] as i32;
            g[3..6].copy_from_slice(&c.tr);
            g[6..9].copy_from_slice(&c.focus);
        }
    }
}

/// The fishing scene's floor-height ladder outside the rungs the swell owns:
/// `-(i * 0x20)`, the scratchpad ladder a retail fishing frame holds at rungs
/// `4..16` (`minigame_fishing_retail_ready`: `-128, -160, .. -480`), with
/// the swell's head at angle `0` over rungs `0..4` until the first venue frame
/// runs it. The `other1` ground only references tiers `0..=3`, so the upper
/// rungs matter only to a solve off the pond's own cells.
pub fn venue_floor_ladder() -> [i16; 16] {
    let mut ladder = [0i16; 16];
    for (i, r) in ladder.iter_mut().enumerate() {
        *r = -(i as i16) * legaia_engine_field::minigame_floor::HEIGHT_RAMP_STEP;
    }
    fc::water_swell(swell_sine_table(), 0, 0)
        .0
        .apply(&mut ladder);
    ladder
}

/// The 4096-step sine table the pond swell samples. Retail reads the
/// shared table through `*_DAT_8007B81C`, which points at the SCUS sine
/// table `0x80070A2C` (runtime data the port does not stage); this is that
/// table, entry for entry: `sin_4096` truncates toward zero as the disc
/// table does, where a rounded sine misses it by one on a fraction of the
/// entries and lifts those swell rungs by one.
pub fn swell_sine_table() -> &'static [i16] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<i16>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..fc::SINE_TURN)
            .map(|i| legaia_minigame_formats::minigame_slot_scene::sin_4096(i) as i16)
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
/// that raised them) and seats the retarget ripples and the catch's parts in
/// [`FishingVenue::ripple_fx`], which every host draws.
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
        held_packed,
        floor,
    )
}

/// One venue frame over the pieces it touches, for a host that holds a
/// [`PondSession`](legaia_engine_fishing::fishing::PondSession) and an effect
/// pool but no [`MinigameState`] - the minigames page. [`tick_fishing_venue`]
/// is this over the world's own fields, so the two run one step.
///
/// `events` is this tick's session events and `exchange_open` whether the
/// point-exchange list is up. The retarget ripples and the catch's parts
/// are the overlay's own move-VM records, seated in
/// [`FishingVenue::ripple_fx`].
pub fn tick_venue_actors(
    v: &mut FishingVenue,
    session: &legaia_engine_fishing::fishing::PondSession,
    events: &[PondEvent],
    held_packed: u16,
    floor: impl FnOnce() -> Option<Vec<u8>>,
) -> VenueCameraWrites {
    let mut out = VenueCameraWrites::default();
    let phase = session.phase();
    // The pond swell, from the driver's shared tail (`jal 0x801D03B0` at
    // `0x801D037C`): every running frame, whatever the state or sub-screen.
    // One venue frame is one vsync, so the step is `1` - the angle keeps
    // retail's `0x10` per vsync (`DAT_1F800393 << 4` once per game frame).
    let (swell, next) = fc::water_swell(swell_sine_table(), v.swell_angle, 1);
    v.swell_angle = next;
    swell.apply(&mut v.floor_ladder);
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
    // The wander runs while the cast is idle: the lead's tick `FUN_801D2050`
    // calls it only while his `+0x10` bit `0x80000` is clear
    // (`0x801D20DC..0x801D20F0`), and the cast press sets that bit until the
    // next state `10`. The D-pad steers the lead.
    let mut ripple = None;
    if phase == PondPhase::Idle {
        let mut rng = v.rng;
        let rolled = v.wander.as_mut().and_then(|w| {
            // One venue frame is one vsync, so the dwell steps by one; the
            // draws keep `rand()`'s fifteen bits, which the dwell's signed
            // `% 200` needs to stay non-negative.
            w.tick(held_packed, 1, || {
                let mut x = rng;
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                rng = x;
                x >> 16 & 0x7FFF
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
        let grid = legaia_engine_field::minigame_floor::FloorGrid::new(buf);
        let t = fc::float_actor_tick(grid, w.x, w.z, 0, &v.floor_ladder);
        w.y = t.y;
    }
    if let Some(w) = v.wander.as_ref() {
        out.publish = Some(w.camera());
    }
    // The line actor: armed on the hook event, landed on the catch event,
    // dropped on a snap.
    for e in events {
        match *e {
            PondEvent::Hooked(_) => {
                v.line = Some(legaia_engine_fishing::fishing_actors::LineActorSim::hooked())
            }
            // The celebration's tiers climb the fight strength
            // `DAT_801D91B8` (`0x801D4F34`), not the points it scored.
            PondEvent::Landed(_) => match v.line.as_mut() {
                Some(line) => line.land(session.strength()),
                None => v.line = None,
            },
            PondEvent::Snapped => v.line = None,
            // The ring is seated on the water plane: `sh zero,0x16(a0)` at
            // `0x801D39B8` on the in-water arm, the trio zeroed on landing.
            PondEvent::LureRing(x, z) => {
                v.seat_ring(LANDING_RING_RECORD_VA, [x, 0, z], [0, 0, 0]);
            }
            PondEvent::Splash | PondEvent::Recast => {}
        }
    }
    // The scripted camera: the retail write site this phase runs, over the
    // lead's seat and aim, the line record and the line actor's close-up
    // timer as this frame starts (the close-up ends on the frame that timer
    // is past its last stage).
    let close_up = v.line.as_ref().and_then(|line| {
        matches!(
            legaia_engine_fishing::fishing_actors::LinePhase::from_raw(line.raw),
            legaia_engine_fishing::fishing_actors::LinePhase::Celebrate
        )
        .then_some(line.timer)
    });
    if let Some(w) = v.wander.as_ref() {
        let lure_drift = match (session.lure_actor(), session.venue_map()) {
            (Some(l), Some(_)) => i32::from(l.x()) - i32::from(l.spawn.0),
            _ => 0,
        };
        let last = legaia_engine_fishing::fishing_actors::CELEBRATION_STAGE_FRAMES.2;
        v.camera
            .frame(legaia_engine_fishing::fishing_actors::VenueCameraFrame {
                phase,
                lead: (w.x, w.y, w.z, w.facing),
                record: session.line_record(),
                lure_height: session.lure_actor().map(|l| l.y_fixed),
                lure_drift,
                venue: session.venue,
                close_up,
                close_up_done: close_up.is_some_and(|t| t > last),
                step: 1,
            });
        out.camera = Some(*v.camera.camera());
    }
    // The ripple parts tick first (the actor list runs the parts before the
    // lead's tick seats a new one), then the roll's spawn runs its first
    // slice.
    // The white-out's fade actors tick ahead of this frame's spawn
    // (`FUN_80025000` -> `FUN_80020C14`), each dropped once its ramp and
    // hold have run.
    v.catch_flash.retain_mut(|f| f.step());
    v.tick_ripples();
    if let Some((variant, pos, rot)) = ripple {
        v.spawn_ripple(variant, pos, rot);
    }
    if let Some(mut line) = v.line.take() {
        let start = line.timer;
        let celebrating = matches!(
            legaia_engine_fishing::fishing_actors::LinePhase::from_raw(line.raw),
            legaia_engine_fishing::fishing_actors::LinePhase::Celebrate
        );
        let f = line.tick(1);
        // The close-up's parts ride the line actor, which sits on the lure
        // and carries the lift's height - not the free-swimming fish the
        // venue also draws. The bursts' *cues* are not this actor's: the
        // hook cue and the celebration tiers are queued by
        // `World::tick_fishing` off the session's own events, where every
        // host drains them.
        if celebrating && let Some((x, y, z)) = v.camera.lift_point() {
            if start == 0 {
                v.seat_catch_sprites(x, z);
            }
            v.catch_frames = v.catch_frames.wrapping_add(1);
            v.seat_catch_trail([x, y, z], v.catch_frames & 1 != 0);
        }
        for b in &f.bursts {
            v.seat_catch_rings([b.offset.0, b.offset.1, b.offset.2]);
        }
        if f.flash {
            for t in &legaia_engine_fishing::fishing_actors::CATCH_FLASH_FADES {
                v.catch_flash
                    .push(legaia_engine_system::fade::FadeState::load(t));
            }
        }
        if !f.done {
            v.line = Some(line);
        }
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
///
/// `venue_camera` says `view` is the venue's own retail camera
/// ([`venue_view`]); only then does the tail's off-screen test run
/// (`PondSession::line_frame_retail`), since a fallback framing would put
/// the cast's spawn point off its screen for no reason retail has.
pub fn fishing_line_frame(
    mg: &mut MinigameState,
    view: Option<&legaia_engine_camera::psx_camera::FieldCameraView>,
    venue_camera: bool,
) -> Option<legaia_engine_fishing::fishing_actors::FishingLine> {
    let view = view?;
    let pond = mg.fishing.as_mut()?;
    let project = |p| project_to_retail_screen(view, p);
    if venue_camera {
        pond.line_frame_retail(project)
    } else {
        pond.line_frame(project)
    }
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
        for va in [
            LANDING_RING_RECORD_VA,
            RIPPLE_RECORD_A_VA,
            RIPPLE_RECORD_B_VA,
        ] {
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
    fn the_hook_arms_the_line_and_the_catch_runs_it_out() {
        let mut mg = MinigameState::new();
        mg.fishing = Some(session());
        tick_fishing_venue(&mut mg, true, 0, || None);
        mg.fishing_events = vec![PondEvent::Hooked(3)];
        tick_fishing_venue(&mut mg, true, 0, || None);
        assert!(mg.fishing_venue.line.is_some());
        mg.fishing_events = vec![PondEvent::Landed(9000)];
        let before = mg.fx.len();
        let mut flashed = false;
        for _ in 0..600 {
            tick_fishing_venue(&mut mg, true, 0, || None);
            mg.fishing_events.clear();
            flashed |= !mg.fishing_venue.catch_flash.is_empty();
            if mg.fishing_venue.line.is_none() {
                break;
            }
        }
        assert!(mg.fishing_venue.line.is_none(), "the close-up hands back");
        assert!(flashed, "the close-up whites out");
        // The celebration's rings are the overlay's own move-VM parts
        // (`FishingVenue::ripple_fx`), not effect-pool sprites.
        assert_eq!(mg.fx.len(), before);
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
            camera: None,
        };
        let before_y = cam.globals.0[4];
        w.apply(&mut cam);
        assert_eq!(cam.globals.0[3], reset.tr_x);
        assert_eq!(cam.globals.0[5], reset.tr_z);
        assert_eq!(cam.globals.0[4], before_y, "the reset leaves TR.y alone");
    }

    /// The pond's camera is retail's scripted one: the shore view while
    /// idle, then the cast press takes it from the lead and the wind-up
    /// dollies it out over the water - the view every host draws with.
    #[test]
    fn the_cast_takes_the_camera_out_over_the_water() {
        use legaia_engine_fishing::fishing::{PondInput, WINDUP_FRAMES};
        use legaia_engine_fishing::fishing_actors as fa;
        let mut mg = MinigameState::new();
        mg.fishing = Some(session());
        tick_fishing_venue(&mut mg, true, 0, || None);
        let shore = venue_view(&mg).unwrap();
        assert_eq!(shore, venue_camera_view(4736, 0, 10752, 0x800));
        let press = PondInput {
            cast_edge: true,
            ..Default::default()
        };
        mg.fishing.as_mut().unwrap().tick(press, 1, 0x80);
        let w = tick_fishing_venue(&mut mg, true, 0, || None);
        assert!(!mg.fishing_venue.camera.camera().lead_drives);
        assert_eq!(
            w.camera.unwrap().tr[2],
            0x974,
            "the press frame only takes it"
        );
        for _ in 0..WINDUP_FRAMES {
            mg.fishing
                .as_mut()
                .unwrap()
                .tick(PondInput::default(), 1, 0x80);
            tick_fishing_venue(&mut mg, true, 0, || None);
        }
        let cam = *mg.fishing_venue.camera.camera();
        assert_eq!(cam.tr, [0, fa::WIND_UP_TR_Y_FLOOR, fa::WIND_UP_TR_Z_FLOOR]);
        assert_eq!(cam.rot[0], fa::WIND_UP_PITCH_CAP);
        assert_eq!(cam.focus, [-4736, 0, -10752 - 13 * fa::WIND_UP_FOCUS_STEP]);
        let cast = venue_view(&mg).unwrap();
        assert_eq!(cast, venue_camera_globals_view(&cam));
        // The lead now sits behind the eye: his seat no longer projects.
        assert!(project_to_retail_screen(&shore, [4736, -400, 10752]).is_some());
        assert!(project_to_retail_screen(&cast, [4736, -400, 10752]).is_none());
        // The host's engine camera takes the same globals.
        let mut engine_cam = legaia_engine_field::camera::Camera::default();
        VenueCameraWrites {
            camera: Some(cam),
            ..Default::default()
        }
        .apply(&mut engine_cam);
        assert_eq!(engine_cam.globals.0[5], fa::WIND_UP_TR_Z_FLOOR);
        assert_eq!(engine_cam.globals.0[8], cam.focus[2]);
    }
}
