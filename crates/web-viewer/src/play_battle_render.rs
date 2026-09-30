//! Browser battle **3D presentation**: the browser twin of the native
//! window's `enter_battle_render` / `exit_battle_render` / `build_battle_stage`
//! (`crates/engine-shell/.../window/battle.rs`). [`crate::play_battle`] owns
//! the rules-side presentation (HUD rows, banner, submenus); this module owns
//! what stands *behind* that overlay once a random encounter fires:
//!
//! * the **battle-stage backdrop**: the scene's `scene_tmd_stream` half-dome
//!   rebuilt through [`SceneLoadKind::Battle`] (the Field build excludes the
//!   stage textures), object-list-edited by
//!   [`legaia_asset::battle_backdrop::drawn_objects_tmd`] (`FUN_800513F0`
//!   drops object 1) and drawn **twice** - the second copy pre-appended here
//!   under the per-stage transform (`SCUS_942.54` mirror table
//!   `DAT_80078B50`, [`legaia_asset::battle_backdrop::MirrorXTable`];
//!   half-turn fallback) with [`legaia_tmd::mesh::VramMesh::append_scaled`]'s
//!   winding flip, so the page uploads ONE mesh and the horizon closes;
//! * the **ground grid** (`func_0x801d02c0`,
//!   [`legaia_asset::battle_backdrop::build_ground_grid`]) plus its per-stage
//!   depth-cue far colour (`DAT_80078C1C`,
//!   [`legaia_engine_vm::battle_ground_grid::OutdoorCueTable`]);
//! * the **battle VRAM**: stage build + flame atlas (PROT 870,
//!   [`legaia_engine_core::scene::upload_flame_atlas_into_vram`]) + per-slot
//!   monster texture injection + the party texture bands, all into a
//!   throwaway copy the page uploads while the fight runs - battle exit
//!   re-uploads the untouched field VRAM (the restore
//!   `crate::runtime::LegaiaRuntime::step_field_vram_fx`'s battle guard
//!   always assumed);
//! * **monster meshes** from the PROT 867 archive
//!   ([`legaia_asset::monster_archive::MonsterMesh::battle_render_mesh`],
//!   per-slot CBA/TSB relocation + texture injection), bound to the live
//!   enemy actor slots with their idle + action clips installed on the world
//!   so the shared battle SM poses them;
//! * **party battle forms**, assembled per character from the player battle
//!   files' equipment-id sections
//!   ([`legaia_asset::battle_char_assembly`], PROT 863..866) and relocated
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
//! window runs - [`legaia_engine_vm::battle_cam_script`] (`FUN_801D5854`
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
//! (`engine-core::battle_status_clut`, `FUN_8004CE2C` pass 4) and the effect
//! CLUT stage (`engine-core::battle_effect_clut`, `FUN_801DEA50`'s palette
//! arm) - run here through [`crate::play_battle_vram`] against the
//! [`BattleRender::vram`] copy, and the page re-uploads it on
//! [`LegaiaRuntime::play_battle_vram_take_dirty`]. The party build below
//! collects each member's face tracks ([`BattleRender::faces`]) for that
//! channel, from the same player-file entries the native loader reads.

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::scene::Scene;
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_engine_core::world::SceneMode;
use wasm_bindgen::prelude::*;

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
    second: legaia_asset::battle_backdrop::SecondCopy,
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
    second: legaia_asset::battle_backdrop::SecondCopy,
    objects: &[usize],
) -> Option<BattleMesh> {
    let tmd0 = legaia_asset::battle_backdrop::objects_tmd(tmd, objects);
    let (mut vmesh, _oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd0, raw);
    let mut flat = crate::packet_color::hybrid(&vmesh, &shading);
    let first = vmesh.clone();
    vmesh.append_scaled(&first, second.scale());
    let flat_copy = flat.clone();
    flat.extend(flat_copy);
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
    /// Enemy-side flag: the archive meshes rest facing `+Z`, so the enemy
    /// side carries the half-turn toward the party (the native
    /// `actor_model` battle rule).
    monster: bool,
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
            // The summon rides the enemy animation pipeline but stands on the
            // party side; the archive meshes rest facing `+Z`, which is the
            // direction the party already faces, so no half-turn.
            monster: false,
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
    second: legaia_asset::battle_backdrop::SecondCopy,
    grid_far: [f32; 3],
    outdoor: bool,
}

