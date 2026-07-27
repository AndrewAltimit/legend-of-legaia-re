//! Ambient field-effect parts - the scene-entry move-VM effect tree.
//!
//! Retail stages a scene's ambience through the prescript stager bundle: at
//! scene entry each MAN partition-1 **effect-actor** script (a dedicated
//! `install id N` + infinite-loop record) installs one stager record via
//! field-VM op `0x34` sub-3 → `FUN_800252EC` → the part stager
//! `FUN_80021B04`, which runs the record's move-VM bytecode **once
//! immediately** and then every game tick. Installer records fan out with
//! op `0x25` child spawns (each child also first-run at spawn), building the
//! scene's ambient tree - for jou: the lightning director, fifteen CLUT-cell
//! cyclers tiling the flesh palette row, the flag-gated lightning palette,
//! and the ambient SFX loop. See `docs/subsystems/field-ambient-fx.md`.
//!
//! Two properties force this path off the [`crate::summon::SummonScene`]
//! stand-in and onto the shared bundle:
//!
//! 1. **In-place self-modification.** The spawn fan-out relies on ext op
//!    `0x1E` patching the shared bundle between spawns (jou's cycler record
//!    steps its own op-`0x2C` capture `x` by 16 per instance, tiling the
//!    CLUT row). Parts therefore snapshot their bytecode **at spawn time**
//!    from the live bundle, and every tick's deferred bytecode writes are
//!    flushed back into both the bundle and the part's own snapshot.
//!    (Within a single tick an op still reads its operands from the
//!    snapshot, so a self-write lands one instruction late relative to
//!    retail's direct memory writes - for the spawn-stepping idiom this
//!    shifts which 16-halfword cell each instance captures by one step, an
//!    accepted engine divergence recorded in the subsystem doc.)
//! 2. **The mode-3 render tail.** `0x4000` render-mode parts run the
//!    per-frame CLUT-cell integrator (`FUN_80021DF4` mode-3 arm) and emit
//!    HSV palette rewrites ([`crate::clut_cell_fx`]), which need a VRAM
//!    surface - the renderer drives [`World::step_ambient_fx`] with its
//!    software VRAM exactly like the scripted-CLUT sibling
//!    [`World::step_clut_fx`].
//!
//! PORT: FUN_80021B04 (spawn-time first run + per-tick move-VM drive for
//! the prescript-record parts)
//! REF: FUN_800252EC, FUN_80021DF4, FUN_80019D50

use super::*;
use crate::clut_cell_fx::{self, ClutCellFx};
use legaia_engine_vm::move_vm::{self, ActorState, ActorTickOutcome};

/// Per-tick opcode budget for one ambient part (the records are small; the
/// budget only guards a malformed stream).
const AMBIENT_PART_BUDGET: usize = 512;

/// Recursion guard for op-`0x25` spawn chains within one tick.
const MAX_SPAWN_DEPTH: usize = 16;

/// Cap on simultaneously live ambient parts (retail's effect-actor pool is
/// far smaller; this only bounds a runaway spawn loop).
const MAX_AMBIENT_PARTS: usize = 128;

/// One live ambient move-VM part.
#[derive(Debug, Clone)]
pub struct AmbientPart {
    /// Byte offset of the part's record inside the shared stager bundle
    /// (`World::field_stager_bytes`) - the base `move_bytecode_*` word
    /// offsets translate against.
    pub record_off: usize,
    /// `record[+0]` mesh selector (`-1` transform node, `0x4000`/`0x4001`
    /// render-mode nodes, `>= 0` library mesh).
    pub model_sel: i16,
    /// `record[+2]` flags word.
    pub flags: u16,
    /// Snapshot of the record's u16 words (header + bytecode), taken from
    /// the live bundle at spawn time and kept in sync with flushed
    /// bytecode writes.
    pub buf: Vec<u16>,
    /// Move-VM actor state (PC in u16 units over `buf`).
    pub state: ActorState,
    /// Set once the part halts / runs off its buffer.
    pub finished: bool,
    /// Latest mode-3 CLUT-cell write this part emitted (refreshed per game
    /// tick; applied to VRAM by [`World::step_ambient_fx`]).
    pub cell_fx: Option<ClutCellFx>,
    /// Morph-lane weight snapshot from the previous tick (change detection
    /// for the VDF render substitution's dirty set).
    pub prev_morph_weights: Vec<u16>,
}

