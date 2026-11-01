//! Living ground: a pack map's floor that moves its **geometry** - a swell
//! rolling across a lava moat, a flesh floor that breathes - the way the
//! retail field moves its own.
//!
//! Retail animates a living floor through the scene's sixteen-rung
//! floor-height ladder (`docs/subsystems/script-vm-menuctrl.md`, "Nibble 9 is
//! the floor-height ladder"): field-VM op `4C 90` sets one rung oscillating
//! (`FUN_801DDE34` -> `FUN_801DA930`), and three readers follow the live
//! rungs every frame - the ground pass `FUN_801F6D48` (each ground vertex
//! takes its corner tile's rung, so the floor deforms **per vertex**), the
//! decoration sweep `FUN_801F7088`, and the floor sampler `FUN_80019278`, so
//! the party walks the shape that is drawn. `jou`'s entry script sets
//! consecutive rungs oscillating with rising phases: a travelling wave across
//! the Juggernaut's organic floor.
//!
//! A [`GroundMotionDef`] is that mechanism authored per legend character.
//! Its tiles move onto **rungs of their own** - spare ladder slots no tile of
//! the map otherwise uses, each resting at the height of the tier it stands
//! in for ([`MapDef::floor_ladder`] fills them) - one rung per phase step of
//! the wave. The map's field image writes each animated tile's collision
//! nibble as its phase rung, and the world drives the rungs every tick
//! ([`MotionRung::height`]). Nothing new reaches either host: both already
//! redraw the ground and the decoration cells from the live ladder, and the
//! floor sampler already reads it, so what moves is drawn, walked and shared
//! by one kernel.
//!
//! The wave is the closed form of retail's rung oscillator. `FUN_801DA930`
//! accelerates the rung toward its rest height by a constant step every tick
//! and never damps, so each excursion is a parabola and the two meet at the
//! rest height at equal speed ([`parabolic_wave`]). It is integer arithmetic
//! end to end, so every host and every run draws the same floor on the same
//! frame.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::map::{FLOOR_TIERS, MapDef};

/// The largest swing a ground motion may author, world units either way.
pub const MAX_GROUND_MOTION_AMPLITUDE: i16 = 64;

/// The most phase steps (and so ladder rungs per base tier) one motion takes.
pub const MAX_GROUND_MOTION_PHASES: u8 = 8;

/// How the motion's phase spreads over its tiles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroundMotionKind {
    /// Every tile rises and falls together: a chest breathing.
    #[default]
    Breathe,
    /// A travelling wave along `angle`: crests roll across the floor.
    Swell,
    /// Rings travelling out from `centre`: a pulse spreading from a heart.
    Ripple,
}

/// One living-ground motion over the tiles of some legend characters.
///
/// ```toml
/// [[ground_motion]]
/// kind = "swell"
/// chars = "Ll"
/// amplitude = 10      # world units either side of rest, up positive
/// period = 120        # frames a cycle
/// phases = 4          # phase steps (ladder rungs per tier)
/// wavelength = 4.0    # tiles from crest to crest
/// angle = 90.0        # the way crests travel, degrees, 0 = +Z, 90 = +X
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroundMotionDef {
    pub kind: GroundMotionKind,
    /// The legend characters whose tiles move.
    pub chars: String,
    /// Swing either side of the rest height, world units (up positive).
    pub amplitude: i16,
    /// Frames a full cycle (at least 2).
    pub period: i32,
    /// Where in its cycle the motion starts, a fraction of the period.
    pub phase: f32,
    /// Phase steps the wave is cut into: each is a ladder rung of its own
    /// for every tier the motion's tiles stand on. A [`GroundMotionKind::
    /// Breathe`] takes one whatever this says.
    pub phases: u8,
    /// Tiles from crest to crest ([`GroundMotionKind::Swell`],
    /// [`GroundMotionKind::Ripple`]). With `wavelength == phases` each tile
    /// corner of a row is one phase step on from the last.
    pub wavelength: f32,
    /// The direction a swell's crests travel, degrees (`0` = `+Z`, `90` =
    /// `+X`, the fog regions' convention).
    pub angle: f32,
    /// A ripple's centre, map-local tile coordinates as floats; `None` takes
    /// the middle of the motion's tiles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub centre: Option<[f32; 2]>,
}

