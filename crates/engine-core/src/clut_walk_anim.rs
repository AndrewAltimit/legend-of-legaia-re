//! The CLUT-walk shimmer's **stepper**: the water / waterfall / ocean palette
//! animation every host runs over its scene VRAM.
//!
//! [`legaia_asset::clut_walk`] parses the walker table and the source strips;
//! this is what installs them and steps them. Each table entry is one retail
//! walker actor (`FUN_80024cfc` spawns it, accumulator seeded to
//! [`legaia_asset::clut_walk::ACCUMULATOR_SEED`] so every entry's first copy
//! fires at scene entry). Per retail game tick every accumulator banks `dt`
//! vsyncs (`DAT_1F800393`); when one crosses its frame's hold it emits a 16x1
//! VRAM-to-VRAM `MoveImage` from the parked source strip onto its destination
//! cell, **resets** to zero (not subtract-remainder: captures show strictly
//! constant intervals - hold 8 at dt 3 fires every 9 vsyncs with no jitter,
//! which only a reset produces) and advances its frame with wrap-around.
//!
//! The native window and the browser page each carried their own copy of the
//! resolve, the park and the step, and the copies had drifted in which rows a
//! field scene parked (the page never ran the Drake complement's coverage
//! pass on a field scene) and in which column the coverage test checked (the
//! page tested column 0 only, where a cell can sit at any x). One copy now.
//!
//! PORT: FUN_8001ada4 - the SCUS actor walker's case 0xB, the CLUT-walk
//! stepper (acc += DAT_1F800393; on acc >= hold: MoveImage 16x1, acc = 0,
//! frame++ wrapping).

use legaia_asset::clut_walk::{self, ClutWalkTable, ParkStrip};
use legaia_tim::Vram;

use crate::scene::{ProtIndex, Scene, is_world_map_scene};

/// The legacy ocean-head fallback's hold, in vsyncs (the slot-5 ocean-head
/// entry's own hold).
pub const OCEAN_ANIM_VSYNCS_PER_FRAME: u32 = 8;

/// The ocean-head CLUT row the legacy fallback writes: VRAM `(0, 506)`.
pub const OCEAN_HEAD_CLUT: (u16, u16) = (0, 506);

/// One scene's CLUT-walk animation.
#[derive(Debug, Clone)]
pub enum ClutWalkAnim {
    /// The table-driven walker: `(accumulator vsyncs, frame index)` per
    /// table entry, all on one shared game-tick clock.
    Walk {
        table: ClutWalkTable,
        state: Vec<(u32, usize)>,
    },
    /// The legacy single-cell ocean-head cycle, kept only for a kingdom
    /// bundle without a parseable slot-5 table (no retail bundle hits this;
    /// it keeps the sea moving on a modified or damaged disc). `frames` is
    /// the decoded `N x 32`-byte run.
    Ocean {
        frames: Vec<u8>,
        cur: usize,
        accum: u32,
    },
}

/// What [`ClutWalkAnim::install`] resolved: the animation, and every walker
/// source cell still blank after parking - a real residency gap the host
/// reports rather than papers over.
#[derive(Debug, Clone)]
pub struct ClutWalkInstall {
    pub anim: ClutWalkAnim,
    pub missing_cells: Vec<(u16, u16)>,
    /// `true` when the kingdom bundle had no slot-5 table and the legacy
    /// ocean-head cycle stands in.
    pub ocean_fallback: bool,
}

impl ClutWalkAnim {
    fn walk(table: ClutWalkTable) -> Self {
        let state = vec![(clut_walk::ACCUMULATOR_SEED, 0usize); table.entries.len()];
        Self::Walk { table, state }
    }

