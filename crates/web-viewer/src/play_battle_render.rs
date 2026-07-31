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
//! What the native battle render still has that this host lacks: the per-tick
//! facial-animation VRAM re-stamps (`tick_battle_face_stamps`), the
//! battle-intro screen-prim emitter, and the field move-VM stager parts
//! (`build_field_fx_part_draws`, which resolve against the scene TMD pack the
//! page does not upload while a battle is on screen).

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::scene::{Scene, SceneHost};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_engine_core::world::SceneMode;
use wasm_bindgen::prelude::*;

/// Retail 4x uniform battle world scale (base matrix `0x8007BF10` =
/// `16384 * I`; GTE `4096` = 1.0). The page composes it onto the actor draws
/// (mesh scale + pre-scaled translation) exactly as the native redraw
/// composes it onto the actor camera - the backdrop + grid stay at raw world
/// coordinates, like retail.
const BATTLE_WORLD_SCALE: f32 = 4.0;

/// PROT entry of the monster stat/mesh archive (`0867_battle_data`).
const MONSTER_ARCHIVE_PROT_INDEX: u32 = 867;

/// PROT entry of the first player battle file (`data\battle\PLAYER1`,
/// extraction 863; `+ char_slot` for Noa / Gala / Terra).
const PLAYER_BATTLE_FILE_BASE: u32 = 863;

/// readef.DAT (extraction PROT 894) - the battle side-band whose slots carry
/// the per-character art-animation "ME" archives.
const READEF_PROT_INDEX: u32 = 894;

/// Derive the shared battle-camera drive inputs from the live world state -
/// phase, acting actor, formation box. The browser mirror of the native
/// window's `battle_cam_inputs` (`engine-shell` `window/camera.rs`) over the
/// same `World` type: same phase booleans, same acting-actor formula
/// (facing `render_26 & 0xFFF`, height keyed on `party_slot + 1` through the
/// disc table, focus = the actor's world position), same case-9 min/max
/// walk. The one host difference is the presence predicate: this host keys
/// monsters on `battle_monster_id` (the world-side fact) where the native
/// window keys on its own `tmd_binding` mesh bind - the two coincide for
/// every monster whose mesh decodes. Pinned to the native derivation by
/// `web_pose_matches_the_native_recipe`.
fn derive_battle_cam(
    world: &legaia_engine_core::world::World,
) -> legaia_engine_vm::battle_cam_script::BattleCamInputs {
    use legaia_engine_vm::battle_cam_script as script;
    // The submenu close-up frames whoever owns the menu; the action framing
    // frames whoever is acting (`ctx[+0x13]`).
    let acting_slot = world
        .battle_command
        .as_ref()
        .map(|c| c.actor)
        .unwrap_or(world.battle_ctx.active_actor);
    let phase = script::phase_for(
        world.current_dialog.is_some() || world.inline_dialogue.is_some(),
        world.battle_command.is_some()
            || world.battle_arts_menu.is_some()
            || world.battle_spell_menu.is_some()
            || world.battle_item_menu.is_some(),
        script::action_state_frames_the_action(world.battle_ctx.action_state),
    );
    let actor_at = |slot: u8, party_slot: Option<u8>| {
        let a = world.actors.get(slot as usize)?;
        // Retail's height key is `DAT_8007BD10[slot]`, the 1-based
        // party-record selector; the engine's party rows are that record
        // order, so the row index + 1 is the same id.
        let height = party_slot.and_then(|p| {
            world
                .battle_camera_heights
                .as_ref()
                .and_then(|t| t.height_for_char_id(p + 1))
                .map(|h| h as f32)
        });
        Some(script::BattleCamActor {
            // The port's actors carry their heading in `render_26` (the
            // retail `+0x26` 12-bit angle); it stands in for the battle
            // actor record's `+0x46` the framing formula subtracts.
            facing: (a.move_state.render_26 as u16 & 0xFFF) as i32,
            world: [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ],
            height,
        })
    };
    let acting = match world.battle_command.as_ref() {
        Some(c) => actor_at(c.actor, Some(c.party_slot)),
        None => actor_at(acting_slot, None),
    };
    // The far menu framing sizes its depth to - and centres on - the live
    // formation's X/Z bounding box (`FUN_801D5854` case 9).
    let pc = world.party_count as usize;
    let mut formation: Option<script::FormationBox> = None;
    for (i, a) in world.actors.iter().enumerate() {
        if !(i < pc || a.battle_monster_id.is_some()) {
            continue;
        }
        script::FormationBox::extend(
            &mut formation,
            a.move_state.world_x as f32,
            a.move_state.world_z as f32,
        );
    }
    // `FUN_801D5854` case 6: `party_slot` is retail's `ctx[+0x13] < 3` over
    // the engine's party band, `char_id` its `DAT_8007BD10[slot]`, and
    // `depth_raw` is `ctx[+0x6D0]` - what `camera_height_for_frame` last
    // computed. `flow_active` is `_DAT_8007BD71 == 0xFE`, which the engine
    // has no byte for and which is true whenever it runs this camera.
    let party = usize::from(acting_slot) < pc;
    script::BattleCamInputs {
        phase,
        acting,
        formation,
        action: script::ActionFraming {
            party_slot: party,
            flow_active: true,
            depth_raw: world.battle_camera_frame_height as i32,
            yaw_base: 0,
            style: 0,
            char_id: if party { acting_slot + 1 } else { 0 },
        },
        shake_amplitude: world.camera_shake_amplitude,
    }
}

