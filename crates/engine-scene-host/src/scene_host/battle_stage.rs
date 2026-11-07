//! Which PROT entry a battle is fought inside, resolved from the live scene
//! host - the one call both play hosts build the backdrop from.
//!
//! Retail does not look the backdrop up per scene. The region reader
//! `FUN_801D9E1C` stamps the stage variant `_DAT_8007BD60 = region[+8] &
//! 0x1F` for the region the player stands in - every step, and again on the
//! op-`0x3E` scripted-battle install - and battle init `FUN_800513F0` loads
//! entry `scene_index + variant` through `FUN_8001FA88`
//! ([`legaia_scene_formats::battle_backdrop::battle_stage_entry_for_variant`]). So a scene
//! with several sub-area backdrops fights in whichever one the region names.

use std::sync::Arc;

use super::SceneHost;
use legaia_engine_scene::scene::Scene;
use legaia_engine_scene::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_scene_formats::battle_backdrop::SecondCopy;

/// What a fight's stage loads off the disc: the part of the stage the
/// `Field -> Battle` edge can reuse from the last fight in the same place.
pub struct BattleStageResources {
    /// The PROT entry of the stage stream the fight's region names.
    pub stage_entry: u32,
    /// Scene + stage-dome textures resident - becomes the battle VRAM base.
    pub vram: psx_tim::Vram,
    /// The backdrop shell TMD plus the raw bytes it was parsed from.
    pub dome: (legaia_tmd::Tmd, Vec<u8>),
    /// How many stage TIMs the stage stream re-uploaded over its siblings.
    pub restored_tims: usize,
}

/// What [`BattleStageResources`] are a pure function of, past the disc: the
/// block the stage comes from, the stage stream and a pack's tint. A scene
/// standing over an expansion map's synthetic layer is not memoised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BattleStageKey {
    scene: String,
    start: u32,
    end: u32,
    stage_entry: u32,
    tint: Option<[u8; 3]>,
}

/// The last stage built, by [`BattleStageKey`].
pub(super) type MemoisedStage = (BattleStageKey, Arc<BattleStageResources>);

/// Build a stage's resources off the disc: the battle-kind scene build over
/// the shared field blocks, then the stage's own TIMs back on top.
fn build_stage_resources(
    host: &SceneHost,
    scene: &Scene,
    stage_entry: u32,
    tint: Option<[u8; 3]>,
) -> Option<BattleStageResources> {
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|name| Scene::load(&host.index, name).ok())
        .collect();
    let refs: Vec<&Scene> = shared.iter().collect();
    let system_ui = host.index.system_ui_bundle().ok();
    let (res, _) = SceneResources::build_targeted_with_options(
        scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Battle,
            upload_all_tims: true,
            // Boot-resident system-UI bundle: resident through battle in
            // retail (uploaded once at boot, never evicted).
            system_ui: system_ui.as_deref(),
        },
    )
    .ok()?;
    // The stage dome is the leading TMD of the scene_tmd_stream stage entry.
    let dome = res.tmds.iter().find(|t| t.entry_idx == stage_entry)?;
    // Take back the VRAM this stage owns. A bundle carries one stream per
    // sub-area and they all declare the SAME pages + CLUT rows, so the
    // DMA-every-TIM build leaves whichever sibling was written last holding
    // them. Retail only ever has the recorded stream resident.
    let mut vram = res.vram.clone();
    // An expansion pack's stage tint rides the stage's own palettes, so the
    // shell and the ground tile take it on both hosts alike.
    let restored_tims = legaia_engine_scene::scene::upload_battle_stage_tims_into_vram_tinted(
        scene,
        stage_entry,
        &mut vram,
        tint,
    );
    Some(BattleStageResources {
        stage_entry,
        vram,
        dome: (dome.tmd.clone(), dome.raw.clone()),
        restored_tims,
    })
}

/// A backdrop's drawn objects split by whether the backdrop draw spins them.
///
/// `FUN_8001ADA4`'s backdrop arm (draw kind 3, `0x8001AF04`) walks the
/// actor's object table and, ahead of each object's prim walk, post-rotates
/// the composed matrix about Y through `FUN_8004629C` by a per-**slot** angle
/// read from the table at `0x800891C8` (`lh a0,0x2(s1)` at `0x8001AFEC` /
/// `0x8001B004`, `s1` stepping `8` a slot). Only slot 1's angle is ever
/// written ([`legaia_engine_core::world::World::tick_battle_backdrop_spin`]), so slot 1
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

