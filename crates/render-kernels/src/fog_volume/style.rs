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
    /// Bank depth relative to a style's (field-unit) height: the battle
    /// forms stand several times taller in stage units than the field forms
    /// do in world units, so the same knee-to-waist bank is deeper there.
    /// [`FOG_SHADER_CONSTANTS`] for this space: the noise scales follow the
    /// bank's size ([`Self::height_scale`]), so a battle arena shows as many
    /// wisps across it as a field screen does.
    pub fn shader_constants(self) -> [f32; 4] {
        let k = self.height_scale();
        let c = FOG_SHADER_CONSTANTS;
        // A battle's low, close framing looks through the bank at a grazing
        // angle across few sheets' worth of crest; it takes a larger gain to
        // read as the thick layer the field shows from above.
        let gain = match self {
            FogSpace::Field => 1.0,
            FogSpace::Battle => 2.6,
        };
        [c[0] / k, c[1] / k, c[2] * gain, c[3]]
    }

    /// Opacity relative to a style's: a battle is framed low and close on
    /// the fighters' legs, where the concept is a thick swirling layer.
    pub fn density_scale(self) -> f32 {
        match self {
            FogSpace::Field => 1.0,
            FogSpace::Battle => 2.2,
        }
    }

    /// View-depth span (clip `w` of the host's matrix for this space) over
    /// which a sheet fades into the surface behind it - the soft
    /// intersection that keeps walls, ledges and legs from cutting it with
    /// a hard line. The battle matrix carries the stage's world scale.
    pub fn soft_distance(self) -> f32 {
        match self {
            FogSpace::Field => 50.0,
            FogSpace::Battle => 160.0,
        }
    }

    pub fn height_scale(self) -> f32 {
        match self {
            FogSpace::Field => 1.0,
            FogSpace::Battle => 2.5,
        }
    }
}

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
