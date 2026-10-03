//! The fishing **venue actors**' per-frame step, shared by every host that
//! runs the fishing minigame over a world.
//!
//! The session ([`crate::fishing::PondSession`]) is the minigame's rules: the
//! cast, the band check, the fight, the lure's own walk-grid drift. Around it
//! the overlay runs a handful of venue actors that are presentation rather
//! than rules, and this module is their frame:
//!
//! - the free-swimming fish wander (`FUN_801D2278`,
//!   [`crate::fishing_actors::FishWander`]) at the shore while the cast is
//!   idle, steered by the held D-pad, with its **retarget ripple**
//!   ([`crate::fishing_chrome::ripple_spawn`]) on every new wander target;
//! - the wander actor settled onto the venue floor through the shared ground
//!   solver ([`crate::fishing_chrome::float_actor_tick`]) and its retail
//!   camera publish;
//! - the reeling-line actor (`FUN_801D4948`,
//!   [`crate::fishing_actors::LineActorSim`]) across hook, fight and the
//!   catch celebration, whose **bursts** ride the session's lure;
//! - the point-exchange sub-screen's idle sway
//!   ([`crate::fishing_chrome::sway_vector`]).
//!
//! It used to live inside the native window's `tick_fishing_actors`, which
//! made the ripples, the bursts, the venue camera and the swaying prize panel
//! native-only. The step is host-agnostic - every input is world state, the
//! held pad or the scene's `.MAP` bytes - so it lives here and each host calls
//! [`tick_fishing_venue`] from its own minigame frame, then applies the
//! returned camera writes to its own [`crate::camera::Camera`].

use crate::fishing::{PondEvent, PondPhase};
use crate::fishing_actors as fa;
use crate::fishing_chrome as fc;
use crate::world::MinigameState;

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
/// tick's publish ([`fa::fish_camera`]: yaw off the facing, `TR.y = 0x400 -
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
) -> legaia_engine_vm::psx_camera::FieldCameraView {
    let reset = fc::venue_camera_reset();
    let publish = fa::fish_camera(x, y, z, facing);
    let s = crate::camera_view::CUTSCENE_WORLD_SCALE;
    let to_rad = |u: i32| u as f32 * (std::f32::consts::TAU / 4096.0);
    legaia_engine_vm::psx_camera::FieldCameraView {
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
pub fn venue_view(mg: &MinigameState) -> Option<legaia_engine_vm::psx_camera::FieldCameraView> {
    mg.fishing.as_ref()?;
    let w = mg.fishing_venue.wander.as_ref()?;
    Some(venue_camera_view(w.x, w.y, w.z, w.facing))
}

/// Where the lead's actor spawns for `venue`, `(x, y, z)` world units - the
/// actor [`fa::FishWander`] models (the lead's tick `FUN_801D2050` /
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
    pub wander: Option<fa::FishWander>,
    /// The reeling-line actor, armed on the hook event.
    pub line: Option<fa::LineActorSim>,
    /// The venue scene's `.MAP` extended footprint (the engine's
    /// `_DAT_1F8003EC` floor buffer), loaded once at arm.
    floor: Option<Vec<u8>>,
    /// The sub-screen sway phase (`0x801D9118`).
    sway_angle: i32,
    /// This frame's sway offset, applied to the point-exchange panel.
    pub sway_offset: (i16, i16),
    rng: u32,
}

impl Default for FishingVenue {
    fn default() -> Self {
        Self {
            wander: None,
            line: None,
            floor: None,
            sway_angle: 0,
            sway_offset: (0, 0),
            rng: VENUE_RNG_SEED,
        }
    }
}

impl FishingVenue {
    /// Whether the venue has been armed (its wander actor exists).
    pub fn armed(&self) -> bool {
        self.wander.is_some()
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
    pub publish: Option<fa::FishCamera>,
}

impl VenueCameraWrites {
    /// Write these into the engine camera's retail globals.
    pub fn apply(&self, camera: &mut crate::camera::Camera) {
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
/// `in_fishing` is whether the world is in [`crate::world::SceneMode::Fishing`];
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
    let mut out = VenueCameraWrites::default();
    let v = &mut mg.fishing_venue;
    if !in_fishing {
        *v = FishingVenue::default();
        return out;
    }
    let Some(session) = mg.fishing.as_ref() else {
        return out;
    };
    let phase = session.phase();
    // One-time venue arm: the wander actor, the floor buffer, and the venue
    // camera reset.
    if v.wander.is_none() {
        let (x, y, z) = lead_spawn(session.venue);
        v.wander = Some(fa::FishWander::new(x, y, z));
        v.floor = floor();
        out.reset = Some(fc::venue_camera_reset());
    }
    // The wander runs while the cast is idle (retail's shore states `0xc` /
    // `0xd` / `0x14`, before the lure flies); the D-pad steers the fish.
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
        // Retail spawns the ripple at the rolled point (the on-stack copy
        // the roll offsets, `FUN_80021B04(sp+0x10, ..)` at `0x801D23FC`);
        // the actor itself never moves.
        if let Some(t) = rolled
            && let Some(r) = fc::ripple_spawn(t.x as i16, t.z as i16, 0)
        {
            mg.fx.spawn_ripple(&r);
        }
    }
    // Settle the actor onto the venue floor (the `.MAP` height grid through
    // the shared ground solver) and publish its camera.
    if let (Some(w), Some(buf)) = (v.wander.as_mut(), v.floor.as_ref()) {
        let ramp = crate::minigame_floor::height_ramp();
        let grid = crate::minigame_floor::FloorGrid::new(buf);
        let t = fc::float_actor_tick(grid, w.x, w.z, 0, &ramp);
        w.y = t.y;
    }
    if let Some(w) = v.wander.as_ref() {
        out.publish = Some(w.camera());
    }
    // The line actor: armed on the hook event, landed on the catch event,
    // dropped on a snap.
    for e in &mg.fishing_events {
        match *e {
            PondEvent::Hooked(_) => v.line = Some(fa::LineActorSim::hooked()),
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
            mg.fx.spawn_burst(b, origin);
        }
        if !f.done {
            v.line = Some(line);
        }
    }
    // Sub-screen idle sway while the point-exchange list is up.
    if mg.fishing_exchange.is_some() {
        let (sv, next) = fc::sway_vector(sway_sine_table(), v.sway_angle, 1);
        v.sway_angle = next;
        v.sway_offset = (sv.x, sv.y);
    } else {
        v.sway_offset = (0, 0);
    }
    out
}

/// This frame's fishing line on a world host: [`crate::fishing::PondSession::line_frame`]
/// with the fish end projected through `view`, the scene camera the host
/// resolved for the frame it draws (`camera_view::resolve_field_camera(..)
/// .field_view()`), as retail's lure tick projects it through the scene
/// view the field matrix build leaves in the GTE.
///
/// The native window and the browser play page both call this and wrap the
/// result with the one builder `legaia_engine_ui::ui_fishing_line`. `None`
/// with no session, no camera or no line out.
pub fn fishing_line_frame(
    mg: &mut MinigameState,
    view: Option<&legaia_engine_vm::psx_camera::FieldCameraView>,
) -> Option<fa::FishingLine> {
    let view = view?;
    mg.fishing
        .as_mut()?
        .line_frame(|p| project_to_retail_screen(view, p))
}

/// Project a raw retail-world point through a host camera pose into retail
/// 320x240 screen space - `RTPS`'s `OFX/OFY + H * xy / z`, saturated to the
/// `SXY` range, floored as the GTE's `>> 16` floors. `None` behind the eye.
pub fn project_to_retail_screen(
    view: &legaia_engine_vm::psx_camera::FieldCameraView,
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
        axis(e[0], fa::GTE_SCREEN_CENTRE.0),
        axis(e[1], fa::GTE_SCREEN_CENTRE.1),
    ))
}