/// The NCLIP word (`Renderer::set_backface_cull` encoding) one backdrop copy
/// draws its slot-1 object with.
///
/// The backdrop arm hands the prim dispatcher `FUN_80043390` the node's
/// colour word ORed with `0x40000000` for copy A and with `0x48000000` for a
/// copy whose `+0x5A` reflection bits are up (`lui a1,0x4000` at `0x8001B014`
/// against `lui a1,0x4800` at `0x8001AFFC`). Bit `0x08000000` lowers the
/// winding mask to `0x7FFFFFFF` (`0x80043520..0x80043540`), which every
/// textured leaf's two-half `NCLIP` test then can no longer fail (the
/// flat-textured quad leaf `FUN_80044DC8` at `0x80044E68..0x80044EAC`). So the
/// mirrored copy draws both sides - a reflection reverses its winding - and
/// every other copy draws only its front faces, under the same parity as a
/// battle body ([`legaia_engine_core::world::BATTLE_BODY_NCLIP`]).
///
/// A half-ring layer such as nilboa's horizon mist ribbon is where that shows:
/// the camera stands outside the ring, so copy A's near quads face away and
/// retail drops them, leaving the band behind the combatants.
///
/// REF: FUN_8001ADA4, FUN_80043390, FUN_80044DC8
pub fn backdrop_copy_nclip(second: SecondCopy, copy_b: bool) -> u32 {
    if copy_b && second == SecondCopy::MirrorX {
        0
    } else {
        legaia_engine_core::world::BATTLE_BODY_NCLIP
    }
}

/// The fixed backdrop shell's textured mesh, both copies in one draw: copy A,
/// then copy B under the second-copy transform, and - when copy B draws both
/// sides ([`backdrop_copy_nclip`]) - copy B once more with its winding
/// reversed. Drawn under [`backdrop_shell_nclip`], so copy A keeps its front
/// faces only and copy B shows both: of each reversed pair exactly one
/// triangle survives the cull, which is a both-sided draw.
///
/// Every copy of the backdrop goes through the one arm and the one word rule
/// ([`backdrop_copy_nclip`]), not only the spun slot 1. On nilboa's stage the
/// fixed object's additive ice crystals (tpage `0x2C`, ABR 1) show it: the
/// mirrored copy's crystal behind Koru draws both its faces, copy A's only the
/// ones facing the camera (`koru_strip_forced`).
///
/// REF: FUN_8001ADA4, FUN_80043390
pub fn backdrop_shell_mesh(
    first: &legaia_tmd::mesh::VramMesh,
    second: SecondCopy,
) -> legaia_tmd::mesh::VramMesh {
    let mut out = first.clone();
    out.append_scaled(first, second.scale());
    if backdrop_copy_nclip(second, true) == 0 {
        let mut reversed = first.clone();
        for t in reversed.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        out.append_scaled(&reversed, second.scale());
    }
    out
}

/// [`backdrop_shell_mesh`] for the shell's untextured `F*` / `G*` half (the
/// native window's colour pipeline): the same copies, so the same
/// [`backdrop_shell_nclip`] word leaves it both-sided exactly where the
/// textured half is.
pub fn backdrop_shell_color_mesh(
    first: &legaia_tmd::mesh::ColorMesh,
    second: SecondCopy,
) -> legaia_tmd::mesh::ColorMesh {
    let mut out = first.clone();
    out.append_scaled(first, second.scale());
    if backdrop_copy_nclip(second, true) == 0 {
        let mut reversed = first.clone();
        for t in reversed.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        out.append_scaled(&reversed, second.scale());
    }
    out
}

/// The NCLIP word the fixed backdrop shell ([`backdrop_shell_mesh`]) draws
/// under: copy A's, which keeps front faces only.
pub fn backdrop_shell_nclip(second: SecondCopy) -> u32 {
    backdrop_copy_nclip(second, false)
}

/// The CDNAME block a Muscle Dome leg's battle stage comes from: the arena
/// data file `data\field\other6.lzs`, whose only `scene_tmd_stream` is the
/// arena backdrop.
pub const ARENA_STAGE_SCENE: &str = "other6";

impl SceneHost {
    /// Spoil the held stage's VRAM, so a test can see the verify switch catch
    /// a hit that differs. `false` with no stage held.
    #[cfg(test)]
    pub(crate) fn corrupt_battle_stage_memo(&self) -> bool {
        let mut memo = self
            .battle_stage_memo
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let Some((_, res)) = memo.as_mut() else {
            return false;
        };
        let mut vram = res.vram.clone();
        let word = vram.pixel(0, 0) ^ 1;
        vram.write_block(0, 0, 1, 1, &word.to_le_bytes());
        *res = Arc::new(BattleStageResources {
            stage_entry: res.stage_entry,
            vram,
            dome: res.dome.clone(),
            restored_tims: res.restored_tims,
        });
        true
    }

