//! CLUT cell and CLUT blend effects: the mode-3 cell cycler and the blend ramp,
//! written through to the software VRAM.

use super::*;
use legaia_engine_field::clut_fx::{
    CLUT_CELL_ENTRIES, ClutCellFxOp, ClutFade, ClutFadeStep, flat_b_row,
};

// --- scripted CLUT-cell effects (field-VM 0x4C n6 sub-0x61) ----------------

/// Execution phase of one scripted CLUT-cell effect.
#[derive(Debug, Clone)]
pub enum ClutCellFxPhase {
    /// `frames == 0` one-shot (`FUN_801E4C58` inline path): apply the cell
    /// copy / flat fill on the next [`World::step_clut_fx`] and retire.
    Immediate,
    /// `frames != 0` fade, spawned but not yet initialised: the first game
    /// tick `StoreImage`s cells A + B out of the host VRAM and steps once.
    Pending,
    /// In-flight cross-fade.
    Running(ClutFade),
}

/// One live scripted CLUT-cell effect - the engine's analogue of the retail
/// fade actor (descriptor `DAT_801F2918`) / inline one-shot.
///
/// PORT: FUN_801E4C58
#[derive(Debug, Clone)]
pub struct ClutCellFx {
    pub op: ClutCellFxOp,
    pub phase: ClutCellFxPhase,
}

/// One live single-source CLUT blend fade - the actor field-VM `4C DB`
/// spawns (`FUN_801E57F0`: descriptor `0x801F2930`, handler `0x801E4D8C`,
/// record pointer at `+0x90`). `fade` is `None` until the first game tick
/// loads the cell out of VRAM (the handler's `+0x54 == 0` init block).
#[derive(Debug, Clone)]
pub struct ClutBlendFx {
    pub record: legaia_engine_field::clut_fade_actor::FadeRecord,
    pub fade: Option<legaia_engine_field::clut_fade_actor::ClutBlendFade>,
}

/// Read one 16x1 CLUT cell out of software VRAM (the `StoreImage`
/// equivalent).
fn read_cell(vram: &legaia_tim::Vram, x: i16, y: i16) -> [u16; CLUT_CELL_ENTRIES] {
    let mut row = [0u16; CLUT_CELL_ENTRIES];
    for (i, r) in row.iter_mut().enumerate() {
        *r = vram.pixel(x as usize + i, y as usize);
    }
    row
}

/// Write one 16x1 CLUT cell into software VRAM (the `LoadImage` equivalent).
fn write_cell(vram: &mut legaia_tim::Vram, x: i16, y: i16, row: &[u16; CLUT_CELL_ENTRIES]) {
    let mut bytes = [0u8; CLUT_CELL_ENTRIES * 2];
    for (i, v) in row.iter().enumerate() {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }
    vram.write_clut_row(x as u16, y as u16, &bytes);
}

/// Apply the shared completion / one-shot write: `MoveImage` cell B onto the
/// destination, or flat-fill the destination with `B.x` when `B.y == 0`
/// (both `FUN_801E4C58`'s inline path and `FUN_801E4794`'s completion arm).
fn apply_cell_write(vram: &mut legaia_tim::Vram, op: &ClutCellFxOp) {
    let row = if op.b_is_flat() {
        [op.b.0 as u16; CLUT_CELL_ENTRIES]
    } else {
        read_cell(vram, op.b.0, op.b.1)
    };
    write_cell(vram, op.dest.0, op.dest.1, &row);
}

impl World {
    /// Spawn a scripted CLUT-cell effect from the field-VM `0x4C` n6
    /// sub-`0x61` operand payload (the `spawn_clut_cell_fx` host hook).
    /// `frames == 0` queues the retail inline one-shot; `frames != 0` queues
    /// a cross-fade (the retail fade-actor spawn). Both are applied against
    /// the host's software VRAM by [`Self::step_clut_fx`].
    ///
    /// PORT: FUN_801E4C58
    pub fn spawn_clut_cell_fx(&mut self, payload: &[u8; 14]) {
        let op = ClutCellFxOp::from_payload(payload);
        let phase = if op.frames == 0 {
            ClutCellFxPhase::Immediate
        } else {
            ClutCellFxPhase::Pending
        };
        self.ambient.clut_fx.push(ClutCellFx { op, phase });
    }

    /// Spawn the single-source CLUT blend fade from field-VM `4C DB`'s
    /// operand stream (`record` starts at the `0xDB` byte, the pointer
    /// retail stores at actor `+0x90`; offsets `+1..=+0xB` are the record
    /// [`legaia_engine_field::clut_fade_actor::FadeRecord`] decodes).
    /// Stepped against the host's software VRAM by [`Self::step_clut_fx`].
    /// A record shorter than twelve bytes is dropped.
    ///
    /// PORT: FUN_801E57F0 (the spawn: descriptor `0x801F2930` through the
    /// actor allocator, then `+0x90 = record`, `+0x54 = 0`)
    pub fn spawn_clut_blend_fx(&mut self, record: &[u8]) {
        if record.len() < 12 {
            return;
        }
        let record = legaia_engine_field::clut_fade_actor::FadeRecord::from_bytes(record);
        self.ambient
            .clut_blend_fx
            .push(ClutBlendFx { record, fade: None });
    }