/// What the mutate phase installs on a monster once the build borrow ends:
/// its texture slot and archive idle clip. (The party's forms are the
/// engine's to install - `SceneHost::ensure_battle_party_forms`.)
struct PendingMonsterInstall {
    actor_idx: usize,
    tex_slot: u8,
    idle: Option<legaia_asset::monster_archive::MonsterAnimation>,
}

/// Flatten one animation frame to the `[tx, ty, tz, rx, ry, rz] x parts`
/// stream the page animator consumes (angles unsigned 12-bit, `4096` = turn).
fn flatten_frame(frame: &[legaia_asset::monster_archive::PartPose]) -> Vec<i32> {
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
        let host = self.scene_host.as_ref()?;
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
        legaia_engine_core::scene::upload_battle_stage_tims_into_vram(
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
            .and_then(legaia_asset::battle_backdrop::MirrorXTable::from_scus)
            .map(|t| t.second_copy_for_prot_index(stage_entry))
            .unwrap_or(legaia_asset::battle_backdrop::SecondCopy::HalfTurn);
        // Grid depth-cue far colour per stage class (`FUN_80050120`): the
        // sibling SCUS table picks the brightened outdoor arm on the 13
        // wide-open stages; indoor `>> 1` grey covers everything else.
        let grid_far_bytes = self
            .scus
            .as_deref()
            .and_then(legaia_engine_vm::battle_ground_grid::OutdoorCueTable::from_scus)
            .map(|t| t.far_colour_for_prot_index(stage_entry))
            .unwrap_or(legaia_engine_vm::battle_ground_grid::GRID_FAR_INDOOR);
        let outdoor = self
            .scus
            .as_deref()
            .and_then(legaia_engine_vm::battle_ground_grid::OutdoorCueTable::from_scus)
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
        if let Some(host) = self.scene_host.as_mut() {
            host.ensure_battle_party_forms();
        }
        let Some(host) = self.scene_host.as_ref() else {
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
            legaia_engine_core::scene::upload_flame_atlas_into_vram(&host.index, &mut vram, true)
        {
            web_log(&format!("play battle: flame-atlas upload skipped: {e:#}"));
        }

        let mut backdrop = None;
        let mut shell = None;
        let mut ground = None;
        let mut grid_far = None;
        let outdoor = stage.as_ref().is_some_and(|st| st.outdoor);
        // REF: FUN_800513f0 - the backdrop registration whose object-list
        // edit + second-copy transform this host consumes through
        // `legaia_asset::battle_backdrop`, exactly like the native window.
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
                .as_ref()
                .map(|h| h.battle_stage_object_indices(tmd.objects.len()))
                .unwrap_or_else(|| {
                    legaia_asset::battle_backdrop::drawn_object_indices(tmd.objects.len())
                });
            backdrop = stage_shell_mesh(tmd, raw, *second, &objects);
            shell = Some(WebStageShell {
                tmd: tmd.clone(),
                raw: raw.clone(),
                second: *second,
                objects,
            });
            // Flat tiled ground grid under the actors (retail's
            // func_0x801d02c0), textured from the constant retail
            // page/CLUT/UV window the scene battle VRAM populates.
            let grid = legaia_asset::battle_backdrop::build_ground_grid();
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
            let mesh = match legaia_asset::monster_archive::mesh(&archive, monster_id) {
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
            let idle = legaia_asset::monster_archive::idle_animation(&archive, monster_id)
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
                monster: true,
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
        if let Some(host) = self.scene_host.as_mut() {
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
        self.battle_render = Some(BattleRender {
            vram,
            backdrop,
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
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        let Some(br) = self.battle_render.as_mut() else {
            host.world.battle.vram_moves.clear();
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
        br.backdrop = stage_shell_mesh(&sh.tmd, &sh.raw, sh.second, &objects);
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
            && let Some(host) = self.scene_host.as_mut()
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
        monster: false,
        mesh: BattleMesh::textured(vmesh),
        object_ids,
        rest_pose,
    })
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// What this frame's 3D pass should clear to: four linear RGBA floats
    /// from the shared
    /// [`legaia_engine_ui::battle_stage_clear`] selector, the one the native
    /// window renders with.
    ///
    /// The stage dome is a front half, so the open band above it is read as
    /// sky and the clear colour *is* that sky. This page's WebGL path cleared
    /// every mode to one hard-coded near-black, which put a black ceiling
    /// over every browser battle while the native window showed sky.
    pub fn play_scene_clear_color(&self) -> Vec<f32> {
        let stage_battle = self.play_battle_active()
            && self
                .battle_render
                .as_ref()
                .is_some_and(|b| b.stage_present());
        legaia_engine_ui::battle_stage_clear::scene_clear(false, stage_battle).to_vec()
    }

    /// `true` while a battle 3D render is built and the world is in
    /// [`SceneMode::Battle`] - the page's per-frame branch gate.
    pub fn play_battle_active(&self) -> bool {
        self.battle_render.is_some()
            && self
                .scene_host
                .as_ref()
                .is_some_and(|h| h.world.mode == SceneMode::Battle)
    }

    /// Bumps once per battle entry; the page re-uploads the battle scene
    /// when it changes.
    pub fn play_battle_generation(&self) -> u32 {
        self.battle_render
            .as_ref()
            .map(|b| b.generation)
            .unwrap_or(0)
    }

    /// The retail 4x battle world scale the page composes onto actor draws.
    pub fn play_battle_world_scale(&self) -> f32 {
        BATTLE_WORLD_SCALE
    }

    /// The battle VRAM (1 MB): stage + flame atlas + monster/party bands.
    pub fn play_battle_vram_bytes(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .map(|b| b.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_positions(&self) -> Vec<f32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.stage_positions())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_uvs(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.uvs())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_cba_tsb(&self) -> Vec<u16> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_indices(&self) -> Vec<u32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.mesh.indices.clone())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_flat_rgba(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.flat.clone())
            .unwrap_or_default()
    }

    pub fn play_battle_ground_positions(&self) -> Vec<f32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.stage_positions())
            .unwrap_or_default()
    }

    pub fn play_battle_ground_uvs(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.uvs())
            .unwrap_or_default()
    }

    pub fn play_battle_ground_cba_tsb(&self) -> Vec<u16> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_battle_ground_indices(&self) -> Vec<u32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, 255]` packet colours of the ground grid - the
    /// modulation half of retail's `texel * colour / 128`.
    pub fn play_battle_ground_flat_rgba(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.flat.clone())
            .unwrap_or_default()
    }

    /// Ground-grid depth-cue parameters:
    /// `{"far":[r,g,b],"near_z":0,"far_z":Z,"max_ir0":M}` (display 0..1
    /// colour), or `null` when no grid is up. The page attaches this to the
    /// grid placement as a **per-draw** cue, so the browser grid fogs into
    /// the stage's far colour exactly as the native `DrawCue` seam does.
    ///
    /// `far_z` is a VIEW-depth window, so it rides the same
    /// [`BATTLE_WORLD_SCALE`] the grid's vertices do
    /// ([`BattleMesh::stage_positions`]) - otherwise the whole ramp would
    /// collapse into the near field and the floor would read fully fogged.
    pub fn play_battle_ground_cue_json(&self) -> String {
        let Some(far) = self.battle_render.as_ref().and_then(|b| b.grid_far) else {
            return "null".to_string();
        };
        use legaia_engine_vm::battle_ground_grid as grid;
        format!(
            r#"{{"far":[{},{},{}],"near_z":0.0,"far_z":{},"max_ir0":{}}}"#,
            far[0],
            far[1],
            far[2],
            grid::grid_cue_far_z() * BATTLE_WORLD_SCALE,
            grid::grid_cue_max_ir0()
        )
    }

    /// Number of bound battle actor meshes (monsters + party).
    pub fn play_battle_actor_count(&self) -> u32 {
        self.battle_render
            .as_ref()
            .map(|b| b.actors.len() as u32)
            .unwrap_or(0)
    }

    pub fn play_battle_actor_positions(&self, i: u32) -> Vec<f32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.positions())
            .unwrap_or_default()
    }

    pub fn play_battle_actor_uvs(&self, i: u32) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.uvs())
            .unwrap_or_default()
    }

    pub fn play_battle_actor_cba_tsb(&self, i: u32) -> Vec<u16> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_battle_actor_indices(&self, i: u32) -> Vec<u32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, 255]` packet colours of one bound actor mesh.
    /// Without this the combatants drew at the raw texel while the backdrop
    /// carried its modulation, so the party read brighter than the arena.
    pub fn play_battle_actor_flat_rgba(&self, i: u32) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.mesh.flat.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object ids (the pose rig); empty = the upload is
    /// statically posed and must not be re-posed.
    pub fn play_battle_actor_object_ids(&self, i: u32) -> Vec<u32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.actors.get(i as usize))
            .map(|a| a.object_ids.clone())
            .unwrap_or_default()
    }

    /// Live world transforms of every battle actor mesh, flattened
    /// `[x, y, z, monster_flip, active]` per actor in mesh order. Positions
    /// are RAW battle world units - the page multiplies by
    /// [`Self::play_battle_world_scale`] (retail composes the same 4x on
    /// the actor camera).
    ///
    /// `active` is the draw gate: it also carries the summon band's hide
    /// (`+0x21C = 0xFF`, `RENDER_FLAG_HIDDEN`) - every party seat and living
    /// monster is off screen while the creature performs - and retail's
    /// per-body battle draw verdict ([`Self::battle_draw_plan`]: the
    /// `FUN_800480D8` colour-word / grey gate over the `FUN_8004A908` tint,
    /// plus the dispatcher's near-plane reject) - the browser twin of the
    /// native draw loop's skips.
    pub fn play_battle_actor_transforms(&self) -> Vec<f32> {
        use legaia_engine_vm::battle_target_group::RENDER_FLAG_HIDDEN;
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(br.actors.len() * 5);
        for a in &br.actors {
            match host.world.actors.get(a.actor_idx) {
                Some(actor) => out.extend_from_slice(&[
                    actor.move_state.world_x as f32,
                    actor.move_state.world_y as f32,
                    actor.move_state.world_z as f32,
                    if a.monster { 1.0 } else { 0.0 },
                    if actor.active
                        && actor.battle.render_flag != RENDER_FLAG_HIDDEN
                        && self.battle_draw_plan(a.actor_idx).is_none_or(|p| p.drawn)
                    {
                        1.0
                    } else {
                        0.0
                    },
                ]),
                None => out.extend_from_slice(&[0.0; 5]),
            }
        }
        out
    }

    /// Battle actor `i`'s current pose: 6 `i32` per part
    /// (`[tx, ty, tz, rx, ry, rz]`, absolute PSX 4096-unit angles) - the
    /// live `pose_frame` the engine's battle-animation tick maintains, or
    /// the build-time rest pose until the first battle tick lands. Empty =
    /// draw the uploaded geometry as-is.
    pub fn play_battle_actor_pose(&self, i: u32) -> Vec<i32> {
        let Some(br) = self.battle_render.as_ref() else {
            return Vec::new();
        };
        let Some(a) = br.actors.get(i as usize) else {
            return Vec::new();
        };
        if a.object_ids.is_empty() {
            return Vec::new();
        }
        if let Some(pose) = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.actors.get(a.actor_idx))
            .and_then(|actor| actor.pose_frame.as_ref())
            && !pose.bone_outputs.is_empty()
        {
            let mut out = Vec::with_capacity(pose.bone_outputs.len() * 6);
            for (t, r) in &pose.bone_outputs {
                out.extend_from_slice(&[
                    t[0] as i32,
                    t[1] as i32,
                    t[2] as i32,
                    r[0] as i32,
                    r[1] as i32,
                    r[2] as i32,
                ]);
            }
            return out;
        }
        a.rest_pose.clone()
    }

    /// Battle actor `i`'s **arts after-image ghosts** this frame - the
    /// browser seat of the retail walk `FUN_80049348`
    /// (`legaia_engine_core::battle_afterimage`; the plan comes from the
    /// shared `World::battle_ghost_draws`). Seven `f32` per ghost:
    /// `[world_x, world_y, world_z, r, g, b, k]` with RGB normalized `0..1`
    /// (the page draws each ghost flat-coloured additive via its cue
    /// override) and `k` the eye-push scale factor: the position is already
    /// pushed to `eye + k * (pos - eye)` and the page multiplies its draw
    /// scale by `k`, which together is retail's deeper-OT-bucket ordering
    /// under a depth buffer (see
    /// `legaia_engine_core::battle_afterimage::ghost_eye_push_scale` - the
    /// screen silhouette is unchanged, the body's opaque depth wins where
    /// they overlap). The matching pose is
    /// [`Self::play_battle_actor_ghost_pose`]. Empty outside a
    /// SpecialStarter dash / non-idle monster clip.
    pub fn play_battle_actor_ghosts(&self, i: u32) -> Vec<f32> {
        use legaia_engine_core::battle_afterimage as ai;
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let Some(a) = br.actors.get(i as usize) else {
            return Vec::new();
        };
        // The eye of the page's own vp (aspect only shapes the projection
        // rows, not the centre of projection). The vp's input space is the
        // page's scaled placement space, so the eye comes back scaled - the
        // ghost push runs in raw actor units.
        let pose = self.battle_cam_pose();
        let vp =
            legaia_engine_vm::battle_cam_script::battle_vp(&pose, BATTLE_WORLD_SCALE, 4.0 / 3.0);
        let eye_raw = ai::camera_eye_from_vp(&vp).map(|e| e.map(|c| c / BATTLE_WORLD_SCALE));
        let body = host.world.actors.get(a.actor_idx).map(|w| {
            [
                f32::from(w.move_state.world_x),
                f32::from(w.move_state.world_y),
                f32::from(w.move_state.world_z),
            ]
        });
        let mut out = Vec::new();
        for g in host.world.battle_ghost_draws() {
            if g.actor_slot as usize != a.actor_idx {
                continue;
            }
            let gpos = [g.pos[0] as f32, g.pos[1] as f32, g.pos[2] as f32];
            let (pos, k) = match (eye_raw, body) {
                (Some(e), Some(b)) => {
                    let k = ai::ghost_eye_push_scale(e, gpos, b, ai::GHOST_EYE_PUSH_MARGIN);
                    (
                        [
                            e[0] + k * (gpos[0] - e[0]),
                            e[1] + k * (gpos[1] - e[1]),
                            e[2] + k * (gpos[2] - e[2]),
                        ],
                        k,
                    )
                }
                _ => (gpos, 1.0),
            };
            out.extend_from_slice(&[
                pos[0],
                pos[1],
                pos[2],
                f32::from(g.color[0]) / 255.0,
                f32::from(g.color[1]) / 255.0,
                f32::from(g.color[2]) / 255.0,
                k,
            ]);
        }
        out
    }

    /// Ghost `g` of battle actor `i`'s pose, in the
    /// [`Self::play_battle_actor_pose`] shape (6 `i32` per part). Empty when
    /// the ghost is not active this frame.
    pub fn play_battle_actor_ghost_pose(&self, i: u32, g: u32) -> Vec<i32> {
        let (Some(br), Some(host)) = (self.battle_render.as_ref(), self.scene_host.as_ref()) else {
            return Vec::new();
        };
        let Some(a) = br.actors.get(i as usize) else {
            return Vec::new();
        };
        let draws = host.world.battle_ghost_draws();
        let Some(d) = draws
            .iter()
            .filter(|d| d.actor_slot as usize == a.actor_idx)
            .nth(g as usize)
        else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.pose.bone_outputs.len() * 6);
        for (t, r) in &d.pose.bone_outputs {
            out.extend_from_slice(&[
                t[0] as i32,
                t[1] as i32,
                t[2] as i32,
                r[0] as i32,
                r[1] as i32,
                r[2] as i32,
            ]);
        }
        out
    }

    /// The battle camera pose for this frame, in the retail value space:
    /// `{"active":true,"pitch":P,"yaw":Y,"tr":[x,y,z],"focus":[x,y,z],
    /// "h":256}` - pitch/yaw in PSX 12-bit units, `tr` the eye-space
    /// translation trio (TR.z already through the `(z << 8) / 0xA0`
    /// projection prescale), `focus` the world point the camera orbits in
    /// RAW battle units. The live pose of the SAME phase-scripted camera
    /// the native window runs ([`legaia_engine_vm::battle_cam_script`]:
    /// dialogue close-up / far menu framing with the idle orbit /
    /// per-character submenu close-up, with the measured glides between).
    /// Kept as a diagnostic/value-space export; the page's projection
    /// consumes [`Self::play_battle_camera_vp`] instead.
    pub fn play_battle_camera_json(&self) -> String {
        if !self.play_battle_active() {
            return r#"{"active":false}"#.to_string();
        }
        let pose = self.battle_cam_pose();
        format!(
            r#"{{"active":true,"phase":"{}","pitch":{},"yaw":{},"tr":[{},{},{}],"focus":[{},{},{}],"h":{}}}"#,
            self.battle_cam_phase_label(),
            pose.pitch,
            pose.yaw,
            pose.tr[0],
            pose.tr[1],
            pose.tr[2],
            pose.focus[0],
            pose.focus[1],
            pose.focus[2],
            legaia_engine_vm::battle_cam_script::GTE_H,
        )
    }

    /// The battle view-projection matrix for this frame: 16 column-major
    /// floats (WebGL `mat4` layout), or empty outside battle. The shared
    /// [`legaia_engine_vm::battle_cam_script::battle_vp`] - the native
    /// window's `psx_camera_mvp` composition - over the live phase-scripted
    /// pose, with the retail 4x battle world scale folded into the focus
    /// exactly like the native `battle_dome_camera_mvp`. The page hands
    /// this straight to its renderer (`cam.vp`), so there is no JS-side
    /// projection model to drift.
    pub fn play_battle_camera_vp(&self, aspect: f32) -> Vec<f32> {
        if !self.play_battle_active() {
            return Vec::new();
        }
        let pose = self.battle_cam_pose();
        legaia_engine_vm::battle_cam_script::battle_vp(&pose, BATTLE_WORLD_SCALE, aspect).to_vec()
    }
}

