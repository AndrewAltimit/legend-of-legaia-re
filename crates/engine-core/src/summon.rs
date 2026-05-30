//! Seru-magic **summon scene-graph driver**.
//!
//! A player Seru-magic cast spawns a hierarchy of move-VM-driven body parts (see
//! [`legaia_asset::summon_overlay`] for the on-disc part records). This module
//! drives them: it seeds one move-VM [`ActorState`] per part from its record and
//! ticks every part through the **already-ported move VM** each frame, exactly
//! as the retail spawn helper `FUN_80021B04` stages a part (`actor[+0x48]` =
//! record move-buffer base, `actor[+0x70] = 2` PC → bytecode at `record+4`) and
//! the per-frame actor tick `FUN_80021DF4` runs `FUN_80023070` on it.
//!
//! ## What is faithful vs. interpreted
//!
//! - **Faithful:** the per-part animation *computation*. Each part's move-VM
//!   program runs through [`legaia_engine_vm::move_vm`] verbatim — the same
//!   opcode handlers, wait-timer gate, and tween/anim-bank state the retail move
//!   VM produces. (Validated: every Gimard *Tail Fire* part record runs without
//!   hitting an unimplemented opcode.)
//! - **Interpreted:** turning each part's move-VM state into a *render
//!   transform*. Retail composes that in the per-summon render overlay (PROT
//!   0900: `RotMatrixX/Y/Z` + GTE prim emit over the part hierarchy), which is
//!   not yet decoded. [`SummonScene::part_draws`] derives a transform from the
//!   move-VM world position + rotation banks as the engine's best
//!   interpretation; it is clearly the open piece, not pinned parity.

use legaia_asset::summon_overlay::{SummonOverlay, SummonPart};
use legaia_engine_vm::move_vm::{self, ActorState, ActorTickOutcome, MoveHost};

/// Per-frame opcode budget for one part's move-VM tick (defensive cap; retail
/// has no explicit limit but breaks on WAIT / HALT / end-of-buffer).
pub const SUMMON_PART_BUDGET: usize = 256;

/// Upper bound on a `model_sel` that names a real mesh (`DAT_8007C018[model_sel
/// + base]`). The model library is small (~30 entries), so a part whose
/// `model_sel` is `-1` (transform node) or a large sentinel (`0x1000`, `0x4000`,
/// `0x4001` — special render-mode markers, per the summon-overlay decode) binds
/// no mesh. Mesh parts have `0 <= model_sel < MAX_MESH_SEL`.
pub const MAX_MESH_SEL: i16 = 0x100;

/// `true` when `model_sel` names a real library mesh (vs. a transform/pivot node
/// or a special-mode sentinel).
fn is_mesh_sel(model_sel: i16) -> bool {
    (0..MAX_MESH_SEL).contains(&model_sel)
}

/// Runtime state of one staged summon part.
#[derive(Debug, Clone)]
pub struct SummonPartRuntime {
    /// `record[+0]` mesh selector (`-1` = transform/pivot node).
    pub model_sel: i16,
    /// `record[+2]` flags.
    pub flags: u16,
    /// The part's move buffer (its record bytes as a u16 array; PC indexes this).
    pub buf: Vec<u16>,
    /// Live move-VM actor state.
    pub state: ActorState,
    /// `true` once the part's program halted or ran off its buffer.
    pub finished: bool,
}

/// A running summon: every spawned part plus the model-library base the parts'
/// mesh selectors index against.
#[derive(Debug, Clone)]
pub struct SummonScene {
    pub parts: Vec<SummonPartRuntime>,
    /// Pool index a part's `model_sel == 0` resolves to — retail
    /// `DAT_8007C018[model_sel + gp[0x754]]`; in the engine this is the offset
    /// into [`crate::world::World::global_tmd_pool`] (for Gimard, the fire
    /// mesh-set base [`crate::scene::GIMARD_TAIL_FIRE_MODEL_INDEX`]).
    pub model_base: usize,
    /// Summon origin in world units (the cast target).
    pub origin: [i16; 3],
    /// Frames ticked since spawn.
    pub frame: u32,
}

/// One mesh-bearing part's render draw. The transform is the engine's
/// interpretation of the move-VM state (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SummonPartDraw {
    /// Index into [`crate::world::World::global_tmd_pool`].
    pub model_index: usize,
    /// World position (move-VM `world_x/y/z`).
    pub world_pos: [f32; 3],
    /// Euler XYZ rotation in radians (from the move-VM rotation banks).
    pub rot: [f32; 3],
}

impl SummonScene {
    /// Spawn every part of a parsed summon overlay at `origin`, seeding each
    /// part's move-VM state from its record (PC = 2 → bytecode at `record+4`,
    /// mirroring `FUN_80021B04`). `record_bytes` is the stager overlay's raw
    /// bytes (e.g. PROT 0905), the same buffer [`SummonOverlay`] was parsed from.
    pub fn spawn(
        overlay: &SummonOverlay,
        record_bytes: &[u8],
        model_base: usize,
        origin: [i16; 3],
    ) -> Self {
        let parts = overlay
            .parts
            .iter()
            .filter_map(|p| seed_part(p, record_bytes, origin))
            .collect();
        Self {
            parts,
            model_base,
            origin,
            frame: 0,
        }
    }

    /// Advance every live part one frame through the move VM. `frame_delta` is
    /// the wait-timer drain (retail's per-actor anim-speed × frame-rate scalar
    /// product); a typical value keeps the parts on their authored timing.
    pub fn tick<H: MoveHost + ?Sized>(&mut self, host: &mut H, frame_delta: u16) {
        self.frame = self.frame.wrapping_add(1);
        for part in &mut self.parts {
            if part.finished {
                continue;
            }
            move_vm::decrement_wait_timer(&mut part.state, frame_delta);
            match move_vm::actor_tick(host, &mut part.state, &part.buf, SUMMON_PART_BUDGET) {
                ActorTickOutcome::Halted | ActorTickOutcome::EndOfBuffer { .. } => {
                    part.finished = true;
                }
                _ => {}
            }
        }
    }

