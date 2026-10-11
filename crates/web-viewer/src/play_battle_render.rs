//! Browser battle **3D presentation**: the browser twin of the native
//! window's `enter_battle_render` / `exit_battle_render` / `build_battle_stage`
//! (`crates/engine-shell/.../window/battle.rs`). [`crate::play_battle`] owns
//! the rules-side presentation (HUD rows, banner, submenus); this module owns
//! what stands *behind* that overlay once a random encounter fires:
//!
//! * the **battle-stage backdrop**: the scene's `scene_tmd_stream` half-dome
//!   rebuilt through [`SceneLoadKind::Battle`] (the Field build excludes the
//!   stage textures), object-list-edited by
//!   [`legaia_scene_formats::battle_backdrop::drawn_objects_tmd`] (`FUN_800513F0`
//!   drops object 1) and drawn **twice** - the second copy pre-appended here
//!   under the per-stage transform (`SCUS_942.54` mirror table
//!   `DAT_80078B50`, [`legaia_scene_formats::battle_backdrop::MirrorXTable`];
//!   half-turn fallback) with [`legaia_tmd::mesh::VramMesh::append_scaled`]'s
//!   winding flip, so the page uploads ONE mesh and the horizon closes;
//! * the **ground grid** (`func_0x801d02c0`,
//!   [`legaia_scene_formats::battle_backdrop::build_ground_grid`]) plus its per-stage
//!   depth-cue far colour (`DAT_80078C1C`,
//!   [`legaia_engine_battle_vm::battle_ground_grid::OutdoorCueTable`]);
//! * the **battle VRAM**: stage build + flame atlas (PROT 870,
//!   [`legaia_engine_scene::scene::upload_flame_atlas_into_vram`]) + per-slot
//!   monster texture injection + the party texture bands, all into a
//!   throwaway copy the page uploads while the fight runs - battle exit
//!   re-uploads the untouched field VRAM (the restore
//!   `crate::runtime::LegaiaRuntime::step_field_vram_fx`'s battle guard
//!   always assumed);
//! * **monster meshes** from the PROT 867 archive
//!   ([`legaia_actor_formats::monster_archive::MonsterMesh::battle_render_mesh`],
//!   per-slot CBA/TSB relocation + texture injection), bound to the live
//!   enemy actor slots with their idle + action clips installed on the world
//!   so the shared battle SM poses them;
//! * **party battle forms**, assembled per character from the player battle
//!   files' equipment-id sections
//!   ([`legaia_actor_formats::battle_char_assembly`], PROT 863..866) and relocated
//!   into the present-party ordinal's runtime VRAM band, with the real
//!   texture-pool pixels + battle palette overlay (PROT 1204/1205 + the
//!   PROT 1203 rest pose as the per-member fallback, exactly the native
//!   fallback ladder).
//!
//! Actors are exported **object-local** with per-vertex object ids; the page
//! poses them per frame from [`LegaiaRuntime::play_battle_actor_pose`] - the
//! live `pose_frame` the engine's own `tick_battle_animations` maintains
//! (rest pose fallback until the first battle tick lands), the same
//! `R.v + T` composition every other site animator runs.
//!
//! The **battle camera** is the SAME phase-scripted retail camera the native
//! window runs - [`legaia_engine_battle_vm::battle_cam_script`] (`FUN_801D5854`
//! framing cases, `FUN_801D829C` glides): the dialogue close-up, the far
//! "menu" framing with the idle orbit, and the per-character submenu
//! close-up, driven from the live world state by the shared `drive` on the
//! retail 2-vsync step cadence. The page consumes a ready view-projection
//! matrix ([`LegaiaRuntime::play_battle_camera_vp`], the shared
//! `battle_vp` = the native `psx_camera_mvp` composition) instead of
//! re-projecting the pose through its orbit camera.
//!
//! The effect layer that draws *on top* of this - effect-pool billboards,
//! `etmd` FX models, the move-VM scene-graph parts, the summon creature and
//! the target-select cursor tint - lives in [`crate::play_battle_fx`].
//!
//! What the native battle render still has that this host lacks: the
//! battle-intro screen-prim emitter, and the field move-VM stager parts
//! (`build_field_fx_part_draws`, which resolve against the scene TMD pack the
//! page does not upload while a battle is on screen).
//!
//! The mid-battle VRAM re-stamps - the per-tick facial animation
//! (`tick_battle_face_stamps`), the status CLUT recolour
//! (`engine-battle::battle_status_clut`, `FUN_8004CE2C` pass 4) and the effect
//! CLUT stage (`engine-battle::battle_effect_clut`, `FUN_801DEA50`'s palette
//! arm) - run here through [`crate::play_battle_vram`] against the
//! [`BattleRender::vram`] copy, and the page re-uploads it on
//! [`LegaiaRuntime::play_battle_vram_take_dirty`]. The party build below
//! collects each member's face tracks ([`BattleRender::faces`]) for that
//! channel, from the same player-file entries the native loader reads.

