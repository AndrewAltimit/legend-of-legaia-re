//! Which PROT entry a battle is fought inside, resolved from the live scene
//! host - the one call both play hosts build the backdrop from.
//!
//! Retail does not look the backdrop up per scene. The region reader
//! `FUN_801D9E1C` stamps the stage variant `_DAT_8007BD60 = region[+8] &
//! 0x1F` for the region the player stands in - every step, and again on the
//! op-`0x3E` scripted-battle install - and battle init `FUN_800513F0` loads
//! entry `scene_index + variant` through `FUN_8001FA88`
//! ([`crate::region_encounter::battle_stage_entry_for_variant`]). So a scene
//! with several sub-area backdrops fights in whichever one the region names.

use super::SceneHost;
use legaia_asset::battle_backdrop::SecondCopy;

/// A backdrop's drawn objects split by whether the backdrop draw spins them.
///
/// `FUN_8001ADA4`'s backdrop arm (draw kind 3, `0x8001AF04`) walks the
/// actor's object table and, ahead of each object's prim walk, post-rotates
/// the composed matrix about Y through `FUN_8004629C` by a per-**slot** angle
/// read from the table at `0x800891C8` (`lh a0,0x2(s1)` at `0x8001AFEC` /
/// `0x8001B004`, `s1` stepping `8` a slot). Only slot 1's angle is ever
/// written ([`crate::world::World::tick_battle_backdrop_spin`]), so slot 1
/// turns and every other slot stands still. On nilboa's stage that slot is
/// the kept object 1, the horizon mist ribbon - an arc of twelve quads, not
/// a full ring, so where it stands is the angle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BattleStageLayers {
    /// The objects of every slot but 1, in draw order.
    pub fixed: Vec<usize>,
    /// The object in drawn slot 1; empty on a one-object draw list.
    pub spun: Vec<usize>,
}

impl BattleStageLayers {
    /// Split a backdrop draw list (the edited object table, in slot order)
    /// at slot 1.
    ///
    /// REF: FUN_8001ADA4
    pub fn split(objects: &[usize]) -> Self {
        let mut out = Self::default();
        for (slot, &o) in objects.iter().enumerate() {
            if slot == 1 {
                out.spun.push(o);
            } else {
                out.fixed.push(o);
            }
        }
        out
    }
}

/// The object-space transform the backdrop draw gives drawn slot 1 under one
/// of its two copies, as the rows of a 3x3 applied to a raw stage vertex
/// (PSX axes, before the host's stage model).
///
/// Copy A is `Ry(yaw)`. Copy B carries the pair's second-copy transform
/// ahead of it: the actor's own half turn (`+0x26 = 0x800`, built by
/// `FUN_80026988`), or the X reflection `ScaleMatrix` applies for
/// `+0x5A & 2` - and on a reflected copy the draw negates the angle
/// (`subu a0,zero,a0` at `0x8001AFF4`, under `+0x5A & 0xE`). Since
/// `S * Ry(-a) = Ry(a) * S`, that is the mirrored object turned the same way
/// round the arena as copy A, so the two halves travel together.
///
/// `FUN_8004629C` multiplies the current rotation by the columns
/// `(cos, 0, -sin)`, `(0, 1, 0)`, `(sin, 0, cos)` - three `MVMVA`s over the
/// sine table at `0x80070A2C` - which is `x' = c*x + s*z`, `z' = c*z - s*x`.
///
/// REF: FUN_8001ADA4, FUN_8004629C
pub fn backdrop_slot_1_basis(second: SecondCopy, yaw: u16, copy_b: bool) -> [[f32; 3]; 3] {
    let mirrored = copy_b && second == SecondCopy::MirrorX;
    let units = i32::from(yaw & 0xFFF);
    let a = (if mirrored { -units } else { units }) as f32 / 4096.0 * std::f32::consts::TAU;
    let (s, c) = a.sin_cos();
    let [sx, sy, sz] = if copy_b {
        second.scale()
    } else {
        [1.0, 1.0, 1.0]
    };
    [
        [sx * c, 0.0, sx * s],
        [0.0, sy, 0.0],
        [-sz * s, 0.0, sz * c],
    ]
}

impl SceneHost {
    /// The battle-stage backdrop entry for a fight starting now: the entry the
    /// last region setup's stage variant names in the loaded scene, falling
    /// back to [`crate::scene::ProtIndex::battle_stage_entry_for_scene`] when
    /// no region setup has been applied (a scene with no region section) or
    /// the variant does not name a stage stream.
    ///
    /// REF: FUN_800513F0, FUN_801D9E1C
    pub fn battle_stage_entry(&self) -> Option<u32> {
        let scene = self.scene.as_ref()?;
        self.world
            .encounters
            .region_setup
            .and_then(|s| {
                self.index
                    .battle_stage_entry_for_region(&scene.name, s.stage_variant)
            })
            .or_else(|| self.index.battle_stage_entry_for_scene(&scene.name))
    }

