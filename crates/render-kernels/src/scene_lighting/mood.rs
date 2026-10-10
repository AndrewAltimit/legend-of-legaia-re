//! Mood: ambient and key light, time of day, scripted sky and the CPU mirror of the shade.

use glam::Vec3;

// ---------------------------------------------------------------------------
// Mood: ambient / key light / time of day
// ---------------------------------------------------------------------------

/// Weight of the orientation (`|N.L|`) term. Shader twins: `DYN_DIFFUSE`.
pub const DIFFUSE: f32 = 0.55;
/// Gain ceiling of the global (ambient + key + pool) term relative to the
/// baked colour. Shader twins: `DYN_MAX_GAIN`.
pub const MAX_GAIN: f32 = 1.3;
/// Gain ceiling with the point-light layer added on top - just below the
/// PSX modulation maximum of 255/128. Shader twins: `DYN_TOTAL_MAX_GAIN`.
pub const TOTAL_MAX_GAIN: f32 = 1.9;
/// Orientation term used when no normal is available. Shader twins:
/// `DYN_LAMBERT_FALLBACK`.
pub const LAMBERT_FALLBACK: f32 = 0.6;
/// Pool centre in 0..1 screen fractions (x, y). Shader twins:
/// `DYN_POOL_CENTER`.
pub const POOL_CENTER: [f32; 2] = [0.5, 0.45];
/// Radius (screen fraction) inside which the pool is full-strength.
pub const POOL_INNER: f32 = 0.15;
/// Radius (screen fraction) beyond which the pool has fully faded.
pub const POOL_OUTER: f32 = 0.75;

/// The lighting conditions one frame is shaded under. Every field is a
/// staged uniform on both hosts (see [`LightingMood::uniforms`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightingMood {
    /// Stable name (HUD / page label, logs).
    pub name: &'static str,
    /// Key light direction (retail Y-down field frame; "from above" is -Y).
    pub key_dir: [f32; 3],
    /// Key light colour x strength (the `|N.L|` and pool terms' tint).
    pub key_rgb: [f32; 3],
    /// Ambient floor gain - what a surface the key light misses gets.
    /// Below 1.0 darkens the baked colour (night, caves).
    pub ambient_rgb: [f32; 3],
    /// Weight of the screen-centred light pool.
    pub pool: f32,
    /// Multiplier on every point light's colour (lamps read stronger at
    /// night than at noon).
    pub point_scale: f32,
    /// Gain an emissive fragment draws at, independent of the mood.
    pub emissive_gain: f32,
    /// Strength of the glow sprites (halos + shafts) 0..=1; 0 draws none.
    pub glow: f32,
    /// How far the [`LIT_WINDOWS`] panes have turned to lamp light, 0..=1
    /// ([`shade_window`]); 0 leaves every window as painted.
    pub window_glow: f32,
}