use crate::runtime::LegaiaRuntime;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::scene::Scene;
use legaia_engine_scene::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use wasm_bindgen::prelude::*;

mod actor_exports;
mod camera;
mod stage_exports;

/// Retail 4x uniform battle world scale (base matrix `0x8007BF10` =
/// `16384 * I`; GTE `4096` = 1.0), composed per drawn object by
/// `FUN_80048A08`.
///
/// **Every battle draw class rides it**, not just the combatants: the arena
/// backdrop is registered as an ordinary background actor and goes through
/// the same path. The page composes it onto the actor draws (mesh scale +
/// pre-scaled translation); the stage class carries it in its uploaded
/// vertices instead ([`BattleMesh::stage_positions`]), because the page's
/// per-draw scale for those is `1.0`. The camera's translation trio is
/// authored in this scaled space, so a class left at raw 1x would be
/// orbited at four times the intended radius.
pub(crate) const BATTLE_WORLD_SCALE: f32 = 4.0;

/// PROT entry of the monster stat/mesh archive (`0867_battle_data`).
const MONSTER_ARCHIVE_PROT_INDEX: u32 = 867;

/// Host-safe log: the browser console on wasm, stderr on the native test
/// build (`crate::console_log` is a hard wasm-only stub that panics off-web,
/// and this module runs under the disc-gated native oracles).
pub(crate) fn web_log(s: &str) {
    #[cfg(target_arch = "wasm32")]
    crate::console_log(s);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{s}");
}

/// The stage shell a [`BattleRender`] can rebuild its backdrop from.
struct WebStageShell {
    tmd: legaia_tmd::Tmd,
    raw: Vec<u8>,
    second: legaia_scene_formats::battle_backdrop::SecondCopy,
    objects: Vec<usize>,
}

/// The backdrop mesh for one object list: the shell drawn twice, the second
/// copy under the per-stage transform. The disc shell is an authored HALF;
/// the second copy closes the horizon. `append_scaled` reverses winding on a
/// negative determinant (the mesh-level analogue of retail's `0x40000000 ->
/// 0x48000000` draw-mode swap).
fn stage_shell_mesh(
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    second: legaia_scene_formats::battle_backdrop::SecondCopy,
    objects: &[usize],
) -> Option<BattleMesh> {
    let tmd0 = legaia_scene_formats::battle_backdrop::objects_tmd(tmd, objects);
    let (mut vmesh, _oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd0, raw);
    let mut flat = crate::packet_color::hybrid(&vmesh, &shading);
    let first = vmesh.clone();
    vmesh.append_scaled(&first, second.scale());
    let flat_copy = flat.clone();
    flat.extend(flat_copy);
    (!vmesh.indices.is_empty()).then_some(BattleMesh { mesh: vmesh, flat })
}

/// One copy of the backdrop's drawn slot 1 - the object the backdrop draw
/// turns about Y every frame (`BattleStageLayers`), so no second copy and no
/// transform is baked in; the page draws it twice under
/// [`LegaiaRuntime::play_battle_backdrop_spun_models`].
fn stage_slot_mesh(tmd: &legaia_tmd::Tmd, raw: &[u8], objects: &[usize]) -> Option<BattleMesh> {
    if objects.is_empty() {
        return None;
    }
    let tmd0 = legaia_scene_formats::battle_backdrop::objects_tmd(tmd, objects);
    let (vmesh, _oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd0, raw);
    let flat = crate::packet_color::hybrid(&vmesh, &shading);
    (!vmesh.indices.is_empty()).then_some(BattleMesh { mesh: vmesh, flat })
}