impl LegaiaRuntime {
    /// The live phase-scripted pose, or the shared BOOT_POSE on the first
    /// frame before the camera tick armed the state (the same fallback the
    /// native `battle_dome_camera_mvp` renders).
    /// Which framing the shared phase script has the camera in, as the JSON
    /// export's `phase` string.
    fn battle_cam_phase_label(&self) -> &'static str {
        use legaia_engine_vm::battle_cam_script::BattleCamPhase as P;
        match self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.battle.camera.as_ref())
            .map(|c| c.phase())
        {
            Some(P::Dialogue) => "dialogue",
            Some(P::Submenu) => "submenu",
            Some(P::Action) => "action",
            Some(P::Recover) => "recover",
            Some(P::ActionEnd) => "action-end",
            Some(P::Menu) | None => "menu",
        }
    }

    /// Battle body `actor_idx`'s draw decision this frame - the shared
    /// `World::battle_actor_draw_plan` under the camera this page projects
    /// with, judged at retail's parked depth when no stage dome is up (the
    /// native window's rule).
    pub(crate) fn battle_draw_plan(
        &self,
        actor_idx: usize,
    ) -> Option<legaia_engine_core::world::BattleActorDrawPlan> {
        let br = self.battle_render.as_ref()?;
        let host = self.scene_host.as_ref()?;
        let pose = br.backdrop.is_some().then(|| self.battle_cam_pose());
        host.world
            .battle_actor_draw_plan(actor_idx, pose.as_ref(), BATTLE_WORLD_SCALE, br.outdoor)
    }

    /// The engine's battle-camera pose (`World::battle_cam_pose` - the state
    /// `World::tick` steps for every host), or the shared boot pose off a
    /// scene host.
    pub(crate) fn battle_cam_pose(&self) -> legaia_engine_vm::battle_cam_script::BattleCamPose {
        self.scene_host
            .as_ref()
            .map(|h| h.world.battle_cam_pose())
            .unwrap_or(legaia_engine_vm::battle_cam_script::BOOT_POSE)
    }
}