/// A mesh-bearing ambient part with armed VDF morph lanes - the input the
/// render substitution consumes ([`World::current_morph_deltas`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbientMorphPart {
    /// Scene environment-pack slot the part's mesh binds
    /// (`model_sel - 5`: the retail global-TMD table keeps the 5
    /// character meshes ahead of the scene pack, `DAT_8007B6F8`).
    pub pack_slot: usize,
    /// `(vdf_sub_entry_index, weight)` per armed lane.
    pub lanes: Vec<(u8, u16)>,
}

impl World {
    /// Spawn prescript stager record `id` as an ambient part at `origin`
    /// and run its first move-VM slice immediately (the `FUN_80021B04`
    /// spawn-time run - op-`0x25` children spawn recursively, each with its
    /// own immediate first run). Returns `false` when the id is out of
    /// range or no stager table is installed.
    pub fn spawn_ambient_record(&mut self, id: usize, origin: [i16; 3]) -> bool {
        let Some(idx) = self.push_ambient_part(id, origin) else {
            return false;
        };
        self.tick_ambient_part(idx, 0);
        true
    }

    /// Seat record `id` as a new ambient part (no first run). Returns the
    /// part index.
    fn push_ambient_part(&mut self, id: usize, origin: [i16; 3]) -> Option<usize> {
        if self.ambient_fx.len() >= MAX_AMBIENT_PARTS {
            return None;
        }
        let rec = self.field_stagers.get(id)?;
        let (record_off, end) = (rec.record_off, rec.bytecode.end);
        let bytes = self.field_stager_bytes.get(record_off..end)?;
        let buf: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let mut state = ActorState::new();
        // FUN_80021B04 stages PC = 2 (bytecode starts past [model_sel][flags])
        // and seats the part at the spawner's world position.
        state.pc = 2;
        state.world_x = origin[0];
        state.world_y = origin[1];
        state.world_z = origin[2];
        state.world_y_mirror = origin[1];
        state.wait_timer = -1;
        self.ambient_fx.push(AmbientPart {
            record_off,
            model_sel: rec.model_sel,
            flags: rec.flags,
            buf,
            state,
            finished: false,
            cell_fx: None,
            prev_morph_weights: Vec::new(),
        });
        Some(self.ambient_fx.len() - 1)
    }

