//! A map's light beams (`[[beams]]`): soft shafts of light falling through
//! a gap, with slow dust drifting in them.
//!
//! A beam is an enhancement: the hosts draw it only with enhanced lighting
//! on, as additive camera-facing ribbons through the field screen-effect
//! pass (`World::expansion_beam_prims`), and with it off nothing of it
//! draws. Its whole look on a frame is a pure function of the definition
//! and the frame number ([`Beam::shaft`], [`Beam::mote`]) - no state, no
//! random stream - so both hosts draw the same beam and a replay never
//! sees one.
//!
//! ```toml
//! [[beams]]
//! tile = [30, 22]
//! height = 900
//! tilt = 12.0
//! angle = 200.0
//! width = [80, 150]
//! intensity = 60
//! motes = 10
//! ```

use serde::{Deserialize, Serialize};

use crate::map::{MapDef, TileGrid};
use crate::validate::Checker;

/// The longest beam, world units.
pub const BEAM_MAX_HEIGHT: u16 = 4096;
/// The widest beam end, world units.
pub const BEAM_MAX_WIDTH: u16 = 1024;
/// The steepest lean from vertical, degrees.
pub const BEAM_MAX_TILT: f32 = 60.0;
/// The widest sway either way, degrees.
pub const BEAM_MAX_SWAY: f32 = 15.0;
/// The shortest breath / sway cycle, frames.
pub const BEAM_MIN_PERIOD: u16 = 30;
/// The most dust motes one beam keeps.
pub const BEAM_MAX_MOTES: u16 = 64;

/// Which end of the shaft the light comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeamProfile {
    /// Light from above: it fades in below the top, holds, and softens a
    /// little toward the foot, where it pools on the floor.
    #[default]
    Fall,
    /// A glow from below: brightest at the foot, fading out upward.
    Rise,
}

/// One placed light beam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeamDef {
    /// Optional name, for messages.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// Map-local tile the shaft lands on.
    pub tile: [u16; 2],
    /// World units from the tile centre on its floor, `y` up. A negative
    /// `y` sinks the foot below the floor: into a pit, a glow far below.
    pub offset: [i16; 3],
    /// Length of the shaft from its foot up, world units.
    pub height: u16,
    /// Lean from vertical, degrees.
    pub tilt: f32,
    /// Heading the shaft's top leans toward, degrees: `0` is `+Z`, `90`
    /// is `+X` (the fog's convention).
    pub angle: f32,
    /// Width at the top and at the foot, world units.
    pub width: [u16; 2],
    /// The light's colour.
    pub color: [u8; 3],
    /// Peak additive brightness, `0..=255`: the shaft's centre line at its
    /// brightest scales `color` by `intensity / 255`.
    pub intensity: u8,
    /// Which end the light comes from.
    pub profile: BeamProfile,
    /// A soft lit pool on the floor at the foot (`fall` only).
    pub pool: bool,
    /// Depth of a slow brightness swell, `0..=1`.
    pub breathe: f32,
    /// Degrees the lean drifts either way.
    pub sway: f32,
    /// Frames per breath and sway cycle.
    pub period: u16,
    /// Dust motes drifting in the shaft.
    pub motes: u16,
    /// The motes' seed.
    pub seed: u64,
}

impl Default for BeamDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            tile: [0, 0],
            offset: [0, 0, 0],
            height: 900,
            tilt: 0.0,
            angle: 0.0,
            width: [80, 150],
            color: [255, 236, 190],
            intensity: 80,
            profile: BeamProfile::Fall,
            pool: true,
            breathe: 0.15,
            sway: 1.5,
            period: 480,
            motes: 10,
            seed: 3,
        }
    }
}

/// A beam's shape on one frame, in world units with `y` **up** and the
/// foot's `y` measured from the floor under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeamShaft {
    /// World `x` / `z` of the foot, and its height above the tile's floor.
    pub foot: [f32; 3],
    /// The top, relative to the foot (`y` up).
    pub axis: [f32; 3],
    /// Width at the top and at the foot.
    pub width: [f32; 2],
    /// Brightness factor this frame (the breath), around `1`.
    pub gain: f32,
}

/// One dust mote on one frame: where it sits along the shaft and across
/// it, and how bright it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeamMote {
    /// `0` at the top, `1` at the foot.
    pub along: f32,
    /// Across the shaft, `-0.5..=0.5` of its width there.
    pub across: f32,
    /// Toward / away from the eye, the same scale (the shaft is round).
    pub depth: f32,
    /// Brightness `0..=1`, the shaft's profile and the mote's fade.
    pub light: f32,
    /// Sprite size, world units.
    pub size: f32,
}

fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl BeamDef {
    /// The shaft on `frame`: its foot at `centre` (the tile's world centre)
    /// plus the offset, leaning by `tilt` (plus the sway) toward `angle`.
    pub fn shaft(&self, frame: u64, centre: (i32, i32)) -> BeamShaft {
        let period = f32::from(self.period.max(BEAM_MIN_PERIOD));
        let phase = (frame % u64::from(self.period.max(BEAM_MIN_PERIOD))) as f32 / period
            * std::f32::consts::TAU;
        // The sway runs a little off the breath, so the two never lock.
        let lean = (self.tilt + self.sway * (phase * 0.5 + 1.3).sin()).to_radians();
        let heading = self.angle.to_radians();
        let h = f32::from(self.height.max(1));
        let reach = h * lean.tan();
        BeamShaft {
            foot: [
                centre.0 as f32 + f32::from(self.offset[0]),
                f32::from(self.offset[1]),
                centre.1 as f32 + f32::from(self.offset[2]),
            ],
            axis: [reach * heading.sin(), h, reach * heading.cos()],
            width: [f32::from(self.width[0]), f32::from(self.width[1])],
            gain: 1.0 + self.breathe.clamp(0.0, 1.0) * phase.sin(),
        }
    }

    /// The shaft's brightness at `along` (`0` the top, `1` the foot), the
    /// profile's shape before the breath, `0..=1`.
    pub fn profile_at(&self, along: f32) -> f32 {
        let a = along.clamp(0.0, 1.0);
        match self.profile {
            BeamProfile::Fall => smoothstep(0.0, 0.35, a) * (1.0 - 0.35 * a),
            BeamProfile::Rise => {
                let up = 1.0 - a;
                (1.0 - up).powf(1.6) * smoothstep(1.0, 0.92, up)
            }
        }
    }

    /// Mote `i` on `frame`, or `None` past [`Self::motes`]. Each mote
    /// lives a slow cycle of its own, drifting a little down the shaft and
    /// to and fro across it, fading in and out.
    pub fn mote(&self, i: u16, frame: u64) -> Option<BeamMote> {
        if i >= self.motes.min(BEAM_MAX_MOTES) {
            return None;
        }
        let key = self.seed ^ (u64::from(i) << 32) ^ 0xD057_BEA3;
        let life = 360 + splitmix64(key) % 360;
        let t = frame + splitmix64(key ^ 1) % life;
        let cycle = t / life;
        let age = (t % life) as f32 / life as f32;
        let r = |k: u64| {
            unit(splitmix64(
                key ^ cycle.wrapping_mul(0x2545_F491) ^ (k << 56),
            ))
        };
        let along = 0.1 + 0.75 * r(2) + 0.12 * age;
        let ph = r(3) * std::f32::consts::TAU;
        let swing = (age * std::f32::consts::TAU + ph).sin() * 0.08;
        let across = (r(4) - 0.5) * 0.7 + swing;
        let depth = (r(5) - 0.5) * 0.7;
        let fade = (age * std::f32::consts::PI).sin();
        Some(BeamMote {
            along,
            across,
            depth,
            light: fade * self.profile_at(along).max(0.25),
            size: 5.0 + 5.0 * r(6),
        })
    }
}

