//! Which scenes carry ground fog and how it looks: `FogSpace`, `FogStyle` and the per-scene table.

use super::*;

/// The coordinate space a bank lives in: the field's world units, or the
/// battle stage's raw units (the frame both battle hosts draw the stage
/// from before their `BATTLE_WORLD_SCALE` model factor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FogSpace {
    Field,
    Battle,
}

impl FogSpace {
    /// World units per disturbance cell.
    pub fn sim_cell(self) -> f32 {
        match self {
            FogSpace::Field => 40.0,
            FogSpace::Battle => 64.0,
        }
    }
    /// World units per sheet-mesh quad.
    pub fn mesh_cell(self) -> f32 {
        match self {
            FogSpace::Field => 128.0,
            FogSpace::Battle => 160.0,
        }
    }
    /// The radius a mover parts the bank over, in world units.
    pub fn mover_radius(self) -> f32 {
        match self {
            FogSpace::Field => 84.0,
            FogSpace::Battle => 180.0,
        }
    }
    /// [`FOG_SHADER_CONSTANTS`] for this space: the noise scales follow the
    /// arena's size ([`BATTLE_FOG`]`.noise_scale`), so a battle arena shows
    /// as many wisps across it as a field screen does, and the sheet gain is
    /// the profile's.
    pub fn shader_constants(self) -> [f32; 4] {
        let c = FOG_SHADER_CONSTANTS;
        match self {
            FogSpace::Field => c,
            FogSpace::Battle => {
                let k = BATTLE_FOG.noise_scale;
                [c[0] / k, c[1] / k, c[2] * BATTLE_FOG.gain, c[3]]
            }
        }
    }

    /// Opacity relative to a style's ([`BATTLE_FOG`] in battle).
    pub fn density_scale(self) -> f32 {
        match self {
            FogSpace::Field => 1.0,
            FogSpace::Battle => BATTLE_FOG.density,
        }
    }

    /// View-depth span (clip `w` of the host's matrix for this space) over
    /// which a sheet fades into the surface behind it - the soft
    /// intersection that keeps walls, ledges and legs from cutting it with
    /// a hard line. The battle matrix carries the stage's world scale.
    pub fn soft_distance(self) -> f32 {
        match self {
            FogSpace::Field => 50.0,
            FogSpace::Battle => BATTLE_FOG.soft_distance,
        }
    }

    /// Bank depth relative to a style's (field-unit) height
    /// ([`BATTLE_FOG`] in battle).
    pub fn height_scale(self) -> f32 {
        match self {
            FogSpace::Field => 1.0,
            FogSpace::Battle => BATTLE_FOG.height,
        }
    }
}

/// How a battle's bank differs from its field scene's - the one parameter
/// set both hosts read (through [`FogSpace`] and the frame header).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BattleFogProfile {
    /// Opacity relative to the field style's.
    pub density: f32,
    /// Bank depth relative to the field style's height (field units), in
    /// stage units. The battle forms stand about 2.5x taller in stage units
    /// than the field forms do in world units, so `1.0` is a bank about
    /// 2.5x lower against the fighters than the field's against the walkers.
    pub height: f32,
    /// Noise-scale divisor: the arena is about 2.5x larger in stage units.
    pub noise_scale: f32,
    /// Sheet gain relative to the field's.
    pub gain: f32,
    /// Soft-intersection span, stage-matrix clip `w` units.
    pub soft_distance: f32,
}

/// The battle profile: a thin, low ground haze so the stage floor reads
/// clearly under it - about a third of the field bank's opacity, at ankle
/// height on the fighters.
pub const BATTLE_FOG: BattleFogProfile = BattleFogProfile {
    density: 0.3,
    height: 1.0,
    noise_scale: 2.5,
    gain: 1.0,
    soft_distance: 160.0,
};

/// One scene's look: what colour the bank is, how thick, how deep, and
/// which way it drifts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogStyle {
    /// Framebuffer-space colour, `0..=1` per channel (the hosts write PSX
    /// framebuffer bytes, so this is not linear light).
    pub color: [f32; 3],
    /// Summed opacity at the floor for an undisturbed bank, `0..=1`.
    pub density: f32,
    /// Bank depth above the floor, world units (retail Y-down frame: the
    /// top sheet sits at `floor - height`).
    pub height: f32,
    /// Ambient drift, world units per tick on X / Z.
    pub wind: [f32; 2],
}

/// Field scenes whose look the bank is tuned for, keyed by CDNAME label.
/// Each is a scene whose own art reads as mist or night: the Mist's attack
/// on Rim Elm (`town0b`, a night scene), the Mist-wrapped Drake Castle
/// (`dolk`), the two Voz forests and the Ravine the walkthroughs call the
/// Valleys of Mist.
const SCENE_STYLES: &[(&str, FogStyle)] = &[
    (
        "town0b",
        FogStyle {
            color: [0.70, 0.74, 0.88],
            density: 0.8,
            height: 95.0,
            wind: [0.55, 0.22],
        },
    ),
    (
        "dolk",
        FogStyle {
            color: [0.52, 0.49, 0.60],
            density: 0.75,
            height: 85.0,
            wind: [0.35, -0.45],
        },
    ),
    (
        "vell",
        FogStyle {
            color: [0.56, 0.62, 0.58],
            density: 0.62,
            height: 85.0,
            wind: [0.40, 0.30],
        },
    ),
    (
        "vozz",
        FogStyle {
            color: [0.56, 0.62, 0.58],
            density: 0.62,
            height: 85.0,
            wind: [-0.30, 0.40],
        },
    ),
    (
        "keikoku",
        FogStyle {
            color: [0.62, 0.63, 0.70],
            density: 0.72,
            height: 110.0,
            wind: [0.60, 0.10],
        },
    ),
];

/// The default bank for a field scene whose retail fog pool is live (gate
/// raised and at least one region enabled) but which has no tuned entry.
pub const POOL_STYLE: FogStyle = FogStyle {
    color: [0.58, 0.60, 0.68],
    density: 0.5,
    height: 85.0,
    wind: [0.45, 0.20],
};

/// The bank a field scene gets: its tuned entry, else [`POOL_STYLE`] when
/// the retail fog pool is live, else none. `pool_live` is the retail gate
/// (`_DAT_8007B854`) raised with at least one enabled section-4 region.
pub fn scene_style(label: &str, pool_live: bool) -> Option<FogStyle> {
    let key = label.trim().to_ascii_lowercase();
    SCENE_STYLES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, s)| *s)
        .or(pool_live.then_some(POOL_STYLE))
}