    /// Tick one ambient part: drain the wait timer, run the move VM, flush
    /// bytecode self-writes into the shared bundle + the part's snapshot,
    /// spawn op-`0x25` children (recursively, spawn-time first run), and
    /// run the mode-3 CLUT-cell integrator on `0x4000` render-mode parts.
    fn tick_ambient_part(&mut self, idx: usize, depth: usize) {
        if depth > MAX_SPAWN_DEPTH {
            return;
        }
        let Some(part) = self.ambient_fx.get(idx) else {
            return;
        };
        if part.finished {
            return;
        }
        let mut state = part.state.clone();
        let buf = part.buf.clone();
        let record_words = part.record_off / 2;
        let model_sel = part.model_sel;

        let mut host = MoveVmHostImpl {
            world: self,
            current_slot: None,
            deferred_writes: std::collections::BTreeMap::new(),
            field_record_words: Some(record_words),
            child_spawns: Vec::new(),
        };
        let outcome = move_vm::actor_tick(&mut host, &mut state, &buf, AMBIENT_PART_BUDGET);
        let writes = std::mem::take(&mut host.deferred_writes);
        let spawns = std::mem::take(&mut host.child_spawns);
        drop(host);

        // Flush self-modifying writes into the live bundle (retail writes
        // `_DAT_8007B8D0` memory directly) and this part's own snapshot.
        for (word, value) in writes {
            let byte = (record_words + word) * 2;
            if let Some(b) = self.field_stager_bytes.get_mut(byte..byte + 2) {
                b.copy_from_slice(&value.to_le_bytes());
            }
            if let Some(slot) = self.ambient_fx[idx].buf.get_mut(word) {
                *slot = value;
            }
        }

        let finished = matches!(
            outcome,
            ActorTickOutcome::Halted | ActorTickOutcome::EndOfBuffer { .. }
        );

        // Morph-lane ramp envelope: retail's per-frame actor tick runs
        // `FUN_80020740` on parts whose `+0x10` flag word carries the
        // envelope-active bit `0x1000` (the bit op `0x0A` sets - see
        // `move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE` and the
        // `cursor_advance` gate). This is what moves the `+0xA0` lane
        // weights the VDF render substitution reads back.
        if state.flags & legaia_engine_vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE != 0 {
            legaia_engine_vm::vdf_morph::envelope_tick_actor(&mut state, self.frame_step.max(1));
        }

        // Mode-3 render tail: the `0x4000` render-mode node's CLUT-cell
        // integrator runs every game tick regardless of the wait timer.
        let cell_fx = if model_sel == legaia_asset::summon_overlay::RENDER_NODE_MODE_A {
            clut_cell_fx::mode3_integrate(&mut state, self.frame_step.max(1))
        } else {
            None
        };

        // VDF morph dirty tracking: when an armed part's lane weights moved
        // this tick, its mesh's `(pack_slot, group)` pairs need a re-stage.
        let mut dirty: Vec<(usize, u32)> = Vec::new();
        let mut new_weights: Option<Vec<u16>> = None;
        if state.flags & legaia_engine_vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE != 0
            && let Some(pack_slot) = pack_slot_of_model_sel(model_sel)
        {
            let lanes = legaia_engine_vm::vdf_morph::actor_morph_lanes(&state);
            let weights: Vec<u16> = lanes.iter().map(|&(_, w)| w).collect();
            if self.ambient_fx[idx].prev_morph_weights != weights {
                for &(vdf_idx, _) in &lanes {
                    if let Some(entry) = self.vdf_record_bytes(vdf_idx) {
                        for rec in legaia_engine_vm::vdf_morph::parse_vdf_morph_records(entry) {
                            if !dirty.contains(&(pack_slot, rec.group_id)) {
                                dirty.push((pack_slot, rec.group_id));
                            }
                        }
                    }
                }
                new_weights = Some(weights);
            }
        }
        self.morph_dirty_slots.extend(dirty);

        {
            let part = &mut self.ambient_fx[idx];
            part.state = state;
            part.finished = finished;
            if cell_fx.is_some() {
                part.cell_fx = cell_fx;
            }
            if let Some(w) = new_weights {
                part.prev_morph_weights = w;
            }
        }

        // Spawn-time first run for each op-0x25 child, in spawn order (the
        // retail chain runs the child's VM inside the parent's spawn op,
        // which is what sequences the self-modifying fan-outs).
        for (slot, origin) in spawns {
            if slot < 0 {
                continue;
            }
            if let Some(child) = self.push_ambient_part(slot as usize, origin) {
                self.tick_ambient_part(child, depth + 1);
            }
        }
    }

    /// Advance every live ambient part one retail game tick.
    pub fn tick_ambient_fx(&mut self) {
        // The wait-timer drain per game tick: retail decrements `+0x54` by
        // `DAT_1F800393 * DAT_1F80037D` (frame step x the pinned 0x10 speed
        // scalar) per tick.
        let drain = u16::from(self.frame_step.max(1)) * clut_cell_fx::SPEED_SCALAR as u16;
        let count = self.ambient_fx.len();
        for idx in 0..count {
            if self.ambient_fx[idx].finished {
                continue;
            }
            move_vm::decrement_wait_timer(&mut self.ambient_fx[idx].state, drain);
            self.tick_ambient_part(idx, 0);
        }
        // The scene-entry VDF pulse (enhancement - `crate::vdf_pulse`) rides
        // the same ambient game tick.
        let step = self.frame_step.max(1);
        if let Some(pulse) = self.entry_vdf_pulse.as_mut() {
            let dirty = pulse.tick(step);
            self.morph_dirty_slots.extend(dirty);
        }
    }