/// Host-safe log: the browser console on wasm, stderr on the native test
/// build (`crate::console_log` is a hard wasm-only stub that panics off-web,
/// and this module runs under the disc-gated native oracles).
pub(crate) fn web_log(s: &str) {
    #[cfg(target_arch = "wasm32")]
    crate::console_log(s);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{s}");
}

/// One page-uploadable battle mesh in the play page's scene-mesh shape.
struct BattleMesh {
    mesh: legaia_tmd::mesh::VramMesh,
    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid shader; empty =
    /// fully textured (the page passes `null`).
    flat: Vec<u8>,
}

impl BattleMesh {
    fn positions(&self) -> Vec<f32> {
        self.mesh.positions.iter().flatten().copied().collect()
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
    ground: Option<BattleMesh>,
    /// Ground-grid depth-cue far colour, display `0..1`, applied by the page
    /// as a **per-draw** cue on the grid mesh (the native `DrawCue` seam).
    grid_far: Option<[f32; 3]>,
    actors: Vec<BattleActorRender>,
    /// How many of the five battle texture slots the entry build consumed.
    /// A mid-battle summon injects its creature texture into the next one
    /// ([`LegaiaRuntime::spawn_summon_creature_web`]) - the browser twin of
    /// the native window's `battle_tex_slots_used`.
    pub(crate) tex_slots_used: u8,
    /// The shared phase-scripted battle camera - the SAME
    /// [`legaia_engine_vm::battle_cam_script::BattleCamera`] the native
    /// window steps, driven by [`LegaiaRuntime::tick_battle_camera_web`].
    camera: Option<legaia_engine_vm::battle_cam_script::BattleCamera>,
    /// Bumped per battle entry - and once more per mid-battle summon spawn -
    /// so the page knows to re-upload.
    pub(crate) generation: u32,
}

impl BattleRender {
    /// World actor-table slot of each bound mesh, in mesh order. The FX
    /// exports key their per-actor rows off this so they stay
    /// index-parallel with `play_battle_actor_transforms`.
    pub(crate) fn actor_slots(&self) -> Vec<usize> {
        self.actors.iter().map(|a| a.actor_idx).collect()
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
            mesh: BattleMesh {
                mesh,
                flat: Vec::new(),
            },
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
}

/// Everything one actor bind needs applied to the world after the read-only
/// build phase (the world borrow is exclusive, so mesh building and world
/// mutation are two passes).
struct PendingClips {
    actor_idx: usize,
    idle: Option<legaia_asset::monster_archive::MonsterAnimation>,
    action_clips: Option<Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>>,
    art_bank: Option<Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>>,
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

/// Hybrid flat-RGBA stream from a `tmd_to_vram_mesh_field_hybrid` shading
/// block: `[r, g, b, textured_flag]` per vertex.
fn hybrid_flat(shading: &legaia_tmd::mesh::VertexShading) -> Vec<u8> {
    let mut flat = Vec::with_capacity(shading.colors.len() * 4);
    for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
        flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
    }
    flat
}

impl LegaiaRuntime {
    /// Build the current scene's battle-stage bundle: stage VRAM, dome TMD,
    /// second-copy transform, grid far colour. The browser copy of the
    /// native `build_battle_stage`; `None` when the scene has no stage entry
    /// or the battle-kind resource build fails.
    fn build_battle_stage(&self) -> Option<WebBattleStage> {
        let host = self.scene_host.as_ref()?;
        let scene = host.scene.as_ref()?;
        let stage_entry = host.index.battle_stage_entry_for_scene(&scene.name)?;
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
        Some(WebBattleStage {
            vram: res.vram.clone(),
            dome: (dome.tmd.clone(), dome.raw.clone()),
            second,
            grid_far: grid_far_bytes.map(|c| f32::from(c) / 255.0),
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
        let mut ground = None;
        let mut grid_far = None;
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
            let tmd0 = legaia_asset::battle_backdrop::drawn_objects_tmd(tmd);
            let (mut vmesh, _oids, shading) =
                legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd0, raw);
            let mut flat = hybrid_flat(&shading);
            // The disc shell is an authored HALF; the second copy under the
            // per-stage transform closes the horizon. `append_scaled`
            // reverses winding on a negative determinant (the mesh-level
            // analogue of retail's 0x40000000 -> 0x48000000 draw-mode swap).
            let first = vmesh.clone();
            vmesh.append_scaled(&first, second.scale());
            let flat_copy = flat.clone();
            flat.extend(flat_copy);
            if !vmesh.indices.is_empty() {
                backdrop = Some(BattleMesh { mesh: vmesh, flat });
            }
            // Flat tiled ground grid under the actors (retail's
            // func_0x801d02c0), textured from the constant retail
            // page/CLUT/UV window the scene battle VRAM populates.
            let grid = legaia_asset::battle_backdrop::build_ground_grid();
            if !grid.indices.is_empty() {
                ground = Some(BattleMesh {
                    mesh: grid,
                    flat: Vec::new(),
                });
                grid_far = Some(*gf);
            }
        }

        // Monster meshes: per-slot texture injection into the battle VRAM +
        // idle / action clips for the shared SM pose hook.
        let mut actors: Vec<BattleActorRender> = Vec::new();
        let mut pending: Vec<PendingClips> = Vec::new();
        // Highest battle texture slot the monster binds consumed; a
        // mid-battle summon injects its creature texture into the next one.
        let mut tex_slots_used: u8 = 0;
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
            tex_slots_used = tex_slots_used.max(slot.saturating_add(1));
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
            let action_clips = match legaia_asset::monster_archive::animations(&archive, monster_id)
            {
                Ok(Some(anims)) if !anims.is_empty() => Some(anims.into_iter().map(Some).collect()),
                _ => None,
            };
            actors.push(BattleActorRender {
                actor_idx,
                monster: true,
                mesh: BattleMesh {
                    mesh: vmesh,
                    flat: Vec::new(),
                },
                object_ids,
                rest_pose,
            });
            pending.push(PendingClips {
                actor_idx,
                idle,
                action_clips,
                art_bank: None,
            });
        }