    /// Resolve the scene's CLUT-walk animation and park its source strips
    /// into `vram`.
    ///
    /// A field scene carries its table in its bundle's type-6 slot (garmel /
    /// dohaty water, the waterfall family); a kingdom overworld carries it in
    /// slot 5 with its strips in slot 0. Two parking layers, mirroring what
    /// retail VRAM holds:
    ///
    /// 1. the bundle's own CLUT-block records (the retail loader
    ///    `LoadImage`s them verbatim; the TIM pre-pass skips them because
    ///    they carry no TIM magic);
    /// 2. the **Drake complement**: map02 / map03 park only rows
    ///    `{501, 503, 505}` while the kingdom-invariant table also sources
    ///    rows 498 / 502 / 504, which retail inherits as VRAM residue from
    ///    the Drake kingdom's upload (map01 is always the first world map).
    ///    Those records come straight from the Drake bundle for any source
    ///    row the scene's own bundle does not cover.
    pub fn install(scene: &Scene, index: &ProtIndex, vram: &mut Vram) -> Option<ClutWalkInstall> {
        if !is_world_map_scene(&scene.name) {
            for entry in &scene.entries {
                let Ok(table) = clut_walk::from_scene_bundle(&entry.bytes) else {
                    continue;
                };
                park(vram, &clut_walk::scene_park_strips(&entry.bytes));
                let missing_cells = park_drake_complement(&table, index, vram);
                return Some(ClutWalkInstall {
                    anim: Self::walk(table),
                    missing_cells,
                    ocean_fallback: false,
                });
            }
            return None;
        }
        for entry in &scene.entries {
            let Ok(table) = clut_walk::from_kingdom_entry(&entry.bytes) else {
                continue;
            };
            if let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&entry.bytes, 0) {
                park(vram, &clut_walk::park_strips(&slot0));
            }
            let missing_cells = park_drake_complement(&table, index, vram);
            return Some(ClutWalkInstall {
                anim: Self::walk(table),
                missing_cells,
                ocean_fallback: false,
            });
        }
        // Slot 5 absent / unparseable: the legacy ocean-head fallback.
        for entry in &scene.entries {
            let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&entry.bytes, 0) else {
                continue;
            };
            if let Some(ocean) = legaia_asset::ocean::find_ocean_assets(&slot0)
                && ocean.animation_frames.len() >= 32
            {
                return Some(ClutWalkInstall {
                    anim: Self::Ocean {
                        frames: ocean.animation_frames,
                        cur: 0,
                        accum: 0,
                    },
                    missing_cells: Vec::new(),
                    ocean_fallback: true,
                });
            }
        }
        None
    }

    /// Step one retail **game tick** of `dt` vsyncs and apply every due copy
    /// to `vram`. Returns `true` when texels changed.
    pub fn game_tick(&mut self, dt: u32, vram: &mut Vram) -> bool {
        let dt = dt.max(1);
        let mut wrote = false;
        match self {
            Self::Walk { table, state } => {
                for (entry, (acc, idx)) in table.entries.iter().zip(state.iter_mut()) {
                    *acc += dt;
                    let frame = &entry.frames[*idx];
                    if *acc < u32::from(frame.hold_vsyncs) {
                        continue;
                    }
                    *acc = 0;
                    // The retail 16x1 CLUT-cell MoveImage (libgpu FUN_80058490).
                    vram.move_image(
                        frame.src_x,
                        frame.src_y,
                        clut_walk::COPY_WIDTH,
                        1,
                        entry.dest_x,
                        entry.dest_y,
                    );
                    *idx = (*idx + 1) % entry.frames.len();
                    wrote = true;
                }
            }
            Self::Ocean { frames, cur, accum } => {
                let nframes = frames.len() / 32;
                if nframes == 0 {
                    return false;
                }
                *accum += dt;
                if *accum >= OCEAN_ANIM_VSYNCS_PER_FRAME {
                    *accum = 0;
                    *cur = (*cur + 1) % nframes;
                    let off = *cur * 32;
                    vram.write_clut_row(
                        OCEAN_HEAD_CLUT.0,
                        OCEAN_HEAD_CLUT.1,
                        &frames[off..off + 32],
                    );
                    wrote = true;
                }
            }
        }
        wrote
    }
}

fn park(vram: &mut Vram, strips: &[ParkStrip]) {
    for s in strips {
        vram.write_block(s.fb_x, s.fb_y, s.w, s.h, &s.data);
    }
}

/// Every walker source cell with no VRAM data, each cell once.
fn missing_source_cells(table: &ClutWalkTable, vram: &Vram) -> Vec<(u16, u16)> {
    let mut missing: Vec<(u16, u16)> = Vec::new();
    for e in &table.entries {
        for f in &e.frames {
            if !vram.region_has_data(
                f.src_x as usize,
                f.src_y as usize,
                clut_walk::COPY_WIDTH as usize,
                1,
            ) && !missing.contains(&(f.src_x, f.src_y))
            {
                missing.push((f.src_x, f.src_y));
            }
        }
    }
    missing
}

/// Park the Drake kingdom's strips for every source row still blank, and
/// return the cells still blank after that.
fn park_drake_complement(
    table: &ClutWalkTable,
    index: &ProtIndex,
    vram: &mut Vram,
) -> Vec<(u16, u16)> {
    let missing = missing_source_cells(table, vram);
    if missing.is_empty() {
        return missing;
    }
    // PROT 0086, not 0085 - see `legaia_asset::kingdom_bundle::BUNDLE_ENTRIES`.
    let drake = legaia_asset::kingdom_bundle::BUNDLE_ENTRIES[0];
    if let Ok(bytes) = index.entry_bytes(drake)
        && let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&bytes, 0)
    {
        let rows: Vec<u16> = missing.iter().map(|&(_, y)| y).collect();
        let strips: Vec<ParkStrip> = clut_walk::park_strips(&slot0)
            .into_iter()
            .filter(|s| rows.contains(&s.fb_y))
            .collect();
        park(vram, &strips);
    }
    missing_source_cells(table, vram)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::clut_walk::{ClutWalkEntry, ClutWalkFrame};

    fn one_entry(hold: u8) -> ClutWalkAnim {
        ClutWalkAnim::walk(ClutWalkTable {
            entries: vec![ClutWalkEntry {
                kind: 0,
                cumulative_size: 0,
                dest_x: 0,
                dest_y: 500,
                frames: vec![
                    ClutWalkFrame {
                        src_x: 16,
                        src_y: 498,
                        hold_vsyncs: hold,
                    },
                    ClutWalkFrame {
                        src_x: 32,
                        src_y: 498,
                        hold_vsyncs: hold,
                    },
                ],
            }],
        })
    }

    /// The seeded accumulator fires on the first game tick, and after that a
    /// hold of 8 at dt 3 fires every third game tick (reset, not
    /// subtract-remainder).
    #[test]
    fn walker_fires_at_entry_then_on_a_reset_interval() {
        let mut vram = Vram::new();
        let mut anim = one_entry(8);
        let fired: Vec<bool> = (0..7).map(|_| anim.game_tick(3, &mut vram)).collect();
        assert_eq!(fired, [true, false, false, true, false, false, true]);
    }
}