impl LightingMood {
    /// Daylight: a warm sun over the baked shading, close to the retail
    /// brightness on average, lamps barely visible.
    pub const DAY: Self = Self {
        name: "day",
        key_dir: [0.32, -0.89, 0.31],
        key_rgb: [0.45, 0.42, 0.36],
        ambient_rgb: [0.70, 0.70, 0.70],
        pool: 0.25,
        point_scale: 0.35,
        emissive_gain: 1.15,
        glow: 0.25,
        window_glow: 0.0,
    };
    /// Dusk: low amber sun, cooling shadows, lamps coming on.
    pub const DUSK: Self = Self {
        name: "dusk",
        key_dir: [0.80, -0.45, 0.40],
        key_rgb: [0.70, 0.45, 0.28],
        ambient_rgb: [0.40, 0.38, 0.48],
        pool: 0.20,
        point_scale: 0.85,
        emissive_gain: 1.30,
        glow: 0.65,
        window_glow: 0.6,
    };
    /// Night: moonlit blue ambient, every lamp and window carries the scene.
    pub const NIGHT: Self = Self {
        name: "night",
        key_dir: [-0.35, -0.85, 0.40],
        key_rgb: [0.34, 0.42, 0.66],
        ambient_rgb: [0.25, 0.29, 0.47],
        pool: 0.18,
        point_scale: 1.35,
        emissive_gain: 1.45,
        glow: 1.0,
        window_glow: 1.0,
    };
    /// Night over a scene the **script** already graded to night
    /// ([`ScriptedSky::Night`]): retail's subtractive wash has darkened and
    /// cooled the frame, so the ambient stays near daylight's floor (a
    /// second darkening crushed `town0b` to near-black) and the mood adds
    /// what retail has no system for - moonlight, lamps, lit windows.
    pub const SCRIPTED_NIGHT: Self = Self {
        name: "night",
        key_dir: [-0.35, -0.85, 0.40],
        key_rgb: [0.30, 0.36, 0.52],
        ambient_rgb: [0.62, 0.66, 0.78],
        pool: 0.20,
        point_scale: 1.35,
        emissive_gain: 1.45,
        glow: 1.0,
        window_glow: 1.0,
    };
    /// Dusk over a scene the script already graded warm
    /// ([`ScriptedSky::Dusk`]); the same reasoning as [`Self::SCRIPTED_NIGHT`].
    pub const SCRIPTED_DUSK: Self = Self {
        name: "dusk",
        key_dir: [0.80, -0.45, 0.40],
        key_rgb: [0.50, 0.36, 0.24],
        ambient_rgb: [0.70, 0.67, 0.68],
        pool: 0.20,
        point_scale: 0.85,
        emissive_gain: 1.30,
        glow: 0.65,
        window_glow: 0.6,
    };
    /// Enclosed spaces (caves, dungeons, interiors): dim neutral ambient,
    /// the authored lamps and torches doing the lighting.
    pub const CAVE: Self = Self {
        name: "cave",
        key_dir: [0.20, -0.95, 0.20],
        key_rgb: [0.42, 0.40, 0.38],
        ambient_rgb: [0.62, 0.60, 0.60],
        pool: 0.30,
        point_scale: 1.2,
        emissive_gain: 1.35,
        glow: 0.8,
        window_glow: 0.0,
    };

    /// The mood a scene is lit under when the player has not picked a time
    /// of day ([`TimeOfDay::Auto`]): a curated table of the enclosed scenes
    /// (caves, towers, dungeons) falls back to daylight for everything else.
    pub fn for_scene(scene: &str) -> Self {
        let s = scene.to_ascii_lowercase();
        if ENCLOSED_SCENE_PREFIXES.iter().any(|p| s.starts_with(p)) {
            Self::CAVE
        } else {
            Self::DAY
        }
    }

    /// The four shader uniforms the mood stages, shared layout on both
    /// hosts: `light_dir = (key_dir, enable)`, `light_color = (key_rgb,
    /// pool)`, `light_ambient = (ambient_rgb, emissive_gain)`. `enable` is
    /// 1.0 for the enhancement on and 0.0 off (the identity).
    pub fn uniforms(&self, enable: bool) -> [[f32; 4]; 3] {
        let d = Vec3::from(self.key_dir).normalize_or_zero();
        [
            [d.x, d.y, d.z, if enable { 1.0 } else { 0.0 }],
            [self.key_rgb[0], self.key_rgb[1], self.key_rgb[2], self.pool],
            [
                self.ambient_rgb[0],
                self.ambient_rgb[1],
                self.ambient_rgb[2],
                self.emissive_gain,
            ],
        ]
    }

    /// The fourth shader word: `(window_glow, 0, 0, 0)`. Native rides the
    /// scene-lights block's `params.w`; the play page's lighting packet
    /// carries it after the three [`Self::uniforms`] words.
    pub fn window_word(&self) -> [f32; 4] {
        [self.window_glow.clamp(0.0, 1.0), 0.0, 0.0, 0.0]
    }