impl Default for GroundMotionDef {
    fn default() -> Self {
        Self {
            kind: GroundMotionKind::Breathe,
            chars: String::new(),
            amplitude: 8,
            period: 120,
            phase: 0.0,
            phases: 4,
            wavelength: 4.0,
            angle: 0.0,
            centre: None,
        }
    }
}

impl GroundMotionDef {
    /// The phase steps the motion actually takes: `1` for a breathe, else
    /// `phases` clamped to `1..=MAX_GROUND_MOTION_PHASES`.
    pub fn steps(&self) -> u8 {
        match self.kind {
            GroundMotionKind::Breathe => 1,
            _ => self.phases.clamp(1, MAX_GROUND_MOTION_PHASES),
        }
    }

    /// The phase step of map-local tile `(col, row)` - which of the motion's
    /// rungs its corner rides. Measured at the tile's low corner, where the
    /// ground pass reads its nibble (`build_walk_heightfield`), in tiles.
    /// `centre` is the ripple's resolved centre.
    pub fn step_at(&self, col: u16, row: u16, centre: [f32; 2]) -> u8 {
        let steps = self.steps();
        if steps <= 1 {
            return 0;
        }
        let (x, z) = (f64::from(col), f64::from(row));
        let s = match self.kind {
            GroundMotionKind::Breathe => return 0,
            GroundMotionKind::Swell => {
                let a = f64::from(self.angle).to_radians();
                x * a.sin() + z * a.cos()
            }
            GroundMotionKind::Ripple => {
                let (dx, dz) = (x - f64::from(centre[0]), z - f64::from(centre[1]));
                (dx * dx + dz * dz).sqrt()
            }
        };
        // Quantise before the floor so a corner on a step boundary lands on
        // the same step whatever the platform's last bit of `sin` says.
        const Q: f64 = 4096.0;
        let wl = (f64::from(self.wavelength.max(0.01)) * Q).round() as i64;
        let s = (s * Q).round() as i64;
        let frac = s.rem_euclid(wl);
        ((frac * i64::from(steps)) / wl) as u8
    }
}

/// The retail rung oscillator's wave in closed form, `-amplitude ..=
/// amplitude`, at `t` frames into a `period`-frame cycle: a parabolic arc
/// above the rest height for the first half, the mirrored arc below it for
/// the second, meeting at rest with equal slope. Integer throughout.
///
/// REF: FUN_801DA930 (constant acceleration toward the rest height, no
/// damping: each half-cycle is a parabola)
pub fn parabolic_wave(t: i64, period: i32, amplitude: i16) -> i16 {
    let p = i64::from(period.max(2));
    // Twice the time, so an odd period's halves stay whole.
    let x = 2 * t.rem_euclid(p);
    let (x, sign) = if x < p { (x, 1) } else { (x - p, -1) };
    let a = i64::from(amplitude);
    (sign * a * 4 * x * (p - x) / (p * p)) as i16
}

/// One ladder rung a ground motion drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionRung {
    /// The ladder slot (`0..16`).
    pub rung: u8,
    /// The tier whose height the rung rests at.
    pub base_tier: u8,
    /// Index into [`MapDef::ground_motion`].
    pub motion: usize,
    /// The rung's phase step, `0..steps`.
    pub step: u8,
}

impl MotionRung {
    /// The rung's offset from its rest height on `frame`, world units (up
    /// positive): the motion's wave, a step's share of the period behind the
    /// one before it, so the crest travels toward rising steps.
    pub fn offset(&self, def: &GroundMotionDef, frame: u64) -> i16 {
        let p = i64::from(def.period.max(2));
        let start = (f64::from(def.phase) * p as f64).round() as i64;
        let lag = i64::from(self.step) * p / i64::from(def.steps());
        parabolic_wave(frame as i64 + start - lag, def.period, def.amplitude)
    }

    /// The rung's height on `frame` in the MAN-header convention (up
    /// positive): its tier's rest height plus [`Self::offset`].
    pub fn height(&self, map: &MapDef, ladder: &[i16; FLOOR_TIERS], frame: u64) -> i16 {
        let rest = ladder[usize::from(self.base_tier)];
        map.ground_motion
            .get(self.motion)
            .map_or(rest, |d| rest.saturating_add(self.offset(d, frame)))
    }
}

/// Where a map's ground motions sit on its ladder ([`MapDef::ground_motion_plan`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroundMotionPlan {
    /// Every rung a motion drives, in allocation order.
    pub rungs: Vec<MotionRung>,
    /// Each motion's resolved ripple centre (map-local tiles), parallel to
    /// [`MapDef::ground_motion`].
    pub centres: Vec<[f32; 2]>,
    /// Rungs the motions needed past the ladder's free slots (`0` when they
    /// fit); a validator reports it, the build drops the motions that miss.
    pub short: usize,
}