        // Party battle forms per present-party ordinal: the CHARACTER picks
        // the content (player file 863 + cslot), the ORDINAL picks the
        // runtime texture band - the live-verified retail rule the native
        // window applies.
        let party_count = host.world.party_count as usize;
        let pack = host
            .index
            .entry_bytes(legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)
            .ok()
            .zip(
                host.index
                    .entry_bytes(legaia_asset::battle_char_pack::ATLAS_PROT_ENTRY_INDEX)
                    .ok(),
            )
            .and_then(|(m, a)| legaia_asset::battle_char_pack::parse(&m, &a).ok());
        if party_count > 0
            && let Some(pack) = pack.as_ref()
        {
            // The 8 Baka Fighter authoring atlases: the fallback meshes
            // sample these rects.
            for atlas in &pack.atlases {
                if let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes) {
                    vram.upload_tim(&tim);
                }
            }
            // PROT 1203: the rest-pose bank for the 1204 FALLBACK meshes
            // only (banks Vahn 0-8 / Noa 9-17 / Gala 18-26; bone i = 1204
            // object i). The assembled path poses from the player file's
            // own idle stream instead.
            let battle_anm = host.index.entry_bytes_extended(1203).ok().and_then(|raw| {
                [6usize, 3, 5, 7].iter().find_map(|&dc| {
                    legaia_asset::player_anm::find_in_entry(&raw, dc)
                        .into_iter()
                        .next()
                })
            });
            for member in 0..party_count.min(3) {
                let cslot = host.world.party_roster_slot(member);
                if let Some((render, clips)) = self.build_party_actor(
                    host,
                    &mut vram,
                    pack,
                    battle_anm.as_ref(),
                    member,
                    cslot,
                ) {
                    actors.push(render);
                    pending.push(clips);
                }
            }
        }