    /// The mood as a small JSON object (the browser play page's staging).
    pub fn to_json(&self) -> String {
        let [d, c, a] = self.uniforms(true);
        format!(
            "{{\"name\":\"{}\",\"dir\":[{},{},{}],\"key\":[{},{},{},{}],\"ambient\":[{},{},{},{}],\"point_scale\":{},\"glow\":{},\"window_glow\":{}}}",
            self.name,
            d[0],
            d[1],
            d[2],
            c[0],
            c[1],
            c[2],
            c[3],
            a[0],
            a[1],
            a[2],
            a[3],
            self.point_scale,
            self.glow,
            self.window_glow
        )
    }
}

/// CDNAME prefixes lit as enclosed spaces under [`TimeOfDay::Auto`]. The
/// labels are the disc's own (`docs/reference/scene-names.md`).
pub const ENCLOSED_SCENE_PREFIXES: &[&str] = &[
    "cave",     // Snowdrift Cave
    "jiji",     // Ancient Wind Cave
    "tunnel",   // Ancient Path / Fire Path
    "retockin", // Mt. Letona interior
    "chitei",   // Jette's Fortress transport system
    "korb",     // Sol Tower underground
    "koin",     // Sol Tower rooms
    "jouin",    // Bio Castle interiors
    "juui",     // Bio Castle
    "garmel",   // Zeto's Dungeon
    "stone",    // Shadow Gate
    "dohaty",   // Dohati's Castle
    "jagaroom", // Juggernaut Room
    "rugi",     // Rogue's Tower
    "deroa",    // Jette's Fortress
];

/// The player-facing time-of-day knob over the mood: `Auto` follows the
/// scene ([`LightingMood::for_scene`]), the rest force one preset. Cycled
/// at runtime on both hosts (`O` in `play-window`, the page's selector).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeOfDay {
    #[default]
    Auto,
    Day,
    Dusk,
    Night,
}

impl TimeOfDay {
    /// Every value, in cycle order.
    pub const ALL: [Self; 4] = [Self::Auto, Self::Day, Self::Dusk, Self::Night];

    /// The next value in cycle order (wraps).
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Stable lower-case name (`auto` / `day` / `dusk` / `night`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Day => "day",
            Self::Dusk => "dusk",
            Self::Night => "night",
        }
    }

    /// Parse [`Self::name`] (case-insensitive).
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(s.trim()))
    }

    /// The mood this setting lights `scene` under.
    pub fn mood(self, scene: &str) -> LightingMood {
        self.mood_graded(scene, None)
    }

    /// [`Self::mood`] with the scene's live **scripted grade** - the held
    /// op-`0x34` sub-0 screen-effect tween as `(blend, target_rgb)`
    /// (`World::held_scene_grade`). Under [`Self::Auto`] a grade that reads
    /// as a time of day ([`ScriptedSky::from_grade`]) picks the mood, so a
    /// scene the script turns to night (Rim Elm under attack, `town0b`; the
    /// night `town01`) is lit as night, never as daylight under retail's own
    /// night wash. A forced time of day still wins.
    ///
    /// The one call both play hosts make per frame.
    pub fn mood_graded(self, scene: &str, grade: Option<(i16, [i16; 3])>) -> LightingMood {
        match self {
            Self::Auto => match grade.and_then(|(b, rgb)| ScriptedSky::from_grade(b, rgb)) {
                Some(ScriptedSky::Night) => LightingMood::SCRIPTED_NIGHT,
                Some(ScriptedSky::Dusk) => LightingMood::SCRIPTED_DUSK,
                None => LightingMood::for_scene(scene),
            },
            Self::Day => LightingMood::DAY,
            Self::Dusk => LightingMood::DUSK,
            Self::Night => LightingMood::NIGHT,
        }
    }
}

/// The time of day a field script's held screen grade reads as.
///
/// Retail has no time-of-day system: a scene goes to night by its entry
/// script holding a **subtractive** full-screen tint - op `34 01 r g b dur`
/// (sub-0 with `op0 & 1`, blend `2`, the `FUN_80024EE4` push held at `-1`).
/// Subtracting a red/green-heavy colour leaves the frame darker and blue:
/// `town0b` and the night `town01` (flag `0x146`) hold `30 30 00`. Subtracting
/// a blue-heavy one leaves it warm: the evening `town01` (flag `0x22E`) holds
/// `00 10 18`. Greys (a plain dim) and additive grades read as neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptedSky {
    Dusk,
    Night,
}