/// One page-uploadable battle mesh in the play page's scene-mesh shape.
struct BattleMesh {
    mesh: legaia_tmd::mesh::VramMesh,
    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid shader; empty =
    /// fully textured (the page passes `null`).
    flat: Vec<u8>,
}

impl BattleMesh {
    /// Wrap a purely-textured mesh, carrying its per-prim packet colours into
    /// the `a_flat_rgba` stream so the browser shader can run retail's
    /// `texel * colour / 128` modulation. An actor or grid uploaded with an
    /// empty stream falls back to the renderer's neutral constant, i.e. draws
    /// at the raw texel with the baked contrast thrown away.
    fn textured(mesh: legaia_tmd::mesh::VramMesh) -> Self {
        let flat = crate::packet_color::textured(&mesh);
        BattleMesh { mesh, flat }
    }

    fn positions(&self) -> Vec<f32> {
        self.mesh.positions.iter().flatten().copied().collect()
    }
    /// [`Self::positions`] lifted into the scaled battle stage space -
    /// what the **stage class** (arena backdrop + ground grid) uploads.
    ///
    /// Retail's `0x8007BF10 = 16384*I` base matrix is composed per drawn
    /// object (`FUN_80048A08`), and the arena is registered as an ordinary
    /// background *actor*, so the stage rides the same scale the combatants
    /// do. The page composes `BATTLE_WORLD_SCALE` onto the actor draws only
    /// (its per-draw `scale` field), so the stage carries it in its
    /// vertices instead - same world, one scale, no page change.
    ///
    /// Leaving the stage at raw 1x under a camera whose translation trio is
    /// authored in the scaled space orbited the eye at four times the
    /// intended radius (straight through the arena shell on one side) and
    /// drew every actor `3 * seat` away from its own ground cell. The
    /// native window's sibling is `PlayWindowApp::battle_stage_model`.
    fn stage_positions(&self) -> Vec<f32> {
        self.mesh
            .positions
            .iter()
            .flatten()
            .map(|v| v * BATTLE_WORLD_SCALE)
            .collect()
    }
    fn uvs(&self) -> Vec<u8> {
        self.mesh.uvs.iter().flatten().copied().collect()
    }
    fn cba_tsb(&self) -> Vec<u16> {
        self.mesh.cba_tsb.iter().flatten().copied().collect()
    }
}

/// One battle actor's render bundle, index-parallel with the JS side's
/// per-actor mesh instances.
struct BattleActorRender {
    /// World actor-table slot this mesh is bound to.
    actor_idx: usize,
    mesh: BattleMesh,
    /// Per-vertex TMD object index (the rigid part each vertex hangs from);
    /// empty = the upload is already statically posed (the PROT 1204
    /// fallback) and must never be re-posed.
    object_ids: Vec<u32>,
    /// Idle frame-0 pose, `6 x i32` per part - what
    /// [`LegaiaRuntime::play_battle_actor_pose`] serves until the world's
    /// first battle tick publishes a live `pose_frame`.
    rest_pose: Vec<i32>,
}

/// The live battle render state, built on the `Field -> Battle` mode edge
/// and dropped on exit.
pub(crate) struct BattleRender {
    /// Battle VRAM (stage + flame atlas + monster/party texture bands). The
    /// page uploads this for the fight and restores the field VRAM after.
    pub(crate) vram: legaia_tim::Vram,
    backdrop: Option<BattleMesh>,
    /// The backdrop's drawn slot 1, one copy ([`stage_slot_mesh`]): the
    /// object the backdrop draw spins, on a keeping stage the horizon mist
    /// ribbon. `None` on a one-object draw list.
    backdrop_spun: Option<BattleMesh>,
    /// The stage shell (TMD + raw bytes + second-copy transform) and the
    /// object list `backdrop` was built from
    /// (`SceneHost::battle_stage_object_indices`); a mid-fight change - the
    /// evolved-Cort arrival's slot-0 rebind - rebuilds `backdrop`
    /// ([`LegaiaRuntime::tick_battle_stage_shell_web`]).
    shell: Option<WebStageShell>,
    ground: Option<BattleMesh>,
    /// Ground-grid depth-cue far colour, display `0..1`, applied by the page
    /// as a **per-draw** cue on the grid mesh (the native `DrawCue` seam).
    grid_far: Option<[f32; 3]>,
    /// `DAT_80078C1C` outdoor-table membership of the stage - the battle
    /// tint pass's `DAT_8007BDA8` flag (`World::battle_actor_draw_plan`).
    outdoor: bool,
    actors: Vec<BattleActorRender>,
    /// How many of the five battle texture slots the entry build consumed.
    /// A mid-battle summon injects its creature texture into the next one
    /// ([`LegaiaRuntime::spawn_summon_creature_web`]) - the browser twin of
    /// the native window's `battle_tex_slots_used`.
    pub(crate) tex_slots_used: u8,
    /// Bumped per battle entry - and once more per mid-battle summon spawn -
    /// so the page knows to re-upload.
    pub(crate) generation: u32,
    /// Party members the per-tick facial animator is registered for
    /// ([`crate::play_battle_vram`]): only assembled members whose band
    /// holds the REAL texture-pool pixels (the face-frame strip the stamps
    /// copy from), chars 0..2 on bands 0..2 - the retail animator's own
    /// coverage.
    pub(crate) faces: Vec<crate::play_battle_vram::BattleMemberFace>,
}