#[cfg(test)]
mod battle_cam_web_tests {
    use super::*;
    use legaia_engine_core::world::World;
    use legaia_engine_vm::battle_cam_script as script;

    /// **Cross-host entry-azimuth pin, browser half.** The twin of the
    /// native window's `battle_entry_yaw_tests` (`engine-shell`
    /// `window/camera.rs`): both hosts must hand `drive` the *guarded*
    /// azimuth, not the raw compass word. This host handed over the raw word,
    /// and since the port's fixed follow camera publishes a constant `0`
    /// while nobody orbits, that is the seat-axis azimuth on which the two
    /// combatant rows project to the same screen X.
    #[test]
    fn web_an_on_axis_entry_azimuth_is_replaced() {
        for az in [0u16, 8, 160, 191, 2048, 2100, 4090] {
            let mut world = World::default();
            world.locomotion.camera_azimuth = az;
            assert_eq!(
                legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world).entry_yaw,
                script::BATTLE_ENTRY_YAW_SAMPLE,
                "azimuth {az} frames both rows at the same screen X"
            );
        }
    }

    /// A real orbit sample survives untouched - the five captured retail
    /// battle yaws.
    #[test]
    fn web_a_real_orbit_azimuth_survives() {
        for az in [224u16, 2632, 3136, 3808, 3882] {
            let mut world = World::default();
            world.locomotion.camera_azimuth = az;
            assert_eq!(
                legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world).entry_yaw,
                f32::from(az)
            );
        }
    }

    /// **Cross-host heading pin, browser half.** The identical world recipe
    /// as the native `the_camera_reads_the_battle_heading_not_the_field_heading`
    /// (`engine-shell` `window/camera.rs` tests): an off-axis seat whose
    /// field heading `+0x26` and battle heading `+0x46` deliberately differ.
    /// Retail's framing subtracts `+0x46` (`FUN_801E295C` case `0x14` writes
    /// it at `0x801E32EC..0x801E3318`), so both hosts must land on the same
    /// yaw literal.
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn web_camera_reads_the_battle_heading_not_the_field_heading() {
        const FIELD_26: u16 = 0x333;
        const BATTLE_46: u16 = 0x600;

        let mut world = World::default();
        world.mode = SceneMode::Battle;
        world.party.party_count = 1;
        let mut vahn = legaia_engine_core::world::Actor::default();
        vahn.active = true;
        vahn.move_state.world_x = 600;
        vahn.move_state.world_z = -775;
        vahn.move_state.render_26 = FIELD_26 as i16;
        vahn.battle.facing_angle = BATTLE_46;
        let mut tetsu = legaia_engine_core::world::Actor::default();
        tetsu.active = true;
        tetsu.move_state.world_x = -400;
        tetsu.move_state.world_z = 900;
        tetsu.battle_monster_id = Some(1);
        tetsu.tmd_binding = Some(0);
        world.actors = vec![vahn, tetsu];
        world.battle.command = Some(legaia_engine_core::battle_input::BattleCommandSession::new(
            0, 0,
        ));
        // The **arts input** picker is what arms retail's case-0 close-up;
        // the command chooser alone keeps the far framing (see
        // `script::phase_for_state`). Same recipe as the native mirror.
        world.battle.arts_menu = Some(legaia_engine_core::battle_arts::BattleArtsSession::new(
            0,
            0,
            Vec::new(),
        ));

        let inputs = legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world);
        assert_eq!(inputs.phase, script::BattleCamPhase::Submenu);
        let acting = inputs.acting.expect("acting actor");
        assert_eq!(acting.facing, i32::from(BATTLE_46), "actor[+0x46]");
        assert_ne!(acting.facing, i32::from(FIELD_26), "not the field +0x26");

        let mut slot = None;
        for f in 0..=6u64 {
            script::drive(&mut slot, true, inputs, f * 2, None);
        }
        let yaw = slot.as_ref().unwrap().pose().yaw.rem_euclid(4096.0);
        // The SAME literals the native mirror asserts.
        assert_eq!(yaw, 752.0, "0x8F0 - 0x600");
        assert_ne!(yaw, 1469.0, "0x8F0 - 0x333 (the field heading)");
    }

    /// **Cross-host presence pin, browser half.** The identical world recipe
    /// as the native `the_formation_box_is_a_world_fact_not_a_render_fact`:
    /// a live combatant seated off the origin whose mesh this host has NOT
    /// bound. Retail's case-9 walk gates on `actor[+0x14c]` (`0x801D7000`)
    /// and consults no mesh, so the box must contain it on both hosts.
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn web_formation_box_is_a_world_fact_not_a_render_fact() {
        let mut world = World::default();
        world.mode = SceneMode::Battle;
        world.party.party_count = 1;
        let mut vahn = legaia_engine_core::world::Actor::default();
        vahn.active = true;
        vahn.move_state.world_z = -800;
        let mut tetsu = legaia_engine_core::world::Actor::default();
        tetsu.active = true;
        tetsu.move_state.world_x = 600;
        tetsu.move_state.world_z = 900;
        tetsu.battle_monster_id = Some(1);
        tetsu.tmd_binding = None;
        world.actors = vec![vahn, tetsu];

        let formation = legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world).formation;
        assert_eq!(
            formation,
            Some(script::FormationBox {
                min: [0.0, -800.0],
                max: [600.0, 900.0],
            }),
            "the unbound-but-live monster is inside the framing box"
        );
        assert_ne!(
            formation,
            Some(script::FormationBox {
                min: [0.0, -800.0],
                max: [0.0, -800.0],
            })
        );
    }

    /// **Cross-host single-model pin, browser half.** The identical world
    /// recipe as the native `native_pose_matches_the_shared_recipe`
    /// (`engine-shell` `window/camera.rs` tests) - solo Vahn at the
    /// tutorial seat, command menu open, one bound monster - driven through
    /// THIS host's derivation + the shared drive must land on the same
    /// literal measured close-up, so the two hosts' poses are equal to each
    /// other by transitivity.
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn web_pose_matches_the_native_recipe() {
        let mut world = World::default();
        world.mode = SceneMode::Battle;
        world.party.party_count = 1;
        let mut vahn = legaia_engine_core::world::Actor::default();
        vahn.active = true;
        vahn.move_state.world_x = 0;
        vahn.move_state.world_y = 0;
        vahn.move_state.world_z = -800;
        vahn.move_state.render_26 = 0;
        let mut tetsu = legaia_engine_core::world::Actor::default();
        tetsu.active = true;
        tetsu.move_state.world_z = 800;
        tetsu.battle_monster_id = Some(1);
        tetsu.tmd_binding = Some(0);
        world.actors = vec![vahn, tetsu];
        world.battle.command = Some(legaia_engine_core::battle_input::BattleCommandSession::new(
            0, 0,
        ));
        // The **arts input** picker is what arms retail's case-0 close-up;
        // the command chooser alone keeps the far framing (see
        // `script::phase_for_state`). Same recipe as the native mirror.
        world.battle.arts_menu = Some(legaia_engine_core::battle_arts::BattleArtsSession::new(
            0,
            0,
            Vec::new(),
        ));

        let inputs = legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world);
        assert_eq!(inputs.phase, script::BattleCamPhase::Submenu);
        assert_eq!(
            inputs.acting,
            Some(script::BattleCamActor {
                facing: 0,
                world: [0.0, 0.0, -800.0],
                height: None,
            })
        );
        assert_eq!(
            inputs.formation,
            Some(script::FormationBox {
                min: [0.0, -800.0],
                max: [0.0, 800.0],
            })
        );
        let mut slot = None;
        for f in 0..=6u64 {
            script::drive(&mut slot, true, inputs, f * 2, None);
        }
        let pose = slot.as_ref().unwrap().pose();
        // The measured solo-Vahn submenu close-up - the SAME literals the
        // native mirror test asserts.
        assert_eq!(pose.pitch, 32.0);
        assert_eq!(pose.yaw.rem_euclid(4096.0), 2288.0);
        assert_eq!(pose.tr, [-512.0, 1152.0, 2457.0]);
        assert_eq!(pose.focus, [0.0, 0.0, -800.0]);
    }
}