    /// Every live mesh-bearing ambient part with the morph envelope armed
    /// (`+0x10` bit `0x1000`, op `0x0A`) - the retail VDF morph carriers
    /// (town0e's flesh lumps, rikuroa's generator sacs).
    pub fn ambient_morph_parts(&self) -> Vec<AmbientMorphPart> {
        self.ambient_fx
            .iter()
            .filter(|p| {
                !p.finished
                    && p.state.flags & legaia_engine_vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE
                        != 0
            })
            .filter_map(|p| {
                let pack_slot = pack_slot_of_model_sel(p.model_sel)?;
                let lanes = legaia_engine_vm::vdf_morph::actor_morph_lanes(&p.state);
                (!lanes.is_empty()).then_some(AmbientMorphPart { pack_slot, lanes })
            })
            .collect()
    }

    /// Summed weighted VDF deltas for `n_verts` vertices of TMD group
    /// `group` under `lanes` (`(sub_entry_index, weight)` pairs resolved
    /// through the scene VDF buffer - retail's `0x80083E58` walk +
    /// `FUN_8005B038` blend). Lanes whose index doesn't resolve are
    /// skipped, matching the retail bail-through.
    pub fn morph_deltas_for(
        &self,
        lanes: &[(u8, u16)],
        group: u32,
        n_verts: usize,
    ) -> Vec<[i16; 3]> {
        let slots: Vec<(&[u8], i16)> = lanes
            .iter()
            .filter_map(|&(idx, w)| Some((self.vdf_record_bytes(idx)?, w as i16)))
            .collect();
        legaia_engine_vm::vdf_morph::sum_group_deltas(n_verts, group, &slots)
    }

    /// The combined live morph deltas for `(pack_slot, group)` - retail
    /// armed ambient parts plus the scene-entry pulse. `None` when no lane
    /// targets the pair (the mesh draws its rest pose).
    pub fn current_morph_deltas(
        &self,
        pack_slot: usize,
        group: u32,
        n_verts: usize,
    ) -> Option<Vec<[i16; 3]>> {
        let mut lanes: Vec<(u8, u16)> = Vec::new();
        for part in self.ambient_morph_parts() {
            if part.pack_slot == pack_slot {
                lanes.extend(part.lanes);
            }
        }
        if let Some(pulse) = self.entry_vdf_pulse.as_ref() {
            lanes.extend(pulse.lanes_for(pack_slot, group));
        }
        if lanes.is_empty() {
            return None;
        }
        Some(self.morph_deltas_for(&lanes, group, n_verts))
    }

    /// Drain the `(pack_slot, group)` pairs whose morph deltas changed
    /// since the last call - the renderer re-stages just those meshes'
    /// positions (`current_morph_deltas` + the authored vertices).
    pub fn take_morph_dirty_slots(&mut self) -> Vec<(usize, u32)> {
        let out: Vec<(usize, u32)> = self.morph_dirty_slots.iter().copied().collect();
        self.morph_dirty_slots.clear();
        out
    }

    /// Install the scene-entry VDF pulse (enhancement) over the current
    /// scene's VDF buffer, targeting `pack_objects[pack_slot][group] =
    /// vertex_count`. No-ops (and returns `false`) when the entry-ambient
    /// tree already armed retail morph lanes - the pulse only stands in
    /// where retail's own entry ambience has no morph carrier (jou).
    pub fn install_entry_vdf_pulse(&mut self, pack_objects: &[Vec<usize>]) -> bool {
        self.entry_vdf_pulse = None;
        if !self.entry_pulse_enabled {
            return false;
        }
        if !self.ambient_morph_parts().is_empty() {
            return false;
        }
        // Stand aside for scenes where the stager table carries op-0x0A
        // morph arming in ANY story state (rikuroa's flag-gated records) -
        // retail owns those morphs; the pulse is only for packs retail
        // never arms from the ambient tree (jou).
        if crate::vdf_pulse::stager_records_arm_morphs(
            &self.field_stagers,
            &self.field_stager_bytes,
        ) {
            return false;
        }
        let Some(count) = self
            .vdf_buffer
            .as_deref()
            .filter(|b| b.len() >= 4)
            .map(|b| u32::from_le_bytes(b[0..4].try_into().unwrap()) as usize)
        else {
            return false;
        };
        let entries: Vec<&[u8]> = (0..count.min(u8::MAX as usize))
            .filter_map(|i| self.vdf_record_bytes(i as u8))
            .collect();
        self.entry_vdf_pulse = crate::vdf_pulse::EntryVdfPulse::build(&entries, pack_objects);
        self.entry_vdf_pulse.is_some()
    }