        if actors.is_empty() {
            return;
        }

        // Mutate phase: install each actor's clips on the world so the
        // engine's own `tick_battle_animations` (already running in the
        // browser tick) maintains `pose_frame` / reaction clips exactly as
        // it does under the native window.
        if let Some(host) = self.scene_host.as_mut() {
            for p in pending {
                if let Some(idle) = &p.idle
                    && let Some(player) =
                        legaia_engine_core::battle_anim::MonsterAnimPlayer::new(idle)
                {
                    host.world.set_actor_battle_animation(p.actor_idx, player);
                }
                if let Some(clips) = p.action_clips {
                    host.world
                        .set_actor_battle_action_clips(p.actor_idx, std::sync::Arc::new(clips));
                }
                if let Some(bank) = p.art_bank.filter(|b| !b.is_empty()) {
                    host.world
                        .set_actor_battle_art_bank(p.actor_idx, std::sync::Arc::new(bank));
                }
            }
        }

        self.battle_render_generation = self.battle_render_generation.wrapping_add(1);
        web_log(&format!(
            "play battle: 3D render built ({} actor meshes, backdrop {}, grid {})",
            actors.len(),
            backdrop.is_some(),
            ground.is_some()
        ));
        self.battle_render = Some(BattleRender {
            vram,
            backdrop,
            ground,
            grid_far,
            actors,
            tex_slots_used,
            camera: None,
            generation: self.battle_render_generation,
        });
    }

    /// Drop the battle render state on the `Battle -> Field` edge. The page
    /// notices `play_battle_active()` fall and restores the field VRAM
    /// texture from `field_vram_bytes` - the field-side copy was never
    /// touched, exactly the native exit contract.
    pub(crate) fn exit_battle_render(&mut self) {
        self.battle_render = None;
    }

    /// Drive the shared phase-scripted battle camera one sim tick: derive
    /// the retail phase + acting actor + formation from the live world
    /// state (the browser twin of the native `tick_battle_camera` /
    /// `battle_cam_inputs`) and step the SAME
    /// [`legaia_engine_vm::battle_cam_script::drive`] on the world's
    /// display-frame counter (one camera step per 2 frames). No-op outside
    /// battle - the render state only exists while one is up, and dropping
    /// it drops the camera so the next battle re-snaps.
    pub(crate) fn tick_battle_camera_web(&mut self) {
        let Some(br) = self.battle_render.as_mut() else {
            return;
        };
        let Some(host) = self.scene_host.as_ref() else {
            br.camera = None;
            return;
        };
        let world = &host.world;
        let active = world.mode == SceneMode::Battle;
        let inputs = derive_battle_cam(world);
        legaia_engine_vm::battle_cam_script::drive(
            &mut br.camera,
            active,
            inputs,
            world.field_frames,
        );
    }