impl BattleRender {
    /// Whether this fight has a stage dome - the browser twin of the native
    /// window's `battle_stage_mesh.is_some()`.
    ///
    /// The dome is what makes the open band above it sky, so it is the
    /// selector behind [`LegaiaRuntime::play_scene_clear_color`]. A battle
    /// whose stage failed to build still gets its monsters, and still gets
    /// the ordinary scene clear behind them.
    pub(crate) fn stage_present(&self) -> bool {
        self.backdrop.is_some() || self.ground.is_some()
    }

    /// World actor-table slot of each bound mesh, in mesh order. The FX
    /// exports key their per-actor rows off this so they stay
    /// index-parallel with `play_battle_actor_transforms`.
    pub(crate) fn actor_slots(&self) -> Vec<usize> {
        self.actors.iter().map(|a| a.actor_idx).collect()
    }

    /// Mesh `i`'s world actor slot, rest packet-colour stream and per-vertex
    /// object ids - what [`crate::play_battle_limb_dim`] re-colours.
    pub(crate) fn actor_colour_stream(&self, i: usize) -> Option<(usize, &[u8], &[u32])> {
        self.actors
            .get(i)
            .map(|a| (a.actor_idx, a.mesh.flat.as_slice(), a.object_ids.as_slice()))
    }

    /// Append a mid-battle summon creature's mesh bundle and adopt the VRAM
    /// its texture was injected into. Called by
    /// [`LegaiaRuntime::spawn_summon_creature_web`] after the world side of
    /// the seat is installed; the caller bumps the generation so the page
    /// re-uploads the battle scene with the new mesh.
    pub(crate) fn push_summon_actor(
        &mut self,
        vram: legaia_tim::Vram,
        tex_slot: u8,
        actor_idx: usize,
        mesh: legaia_tmd::mesh::VramMesh,
        object_ids: Vec<u32>,
        rest_pose: Vec<i32>,
    ) {
        self.vram = vram;
        self.tex_slots_used = self.tex_slots_used.max(tex_slot.saturating_add(1));
        // A second cast reuses the same seat, so replace rather than append -
        // two mesh entries bound to one actor slot would draw the creature
        // twice at identical transforms.
        self.actors.retain(|a| a.actor_idx != actor_idx);
        self.actors.push(BattleActorRender {
            actor_idx,
            mesh: BattleMesh::textured(mesh),
            object_ids,
            rest_pose,
        });
    }
}

/// The stage bundle `build_battle_stage` resolves - the browser copy of the
/// native `BattleStage`.
struct WebBattleStage {
    vram: legaia_tim::Vram,
    dome: (legaia_tmd::Tmd, Vec<u8>),
    second: legaia_scene_formats::battle_backdrop::SecondCopy,
    grid_far: [f32; 3],
    outdoor: bool,
}

/// What the mutate phase installs on a monster once the build borrow ends:
/// its texture slot and archive idle clip. (The party's forms are the
/// engine's to install - `SceneHost::ensure_battle_party_forms`.)
struct PendingMonsterInstall {
    actor_idx: usize,
    tex_slot: u8,
    idle: Option<legaia_actor_formats::monster_archive::MonsterAnimation>,
}