    /// Drain the banked game ticks and apply the live CLUT-cell effects to
    /// `vram`. Returns `true` when any texels changed (the caller re-uploads
    /// its GPU copy). The renderer-facing sibling of [`World::step_clut_fx`].
    pub fn step_ambient_fx(&mut self, vram: &mut legaia_tim::Vram) -> bool {
        let ticks = std::mem::take(&mut self.ambient_pending_game_ticks);
        if self.ambient_fx.is_empty() {
            return false;
        }
        for _ in 0..ticks {
            self.tick_ambient_fx();
        }
        let mut wrote = false;
        let fx: Vec<ClutCellFx> = self
            .ambient_fx
            .iter()
            .filter_map(|p| {
                (!p.finished || p.cell_fx.is_some())
                    .then_some(p.cell_fx)
                    .flatten()
            })
            .collect();
        for f in fx {
            let (x, y, w, h) = f.rect;
            if w == 0 || h == 0 || w > 256 || h > 64 {
                continue;
            }
            let src = self
                .ambient_cell_captures
                .entry(f.rect)
                .or_insert_with(|| read_rect(vram, x, y, w, h))
                .clone();
            let out = clut_cell_fx::apply_hsv_cell(&src, &f);
            let cur = read_rect(vram, x, y, w, h);
            if cur != out {
                write_rect(vram, x, y, w, &out);
                wrote = true;
            }
        }
        wrote
    }

    /// Snapshot of the live ambient CLUT-cell effects (for tests and the
    /// web viewer's status line).
    pub fn active_ambient_cell_fx(&self) -> Vec<ClutCellFx> {
        self.ambient_fx.iter().filter_map(|p| p.cell_fx).collect()
    }
}

/// Scene-pack slot a stager record's `model_sel` binds: the retail global
/// TMD table (`DAT_8007C018`) keeps the five character meshes ahead of the
/// scene pack (`DAT_8007B6F8 = 5`), so pack slot = `model_sel - 5`.
/// `None` for transform nodes (`-1`), the character meshes (`0..5`), and
/// the `0x4000`/`0x4001` render-mode sentinels.
fn pack_slot_of_model_sel(model_sel: i16) -> Option<usize> {
    (5..0x4000)
        .contains(&model_sel)
        .then(|| model_sel as usize - 5)
}

/// Read a `w x h` halfword rect out of the software VRAM.
fn read_rect(vram: &legaia_tim::Vram, x: u16, y: u16, w: u16, h: u16) -> Vec<u16> {
    let mut out = Vec::with_capacity(usize::from(w) * usize::from(h));
    for row in 0..h {
        for col in 0..w {
            out.push(vram.pixel(usize::from(x + col) & 0x3FF, usize::from(y + row) & 0x1FF));
        }
    }
    out
}

/// Write a `w`-wide halfword rect into the software VRAM.
fn write_rect(vram: &mut legaia_tim::Vram, x: u16, y: u16, w: u16, texels: &[u16]) {
    for (row, chunk) in texels.chunks(usize::from(w.max(1))).enumerate() {
        let bytes: Vec<u8> = chunk.iter().flat_map(|t| t.to_le_bytes()).collect();
        vram.write_block(x, y + row as u16, w, 1, &bytes);
    }
}