    /// Drive the live scripted CLUT-cell effects against `vram` (the host's
    /// software VRAM - play-window's `cpu_vram_base`, a test's scratch
    /// [`legaia_tim::Vram`]). One-shots apply immediately; fades consume the
    /// retail game ticks [`World::tick`] accumulated since the last call,
    /// each advancing the fade by [`legaia_engine_vm::frame_clock::FrameClock::frame_step`] vsyncs (the retail
    /// `counter += DAT_1F800393` cadence) and writing the interpolated row to
    /// the destination cell. A completed fade performs the final cell-B copy
    /// / flat fill and clears the script context's halt bit, matching the
    /// retail teardown (`*(ctx+0x94)+0x10 &= ~0x400`; the engine's single
    /// [`Self::field_ctx`] stands in for the spawning context).
    ///
    /// Returns `true` when any VRAM word changed (the host re-uploads its
    /// GPU copy). No-op (and drains the tick backlog) when no effects are
    /// live.
    ///
    /// PORT: FUN_801E4794
    pub fn step_clut_fx(&mut self, vram: &mut legaia_tim::Vram) -> bool {
        let ticks = std::mem::take(&mut self.ambient.clut_pending_game_ticks);
        if self.ambient.clut_fx.is_empty() && self.ambient.clut_blend_fx.is_empty() {
            return false;
        }
        let dt = self.clock.frame_step.max(1);
        let mut wrote = self.step_clut_blend_fx(vram, ticks, dt);
        let mut clear_halt = false;
        let mut still: Vec<ClutCellFx> = Vec::new();
        for fx in std::mem::take(&mut self.ambient.clut_fx) {
            let ClutCellFx { op, phase } = fx;
            let mut fade = match phase {
                ClutCellFxPhase::Immediate => {
                    apply_cell_write(vram, &op);
                    wrote = true;
                    continue;
                }
                ClutCellFxPhase::Pending => None,
                ClutCellFxPhase::Running(f) => Some(f),
            };
            let mut done = false;
            for _ in 0..ticks {
                let f = fade.get_or_insert_with(|| {
                    // First game tick: StoreImage cells A + B (a flat B
                    // synthesises its row from the flat colour + A's STP
                    // bits) and precompute the per-channel deltas.
                    let a_row = read_cell(vram, op.a.0, op.a.1);
                    let b_row = if op.b_is_flat() {
                        flat_b_row(&a_row, op.b.0 as u16)
                    } else {
                        read_cell(vram, op.b.0, op.b.1)
                    };
                    ClutFade::new(&a_row, &b_row, op.frames)
                });
                match f.step(dt) {
                    ClutFadeStep::Row(row) => {
                        write_cell(vram, op.dest.0, op.dest.1, &row);
                        wrote = true;
                    }
                    ClutFadeStep::Done => {
                        apply_cell_write(vram, &op);
                        wrote = true;
                        clear_halt = true;
                        done = true;
                        break;
                    }
                }
            }
            if !done {
                let phase = match fade {
                    Some(f) => ClutCellFxPhase::Running(f),
                    None => ClutCellFxPhase::Pending,
                };
                still.push(ClutCellFx { op, phase });
            }
        }
        self.ambient.clut_fx = still;
        if clear_halt {
            self.field_ctx.flags &= !0x400;
        }
        wrote
    }

    /// The `4C DB` blend fades' share of [`Self::step_clut_fx`]: per game
    /// tick, load the cell on the first one, advance the accumulator by `dt`
    /// vsyncs and write the returned row back to the same cell; a
    /// [`legaia_engine_field::clut_fade_actor::FadeStep::Done`] row is
    /// written and the fade retires (retail sets `+0x10 |= 8`).
    ///
    /// REF: FUN_801E4D8C (the per-tick handler the kernel ports)
    fn step_clut_blend_fx(&mut self, vram: &mut legaia_tim::Vram, ticks: u32, dt: u8) -> bool {
        use legaia_engine_field::clut_fade_actor::{ClutBlendFade, FadeStep};
        let mut wrote = false;
        let mut still = Vec::new();
        for mut fx in std::mem::take(&mut self.ambient.clut_blend_fx) {
            let (cx, cy) = fx.record.cell;
            let mut done = false;
            for _ in 0..ticks {
                let record = fx.record;
                let fade = fx
                    .fade
                    .get_or_insert_with(|| ClutBlendFade::new(&read_cell(vram, cx, cy), &record));
                match fade.tick(dt) {
                    FadeStep::Row(row) => {
                        write_cell(vram, cx, cy, &row);
                        wrote = true;
                    }
                    FadeStep::Done(row) => {
                        write_cell(vram, cx, cy, &row);
                        wrote = true;
                        done = true;
                        break;
                    }
                }
            }
            if !done {
                still.push(fx);
            }
        }
        self.ambient.clut_blend_fx = still;
        wrote
    }
}