    /// The stage resources for a fight starting now, from the last fight's
    /// when it stood on the same stage. `None` when the scene has no stage
    /// entry for the fight's region.
    ///
    /// `WorldToggles::battle_memo_verify` rebuilds behind every hit and panics
    /// on a difference.
    pub fn battle_stage_resources(&self) -> Option<Arc<BattleStageResources>> {
        // A Muscle Dome leg fights on the arena backdrop, which lives in its
        // own data block rather than the departure scene's
        // (`SceneHost::battle_stage_scene`).
        let other;
        let scene = match self.battle_stage_scene() {
            Some(name) => {
                other = Scene::load(&self.index, name).ok()?;
                &other
            }
            None => self.scene.as_ref()?,
        };
        // Not the block's first stage stream: a scene bundle carries one per
        // sub-area, and the region the fight starts in names which one
        // (`SceneHost::battle_stage_entry`, the region reader's stage variant
        // `_DAT_8007BD60`).
        let stage_entry = self.battle_stage_entry()?;
        let tint = self.battle_stage_tint();
        let key = scene.overrides.is_none().then(|| BattleStageKey {
            scene: scene.name.clone(),
            start: scene.start,
            end: scene.end,
            stage_entry,
            tint,
        });
        let mut memo = self
            .battle_stage_memo
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let (Some(key), Some((held, res))) = (&key, memo.as_ref())
            && key == held
        {
            if self.world.toggles.battle_memo_verify {
                let fresh = build_stage_resources(self, scene, stage_entry, tint)?;
                assert!(
                    fresh.vram.as_bytes() == res.vram.as_bytes()
                        && fresh.vram.written_bytes() == res.vram.written_bytes()
                        && fresh.dome.1 == res.dome.1,
                    "memoised battle stage differs from a fresh build: {key:?}"
                );
            }
            return Some(res.clone());
        }
        let res = Arc::new(build_stage_resources(self, scene, stage_entry, tint)?);
        if let Some(key) = key {
            *memo = Some((key, res.clone()));
        }
        Some(res)
    }

    /// The battle-stage backdrop entry for a fight starting now: the entry the
    /// last region setup's stage variant names in the loaded scene, falling
    /// back to [`legaia_engine_scene::scene::ProtIndex::battle_stage_entry_for_scene`] when
    /// no region setup has been applied (a scene with no region section) or
    /// the variant does not name a stage stream.
    ///
    /// REF: FUN_800513F0, FUN_801D9E1C
    pub fn battle_stage_entry(&self) -> Option<u32> {
        // An expansion pack's fight names its own stage by coordinate: the
        // `index`-th stage stream of a block, or the block's own pick.
        if let Some(st) = self.pack_stage_resolved() {
            return match st.index {
                Some(i) => self
                    .index
                    .battle_stage_entries(&st.scene)
                    .get(usize::from(i))
                    .copied(),
                None => self.index.battle_stage_entry_for_scene(&st.scene),
            };
        }
        let variant = self.world.encounters.region_setup.map(|s| s.stage_variant);
        let (scene, variant) = match self.battle_stage_scene() {
            // A Muscle Dome leg's battle: the arena's fight start leaves the
            // map id at `other6` (`_DAT_80084540 = 0x4C6`) and the stage
            // variant at `DAT_8007BD60 & 0x7F = 0` (`0x80`), so the loader's
            // `scene_index + variant` names the arena backdrop.
            Some(name) if self.world.muscle_leg_in_battle() => (name, Some(0)),
            Some(name) => (name, variant),
            None => (self.scene.as_ref()?.name.as_str(), variant),
        };
        variant
            .and_then(|v| self.index.battle_stage_entry_for_region(scene, v))
            .or_else(|| self.index.battle_stage_entry_for_scene(scene))
    }

    /// The CDNAME block the fight's stage stream comes from when it is not
    /// the loaded scene: the arena data file on a Muscle Dome leg, or the
    /// block a seeded map id names
    /// ([`legaia_engine_battle::encounter_state::EncounterState::battle_map_define`]).
    /// `None` - the loaded scene's own block.
    ///
    /// REF: FUN_800513F0
    pub fn battle_stage_scene(&self) -> Option<&str> {
        if self.world.muscle_leg_in_battle() {
            return Some(ARENA_STAGE_SCENE);
        }
        if let Some(st) = self.pack_stage_resolved() {
            return Some(st.scene.as_str())
                .filter(|name| self.scene.as_ref().is_none_or(|s| s.name != *name));
        }
        let define = self.world.encounters.battle_map_define?;
        self.index
            .block_name_for_define(u32::from(define))
            .filter(|name| self.scene.as_ref().is_none_or(|s| s.name != *name))
    }