/// Flatten one animation frame to the `[tx, ty, tz, rx, ry, rz] x parts`
/// stream the page animator consumes (angles unsigned 12-bit, `4096` = turn).
fn flatten_frame(frame: &[legaia_actor_formats::monster_archive::PartPose]) -> Vec<i32> {
    let mut out = Vec::with_capacity(frame.len() * 6);
    for t in frame {
        out.extend_from_slice(&[
            t.tx as i32,
            t.ty as i32,
            t.tz as i32,
            t.rx as i32,
            t.ry as i32,
            t.rz as i32,
        ]);
    }
    out
}

impl LegaiaRuntime {
    /// Build the current scene's battle-stage bundle: stage VRAM, dome TMD,
    /// second-copy transform, grid far colour. The browser copy of the
    /// native `build_battle_stage`; `None` when the scene has no stage entry
    /// or the battle-kind resource build fails.
    fn build_battle_stage(&self) -> Option<WebBattleStage> {
        let host = self.scene_host.host()?;
        let scene = host.scene.as_ref()?;
        // The region the fight starts in names the backdrop
        // (`SceneHost::battle_stage_entry`, retail `_DAT_8007BD60`).
        let stage_entry = host.battle_stage_entry()?;
        let mut shared: Vec<Scene> = Vec::new();
        for name in FIELD_SHARED_BLOCKS {
            if let Ok(s) = Scene::load(&host.index, name) {
                shared.push(s);
            }
        }
        let refs: Vec<&Scene> = shared.iter().collect();
        let system_ui = host.index.system_ui_bundle().ok();
        let (res, _) = SceneResources::build_targeted_with_options(
            scene,
            &refs,
            BuildOptions {
                kind: SceneLoadKind::Battle,
                upload_all_tims: true,
                system_ui: system_ui.as_deref(),
            },
        )
        .ok()?;
        let dome = res.tmds.iter().find(|t| t.entry_idx == stage_entry)?;
        // Take back the VRAM this stage owns - a bundle's stage streams all
        // declare the same pages + CLUT rows, and the DMA-every-TIM build
        // leaves the last sibling holding them (see the shared kernel's doc).
        let mut vram = res.vram.clone();
        legaia_engine_scene::scene::upload_battle_stage_tims_into_vram(
            scene,
            stage_entry,
            &mut vram,
        );
        // Which transform the SECOND backdrop copy takes: the SCUS mirror
        // table names the X-mirrored stages (town01 included - half-turning
        // it plants a second village wall across the open sea side); the
        // default is the half turn, the safer arm with no readable SCUS.
        let second = self
            .scus
            .as_deref()
            .and_then(legaia_scene_formats::battle_backdrop::MirrorXTable::from_scus)
            .map(|t| t.second_copy_for_prot_index(stage_entry))
            .unwrap_or(legaia_scene_formats::battle_backdrop::SecondCopy::HalfTurn);
        // Grid depth-cue far colour per stage class (`FUN_80050120`): the
        // sibling SCUS table picks the brightened outdoor arm on the 13
        // wide-open stages; indoor `>> 1` grey covers everything else.
        let grid_far_bytes = self
            .scus
            .as_deref()
            .and_then(legaia_engine_battle_vm::battle_ground_grid::OutdoorCueTable::from_scus)
            .map(|t| t.far_colour_for_prot_index(stage_entry))
            .unwrap_or(legaia_engine_battle_vm::battle_ground_grid::GRID_FAR_INDOOR);
        let outdoor = self
            .scus
            .as_deref()
            .and_then(legaia_engine_battle_vm::battle_ground_grid::OutdoorCueTable::from_scus)
            .is_some_and(|t| t.contains_prot_index(stage_entry));
        Some(WebBattleStage {
            vram,
            dome: (dome.tmd.clone(), dome.raw.clone()),
            second,
            grid_far: grid_far_bytes.map(|c| f32::from(c) / 255.0),
            outdoor,
        })
    }