impl ScriptedSky {
    /// Classify a held grade `(blend, target_rgb)`; `None` for every grade
    /// that is not a subtractive colour cast.
    pub fn from_grade(blend: i16, rgb: [i16; 3]) -> Option<Self> {
        if blend != 2 {
            return None;
        }
        let [r, g, b] = rgb;
        if b < r.min(g) && r.max(g) >= 0x20 {
            Some(Self::Night)
        } else if b > r && b >= 0x10 {
            Some(Self::Dusk)
        } else {
            None
        }
    }
}

/// CPU mirror of the per-fragment shading law (`dyn_light` in both shader
/// twins).
///
/// ```text
/// base  = ambient + (DIFFUSE * |N.L| + pool_w * pool(frag)) * key
/// gain  = min(min(base, MAX_GAIN) + point_gain, TOTAL_MAX_GAIN)
/// lit   = rgb * gain
/// out   = emissive ? rgb * min(emissive_gain + point_gain, TOTAL_MAX_GAIN)
///                  : lit                                (saturating)
/// ```
///
/// `uniforms` is [`LightingMood::uniforms`]; with its enable at zero the
/// function is the exact identity (the faithful path).
#[allow(clippy::too_many_arguments)] // mirrors the shader twins 1:1
pub fn shade(
    rgb: [f32; 3],
    normal: [f32; 3],
    frag_px: [f32; 2],
    viewport: [f32; 2],
    uniforms: [[f32; 4]; 3],
    point_gain: [f32; 3],
    emissive: bool,
) -> [f32; 3] {
    let [dir, key, amb] = uniforms;
    if dir[3] < 0.5 {
        return rgb;
    }
    if emissive {
        let mut out = [0.0f32; 3];
        for i in 0..3 {
            out[i] = (rgb[i] * (amb[3] + point_gain[i]).min(TOTAL_MAX_GAIN)).clamp(0.0, 1.0);
        }
        return out;
    }
    let mut lambert = LAMBERT_FALLBACK;
    let n = Vec3::from(normal);
    let l = Vec3::new(dir[0], dir[1], dir[2]);
    if n.length() > 1e-6 && l.length() > 0.0 {
        lambert = n.normalize().dot(l.normalize()).abs();
    }
    let pool = pool_factor(frag_px, viewport);
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let base = amb[i] + (DIFFUSE * lambert + key[3] * pool) * key[i];
        let gain = (base.min(MAX_GAIN) + point_gain[i]).min(TOTAL_MAX_GAIN);
        out[i] = (rgb[i] * gain).clamp(0.0, 1.0);
    }
    out
}

/// CPU mirror of the shaders' `dyn_far`: the gain the depth cue's far term
/// takes under the enhancement (`uniforms` as in [`shade`]) - the mood's
/// ambient plus half its key, capped at [`MAX_GAIN`]; `[1; 3]` while off.
pub fn far_gain(uniforms: [[f32; 4]; 3]) -> [f32; 3] {
    let [dir, key, amb] = uniforms;
    if dir[3] < 0.5 {
        return [1.0; 3];
    }
    [0, 1, 2].map(|i| (amb[i] + 0.5 * DIFFUSE * key[i]).min(MAX_GAIN))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The screen-space light-pool factor for a fragment at `frag_px` on a
/// `viewport`-sized surface: 1.0 at [`POOL_CENTER`], fading to 0.0 past
/// [`POOL_OUTER`]. Returns 0.0 for a degenerate viewport.
pub fn pool_factor(frag_px: [f32; 2], viewport: [f32; 2]) -> f32 {
    if viewport[0] <= 0.0 || viewport[1] <= 0.0 {
        return 0.0;
    }
    let dx = frag_px[0] / viewport[0] - POOL_CENTER[0];
    let dy = frag_px[1] / viewport[1] - POOL_CENTER[1];
    let d = (dx * dx + dy * dy).sqrt();
    1.0 - smoothstep(POOL_INNER, POOL_OUTER, d)
}