impl GroundMotionPlan {
    /// The rung tile `(col, row)` of legend character `c` at tier `tier`
    /// rides, `None` when no motion moves it.
    pub fn rung_at(&self, map: &MapDef, col: u16, row: u16, c: char, tier: u8) -> Option<u8> {
        let m = map.ground_motion.iter().position(|d| d.chars.contains(c))?;
        let def = &map.ground_motion[m];
        let step = def.step_at(col, row, self.centres[m]);
        self.rungs
            .iter()
            .find(|r| r.motion == m && r.base_tier == tier && r.step == step)
            .map(|r| r.rung)
    }
}

impl MapDef {
    /// The tiers the map's own tiles stand on: every legend style's height
    /// and every listed `floor_heights` slot.
    fn used_tiers(&self) -> [bool; FLOOR_TIERS] {
        let mut used = [false; FLOOR_TIERS];
        for slot in used.iter_mut().take(self.floor_heights.len()) {
            *slot = true;
        }
        for style in self.tiles.legend.values() {
            if let Some(u) = used.get_mut(usize::from(style.height)) {
                *u = true;
            }
        }
        used
    }

    /// Lay the map's [`Self::ground_motion`] onto its ladder: for each
    /// motion in order, for each tier its characters stand on, one free rung
    /// per phase step, taken from the top of the ladder down. A pure
    /// function of the file, so the field image, the ladder and the world's
    /// tick agree on every host. A map with a terrain, or with no
    /// `floor_heights` of its own, moves nothing (the validator says why).
    pub fn ground_motion_plan(&self) -> GroundMotionPlan {
        let mut plan = GroundMotionPlan {
            centres: self
                .ground_motion
                .iter()
                .map(|d| d.centre.unwrap_or_else(|| self.chars_centre(&d.chars)))
                .collect(),
            ..Default::default()
        };
        if self.ground_motion.is_empty() || self.terrain.is_some() || self.floor_heights.is_empty()
        {
            return plan;
        }
        let used = self.used_tiers();
        let mut free = (0..FLOOR_TIERS as u8)
            .rev()
            .filter(|&r| !used[usize::from(r)]);
        for (m, def) in self.ground_motion.iter().enumerate() {
            // A character an earlier motion claims stays with it.
            let earlier: String = self.ground_motion[..m]
                .iter()
                .flat_map(|d| d.chars.chars())
                .collect();
            let mut tiers: Vec<u8> = def
                .chars
                .chars()
                .filter(|c| !earlier.contains(*c))
                .filter_map(|c| self.style(c).map(|s| s.height))
                .collect();
            tiers.sort_unstable();
            tiers.dedup();
            for tier in tiers {
                for step in 0..def.steps() {
                    match free.next() {
                        Some(rung) => plan.rungs.push(MotionRung {
                            rung,
                            base_tier: tier,
                            motion: m,
                            step,
                        }),
                        None => plan.short += 1,
                    }
                }
            }
        }
        // A tier missing any of its steps would leave tiles on no rung:
        // drop the partial set so those tiles keep their own tier, still.
        let complete: BTreeMap<(usize, u8), usize> =
            plan.rungs.iter().fold(BTreeMap::new(), |mut acc, r| {
                *acc.entry((r.motion, r.base_tier)).or_default() += 1;
                acc
            });
        plan.rungs.retain(|r| {
            complete[&(r.motion, r.base_tier)] == usize::from(self.ground_motion[r.motion].steps())
        });
        plan
    }

    /// The middle of the tiles carrying any of `chars`, map-local tiles
    /// (`[0, 0]` when none does).
    fn chars_centre(&self, chars: &str) -> [f32; 2] {
        let (mut sx, mut sz, mut n) = (0f64, 0f64, 0f64);
        for (row, text) in self.tiles.rows.iter().enumerate() {
            for (col, c) in text.chars().enumerate() {
                if chars.contains(c) {
                    sx += col as f64 + 0.5;
                    sz += row as f64 + 0.5;
                    n += 1.0;
                }
            }
        }
        if n == 0.0 {
            return [0.0, 0.0];
        }
        [(sx / n) as f32, (sz / n) as f32]
    }
}

#[cfg(test)]
mod tests;