    /// React to the `Field -> Battle` mode edge: build the battle VRAM +
    /// meshes and install each actor's clips on the world so the shared
    /// battle SM poses them. The browser twin of the native
    /// `enter_battle_render`; soft-fails leg by leg (a scene with no stage
    /// still gets monsters over the field VRAM, a failed assembly falls back
    /// to PROT 1204, a fully failed build leaves the overlay-only battle
    /// the page had before).
    pub(crate) fn enter_battle_render(&mut self) {
        self.battle_render = None;
        // The party's forms are the engine's to build (normally already done
        // by the tick that entered the battle); this covers a battle entered
        // outside a tick.
        if let Some(host) = self.scene_host.host_mut() {
            host.ensure_battle_party_forms();
        }
        let Some(host) = self.scene_host.host() else {
            return;
        };
        let monsters = host.world.battle_monster_slots();
        if monsters.is_empty() {
            return;
        }
        let Some(field_base) = host.resources.as_ref().map(|r| r.vram.clone()) else {
            return;
        };
        let Ok(archive) = host.index.entry_bytes_extended(MONSTER_ARCHIVE_PROT_INDEX) else {
            return;
        };

        // Stage backdrop (scene battle build). Its VRAM becomes the battle
        // base so the dome renders textured; field VRAM is the fallback.
        let stage = self.build_battle_stage();
        let mut vram = match &stage {
            Some(s) => s.vram.clone(),
            None => field_base,
        };
        // Battle effect-texture atlas (PROT 870) into the battle copy only -
        // battle exit discards it, the field VRAM base is untouched.
        if let Err(e) =
            legaia_engine_scene::scene::upload_flame_atlas_into_vram(&host.index, &mut vram, true)
        {
            web_log(&format!("play battle: flame-atlas upload skipped: {e:#}"));
        }

        let mut backdrop = None;
        let mut backdrop_spun = None;
        let mut shell = None;
        let mut ground = None;
        let mut grid_far = None;
        let mut fog_stage_luma = None;
        let outdoor = stage.as_ref().is_some_and(|st| st.outdoor);
        // REF: FUN_800513f0 - the backdrop registration whose object-list
        // edit + second-copy transform this host consumes through
        // `legaia_scene_formats::battle_backdrop`, exactly like the native window.
        if let Some(WebBattleStage {
            dome: (tmd, raw),
            second,
            grid_far: gf,
            ..
        }) = &stage
        {
            // `_DAT_8007B64B` (region `+8` bit 5) keeps object 1, and the
            // evolved-Cort arrival's hand-back rebinds slot 0 to it - one
            // shared kernel, `SceneHost::battle_stage_object_indices`.
            let objects = self
                .scene_host
                .host()
                .map(|h| h.battle_stage_object_indices(tmd.objects.len()))
                .unwrap_or_else(|| {
                    legaia_scene_formats::battle_backdrop::drawn_object_indices(tmd.objects.len())
                });
            // The volumetric ground fog may not outshine the stage it lies
            // on: the native window's measurement, same kernel, same shell.
            fog_stage_luma =
                legaia_render_kernels::fog_volume::stage_luminance(&vram, tmd, raw, &objects);
            // Every slot but 1 is the baked shell; slot 1 is the spun copy
            // (`SceneHost::battle_stage_layers`, the native window's split).
            let layers = legaia_engine_core::scene_host::BattleStageLayers::split(&objects);
            backdrop = stage_shell_mesh(tmd, raw, *second, &layers.fixed);
            backdrop_spun = stage_slot_mesh(tmd, raw, &layers.spun);
            shell = Some(WebStageShell {
                tmd: tmd.clone(),
                raw: raw.clone(),
                second: *second,
                objects,
            });
            // Flat tiled ground grid under the actors (retail's
            // func_0x801d02c0), textured from the constant retail
            // page/CLUT/UV window the scene battle VRAM populates.
            // Pre-cue colour = the settled battle ambient (`0x8007B7B0` ->
            // `RGBC`), the same one the native host draws with.
            let grid = legaia_scene_formats::battle_backdrop::build_ground_grid_rgbc(
                legaia_engine_battle_vm::battle_ground_grid::GRID_RGBC_SETTLED,
            );
            if !grid.indices.is_empty() {
                ground = Some(BattleMesh::textured(grid));
                grid_far = Some(*gf);
            }
        }

        // Monster meshes: per-slot texture injection into the battle VRAM +
        // idle / action clips for the shared SM pose hook.
        let mut actors: Vec<BattleActorRender> = Vec::new();
        let mut pending: Vec<PendingMonsterInstall> = Vec::new();
        // The texture slots the monster binds consumed; a mid-battle summon
        // injects its creature texture into the next one.
        let mut bound_slots: Vec<u8> = Vec::new();
        for (actor_idx, monster_id, slot) in monsters {
            let mesh = match legaia_actor_formats::monster_archive::mesh(&archive, monster_id) {
                Ok(Some(m)) => m,
                Ok(None) => continue,
                Err(e) => {
                    web_log(&format!(
                        "play battle: monster {monster_id} mesh decode: {e:#}"
                    ));
                    continue;
                }
            };
            let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
                continue;
            };
            let Some(vmesh) = mesh.battle_render_mesh(slot, &mut vram) else {
                continue;
            };
            if vmesh.indices.is_empty() {
                continue;
            }
            bound_slots.push(slot);
            let object_ids =
                legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, mesh.tmd_bytes()).1;
            let idle = legaia_actor_formats::monster_archive::idle_animation(&archive, monster_id)
                .ok()
                .flatten();
            let rest_pose = idle
                .as_ref()
                .and_then(|a| a.frames.first())
                .map(|f| flatten_frame(f))
                .unwrap_or_default();
            // The monster's action clips (the `+0x4C` tag table) are the
            // engine's to install - `SceneHost::tick` stages them the tick a
            // battle is up, independent of this build - so none is staged
            // here; the install below sets the texture slot and the idle.
            actors.push(BattleActorRender {
                actor_idx,
                mesh: BattleMesh::textured(vmesh),
                object_ids,
                rest_pose,
            });
            pending.push(PendingMonsterInstall {
                actor_idx,
                tex_slot: slot,
                idle,
            });
        }
        let tex_slots_used =
            legaia_engine_core::battle_party_form::monster_tex_slots_used(bound_slots);

        // Party battle forms per present-party ordinal: the CHARACTER picks
        // the content (player file 863 + cslot), the ORDINAL picks the
        // runtime texture band - the live-verified retail rule. The engine
        // built and installed them at battle entry
        // (`SceneHost::ensure_battle_party_forms`, run by `SceneHost::tick`
        // for every session): clips, art bank and art records are already on
        // the actors. This page replays the band pixels into its battle VRAM
        // and takes the meshes and face tracks.
        let mut party_faces: Vec<crate::play_battle_vram::BattleMemberFace> = Vec::new();
        if let Some(party) = host.battle_party_forms() {
            party.vram_writes.replay(&mut vram);
            for form in &party.forms {
                if let Some(render) = party_actor_render(form) {
                    actors.push(render);
                    // Facial animation (FUN_8004C7B4): register the member's
                    // per-action face tracks so the per-tick stamp pass
                    // (`tick_battle_vram_channel`) re-stamps the current
                    // eye/mouth frame onto the band's live face rows.
                    if let Some(tracks) = form.face_tracks.clone() {
                        party_faces.push(crate::play_battle_vram::BattleMemberFace {
                            actor_slot: form.member,
                            char_index: form.cslot,
                            tracks,
                            art_tracks: form.art_face_tracks.clone(),
                            last_stamps: None,
                            art_counter: None,
                        });
                    }
                }
            }
        }

        if actors.is_empty() {
            return;
        }

        // Mutate phase: install each monster's texture slot and idle clip
        // so the engine's own `tick_battle_animations` (already running in
        // the browser tick) poses it exactly as it does under the native
        // window.
        let faces = party_faces;
        // The battle boundary: retail rebuilds the battle context (the
        // `+0x220` Stone latches and the per-slot palette copies) per
        // fight, and the bands are re-assigned per fight here too - a
        // pristine copy from the last fight would restage the wrong
        // palette. The latch arm (`sync_status`) runs later this same tick.
        self.battle_hud.status_clut.reset();
        if let Some(host) = self.scene_host.host_mut() {
            for p in pending {
                host.world
                    .install_monster_battle_form(p.actor_idx, p.tex_slot, p.idle.as_ref());
            }
        }

        self.battle_render_generation = self.battle_render_generation.wrapping_add(1);
        web_log(&format!(
            "play battle: 3D render built ({} actor meshes, backdrop {}, grid {}, {} faces)",
            actors.len(),
            backdrop.is_some(),
            ground.is_some(),
            faces.len()
        ));
        if let Some(h) = self.scene_host.host_mut() {
            h.world.fog_volume.battle_luma = fog_stage_luma;
            // The backdrop ramp's ceiling reads the same table byte.
            h.world.battle.stage_outdoor = outdoor;
        }
        self.battle_render = Some(BattleRender {
            vram,
            backdrop,
            backdrop_spun,
            shell,
            ground,
            grid_far,
            outdoor,
            actors,
            tex_slots_used,
            generation: self.battle_render_generation,
            faces,
        });
    }

    /// Stage-module render edits, mid-fight - the browser twin of the native
    /// `tick_battle_stage_shell`: apply the battle `MoveImage`s a stage
    /// module queued (`World::apply_battle_vram_moves`) to the battle VRAM,
    /// and rebuild the backdrop when its object list changed
    /// (`SceneHost::battle_stage_object_indices`), bumping the generation so
    /// the page re-uploads the battle scene.
    pub(crate) fn tick_battle_stage_shell_web(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let Some(br) = self.battle_render.as_mut() else {
            host.world.discard_battle_vram_edits();
            return;
        };
        if host.world.apply_battle_vram_moves(&mut br.vram) {
            self.battle_vram.mark_dirty();
        }
        let Some(sh) = br.shell.as_mut() else {
            return;
        };
        let objects = host.battle_stage_object_indices(sh.tmd.objects.len());
        if objects == sh.objects {
            return;
        }
        let layers = legaia_engine_core::scene_host::BattleStageLayers::split(&objects);
        br.backdrop = stage_shell_mesh(&sh.tmd, &sh.raw, sh.second, &layers.fixed);
        br.backdrop_spun = stage_slot_mesh(&sh.tmd, &sh.raw, &layers.spun);
        sh.objects = objects;
        self.battle_render_generation = self.battle_render_generation.wrapping_add(1);
        br.generation = self.battle_render_generation;
    }

    /// Drop the battle render state on the `Battle -> Field` edge. The page
    /// notices `play_battle_active()` fall and restores the field VRAM
    /// texture from `field_vram_bytes` - the field-side copy was never
    /// touched, exactly the native exit contract.
    pub(crate) fn exit_battle_render(&mut self) {
        self.battle_render = None;
        // Release the host-side summon seat. `World::finish_battle` restores
        // the engine actor table, but the slot index is this host's and used
        // to survive into the next fight, where the second cast would reuse a
        // seat the new battle had already handed out.
        // The same release the native window runs (`World::release_summon_seat`):
        // mesh binding, texture slot, clip and pose all go with the seat.
        if let Some(slot) = self.summon_actor_slot.take()
            && let Some(host) = self.scene_host.host_mut()
        {
            host.world.release_summon_seat(slot);
        }
    }
}