    /// Whether battle init keeps the backdrop shell's object 1: retail
    /// `_DAT_8007B64B`, bit 5 of the last long-layout region record's
    /// `+8` byte (`FUN_800513F0` skips its drop-object-1 pass when set,
    /// `0x80051ABC`). `false` - the drop - until a long-layout region says
    /// otherwise.
    pub fn battle_stage_keeps_object_1(&self) -> bool {
        self.world
            .encounters
            .region_setup
            .and_then(|s| s.keep_backdrop_object_1)
            .unwrap_or(false)
    }

    /// The backdrop TMD objects the two stage actors draw right now, in draw
    /// order: battle init's object edit (object 1 dropped unless
    /// [`Self::battle_stage_keeps_object_1`]) and, once the evolved-Cort
    /// arrival has handed back, its slot-0 rebind
    /// ([`crate::world::BattleState::backdrop_rebound`]). The one kernel both
    /// hosts build the stage shell from; a host rebuilds its shell when the
    /// list changes mid-fight.
    ///
    /// REF: FUN_800513F0, FUN_801F69F4
    pub fn battle_stage_object_indices(&self, object_count: usize) -> Vec<usize> {
        legaia_asset::battle_backdrop::drawn_object_indices_rebound(
            object_count,
            self.battle_stage_keeps_object_1(),
            self.world.battle.backdrop_rebound,
        )
    }

    /// [`Self::battle_stage_object_indices`] split at the slot the backdrop
    /// draw spins ([`BattleStageLayers`]) - the one kernel both hosts build
    /// their backdrop draws from.
    ///
    /// REF: FUN_800513F0, FUN_8001ADA4
    pub fn battle_stage_layers(&self, object_count: usize) -> BattleStageLayers {
        BattleStageLayers::split(&self.battle_stage_object_indices(object_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_1_is_the_spun_layer_whatever_object_sits_in_it() {
        // nilboa's two-object shell with the keep bit: slot 1 is object 1.
        assert_eq!(
            BattleStageLayers::split(&[0, 1]),
            BattleStageLayers {
                fixed: vec![0],
                spun: vec![1],
            }
        );
        // The ordinary drop on a two-object shell: nothing in slot 1.
        assert_eq!(
            BattleStageLayers::split(&[0]),
            BattleStageLayers {
                fixed: vec![0],
                spun: vec![],
            }
        );
        // A four-object overworld dome after the drop: slot 1 is object 2.
        assert_eq!(
            BattleStageLayers::split(&[0, 2, 3]),
            BattleStageLayers {
                fixed: vec![0, 3],
                spun: vec![2],
            }
        );
    }

    fn apply(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
    }

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn copy_a_turns_by_the_slot_angle() {
        // A quarter turn (0x400) under FUN_8004629C's columns:
        // x' = c*x + s*z, z' = c*z - s*x.
        let m = backdrop_slot_1_basis(SecondCopy::HalfTurn, 0x400, false);
        assert!(near(apply(m, [1.0, 5.0, 0.0]), [0.0, 5.0, -1.0]));
        assert!(near(apply(m, [0.0, 5.0, 1.0]), [1.0, 5.0, 0.0]));
        // No angle is the identity; the angle wraps at a full turn.
        let id = backdrop_slot_1_basis(SecondCopy::MirrorX, 0, false);
        assert!(near(apply(id, [3.0, 4.0, 5.0]), [3.0, 4.0, 5.0]));
        let wrapped = backdrop_slot_1_basis(SecondCopy::HalfTurn, 0x1400, false);
        assert!(near(
            apply(wrapped, [1.0, 0.0, 0.0]),
            apply(m, [1.0, 0.0, 0.0])
        ));
    }

    #[test]
    fn a_reflected_copy_b_is_the_mirrored_object_under_copy_a_s_turn() {
        // The reflected copy negates the angle and then flips X, and
        // `S * Ry(-a) = Ry(a) * S`: the mirrored half turns the same way
        // round the arena as the half it mirrors, so the two stay one ring.
        for yaw in [0u16, 0x155, 0x400, 0x9A3, 0xFFF] {
            let a = backdrop_slot_1_basis(SecondCopy::MirrorX, yaw, false);
            let b = backdrop_slot_1_basis(SecondCopy::MirrorX, yaw, true);
            for v in [[1.0, 2.0, 3.0], [-7.0, 0.5, 2.0]] {
                let pa = apply(a, [-v[0], v[1], v[2]]);
                assert!(near(apply(b, v), pa), "yaw {yaw:#x}");
            }
        }
    }

    #[test]
    fn copy_b_is_copy_a_half_a_turn_on_on_a_half_turn_stage() {
        for yaw in [0u16, 0x155, 0x400, 0x9A3] {
            let a = backdrop_slot_1_basis(SecondCopy::HalfTurn, yaw, false);
            let b = backdrop_slot_1_basis(SecondCopy::HalfTurn, yaw, true);
            let pa = apply(a, [1.0, 2.0, 3.0]);
            assert!(near(apply(b, [1.0, 2.0, 3.0]), [-pa[0], pa[1], -pa[2]]));
        }
    }
}