/// The validator's rules for the map's beams.
pub(crate) fn check_beams(m: &MapDef, cx: &mut Checker<'_>, grid: Option<&TileGrid>) {
    for (i, b) in m.beams.iter().enumerate() {
        let at = format!("beams[{i}]");
        let on = grid
            .is_none_or(|g| usize::from(b.tile[0]) < g.width && usize::from(b.tile[1]) < g.height);
        if !on {
            cx.error(
                &format!("{at}.tile"),
                format!("tile {:?} is off the map", b.tile),
            );
        }
        if b.height == 0 || b.height > BEAM_MAX_HEIGHT {
            cx.error(
                &format!("{at}.height"),
                format!("height {} is not in 1..={BEAM_MAX_HEIGHT}", b.height),
            );
        }
        if b.width.iter().any(|&w| w == 0 || w > BEAM_MAX_WIDTH) {
            cx.error(
                &format!("{at}.width"),
                format!("width {:?} is not in 1..={BEAM_MAX_WIDTH}", b.width),
            );
        }
        if !(b.tilt.is_finite() && (0.0..=BEAM_MAX_TILT).contains(&b.tilt)) {
            cx.error(
                &format!("{at}.tilt"),
                format!("tilt {} is not in 0..={BEAM_MAX_TILT} degrees", b.tilt),
            );
        }
        if !b.angle.is_finite() {
            cx.error(&format!("{at}.angle"), "the heading is not a number");
        }
        if !(b.sway.is_finite() && (0.0..=BEAM_MAX_SWAY).contains(&b.sway)) {
            cx.error(
                &format!("{at}.sway"),
                format!("sway {} is not in 0..={BEAM_MAX_SWAY} degrees", b.sway),
            );
        }
        if !(b.breathe.is_finite() && (0.0..=1.0).contains(&b.breathe)) {
            cx.error(
                &format!("{at}.breathe"),
                format!("breathe {} is not in 0..=1", b.breathe),
            );
        }
        if b.period < BEAM_MIN_PERIOD {
            cx.error(
                &format!("{at}.period"),
                format!("period {} is under {BEAM_MIN_PERIOD} frames", b.period),
            );
        }
        if b.motes > BEAM_MAX_MOTES {
            cx.error(
                &format!("{at}.motes"),
                format!("{} motes; a beam keeps at most {BEAM_MAX_MOTES}", b.motes),
            );
        }
        if b.intensity == 0 || b.color == [0, 0, 0] {
            cx.warning(
                &format!("{at}.intensity"),
                "the beam adds no light: it is not drawn",
            );
        }
        if b.pool && b.profile == BeamProfile::Rise {
            cx.warning(
                &format!("{at}.pool"),
                "a rising glow pools nothing; `pool` is read only with `profile = \"fall\"`",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beam_parses_with_defaults_and_round_trips() {
        let m: MapDef = toml::from_str(
            r#"
[[beams]]
tile = [3, 4]
tilt = 10.0
angle = 90.0
profile = "rise"
pool = false
"#,
        )
        .unwrap();
        let b = &m.beams[0];
        assert_eq!(b.tile, [3, 4]);
        assert_eq!(b.profile, BeamProfile::Rise);
        assert_eq!(b.height, 900);
        assert_eq!(b.motes, 10);
        let back: MapDef = toml::from_str(&toml::to_string(&m).unwrap()).unwrap();
        assert_eq!(back.beams, m.beams);
    }

    #[test]
    fn the_shaft_leans_toward_its_heading_and_breathes() {
        let b = BeamDef {
            tilt: 45.0,
            angle: 90.0,
            sway: 0.0,
            height: 100,
            ..BeamDef::default()
        };
        let s = b.shaft(0, (1000, 2000));
        assert_eq!(s.foot, [1000.0, 0.0, 2000.0]);
        assert!((s.axis[0] - 100.0).abs() < 0.01, "{:?}", s.axis);
        assert!(s.axis[2].abs() < 0.01);
        assert_eq!(s.axis[1], 100.0);
        let gains: Vec<f32> = (0..480).map(|f| b.shaft(f, (0, 0)).gain).collect();
        let (lo, hi) = gains
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), &g| (l.min(g), h.max(g)));
        assert!(lo < 0.9 && hi > 1.1 && lo > 0.8 && hi < 1.2);
        // A pure function of the frame.
        assert_eq!(b.shaft(77, (5, 6)), b.shaft(77, (5, 6)));
    }

    #[test]
    fn the_profiles_put_the_light_at_their_own_end() {
        let fall = BeamDef::default();
        assert_eq!(fall.profile_at(0.0), 0.0);
        assert!(fall.profile_at(0.5) > 0.5);
        assert!(fall.profile_at(1.0) > 0.5);
        let rise = BeamDef {
            profile: BeamProfile::Rise,
            ..BeamDef::default()
        };
        assert!(rise.profile_at(1.0) > 0.9);
        assert!(rise.profile_at(0.2) < 0.2);
        assert_eq!(rise.profile_at(0.0), 0.0);
    }

    #[test]
    fn motes_stay_in_the_shaft_and_are_deterministic() {
        let b = BeamDef {
            motes: 20,
            ..BeamDef::default()
        };
        for f in (0..3000).step_by(37) {
            for i in 0..20 {
                let m = b.mote(i, f).unwrap();
                assert!((0.0..=1.0).contains(&m.along), "{m:?}");
                assert!(m.across.abs() <= 0.5 && m.depth.abs() <= 0.5);
                assert!((0.0..=1.0).contains(&m.light));
                assert_eq!(Some(m), b.mote(i, f));
            }
            assert!(b.mote(20, f).is_none());
        }
    }
}