/// The page's render record for a party member's battle form: an assembled
/// form uploads unposed with its object ids and frame-0 rest pose (the page
/// animator poses it per frame); a PROT 1204 fallback uploads pre-posed from
/// its PROT 1203 record and is never re-posed.
fn party_actor_render(
    form: &legaia_engine_core::battle_party_form::PartyBattleForm,
) -> Option<BattleActorRender> {
    let (vmesh, object_ids, rest_pose) = if form.assembled {
        let (vmesh, object_ids) =
            legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&form.tmd, &form.tmd_bytes);
        let rest_pose = form
            .idle
            .as_ref()
            .and_then(|a| a.frames.first())
            .map(|f| flatten_frame(f))
            .unwrap_or_default();
        (vmesh, object_ids, rest_pose)
    } else if form.rest_pose.is_empty() {
        let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&form.tmd, &form.tmd_bytes);
        (vmesh, Vec::new(), Vec::new())
    } else {
        let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(
            &form.tmd,
            &form.tmd_bytes,
            &form.rest_pose,
        );
        (vmesh, Vec::new(), Vec::new())
    };
    if vmesh.indices.is_empty() {
        return None;
    }
    Some(BattleActorRender {
        actor_idx: form.member,
        mesh: BattleMesh::textured(vmesh),
        object_ids,
        rest_pose,
    })
}

#[cfg(test)]
mod battle_cam_web_tests;