    /// Assemble one party member's battle form: the browser port of the
    /// native `assembled_party_battle_mesh` + its caller loop (equipment
    /// splice, band relocation, texture-pool/palette uploads into `vram`,
    /// idle + action + swing + art-bank clips), with the PROT 1204 mesh +
    /// PROT 1203 static rest pose as the fallback ladder. Facial-animation
    /// tracks are not ported on this host (no per-tick VRAM re-stamp pass).
    #[allow(clippy::too_many_arguments)]
    fn build_party_actor(
        &self,
        host: &SceneHost,
        vram: &mut legaia_tim::Vram,
        pack: &legaia_asset::battle_char_pack::BattleCharPack,
        battle_anm: Option<&legaia_asset::player_anm::PlayerAnmBundle>,
        member: usize,
        cslot: usize,
    ) -> Option<(BattleActorRender, PendingClips)> {
        use legaia_asset::battle_char_assembly as bca;
        let prot = PLAYER_BATTLE_FILE_BASE + cslot as u32;
        let raw = host.index.entry_bytes_extended(prot).ok();
        let file_pack = raw
            .as_deref()
            .and_then(|r| legaia_asset::battle_data_pack::parse(r).ok());
        // Equipped item ids from the canonical roster record; an absent or
        // zeroed record assembles the all-default (unequipped) sections.
        let equipped: [u8; 5] = host
            .world
            .roster
            .members
            .get(cslot)
            .map(|rec| {
                let slots = rec.equipment().slots;
                [slots[0], slots[1], slots[2], slots[3], slots[4]]
            })
            .unwrap_or_default();

        // Assembled path first (the faithful source), 1204 fallback second.
        let mut assembled: Option<(legaia_tmd::Tmd, Vec<u8>, Vec<u8>)> = None; // (tmd, bytes, anm_bones)
        if let (Some(raw), Some(fp)) = (raw.as_deref(), file_pack.as_ref())
            && let Ok(mut asm) = bca::assemble_character(raw, fp, &equipped)
            && bca::relocate_tsb_cba(&mut asm.tmd, member as u8).is_ok()
            && let Ok(tmd) = legaia_tmd::parse(&asm.tmd)
        {
            assembled = Some((tmd, asm.tmd, asm.anm_bones));
        }

        if let Some((tmd, tmd_bytes, anm_bones)) = assembled {
            let raw = raw.as_deref()?;
            let fp = file_pack.as_ref()?;
            // Band pixels: the equipped sections' texture pools + record[0]
            // image blocks at the pinned FUN_80052FA0 placement; the 1204
            // atlas pair approximates the band when the pool decode fails.
            let uploads = bca::character_texture_uploads(raw, fp, &equipped, member as u8)
                .unwrap_or_default();
            if uploads.is_empty() {
                for half in 0..2usize {
                    if let Some(atlas) = pack.atlases.get(cslot * 2 + half)
                        && let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes)
                    {
                        let img = &tim.image;
                        vram.write_block(
                            512 + (member * 2 + half) as u16 * 64,
                            256,
                            img.fb_w,
                            img.h,
                            &img.data,
                        );
                    }
                }
            } else {
                for u in &uploads {
                    vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                    if !u.clut.is_empty() {
                        vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                    }
                }
            }
            let (vmesh, object_ids) =
                legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &tmd_bytes);
            if vmesh.indices.is_empty() {
                return None;
            }
            self.overlay_party_palette(vram, raw, cslot, &vmesh);
            // Idle stream (record[0] slot 0) expanded so channel i drives
            // TMD object i, equipment extras riding their attach bone.
            let idle = match bca::idle_battle_animation(raw) {
                Ok(Some(anim)) => Some(bca::expand_animation_for_objects(&anim, &anm_bones)),
                _ => None,
            };
            let rest_pose = idle
                .as_ref()
                .and_then(|a| a.frames.first())
                .map(|f| flatten_frame(f))
                .unwrap_or_default();
            // Full action-clip set: record[0] slots + the equipment-spliced
            // swings at 0xC..0xF, so ready / recover / defeat AND the staged
            // attack-band swings play their real streams.
            let mut clips: Vec<Option<legaia_asset::monster_archive::MonsterAnimation>> =
                vec![None; bca::ACTION_SLOT_COUNT];
            if let Ok(anims) = bca::battle_animations(raw) {
                for a in &anims {
                    if let Some(slot) = clips.get_mut(a.action_id as usize) {
                        *slot = Some(bca::expand_animation_for_objects(a, &anm_bones));
                    }
                }
            }
            if let Ok(swings) = bca::swing_battle_animations(raw, fp, &equipped) {
                for s in &swings {
                    if let Some(slot) = clips.get_mut(s.slot as usize) {
                        *slot = Some(bca::expand_animation_for_objects(&s.anim, &anm_bones));
                    }
                }
            }
            // Art-animation bank (record[0] +0x58) through the character's
            // readef.DAT "ME" archives, so staged ids >= 0x10 resolve.
            let art_bank = self.party_art_bank_web(host, raw, cslot, &anm_bones);
            return Some((
                BattleActorRender {
                    actor_idx: member,
                    monster: false,
                    mesh: BattleMesh {
                        mesh: vmesh,
                        flat: Vec::new(),
                    },
                    object_ids,
                    rest_pose,
                },
                PendingClips {
                    actor_idx: member,
                    idle,
                    action_clips: Some(clips),
                    art_bank: Some(art_bank),
                },
            ));
        }

        // Fallback: the static PROT 1204 mesh, statically posed from the
        // PROT 1203 bank's idle record (identity object->bone). Uploaded
        // pre-posed, so it is never re-posed per frame (empty object_ids).
        web_log(&format!(
            "play battle: party {cslot} assembly failed - PROT 1204 fallback"
        ));
        let slot = pack.slot(cslot)?;
        let tmd = legaia_tmd::parse(&slot.tmd_bytes).ok()?;
        let bone_offsets: Vec<([i16; 3], [i16; 3])> = match (battle_anm, [0usize, 9, 18].get(cslot))
        {
            (Some(b), Some(&rec)) => (0..tmd.objects.len())
                .map(|o| match b.bone_transform(rec, 0, o) {
                    Some(t) => (
                        [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                        [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                    ),
                    None => ([0; 3], [0; 3]),
                })
                .collect(),
            _ => Vec::new(),
        };
        let vmesh = if bone_offsets.is_empty() {
            legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &slot.tmd_bytes)
        } else {
            legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&tmd, &slot.tmd_bytes, &bone_offsets)
        };
        if vmesh.indices.is_empty() {
            return None;
        }
        Some((
            BattleActorRender {
                actor_idx: member,
                monster: false,
                mesh: BattleMesh {
                    mesh: vmesh,
                    flat: Vec::new(),
                },
                object_ids: Vec::new(),
                rest_pose: Vec::new(),
            },
            PendingClips {
                actor_idx: member,
                idle: None,
                action_clips: None,
                art_bank: None,
            },
        ))
    }

    /// Overlay the character's battle palette onto the CLUT rows/columns the
    /// relocated mesh samples (Vahn = the byte-exact record parse, others =
    /// the equipment-robust collector) - the native window's palette leg.
    fn overlay_party_palette(
        &self,
        vram: &mut legaia_tim::Vram,
        raw: &[u8],
        cslot: usize,
        vmesh: &legaia_tmd::mesh::VramMesh,
    ) {
        let mut rows: Vec<u16> = vmesh.cba_tsb.iter().map(|c| (c[0] >> 6) & 0x1FF).collect();
        rows.sort_unstable();
        rows.dedup();
        let mut cols: Vec<u16> = vmesh.cba_tsb.iter().map(|c| (c[0] & 0x3F) * 16).collect();
        cols.sort_unstable();
        cols.dedup();
        let pal = if cslot == 0 {
            legaia_asset::battle_char_palette::find_record0(raw)
                .and_then(|rec0| legaia_asset::battle_char_palette::parse_record(raw, rec0).ok())
        } else {
            legaia_asset::battle_char_palette::collect_palette(raw, 0, &cols).ok()
        };
        if let Some(pal) = pal {
            for &row in &rows {
                for band in &pal.bands {
                    let bytes: Vec<u8> = band
                        .vram_words()
                        .iter()
                        .flat_map(|w| w.to_le_bytes())
                        .collect();
                    vram.write_clut_row(band.base, row, &bytes);
                }
            }
        }
    }

    /// One character's art-animation bank, commit-ready (the browser port of
    /// the native `party_art_bank`, minus the face tracks this host has no
    /// stamp pass for). Failures degrade per record / to an empty bank.
    fn party_art_bank_web(
        &self,
        host: &SceneHost,
        raw: &[u8],
        cslot: usize,
        anm_bones: &[u8],
    ) -> Vec<Option<legaia_asset::monster_archive::MonsterAnimation>> {
        use legaia_asset::battle_char_assembly as bca;
        let Ok(record0) = bca::decode_record0(raw) else {
            return Vec::new();
        };
        let Ok(records) = bca::art_animation_bank(&record0) else {
            return Vec::new();
        };
        let Ok(readef) = host.index.entry_bytes_extended(READEF_PROT_INDEX) else {
            return Vec::new();
        };
        let main = bca::art_me_archive(&readef, cslot, false);
        let base = bca::art_me_archive(&readef, cslot, true);
        let mut bank = vec![None; records.len()];
        for rec in &records {
            let archive = if rec.uses_base_archive() {
                &base
            } else {
                &main
            };
            let Ok(archive) = archive else { continue };
            if let Ok(anim) = bca::art_animation(rec, archive) {
                bank[rec.index] = Some(bca::expand_animation_for_objects(&anim, anm_bones));
            }
        }
        bank
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
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
            .map(|m| m.positions())
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
            .map(|m| m.positions())
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

    /// Ground-grid depth-cue parameters:
    /// `{"far":[r,g,b],"near_z":0,"far_z":Z,"max_ir0":M}` (display 0..1
    /// colour), or `null` when no grid is up. The page attaches this to the
    /// grid placement as a **per-draw** cue, so the browser grid fogs into
    /// the stage's far colour exactly as the native `DrawCue` seam does.
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
            grid::grid_cue_far_z(),
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
    pub fn play_battle_actor_transforms(&self) -> Vec<f32> {
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
                    if actor.active { 1.0 } else { 0.0 },
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
            .battle_render
            .as_ref()
            .and_then(|b| b.camera.as_ref())
            .map(|c| c.phase())
        {
            Some(P::Dialogue) => "dialogue",
            Some(P::Submenu) => "submenu",
            Some(P::Action) => "action",
            Some(P::Menu) | None => "menu",
        }
    }

    fn battle_cam_pose(&self) -> legaia_engine_vm::battle_cam_script::BattleCamPose {
        self.battle_render
            .as_ref()
            .and_then(|b| b.camera.as_ref())
            .map(|c| c.pose())
            .unwrap_or(legaia_engine_vm::battle_cam_script::BOOT_POSE)
    }
}

#[cfg(test)]
mod battle_cam_web_tests {
    use super::*;
    use legaia_engine_core::world::World;
    use legaia_engine_vm::battle_cam_script as script;

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
        world.party_count = 1;
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
        world.battle_command = Some(legaia_engine_core::battle_input::BattleCommandSession::new(
            0, 0,
        ));

        let inputs = derive_battle_cam(&world);
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
            script::drive(&mut slot, true, inputs, f * 2);
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