    /// Whether battle init keeps the backdrop shell's object 1: retail
    /// `_DAT_8007B64B`, bit 5 of the last long-layout region record's
    /// `+8` byte (`FUN_800513F0` skips its drop-object-1 pass when set,
    /// `0x80051ABC`). `false` - the drop - until a long-layout region says
    /// otherwise.
    pub fn battle_stage_keeps_object_1(&self) -> bool {
        if let Some(st) = self.pack_stage_resolved() {
            return st.keep_object_1;
        }
        self.world
            .encounters
            .region_setup
            .and_then(|s| s.keep_backdrop_object_1)
            .unwrap_or(false)
    }

    /// The pack's stage for the fight under way when it names a stage stream
    /// on this disc ([`legaia_engine_core::world::World::expansion_battle_stage`]);
    /// a coordinate that names none leaves the room's own stage standing.
    pub fn pack_stage_resolved(&self) -> Option<&legaia_expansion_format::battle::BattleStageRef> {
        let st = self.world.expansion_battle_stage()?;
        let entries = self.index.battle_stage_entries(&st.scene);
        let ok = match st.index {
            Some(i) => usize::from(i) < entries.len(),
            None => !entries.is_empty(),
        };
        ok.then_some(st)
    }

    /// The stage tint the pack names for the fight under way
    /// ([`legaia_expansion_format::battle::BattleStageRef::tint`]); `None`
    /// with no pack stage or no tint.
    pub fn battle_stage_tint(&self) -> Option<[u8; 3]> {
        self.pack_stage_resolved()?.tint
    }

    /// The backdrop TMD objects the two stage actors draw right now, in draw
    /// order: battle init's object edit (object 1 dropped unless
    /// [`Self::battle_stage_keeps_object_1`]) and, once the evolved-Cort
    /// arrival has handed back, its slot-0 rebind
    /// (`legaia_engine_battle_loop::battle_state::BattleState::backdrop_rebound`). The one kernel both
    /// hosts build the stage shell from; a host rebuilds its shell when the
    /// list changes mid-fight.
    ///
    /// REF: FUN_800513F0, FUN_801F69F4
    pub fn battle_stage_object_indices(&self, object_count: usize) -> Vec<usize> {
        legaia_scene_formats::battle_backdrop::drawn_object_indices_rebound(
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
    fn the_shell_mesh_doubles_only_a_both_sided_copy_b() {
        let first = legaia_tmd::mesh::VramMesh {
            positions: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            uvs: vec![[0, 0]; 3],
            cba_tsb: vec![[0, 0]; 3],
            indices: vec![0, 1, 2],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            colors: vec![[128, 128, 128]; 3],
        };
        let half = backdrop_shell_mesh(&first, SecondCopy::HalfTurn);
        assert_eq!(half.indices.len(), 6, "copy A and copy B, both one-sided");
        let mirror = backdrop_shell_mesh(&first, SecondCopy::MirrorX);
        assert_eq!(mirror.indices.len(), 9, "the mirrored copy twice");
        // Copy B's two triangles wind opposite ways over the same corners.
        let b = &mirror.indices[3..6];
        let b_rev = &mirror.indices[6..9];
        let at = |i: u32| mirror.positions[i as usize];
        assert_eq!(
            [at(b[0]), at(b[1]), at(b[2])],
            [at(b_rev[0]), at(b_rev[2]), at(b_rev[1])]
        );
        assert_eq!(
            backdrop_shell_nclip(SecondCopy::MirrorX),
            legaia_engine_core::world::BATTLE_BODY_NCLIP
        );
    }

    #[test]
    fn only_the_mirrored_copy_draws_both_sides() {
        let one_sided = legaia_engine_core::world::BATTLE_BODY_NCLIP;
        assert_eq!(backdrop_copy_nclip(SecondCopy::MirrorX, true), 0);
        assert_eq!(backdrop_copy_nclip(SecondCopy::MirrorX, false), one_sided);
        assert_eq!(backdrop_copy_nclip(SecondCopy::HalfTurn, true), one_sided);
        assert_eq!(backdrop_copy_nclip(SecondCopy::HalfTurn, false), one_sided);
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