/// The venue scene's `.MAP` extended footprint - the engine's
/// `_DAT_1F8003EC` floor buffer (tile records at `+0`, height/wall grid at
/// `+0x4000`, cell grid at `+0x8000`). `None` when the current scene carries
/// no field map.
pub fn venue_floor_bytes(host: &crate::scene::SceneHost) -> Option<Vec<u8>> {
    // The pond's own `.MAP` (the session's venue), as retail installs the
    // fishing scene's buffer behind `_DAT_1F8003EC`; the departure field's
    // only as a fallback for a session opened with no venue.
    if let Some(v) = host
        .world
        .minigames
        .fishing
        .as_ref()
        .and_then(|s| s.venue_map())
    {
        return Some(v.map.clone());
    }
    let scene = host.scene.as_ref()?;
    let idx = scene.field_map_index(&host.index)?;
    host.index.entry_bytes_extended(idx).ok()
}

/// [`tick_fishing_venue`] over a scene host: the mode test, the held pad
/// (the world's engine mask rotated into the packed retail layout) and the
/// venue floor all come off the host, so the native window and the browser
/// play page run the identical frame and differ only in which
/// [`crate::camera::Camera`] they apply the returned writes to.
pub fn tick_fishing_venue_on_host(host: &mut crate::scene::SceneHost) -> VenueCameraWrites {
    let in_fishing = host.world.mode == crate::world::SceneMode::Fishing;
    let held_packed = host.world.input.pad().rotate_right(8);
    let floor = if in_fishing && !host.world.minigames.fishing_venue.armed() {
        venue_floor_bytes(host)
    } else {
        None
    };
    tick_fishing_venue(&mut host.world.minigames, in_fishing, held_packed, || floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fishing::PondSession;

    fn session() -> PondSession {
        PondSession::new(
            Vec::new(),
            vec![[0u32; 8]; 8],
            Vec::new(),
            0,
            1,
            2,
            60,
            crate::fishing::FishingRecord::default(),
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
    fn the_wander_retargets_into_the_shared_pool() {
        // Held D-pad right steers the fish; the wander's retarget rolls spawn
        // ripples into the pool every host draws, not into a host's own.
        let mut mg = MinigameState::new();
        mg.fishing = Some(session());
        let held = crate::dev_menu::PACK_RIGHT;
        let mut spawned = false;
        for _ in 0..2000 {
            tick_fishing_venue(&mut mg, true, held, || None);
            if !mg.fx.is_empty() {
                spawned = true;
                break;
            }
        }
        assert!(spawned, "no retarget ripple reached the shared pool");
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
        let mut cam = crate::camera::Camera::default();
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
