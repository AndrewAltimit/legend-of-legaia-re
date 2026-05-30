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
//! - **Faithful (translation):** the per-part *position* is decoded from the
//!   PROT 0900 render overlay (phase A, `0x801F82A0`): when the keyframe gate
//!   `*(i16)(actor+0x9C) == *(i16)(actor+0x9E)` holds, the world position is
//!   overwritten by the move-VM anim-bank slots (`anim_3c/3e/40`, op `0x00`,
//!   `v << 3`) and `+0x9E` is cleared. [`tick`](SummonScene::tick) applies that
//!   latch (the anim banks are summon-local, so [`SummonScene::origin`] is
//!   added). This is why a part animates off the spawn point with no `WORLD_ADD`
//!   op — its motion lives in the anim banks.
//! - **Interpreted (rotation):** the part's *orientation*. Retail composes it in
//!   PROT 0900 with `RotMatrixX/Y/Z` (a per-part local rotation plus the camera
//!   angles `_DAT_8007B790/2/4`, gated per axis) over the part hierarchy. The
//!   exact actor→render-node rotation source isn't pinned yet, so
//!   [`SummonScene::part_draws`] derives Euler angles from the move-VM rotation
//!   banks as the engine's interpretation — the remaining open piece.

use legaia_asset::summon_overlay::{SummonOverlay, SummonPart};
use legaia_engine_vm::move_vm::{self, ActorState, ActorTickOutcome, MoveHost};

/// Per-frame opcode budget for one part's move-VM tick (defensive cap; retail
/// has no explicit limit but breaks on WAIT / HALT / end-of-buffer).
pub const SUMMON_PART_BUDGET: usize = 256;

/// Player Seru-magic spell-id range that resolves to a per-summon overlay at
/// the battle-action cast band (`FUN_801E295C` state `0x29`: `actor[+0x1DF] >=
/// 0x81`). Gimard *Tail Fire* = `0x81`.
pub const SERU_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x81..=0x8B;

/// PROT entry holding the per-summon stager overlay for a Seru-magic `spell_id`,
/// or `None` if `spell_id` is not a summon. Retail: `FUN_8003EC70(id - 0x79)`
/// loads PROT `(id - 0x79) + 0x381`, i.e. `0x81..=0x8B → 905..=915`.
pub fn summon_stager_prot_entry(spell_id: u8) -> Option<u32> {
    SERU_SUMMON_IDS
        .contains(&spell_id)
        .then(|| 905 + (spell_id - 0x81) as u32)
}

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
    ///
    /// After the move-VM step, applies the **render-side translation latch**
    /// decoded from the PROT 0900 render overlay (phase A at `0x801F82A0`):
    /// when the keyframe gate `*(i16)(actor+0x9C) == *(i16)(actor+0x9E)` holds,
    /// the part's world position is **overwritten** by the move-VM anim-bank
    /// slots (`anim_3c/3e/40`, set by op `0x00 ANIM_BANK_SET` as `v << 3`) and
    /// `+0x9E` is cleared (`sh zero, 0x9E(s0)`). The anim banks are summon-local
    /// offsets, so the engine adds [`Self::origin`] to place the part at the
    /// cast target. This is why a summon part animates off the spawn point even
    /// though no `WORLD_ADD` op runs — its motion lives in the anim banks.
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
            apply_translation_latch(&mut part.state, self.origin);
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

/// Render-side translation latch — port of the PROT 0900 render overlay's
/// phase A (`0x801F82A0`):
///
/// ```text
///   lh v1, 0x9c(s0) ; lh v0, 0x9e(s0) ; bne v1, v0, skip
///   lhu (anim_3c/3e/40/42) ; sh -> 0x14/0x16/0x18/0x1a(s0) ; sh zero, 0x9e(s0)
/// ```
///
/// When the keyframe gate holds, the part's world position is overwritten by
/// the anim-bank slots (op `0x00`, summon-local), then `+0x9E` is cleared. The
/// anim banks are local offsets, so `origin` (the cast target) is added to seat
/// the part in world space — the engine renders parts directly (no parent
/// transform), where retail places the whole summon via the camera/parent.
fn apply_translation_latch(state: &mut ActorState, origin: [i16; 3]) {
    // Retail reads `*(i16)(actor+0x9C)` (low half of the i32 field_9c) and
    // `*(i16)(actor+0x9E)` (field_9e).
    let gate_a = (state.field_9c & 0xFFFF) as i16;
    let gate_b = state.field_9e as i16;
    if gate_a == gate_b {
        state.world_x = origin[0].wrapping_add(state.anim_3c);
        state.world_y = origin[1].wrapping_add(state.anim_3e);
        state.world_z = origin[2].wrapping_add(state.anim_40);
        state.world_y_mirror = state.world_y;
        state.field_9e = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::summon_overlay::{SummonOverlay, SummonPart};

    /// A synthetic overlay: one transform node + one mesh part with a tiny
    /// move-VM program (`0x00 ANIM_BANK_SET 1,2,3` then `0x08 HALT`) — the
    /// anim banks are the per-part position the render-side latch reads.
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
        // record 1 @ next: model_sel = 0 (mesh), program: ANIM_BANK_SET 1,2,3 ; HALT.
        // Op 0x00 sets anim_3c/3e/40 = v << 3 -> (8, 16, 24).
        let r1 = bytes.len();
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0x00u16.to_le_bytes()); // ANIM_BANK_SET
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
    fn tick_latches_anim_banks_into_world_pos_plus_origin() {
        let (bytes, ov) = synthetic();
        let mut scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
        let mut host = H;
        scene.tick(&mut host, 0x1000);
        // The mesh part ran ANIM_BANK_SET 1,2,3 (-> anim = 8,16,24). With the
        // keyframe gate held (field_9c == field_9e == 0), the render-side latch
        // overwrites world pos with origin + anim bank.
        let mesh = scene.parts.iter().find(|p| p.model_sel == 0).unwrap();
        assert_eq!(
            (mesh.state.anim_3c, mesh.state.anim_3e, mesh.state.anim_40),
            (8, 16, 24),
            "op 0x00 set the anim banks to v << 3"
        );
        assert_eq!(
            (mesh.state.world_x, mesh.state.world_y, mesh.state.world_z),
            (108, 216, 324),
            "latch: world = origin + anim bank"
        );
        // Both tiny programs HALT on the first frame.
        scene.tick(&mut host, 0x1000);
        assert!(scene.finished(), "both parts halted");
    }

    #[test]
    fn latch_holds_part_at_origin_when_anim_banks_are_zero() {
        // The transform-node part (no anim ops) stays at the origin after the
        // latch (origin + 0).
        let (bytes, ov) = synthetic();
        let mut scene = SummonScene::spawn(&ov, &bytes, 26, [40, 50, 60]);
        let mut host = H;
        scene.tick(&mut host, 0x1000);
        let node = &scene.parts[0];
        assert_eq!(
            (node.state.world_x, node.state.world_y, node.state.world_z),
            (40, 50, 60)
        );
    }

    #[test]
    fn summon_prot_entry_maps_the_seru_block() {
        assert_eq!(summon_stager_prot_entry(0x81), Some(905)); // Gimard Tail Fire
        assert_eq!(summon_stager_prot_entry(0x8B), Some(915)); // last player summon
        assert_eq!(summon_stager_prot_entry(0x80), None); // below the block
        assert_eq!(summon_stager_prot_entry(0x8C), None); // above the block
        assert_eq!(summon_stager_prot_entry(0x27), None); // a monster attack id
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