    /// `true` once every part has halted / ended.
    pub fn finished(&self) -> bool {
        self.parts.iter().all(|p| p.finished)
    }

    /// Render draws for the mesh-bearing parts (`0 <= model_sel < MAX_MESH_SEL`). Transform
    /// nodes (`model_sel == -1`) carry no mesh and are skipped. The transform is
    /// the engine's interpretation (see module docs).
    pub fn part_draws(&self) -> Vec<SummonPartDraw> {
        // PSX 12-bit angle (4096 = 360°) → radians.
        const A: f32 = std::f32::consts::TAU / 4096.0;
        self.parts
            .iter()
            .filter(|p| is_mesh_sel(p.model_sel))
            .map(|p| {
                let s = &p.state;
                SummonPartDraw {
                    model_index: self.model_base.wrapping_add(p.model_sel as usize),
                    world_pos: [s.world_x as f32, s.world_y as f32, s.world_z as f32],
                    rot: [
                        (s.render_24 as f32) * A,
                        (s.y_rot.wrapping_add(s.render_26) as f32) * A,
                        (s.render_28 as f32) * A,
                    ],
                }
            })
            .collect()
    }

    /// Number of mesh-bearing parts (`is_mesh_sel`).
    pub fn mesh_part_count(&self) -> usize {
        self.parts
            .iter()
            .filter(|p| is_mesh_sel(p.model_sel))
            .count()
    }
}

/// Seed one part's runtime from its record. Returns `None` if the record offset
/// is past the buffer.
fn seed_part(p: &SummonPart, record_bytes: &[u8], origin: [i16; 3]) -> Option<SummonPartRuntime> {
    let rec = record_bytes.get(p.record_off..)?;
    let buf: Vec<u16> = rec
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut state = ActorState::new();
    // FUN_80021B04 stages the move buffer at PC = 2 (u16 units) → the bytecode
    // begins at record+4, just past [model_sel][flags].
    state.pc = 2;
    state.world_x = origin[0];
    state.world_y = origin[1];
    state.world_z = origin[2];
    state.world_y_mirror = origin[1];
    // Negative wait-timer so the gate runs the VM on the first frame.
    state.wait_timer = -1;
    Some(SummonPartRuntime {
        model_sel: p.model_sel,
        flags: p.flags,
        buf,
        state,
        finished: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::summon_overlay::{SummonOverlay, SummonPart};

    /// A synthetic overlay: one transform node + one mesh part with a tiny
    /// move-VM program (`0x01 WORLD_ADD 1,2,3` then `0x08 HALT`).
    fn synthetic() -> (Vec<u8>, SummonOverlay) {
        // Record layout: [i16 model_sel][u16 flags][u16 move-VM bytecode...].
        // Build two records back-to-back in one byte buffer.
        let mut bytes = Vec::new();
        // record 0 @ 0x00: model_sel = -1 (transform node)
        let r0 = bytes.len();
        bytes.extend_from_slice(&(-1i16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
        bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT
        bytes.extend_from_slice(&0u16.to_le_bytes()); // pad to even record
        // record 1 @ next: model_sel = 0 (mesh), program: WORLD_ADD 1,2,3 ; HALT
        let r1 = bytes.len();
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0x01u16.to_le_bytes()); // WORLD_ADD
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT

        let overlay = SummonOverlay {
            link_base: 0x801F_69D8,
            spawn_sites: 2,
            parts: vec![
                SummonPart {
                    record_off: r0,
                    model_sel: -1,
                    flags: 0,
                    bytecode: (r0 + 4)..r1,
                },
                SummonPart {
                    record_off: r1,
                    model_sel: 0,
                    flags: 0,
                    bytecode: (r1 + 4)..bytes.len(),
                },
            ],
        };
        (bytes, overlay)
    }

    struct H;
    impl MoveHost for H {}

    #[test]
    fn spawns_one_state_per_part() {
        let (bytes, ov) = synthetic();
        let scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
        assert_eq!(scene.parts.len(), 2);
        assert_eq!(scene.mesh_part_count(), 1, "one mesh part (model_sel >= 0)");
        // Each part seeded at the origin with PC = 2.
        for part in &scene.parts {
            assert_eq!(part.state.pc, 2);
            assert_eq!(part.state.world_x, 100);
        }
    }

    #[test]
    fn ticks_parts_through_the_move_vm_and_applies_world_add() {
        let (bytes, ov) = synthetic();
        let mut scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
        let mut host = H;
        scene.tick(&mut host, 0x1000);
        // The mesh part ran WORLD_ADD 1,2,3 then HALT.
        let mesh = scene.parts.iter().find(|p| p.model_sel == 0).unwrap();
        assert_eq!(
            (mesh.state.world_x, mesh.state.world_y, mesh.state.world_z),
            (101, 202, 303),
            "WORLD_ADD moved the part off the origin"
        );
        // Both tiny programs HALT on the first frame.
        scene.tick(&mut host, 0x1000);
        assert!(scene.finished(), "both parts halted");
    }

    #[test]
    fn part_draws_map_model_sel_against_the_base() {
        let (bytes, ov) = synthetic();
        let scene = SummonScene::spawn(&ov, &bytes, 26, [0, 0, 0]);
        let draws = scene.part_draws();
        assert_eq!(draws.len(), 1, "only the mesh part draws");
        assert_eq!(draws[0].model_index, 26, "model_sel 0 + base 26");
    }
}
