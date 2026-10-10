//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;
use legaia_engine_render::battle_intro::BattleIntro;
use legaia_engine_render::screen_overlay::ScreenPrim;

// The battle-HUD row fold and the encounter-banner label are shared with the
// browser play page: both live in `legaia_engine_core::battle_hud` and each
// host projects the resulting `BattleHud` into its own renderer's view types.
// Re-exported here so the window modules (and their wiring tests) keep their
// `battle::` paths.
pub(super) use legaia_engine_core::battle_hud::{encounter_banner_label, sync_battle_hud_rows};

impl PlayWindowApp {
    /// Drain world battle events and append a one-line summary to the HUD
    /// ring. Called once per simulation tick from the redraw handler.
    ///
    /// **Observation only - do not fold here.** `World::live_battle_tick`
    /// owns the gameplay fold and re-publishes the stream afterwards, so a
    /// second `fold_battle_event` would apply an art strike's HP twice.
    pub(super) fn drain_and_log_battle_events(&mut self) {
        let events = self.session.host.world.drain_battle_events();
        for ev in events {
            // The audio duck: the summon / capture arms lower the BGM to 75%
            // of its reference, the Done band's `0x51` arm raises it back.
            // The director ramps one retail unit per frame (`tick_duck`).
            if let legaia_engine_core::battle_events::BattleEvent::DuckAudioLevel { target_pct } =
                &ev
                && let Some(bgm) = self.session.bgm.as_mut()
            {
                bgm.set_duck_pct(*target_pct);
            }
            // Surface in the HUD ring.
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            self.battle_event_log.push_back(ev.summary());
        }

        // Floating damage / heal numbers: the live battle loop resolves and
        // applies HP itself, then queues a presentation-only FX per strike.
        // Feed those into the popup model and log the magnitude (the typed
        // events above are consumed inside the live loop, so this is the
        // only place per-strike damage surfaces while live).
        let fx = self.session.host.world.drain_battle_hit_fx();
        for f in fx {
            if f.is_heal {
                self.battle_hud.push_heal(f.target_slot, f.amount);
            } else if f.is_crit {
                self.battle_hud.push_popup(
                    legaia_engine_core::battle_hud::DamagePopup::damage(f.target_slot, f.amount)
                        .crit(),
                );
            } else {
                self.battle_hud.push_damage(f.target_slot, f.amount);
            }
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            let sign = if f.is_heal { '+' } else { '-' };
            self.battle_event_log
                .push_back(format!("slot {} {}{} HP", f.target_slot, sign, f.amount));
        }

        // Per-hit events of the attack band (`World::tick_battle_hit_events`,
        // one per damage-kernel resolution): the channel the impact-FX and
        // HIT / TOTAL counter layers read. Drained here so the world never
        // accumulates it; each surfaces as a diag line (the ring only draws
        // under `LEGAIA_DIAG_HUD` / `F1`, so this is off by default).
        let hits = self.session.host.world.drain_battle_hit_events();
        for h in hits {
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            self.battle_event_log.push_back(format!(
                "hit {} {}->{} pb {:#04x} dmg {} total {}{}{}",
                h.hit_index,
                h.attacker_slot,
                h.target_slot,
                h.power_byte,
                h.damage,
                h.running_total,
                if h.applied { " APPLIED" } else { "" },
                if h.is_art { " art" } else { "" },
            ));
        }

        // Battle sound cues: the art-strike outcomes resolve per-strike SFX
        // cues (kind = the SfxBank id, played directly without classify_cue).
        // Enqueue each into the director's SfxScheduler at its strike-relative
        // timing, then advance the scheduler one frame and fire any matured cue
        // through the scene VAB. Drain first so the world borrow ends before
        // the director borrow. SFX touch the SPU only - no RNG - so battle
        // determinism stays bit-exact.
        let cues = self.session.host.world.drain_battle_sfx_cues();
        // Arts-voice shouts: one cue per executed party art, queued on the
        // art's animation-start frame. The director resolves the (character,
        // action) pair against the demuxed XA clip bank and fires the CD-XA
        // shout with the modeled CD-response delay, so the voice trails the
        // animation (the retail contract) instead of leading it.
        let shouts = self.session.host.world.drain_battle_shout_cues();
        // One-shot CD-XA clip requests (the melee grunt / attack sting):
        // `(clip_slot, channel, dur)` in the retail starter's terms, played
        // off the boot-staged clip bank through the same XA mixing path.
        let xa_cues = self.session.host.world.drain_battle_xa_cues();
        // The round's cast-voice prestage list is for a host that decodes
        // clips asynchronously (the browser play page); this director cuts
        // clips out of its boot-staged bank on demand, so it has nothing to
        // stage ahead. Drained so the list does not grow.
        let _ = self.session.host.world.drain_battle_xa_prestage();
        let duck_ref = self.session.host.world.audio.levels.configured_level;
        if let Some(bgm) = self.session.bgm.as_mut() {
            for xa in &xa_cues {
                let fired = bgm
                    .play_xa_clip(xa.clip, xa.channel, xa.duration_sectors)
                    .is_some();
                log::debug!(
                    "battle XA clip slot {} ch {} dur {} -> {}",
                    xa.clip,
                    xa.channel,
                    xa.duration_sectors,
                    if fired { "playing" } else { "not staged" }
                );
            }
            bgm.enqueue_battle_cues(&cues, &self.session.host.index);
            for (id, voice) in bgm.tick_audio_frame(duck_ref).fired {
                log::debug!("battle SFX cue {id:#04x} fired on voice {voice}");
            }
            for shout in &shouts {
                match bgm.play_art_shout(shout.cslot, shout.action) {
                    Some(ch) => log::debug!(
                        "arts shout cslot {} action {:#04x} fired on XA channel {ch}",
                        shout.cslot,
                        shout.action
                    ),
                    None => log::debug!(
                        "arts shout cslot {} action {:#04x} unvoiced / bank absent",
                        shout.cslot,
                        shout.action
                    ),
                }
            }
        } else {
            for cue in &cues {
                log::debug!(
                    "battle SFX cue {:#04x} @ +{} frames (actor {} -> target {}; no audio)",
                    cue.kind,
                    cue.timing_frames,
                    cue.actor_slot,
                    cue.target_slot
                );
            }
        }

        // Battle effect-script spawn requests: one per effect record the
        // per-actor effect-script walk consumed this tick (the FUN_801DEA50
        // port driven from the animation tick). Route each into the world's
        // matching spawn path - the render layers this window already
        // composes then draw them: the direct (0x80-flagged) form lands in
        // the 2D effect pool (effect billboards / outline markers), the
        // table form stages a 0x801F6324 prototype scene whose parts ride
        // the move-FX part-draw seam.
        // The routing is the engine's (`World::route_battle_effect_spawns`),
        // shared with the browser play page.
        for r in self.session.host.world.route_battle_effect_spawns() {
            log::debug!(
                "effect-script spawn effect {:#04x} actor {} at {:?} direct={} staged={:?}",
                r.spawn.effect,
                r.spawn.actor_slot,
                r.spawn.at,
                r.spawn.direct,
                r.table_staged
            );
        }

        // Refresh per-slot rows + status icons, then age the popups one frame.
        if self.session.host.world.mode == SceneMode::Battle {
            self.sync_battle_hud_rows();
            for slot in 0..self.battle_hud.slots.len() as u8 {
                self.battle_hud
                    .sync_status(slot, &self.session.host.world.battle.status_effects);
            }
        } else if self.session.host.world.mode == SceneMode::MuscleDome {
            // A dome leg is a battle on the same HUD: the shared fold seats
            // the lead fighter's row off the dome session.
            self.sync_battle_hud_rows();
        }
        self.battle_hud.tick();

        // Age the encounter-transition banner one frame; drop it at zero.
        if let Some((frames, _)) = &mut self.encounter_banner {
            *frames = frames.saturating_sub(1);
            if *frames == 0 {
                self.encounter_banner = None;
            }
        }
    }

    /// Fold the live battle-actor table into [`Self::battle_hud`]'s per-slot
    /// rows. Thin wrapper over [`sync_battle_hud_rows`], which is a free
    /// function so it can be exercised against a bare `World`.
    pub(super) fn sync_battle_hud_rows(&mut self) {
        sync_battle_hud_rows(&mut self.battle_hud, &self.session.host.world);
    }

    /// React to a `Field <-> Battle` scene-mode change once per transition:
    /// on entering battle, decode each enemy's mesh and inject it; on leaving,
    /// restore the clean field VRAM and drop the battle meshes. Called each
    /// frame before the render borrows `uploaded_vram`.
    pub(super) fn sync_battle_render(&mut self) {
        let mode = self.session.host.world.mode;
        let prev = self.prev_scene_mode.replace(mode);
        if prev == Some(mode) {
            return;
        }
        match (prev, mode) {
            (_, SceneMode::Battle) => {
                self.arm_encounter_banner();
                self.enter_battle_render();
            }
            (Some(SceneMode::Battle), _) => {
                self.encounter_banner = None;
                // Retire this fight's floating numerals and reset the status
                // CLUT's pristine-palette copy + Stone latch at the battle
                // boundary, where retail rebuilds its context. Both are
                // host-side state that outlives the engine object; without
                // them a numeral could survive the mode edge and a petrified
                // member in the next fight restaged the previous fight's
                // palette. The browser play page already did both.
                self.battle_hud.clear_popups();
                self.battle_hud.status_clut.reset();
                self.exit_battle_render();
            }
            _ => {}
        }
    }

    /// Frames the encounter-transition banner stays on screen after a
    /// `Field -> Battle` mode change (~1.5 s at the 60 Hz sim tick).
    const ENCOUNTER_BANNER_FRAMES: u16 = 90;

    /// Arm the HUD's "ENCOUNTER!" banner for the battle just entered.
    fn arm_encounter_banner(&mut self) {
        let label = encounter_banner_label(&self.session.host.world);
        self.encounter_banner = Some((Self::ENCOUNTER_BANNER_FRAMES, label));
    }

    /// Bridge the decoded monster meshes for the current battle into the draw
    /// list: inject each enemy's texture pool into a clone of the field VRAM
    /// at the loader's per-slot coords, upload the relocated mesh, and bind it
    /// to the enemy actor. Re-uploads the edited VRAM so the injected texture
    /// pages resolve.
    /// Build the current scene's battle-stage backdrop, if it has one. Returns
    /// the battle VRAM (scene + stage-dome textures resident) and the stage
    /// dome's `(Tmd, raw)`. The faithful battle backdrop is the scene's
    /// `scene_tmd_stream` half-dome (sky + mountain ring + ground); building the
    /// scene in `SceneLoadKind::Battle` makes that dome TMD + its textures
    /// resident (the Field build excludes them). `None` when the scene has no
    /// stage entry.
    /// Read `SCUS_942.54` from whichever boot source this window opened, if it
    /// is reachable. Used to resolve the backdrop's second-copy transform;
    /// `None` just means the port falls back to the default half turn.
    fn scus_bytes(&self) -> Option<Vec<u8>> {
        use legaia_engine_core::Vfs;
        if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        }
    }
}

/// The stage shell's two meshes for one object list: the textured half and
/// the untextured (`F*` / `G*`) half, each drawn twice - the second copy
/// under the per-stage transform.
///
/// Retail draws the shell TWICE, the second copy under a per-stage diagonal
/// transform. The shell on the disc is an authored HALF (open toward -X, -Z
/// or +X, never +Z); the second copy is what closes the horizon, so a
/// single-copy backdrop is a half dome no matter how the camera orbits.
/// `append_scaled` reverses winding and flips normals when the transform has
/// negative determinant, which is the mesh-level analogue of retail's
/// `0x40000000 -> 0x48000000` draw-mode swap.
///
/// The shell's UNTEXTURED half: between a fifth and a tenth of a backdrop's
/// prims are `F*`/`G*` flat / gouraud panels carrying a baked colour word and
/// no UVs - the sky band, the painted wall faces, the flat water. The
/// VRAM-mesh builder drops every prim with no UVs, so those ride a colour
/// mesh with the same second copy. Retail has no such split - `FUN_8001ADA4`
/// case 3 walks the whole primitive list and the GPU takes
/// `POLY_F*`/`POLY_G*` packets as readily as `POLY_*T*` ones.
fn stage_shell_meshes(
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    second: legaia_asset::battle_backdrop::SecondCopy,
    objects: &[usize],
) -> (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh) {
    let tmd0 = legaia_asset::battle_backdrop::objects_tmd(tmd, objects);
    let mut vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd0, raw);
    let first = vmesh.clone();
    vmesh.append_scaled(&first, second.scale());
    let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&tmd0, raw);
    let cfirst = cmesh.clone();
    cmesh.append_scaled(&cfirst, second.scale());
    (vmesh, cmesh)
}

/// One copy of the given backdrop objects, textured and untextured halves -
/// the mesh of a slot the backdrop draw transforms per frame, so nothing is
/// baked in ([`legaia_engine_core::scene::BattleStageLayers`]).
fn stage_slot_meshes(
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    objects: &[usize],
) -> (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh) {
    let tmd0 = legaia_asset::battle_backdrop::objects_tmd(tmd, objects);
    (
        legaia_tmd::mesh::tmd_to_vram_mesh(&tmd0, raw),
        legaia_tmd::mesh::tmd_to_color_mesh(&tmd0, raw),
    )
}

/// The loaded battle-stage backdrop bundle `build_battle_stage` returns.
pub(super) struct BattleStage {
    /// Scene + stage-dome textures resident - becomes the battle VRAM base.
    pub(super) vram: legaia_tim::Vram,
    /// The backdrop shell TMD plus the raw bytes it was parsed from.
    pub(super) dome: (legaia_tmd::Tmd, Vec<u8>),
    /// The second backdrop copy's per-stage transform (`DAT_80078B50`).
    pub(super) second: legaia_asset::battle_backdrop::SecondCopy,
    /// Ground-grid depth-cue far colour, display `0..1` - the backdrop far
    /// colour per stage class (`DAT_80078C1C` outdoor table).
    pub(super) grid_far: [f32; 3],
    /// `DAT_80078C1C` outdoor-table membership - the battle tint pass's
    /// `DAT_8007BDA8` flag (`World::battle_actor_draw_plan`).
    pub(super) outdoor: bool,
}

impl PlayWindowApp {
    /// Build the current scene's battle-stage backdrop bundle: the stage
    /// VRAM, the dome TMD (with its raw bytes), the second-copy transform
    /// and the ground grid's depth-cue far colour.
    pub(super) fn build_battle_stage(&self) -> Option<BattleStage> {
        let scene = self.session.host.scene.as_ref()?;
        let scene_name = scene.name.clone();
        // Not the block's first stage stream: a scene bundle carries one per
        // sub-area, and the region the fight starts in names which one
        // (`SceneHost::battle_stage_entry`, the region reader's stage
        // variant `_DAT_8007BD60`).
        let stage_entry = self.session.host.battle_stage_entry()?;
        let mut shared: Vec<Scene> = Vec::new();
        for name in FIELD_SHARED_BLOCKS {
            if let Ok(s) = Scene::load(&self.session.host.index, name) {
                shared.push(s);
            }
        }
        let refs: Vec<&Scene> = shared.iter().collect();
        let system_ui = self.session.host.index.system_ui_bundle().ok();
        let (res, _) = SceneResources::build_targeted_with_options(
            scene,
            &refs,
            BuildOptions {
                kind: SceneLoadKind::Battle,
                upload_all_tims: true,
                // Boot-resident system-UI bundle: resident through battle
                // in retail (uploaded once at boot, never evicted).
                system_ui: system_ui.as_deref(),
            },
        )
        .ok()?;
        // The stage dome is the leading TMD of the scene_tmd_stream stage entry.
        let dome = res.tmds.iter().find(|t| t.entry_idx == stage_entry)?;
        // Take back the VRAM this stage owns. A bundle carries one stream per
        // sub-area and they all declare the SAME pages + CLUT rows, so the
        // DMA-every-TIM build leaves whichever sibling was written last
        // holding them. Retail only ever has the recorded stream resident.
        let mut vram = res.vram.clone();
        let restored = legaia_engine_core::scene::upload_battle_stage_tims_into_vram(
            scene,
            stage_entry,
            &mut vram,
        );
        // Which transform retail gives the SECOND backdrop copy. The default
        // is a half turn about Y; the stages named by the `SCUS_942.54` table
        // at `DAT_80078B50` get an X mirror instead. Getting this wrong is not
        // cosmetic: `town01` IS on the mirror list, so half-turning it plants
        // a second village wall across the open sea side. With no readable
        // SCUS the default is the safer of the two - it never reflects.
        let scus = self.scus_bytes();
        let second = scus
            .as_deref()
            .and_then(legaia_asset::battle_backdrop::MirrorXTable::from_scus)
            .map(|t| t.second_copy_for_prot_index(stage_entry))
            .unwrap_or(legaia_asset::battle_backdrop::SecondCopy::HalfTurn);
        // The ground grid's depth-cue far colour: the backdrop far colour
        // retail derives per stage class (`FUN_80050120`) - the sibling
        // SCUS table at `DAT_80078C1C` picks the brightened outdoor arm on
        // the 13 wide-open stages, everything else takes the indoor `>> 1`
        // arm. With no readable SCUS the indoor grey covers the vast
        // majority of the stage corpus. Capture provenance:
        // `scripts/pcsx-redux/autorun_grid_far_colour.lua`.
        let grid_far_bytes = scus
            .as_deref()
            .and_then(legaia_engine_vm::battle_ground_grid::OutdoorCueTable::from_scus)
            .map(|t| t.far_colour_for_prot_index(stage_entry))
            .unwrap_or(legaia_engine_vm::battle_ground_grid::GRID_FAR_INDOOR);
        let grid_far = grid_far_bytes.map(|c| f32::from(c) / 255.0);
        let outdoor = scus
            .as_deref()
            .and_then(legaia_engine_vm::battle_ground_grid::OutdoorCueTable::from_scus)
            .is_some_and(|t| t.contains_prot_index(stage_entry));
        log::info!(
            "play-window: battle stage = scene '{scene_name}' PROT {stage_entry} \
             ({} objects, drawn twice, {} second copy, {restored} stage TIM(s) re-uploaded)",
            dome.tmd.objects.len(),
            second.label()
        );
        Some(BattleStage {
            vram,
            dome: (dome.tmd.clone(), dome.raw.clone()),
            second,
            grid_far,
            outdoor,
        })
    }

    pub(super) fn enter_battle_render(&mut self) {
        let monsters = self.session.host.world.battle_monster_slots();
        if monsters.is_empty() {
            return;
        }
        let Some(field_base) = self.cpu_vram_base.clone() else {
            return;
        };
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        // Fresh battle: drop any previous battle's facial-animation state
        // (re-registered per assembled member below) and make sure the
        // static SCUS face-frame tables are loaded. Done before the
        // renderer borrow below (both take `&mut self`).
        self.battle_faces.clear();
        self.load_face_tables();
        // The party's forms are the engine's to build (normally already done
        // by the tick that entered the battle); this covers a battle entered
        // outside a tick.
        self.session.host.ensure_battle_party_forms();
        // Build the battle-stage backdrop (the scene's scene_tmd_stream
        // half-dome). Its VRAM (scene + stage-dome textures resident) becomes
        // the battle base, so the dome renders textured behind the actors;
        // fall back to the field VRAM when the scene has no stage.
        let stage = self.build_battle_stage();
        let base = match &stage {
            Some(s) => s.vram.clone(),
            None => field_base,
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };

        // Work on a throwaway copy so the field VRAM stays clean for the
        // restore on battle exit.
        let mut vram = base;
        // Blit the battle effect-texture atlas (PROT 870) into the battle VRAM
        // copy. Its pages land at fb_y=0 in the same columns the field stage
        // textures occupy, so this is a battle-only upload that battle exit
        // discards (the field VRAM base is untouched). Byte-verified against
        // live battle captures; soft-fails so a missing disc entry just leaves
        // the flame pages absent.
        if let Err(e) = legaia_engine_core::scene::upload_flame_atlas_into_vram(
            &self.session.host.index,
            &mut vram,
            true,
        ) {
            log::warn!("play-window: flame-atlas VRAM upload skipped: {e:#}");
        }
        self.battle_mesh_base = self.meshes.len();
        self.battle_rest_vmesh.clear();
        self.battle_color_mesh_base = self.color_meshes.len();
        // Upload the stage dome mesh (drawn as the backdrop). Its textures live
        // in the stage VRAM, so build it unfiltered (all textured prims are
        // resident). Appended after `battle_mesh_base`, so battle exit truncates
        // it away with the monster meshes.
        self.battle_stage_mesh = None;
        self.battle_stage_color_mesh = None;
        self.battle_stage_spun_mesh = None;
        self.battle_stage_spun_color_mesh = None;
        self.battle_stage_shell = None;
        self.battle_stage_outdoor = false;
        self.session.host.world.fog_volume.battle_luma = None;
        // REF: FUN_800513f0 - the backdrop registration whose object-list edit
        // and second-copy transform this host consumes through
        // `legaia_asset::battle_backdrop`.
        if let Some(BattleStage {
            dome: (tmd, raw),
            second,
            grid_far,
            outdoor,
            ..
        }) = &stage
        {
            // Retail's backdrop registration edits the object list rather
            // than truncating it: it drops index 1 and keeps the rest
            // (`legaia_asset::battle_backdrop`, ported from `FUN_800513f0`).
            // On the two-object stage shells that is object 0 alone - object 1
            // is the ground-level ribbon of near props no retail capture shows,
            // and drawing it painted an engine-only white streak across the
            // Tetsu arena floor. On the four-object overworld domes it keeps
            // objects 0 (sky), 2 (mountains) and 3 (the flat ground ring).
            // `_DAT_8007B64B` (region `+8` bit 5) keeps object 1, and the
            // evolved-Cort arrival's hand-back rebinds slot 0 to it - the one
            // shared kernel is `SceneHost::battle_stage_object_indices`.
            let objects = self
                .session
                .host
                .battle_stage_object_indices(tmd.objects.len());
            // The volumetric ground fog may not outshine the stage it lies
            // on: measure the shell as drawn (the browser play page takes the
            // same measurement in `enter_battle_render`).
            self.session.host.world.fog_volume.battle_luma =
                legaia_engine_core::fog_volume::stage_luminance(&vram, tmd, raw, &objects);
            // The shell proper is every slot but 1, baked with its second
            // copy. Slot 1 is the object the backdrop draw turns about Y
            // each frame (`BattleStageLayers`), so it is one copy of its
            // own, drawn twice under the frame's angle.
            let layers = self.session.host.battle_stage_layers(tmd.objects.len());
            let (vmesh, cmesh) = stage_shell_meshes(tmd, raw, *second, &layers.fixed);
            let spun = (!layers.spun.is_empty()).then(|| stage_slot_meshes(tmd, raw, &layers.spun));
            self.battle_stage_shell = Some((objects, *second));
            if !cmesh.is_empty()
                && let Ok(cm) = r.upload_color_mesh_blended(
                    &cmesh.positions,
                    &cmesh.colors,
                    &cmesh.indices,
                    &cmesh.blend,
                )
            {
                self.battle_stage_color_mesh = Some(self.color_meshes.len());
                self.color_meshes.push(cm);
            }
            if !vmesh.indices.is_empty()
                && let Ok(m) = r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.colors,
                    &vmesh.indices,
                )
            {
                self.battle_stage_mesh = Some(self.meshes.len());
                self.meshes.push(m);
                self.scene_tmd_data.push((tmd.clone(), raw.clone()));
                if let Some((o, oc)) = spun {
                    if !o.indices.is_empty()
                        && let Ok(om) = r.upload_vram_mesh(
                            &o.positions,
                            &o.uvs,
                            &o.cba_tsb,
                            &o.normals,
                            &o.colors,
                            &o.indices,
                        )
                    {
                        self.battle_stage_spun_mesh = Some(self.meshes.len());
                        self.meshes.push(om);
                        self.scene_tmd_data.push((tmd.clone(), raw.clone())); // keep meshes/data aligned
                    }
                    if !oc.is_empty()
                        && let Ok(cm) = r.upload_color_mesh_blended(
                            &oc.positions,
                            &oc.colors,
                            &oc.indices,
                            &oc.blend,
                        )
                    {
                        self.battle_stage_spun_color_mesh = Some(self.color_meshes.len());
                        self.color_meshes.push(cm);
                    }
                }
                // Flat tiled ground grid under the actors (retail's
                // `func_0x801d02c0` grid), textured from the constant
                // retail page/CLUT/UV-window address - the scene battle
                // VRAM holds that scene's own ground tile there (see
                // `build_battle_ground_grid`).
                self.battle_ground_mesh = None;
                self.battle_ground_cue_far = None;
                self.battle_stage_outdoor = *outdoor;
                // The backdrop ramp's ceiling reads the same table byte.
                self.session.host.world.battle.stage_outdoor = *outdoor;
                // Pre-cue colour = the settled battle ambient
                // (`0x8007B7B0` -> `RGBC`), see `build_ground_grid_rgbc`.
                let grid = build_battle_ground_grid(
                    legaia_engine_vm::battle_ground_grid::GRID_RGBC_SETTLED,
                );
                match r.upload_vram_mesh(
                    &grid.positions,
                    &grid.uvs,
                    &grid.cba_tsb,
                    &grid.normals,
                    &grid.colors,
                    &grid.indices,
                ) {
                    Ok(gm) => {
                        self.battle_ground_mesh = Some(self.meshes.len());
                        self.battle_ground_rgbc =
                            legaia_engine_vm::battle_ground_grid::GRID_RGBC_SETTLED;
                        // The grid's GTE depth cue: far colour per stage
                        // class (resolved in `build_battle_stage`), ramped
                        // by the emitter's per-vertex `SZ >> 2` law at draw
                        // time (see the redraw grid push).
                        self.battle_ground_cue_far = Some(*grid_far);
                        self.meshes.push(gm);
                        self.scene_tmd_data.push((tmd.clone(), raw.clone())); // keep meshes/data aligned
                    }
                    Err(e) => log::warn!("play-window: battle ground grid upload: {e:#}"),
                }
            }
        }
        let mut bound = 0usize;
        let mut bound_slots: Vec<u8> = Vec::new();
        // Every actor slot this call registers a battle mesh on. It is the
        // battle's whole display list - see the un-register pass after the
        // party loop for why the complement matters.
        let mut registered: Vec<usize> = Vec::new();
        for (actor_idx, monster_id, slot) in monsters {
            let mesh = match legaia_asset::monster_archive::mesh(&archive, monster_id) {
                Ok(Some(m)) => m,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("play-window: monster {monster_id} mesh decode failed: {e:#}");
                    continue;
                }
            };
            // Parse the embedded TMD up front so it can be retained parallel
            // to `meshes`; `battle_render_mesh` only yields a mesh when this
            // same parse succeeds.
            let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
                continue;
            };
            let Some(vmesh) = mesh.battle_render_mesh(slot, &mut vram) else {
                continue;
            };
            if vmesh.indices.is_empty() {
                continue;
            }
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
            ) {
                Ok(m) => {
                    let idx = self.meshes.len();
                    self.meshes.push(m);
                    self.battle_rest_vmesh.insert(idx, vmesh);
                    // Keep `scene_tmd_data` length-parallel with `meshes`.
                    self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
                    self.session.host.world.actors[actor_idx].tmd_binding = Some(idx);
                    registered.push(actor_idx);
                    // The texture slot (the posed rebuild re-applies its
                    // CBA/TSB relocation, or the animated monster samples
                    // the wrong page and renders white) and the idle clip,
                    // through the install the browser page runs too.
                    let idle =
                        match legaia_asset::monster_archive::idle_animation(&archive, monster_id) {
                            Ok(idle) => idle,
                            Err(e) => {
                                log::warn!(
                                    "play-window: monster {monster_id} idle anim decode: {e:#}"
                                );
                                None
                            }
                        };
                    self.session.host.world.install_monster_battle_form(
                        actor_idx,
                        slot,
                        idle.as_ref(),
                    );
                    bound_slots.push(slot);
                    // The archive-order action-clip set (the `+0x4C` tag
                    // table the hit-reaction family and the AI's picked
                    // swings index) is the engine's to install:
                    // `SceneHost::tick` stages it the tick a battle is up,
                    // for a monster this window draws or not.
                    bound += 1;
                }
                Err(e) => log::warn!("play-window: monster {monster_id} mesh upload: {e:#}"),
            }
        }

        // Load the REAL battle party meshes and bind them to the party actor
        // slots. The faithful source is the retail battle loader's per-character
        // ASSEMBLY: each member's mesh is spliced from their player battle file's
        // equipment-id sections (`legaia_asset::battle_char_assembly`, extraction
        // PROT 863..865) and relocated into the slot's runtime VRAM band
        // (`relocate_tsb_cba`, the registration-time TSB/CBA pass of
        // FUN_800513F0). The band's PIXELS come from the same player file: the
        // equipped sections' texture pools + the two record[0] image blocks,
        // uploaded at the pinned `FUN_80052FA0`/`FUN_80053B9C` placement
        // (`character_texture_uploads`; byte-exact vs live battle VRAM - see
        // docs/formats/battle-data-pack.md § Texture-pool VRAM placement).
        // PROT 1204 (the Baka Fighter / default-equipment sibling pack) stays
        // as the per-member fallback: its meshes when assembly fails, its
        // atlases (a 73-98% approximation of the band) when the texture-pool
        // decode fails. Each character's decoded battle palette overlays the
        // rows its mesh CBA samples (= 481 + slot after relocation).
        let mut party_bound = 0usize;
        // The engine built and installed each member's form at battle entry
        // (`SceneHost::ensure_battle_party_forms`, run by `SceneHost::tick`
        // for every session, headless included): the decode, the clips, the
        // art bank and records. This window adds only what is a renderer's -
        // the band pixels replayed into its battle VRAM, the GPU upload with
        // the rest-pose bake, and the facial animator's registration.
        let party = self.session.host.battle_party_forms().cloned();
        if let Some(party) = party {
            party.vram_writes.replay(&mut vram);
            for form in party.forms {
                // Rest pose: frame 0 of the assembled mesh's own idle stream
                // (the combat stance retail holds at battle start), or the
                // PROT 1203 bank's idle record for a fallback.
                let vmesh = if form.rest_pose.is_empty() {
                    legaia_tmd::mesh::tmd_to_vram_mesh(&form.tmd, &form.tmd_bytes)
                } else {
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(
                        &form.tmd,
                        &form.tmd_bytes,
                        &form.rest_pose,
                    )
                };
                if vmesh.indices.is_empty() {
                    continue;
                }
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.colors,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        let idx = self.meshes.len();
                        self.meshes.push(m);
                        self.battle_rest_vmesh.insert(idx, vmesh);
                        let member = form.member;
                        self.session.host.world.actors[member].tmd_binding = Some(idx);
                        registered.push(member);
                        // Facial animation (FUN_8004C7B4): the per-tick stamp
                        // pass (`tick_battle_face_stamps`) re-stamps the
                        // current eye/mouth frame onto the band's live face
                        // rows; the form only carries tracks when the band
                        // holds the real face-frame strip.
                        if let Some(tracks) = form.face_tracks {
                            self.battle_faces.push(BattleMemberFace {
                                actor_slot: member,
                                char_index: form.cslot,
                                tracks,
                                art_tracks: form.art_face_tracks,
                                last_stamps: None,
                                art_counter: None,
                            });
                        }
                        self.scene_tmd_data.push((form.tmd, form.tmd_bytes));
                        party_bound += 1;
                    }
                    Err(e) => log::warn!("play-window: party {} mesh upload: {e:#}", form.cslot),
                }
            }
        }

        // The battle display list is EXACTLY what this loader registered.
        let unregistered = unregister_non_battle_meshes(&mut self.session.host.world, &registered);
        let unregistered_count = unregistered.len();
        if bound > 0 || party_bound > 0 {
            match r.upload_vram(&vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle VRAM re-upload: {e:#}"),
            }
            log::info!(
                "play-window: battle render bound {bound} monster + {party_bound} party mesh(es); \
                 {unregistered_count} field actor mesh(es) un-registered"
            );
        }
        // Stash the battle VRAM + the monster-slot count so a mid-battle player
        // summon can inject its creature texture into the next free slot.
        // One past the highest slot bound - repeated species share a slot,
        // so the bound count over-reads it (the shared kernel the page uses).
        self.battle_tex_slots_used =
            legaia_engine_core::battle_party_form::monster_tex_slots_used(bound_slots).min(4);
        self.battle_vram = Some(vram);
        self.log_battle_display_list(&unregistered);
    }

    /// `LEGAIA_DIAG_BATDRAW=1`: dump the battle display list at battle entry -
    /// one line per actor slot that carries a `tmd_binding`, with its role
    /// (party ordinal / monster id / **STRAY**), its battle-world seat, the
    /// bound mesh's vertex count, and where that seat projects.
    ///
    /// Off by default. It exists because "the party mesh is bound" and "the
    /// party member is on screen" are different claims, and the startup log
    /// only ever made the first one: a bound mesh can sit behind the camera,
    /// off-frame, or - the case this was written for - under a scene prop
    /// that leaked into the fight. A `STRAY` row is a registration bug.
    ///
    /// The `clipw` / `ndc` columns are taken under the battle camera pose that
    /// is live **at registration**, i.e. before `tick_battle_camera` snaps the
    /// entry framing, so they answer "is this seat in front of a battle camera
    /// at all", not "where on screen does the player see it". The per-frame
    /// framing question is `LEGAIA_DIAG_BATCAM`'s (see `window/camera.rs`),
    /// and the per-frame mesh question is `LEGAIA_DIAG_POSE`'s.
    ///
    /// `unregistered` is the set of field slots the loader just dropped from
    /// the display list, printed so the pass is visible rather than silent.
    fn log_battle_display_list(&self, unregistered: &[usize]) {
        if std::env::var_os("LEGAIA_DIAG_BATDRAW").is_none() {
            return;
        }
        let world = &self.session.host.world;
        let pc = world.party.party_count as usize;
        let cam = self.battle_dome_camera_mvp(4.0 / 3.0)
            * Mat4::from_scale(Vec3::splat(BATTLE_WORLD_SCALE));
        eprintln!(
            "BATDRAW display list ({} actor slots, un-registered field slots {unregistered:?})",
            world.actors.len()
        );
        for (i, a) in world.actors.iter().enumerate() {
            let Some(idx) = a.tmd_binding else { continue };
            let role = match (i < pc, a.battle_monster_id) {
                (_, Some(id)) => format!("monster id {id}"),
                (true, None) => format!("party ordinal {i}"),
                (false, None) => "STRAY (field actor)".to_string(),
            };
            let verts: usize = self
                .scene_tmd_data
                .get(idx)
                .map(|(t, _)| t.objects.iter().map(|o| o.vertices.len()).sum())
                .unwrap_or(0);
            let w = Vec3::new(
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            );
            let clip = cam * w.extend(1.0);
            eprintln!(
                "  actor{i} {role} active={} mesh={idx} verts={verts} world={w:?} \
                 clipw={:.1} ndc=({:.2},{:.2})",
                a.active,
                clip.w,
                clip.x / clip.w,
                clip.y / clip.w
            );
        }
    }

    /// Spawn the player Seru-magic summon as a battle creature, the faithful
    /// render (the summon reuses its namesake `battle_data` enemy creature -
    /// see `summon::summon_creature_id`). Loads that creature's mesh + texture
    /// (`battle_render_mesh`) into a free battle texture slot and binds it to a
    /// high actor slot with its idle [`MonsterAnimPlayer`], so the existing
    /// battle render animates + textures it exactly like an enemy. Replaces the
    /// move-VM `SummonScene` stand-in for the visual. No-op outside battle.
    pub(super) fn spawn_summon_creature(&mut self, spell_id: u8) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        // The cast's own body: the archive twin for 0x81..=0x95, the cast's
        // `summon.dat` record (PROT 893) for the high block.
        let summon_dat = self
            .session
            .host
            .index
            .entry_bytes(u32::from(legaia_asset::summon_readef::SUMMON_PROT_INDEX))
            .ok();
        let Some(asset) = legaia_engine_core::summon::summon_spawn_asset(
            spell_id,
            &archive,
            summon_dat.as_ref().map(|b| b.as_slice()),
        ) else {
            return;
        };
        let Some(mut vram) = self.battle_vram.clone() else {
            return;
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let mesh = &asset.mesh;
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return;
        };
        // Inject the creature texture into the next free battle slot.
        let tex_slot = self.battle_tex_slots_used.min(4);
        let Some(vmesh) = mesh.battle_render_mesh(tex_slot, &mut vram) else {
            return;
        };
        if vmesh.indices.is_empty() {
            return;
        }
        let uploaded = match r.upload_vram_mesh(
            &vmesh.positions,
            &vmesh.uvs,
            &vmesh.cba_tsb,
            &vmesh.normals,
            &vmesh.colors,
            &vmesh.indices,
        ) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("play-window: summon mesh upload: {e:#}");
                return;
            }
        };
        let idx = self.meshes.len();
        self.meshes.push(uploaded);
        self.battle_rest_vmesh.insert(idx, vmesh);
        self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
        match r.upload_vram(&vram) {
            Ok(v) => {
                // A mid-battle summon spawn is a legitimate battle-VRAM
                // refresh: re-stamp the expected resident generation.
                self.battle_vram_generation = Some(v.generation());
                self.uploaded_vram = Some(v);
            }
            Err(e) => log::error!("play-window: summon VRAM re-upload: {e:#}"),
        }
        // Keep the stashed battle VRAM in sync with what's now resident, so
        // later battle-VRAM refreshes (the residency heal, the per-frame
        // face stamps) don't drop the injected summon texture.
        self.battle_vram = Some(vram);

        // Seat the summon in a free high actor slot (>= 8) so it never collides
        // with the party/monster battle slots. Place it on the party side
        // (`enter_battle` seats party at negative Z, enemies at positive Z),
        // in front of the party and clearly clear of the enemy cluster, so
        // the battle camera frames it distinct from the enemies it attacks.
        let slot = self
            .summon_actor_slot
            .unwrap_or_else(|| 8 + (self.session.host.world.party.party_count as usize));
        self.summon_actor_slot = Some(slot);
        if let Some(a) = self.session.host.world.actors.get_mut(slot) {
            a.active = true;
            a.tmd_binding = Some(idx);
            a.battle_tex_slot = Some(tex_slot);
            a.move_state.world_x = 0;
            a.move_state.world_y = 0;
            a.move_state.world_z = -350;
        }
        if let Some(idle) = asset.idle.as_ref()
            && let Some(player) = legaia_engine_core::battle_anim::MonsterAnimPlayer::new(idle)
        {
            self.session
                .host
                .world
                .set_actor_battle_animation(slot, player);
        }
        // The creature's archive-order clip set, so the stager's staged ids
        // (the walk, clip 1) resolve through the same commit as a monster's.
        if !asset.clips.is_empty() {
            let clips: Vec<_> = asset.clips.iter().cloned().map(Some).collect();
            self.session
                .host
                .world
                .set_actor_battle_action_clips(slot, std::sync::Arc::new(clips));
        }
        // Hand the seat to the world: a cast in its summon band places the
        // creature at the stager's spawn point (behind the caster, facing
        // the enemy - the capture's slot-7 record) and retires it when the
        // choreography ends; a debug spawn keeps the placement above.
        self.session.host.world.seat_summon_actor(slot);
        log::info!(
            "play-window: summon spell {spell_id:#04x} -> creature {:?} \
             (mesh slot {idx}, tex slot {tex_slot}, actor slot {slot})",
            asset.creature_id
        );
    }

    // -----------------------------------------------------------------------
    // The field-to-battle intro transition
    // -----------------------------------------------------------------------

    /// Arm, advance or drop the field-to-battle intro emitter so it tracks the
    /// encounter session's `Transition` phase, and return this frame's screen
    /// primitives.
    ///
    /// This is the host half of `legaia_engine_render::battle_intro`. The
    /// simulation half is already live: `World::tick_encounter` runs
    /// `tick_transition` every frame the session sits in `Transition`, and
    /// `World::battle.intro` carries the entity whose `+0x1A` clock the styles
    /// ride. What was missing was an owner for the per-style working set and
    /// a route from its output into a draw call - both of which land here.
    ///
    /// The emitter adopts the live entity's clock rather than counting for
    /// itself, so a dropped or repeated simulation tick cannot desynchronise
    /// the visuals from the handoff the same state machine performs.
    ///
    /// Returns an empty list whenever no transition is running, which is also
    /// what makes the caller's target choice a two-arm match rather than a
    /// mode test.
    pub(super) fn take_battle_intro_frame(&mut self) -> (Option<BattleIntro>, Vec<ScreenPrim>) {
        use legaia_engine_core::encounter::EncounterPhase;

        let phase = self
            .session
            .host
            .world
            .encounters
            .session
            .as_ref()
            .map(|s| s.phase());
        let Some(EncounterPhase::Transition { roll, .. }) = phase else {
            self.battle_intro = None;
            self.battle_intro_vram = None;
            return (None, Vec::new());
        };
        let Some(entity) = self.session.host.world.battle.intro else {
            return (self.battle_intro.take(), Vec::new());
        };
        let total = self
            .session
            .host
            .world
            .encounters
            .session
            .as_ref()
            .map(|s| i32::from(s.transition_frames))
            .unwrap_or(0);

        if self.battle_intro.is_none() {
            self.battle_intro = Some(self.arm_battle_intro(roll.formation_id, total));
        }
        // Hand the emitter out by value: the render pass needs it while the
        // renderer holds an immutable borrow of `self`, which a `&mut self`
        // method could not have. The caller puts it back.
        let mut intro = self.battle_intro.take().expect("armed above");
        // Stepped to the entity's clock, one step per clock unit: a redraw can
        // drain several world ticks, and stepping once per redraw left the
        // shatter behind the browser page's (`BattleIntro::advance_to`).
        let prims = intro.advance_to(entity.elapsed).prims;
        // Per-frame emitter yield. The styles differ by what they draw on top
        // of the captured field frame, so a style that emits nothing but the
        // fade is indistinguishable from a blank transition on screen - this
        // is the cheapest way to tell the two apart in a screenshot run.
        if log::log_enabled!(log::Level::Debug) {
            let mut lo = [i16::MAX; 2];
            let mut hi = [i16::MIN; 2];
            for p in &prims {
                let xy = match p {
                    ScreenPrim::Textured(q) => q.xy,
                    ScreenPrim::Flat(q) => q.xy,
                };
                for (x, y) in xy {
                    lo[0] = lo[0].min(x);
                    lo[1] = lo[1].min(y);
                    hi[0] = hi[0].max(x);
                    hi[1] = hi[1].max(y);
                }
            }
            log::debug!(
                "battle intro {:?} frame {} -> {} prim(s) screen bbox {lo:?}..{hi:?}",
                intro.style(),
                entity.elapsed,
                prims.len()
            );
        }
        (Some(intro), prims)
    }

    /// Build the emitter for the battle about to open.
    ///
    /// The style is **not** a choice the port makes: `select_intro_style` is a
    /// port of the intro overlay's own init block, which keys on the battle
    /// flags byte, the formation's first monster id and the current scene. The
    /// engine feeds it the resolved formation and the loaded scene so a given
    /// battle gets the style retail gives it.
    fn arm_battle_intro(&self, formation_id: u16, total: i32) -> BattleIntro {
        // All three selector inputs come out of one engine-side resolver
        // (`SceneHost::battle_intro_style_inputs`), and the arming itself -
        // the PROT 0979 tables with their disc-free fallbacks, the shade
        // pack, the env seeds - is the one `BattleIntro::arm_for_battle` the
        // browser play page calls too.
        let host = &self.session.host;
        let inputs = host.battle_intro_style_inputs(formation_id);
        let overlay = host
            .index
            .entry_bytes_extended(legaia_engine_render::battle_intro::INTRO_OVERLAY_PROT)
            .ok();
        let shade = host
            .index
            .entry_bytes(legaia_asset::field_char_textures::PROT_ENTRY_INDEX)
            .ok();
        let intro = BattleIntro::arm_for_battle(
            &inputs,
            total,
            overlay.as_deref(),
            shade.as_deref().map(Vec::as_slice),
            host.world.rng_state,
        );
        log::info!(
            "play-window: battle intro style {:?} (sub {}) for formation slot0 {:#04x}, \
             battle flags {:#04x}",
            intro.style(),
            intro.sub_style(),
            inputs.formation_slot0,
            inputs.battle_flags
        );
        intro
    }

    /// Bring the transition's private VRAM page up to date for this frame and
    /// upload it.
    ///
    /// This is the port's equivalent of the console's framebuffer *being*
    /// VRAM. Two things ride on it. The **capture** is a one-shot: retail's
    /// strips sample the frame the GPU drew moments earlier, so the port
    /// re-renders the field scene offscreen and blits it into the rect the
    /// style's texture pages decode to. The **curtain's intermediate** is
    /// per-frame: its column pass renders into VRAM `(320, 0)` and its row
    /// pass samples that, so the page changes every frame of a curtain
    /// transition (see `legaia_engine_render::battle_intro`).
    ///
    /// An **associated** function, not a method: the render pass calls it
    /// while the renderer is borrowed out of `self`, so it takes the three
    /// pieces it needs rather than `&mut self`. The scene's pristine page is
    /// borrowed and cloned inside, never edited.
    ///
    /// Returns a freshly uploaded page whenever the contents changed, or
    /// `None` when the previously uploaded one is still correct (no emitter,
    /// no base, or a style whose page is a one-shot already landed).
    pub(super) fn capture_battle_intro_frame(
        intro: Option<&mut BattleIntro>,
        renderer: &legaia_engine_render::Renderer,
        scene: &RenderScene<'_>,
        base: Option<&legaia_tim::Vram>,
    ) -> Option<legaia_engine_render::UploadedVram> {
        let intro = intro?;
        let base = base?;
        let first = intro.needs_capture();
        let page = match legaia_engine_render::battle_intro::update_field_capture(
            intro,
            renderer,
            legaia_engine_render::RenderTarget::Scene(scene),
            base,
        ) {
            Ok(Some(p)) => p,
            Ok(None) => return None,
            Err(e) => {
                log::error!("play-window: battle intro capture: {e:#}");
                return None;
            }
        };
        match renderer.upload_vram(page) {
            Ok(v) => {
                if first {
                    log::info!("play-window: battle intro captured the field frame into VRAM");
                }
                Some(v)
            }
            Err(e) => {
                log::error!("play-window: battle intro VRAM upload: {e:#}");
                None
            }
        }
    }

    /// Leave battle: restore the clean field VRAM and drop the appended
    /// battle monster meshes (the field actor table was already restored from
    /// the pre-battle snapshot, so those slots no longer reference them).
    ///
    /// That same restore is what puts the field scene's mesh bindings back
    /// after [`unregister_non_battle_meshes`] took them out of the battle
    /// display list - the two share one channel, so neither stashes anything.
    pub(super) fn exit_battle_render(&mut self) {
        if let (Some(r), Some(base)) = (self.win.renderer.as_ref(), self.cpu_vram_base.as_ref()) {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: field VRAM restore: {e:#}"),
            }
        }
        let keep = self.battle_mesh_base.min(self.meshes.len());
        self.meshes.truncate(keep);
        self.battle_rest_vmesh.clear();
        self.scene_tmd_data
            .truncate(keep.min(self.scene_tmd_data.len()));
        let keep_color = self.battle_color_mesh_base.min(self.color_meshes.len());
        self.color_meshes.truncate(keep_color);
        // Tear down a spawned player-summon creature.
        if let Some(slot) = self.summon_actor_slot.take() {
            self.session.host.world.release_summon_seat(slot);
        }
        self.battle_vram = None;
        self.battle_vram_generation = None;
        self.battle_tex_slots_used = 0;
        self.battle_stage_mesh = None;
        self.battle_stage_color_mesh = None;
        self.battle_stage_spun_mesh = None;
        self.battle_stage_spun_color_mesh = None;
        self.battle_stage_shell = None;
        self.battle_ground_mesh = None;
        self.battle_ground_cue_far = None;
        self.battle_stage_outdoor = false;
        self.battle_faces.clear();
    }

    /// Lazily read the static face-frame tables out of the boot source's
    /// `SCUS_942.54` (`legaia_asset::face_anim::FaceFrameTables`). A failed
    /// attempt (disc-free run / unparsable executable) is remembered so the
    /// probe only runs once; facial animation is simply skipped then.
    pub(super) fn load_face_tables(&mut self) {
        if self.face_tables_attempted {
            return;
        }
        self.face_tables_attempted = true;
        use legaia_engine_core::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.face_tables = scus
            .as_deref()
            .and_then(legaia_asset::face_anim::FaceFrameTables::from_scus);
        self.art_mouth_tables = scus
            .as_deref()
            .and_then(legaia_asset::face_anim::ArtMouthTables::from_scus);
        if self.face_tables.is_none() {
            log::info!(
                "play-window: SCUS face-frame tables unavailable - battle faces stay neutral"
            );
        }
    }

    /// Lazily read the spell/seru display-name table from the boot SCUS so the
    /// seru-trade overlay can label each offer. Cached after the first read;
    /// a disc-free run leaves it `None` (offers fall back to "Seru NN").
    pub(super) fn ensure_seru_names(&mut self) {
        if self.seru_names.is_some() {
            return;
        }
        use legaia_engine_core::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.seru_names = scus
            .as_deref()
            .and_then(legaia_asset::spell_names::SpellNameTable::from_scus);
    }

    /// Per-tick battle facial animation: re-stamp each registered party
    /// member's current eye + mouth face frame onto the band's live face
    /// rows, exactly like the retail per-frame animator. The playing clip's
    /// `action_id` selects the member's face tracks, its integer keyframe
    /// cursor is the frame counter, and the selected frames are VRAM-to-VRAM
    /// copies from the band's face-frame strip (`Vram::move_image`, the
    /// `MoveImage` stamp). During the victory window - the battle ended in
    /// a monster wipe while a member still plays a dynamic-art-slot clip
    /// (staged id `0x11..=0x18`, e.g. the killing art) - the mouth records
    /// come from the static `0x80077E80` override table and the animator
    /// clocks on the halved victory counter instead (the win-quote mouth
    /// flap). The GPU texture is only re-uploaded on a frame
    /// whose stamp set differs from the previous one (retail re-issues
    /// identical `MoveImage`s every frame; the visible result is the same).
    // PORT: FUN_80047430 (facial-animator dispatch): per visible party
    // node, call the stamp pass with (band slot, char index, cursor
    // keyframes, playing action entry), skipping char 3 (Terra) and bands
    // >= 3. The stamp-selection half is `FaceFrameTables::stamps_with_art_window`
    // (PORT: FUN_8004C7B4 in legaia_asset::face_anim).
    pub(super) fn tick_battle_face_stamps(&mut self) {
        use legaia_asset::face_anim::{ART_BAND_FIRST, ART_BAND_LAST, ArtMouthOverride};
        use legaia_engine_vm::battle_action::BattleEndCause;
        if self.session.host.world.mode != SceneMode::Battle || self.battle_faces.is_empty() {
            return;
        }
        let Some(tables) = self.face_tables.as_ref() else {
            return;
        };
        let art_tables = self.art_mouth_tables.as_ref();
        let Some(vram) = self.battle_vram.as_mut() else {
            return;
        };
        // Retail gate 1 of the victory-window mouth override: the
        // battle-end signal `DAT_8007BD71 == 0xFE` raised by a monster
        // wipe (the SM `0x5A` arm; the engine mirror is the
        // `BattleActionHost::battle_end` latch). Gates 2/3 - the victory
        // sequencer's phase halfword `ctx+0x6CE != 0` and the celebration
        // flag `DAT_8007BD60 & 0x80` (which the party-wipe path clears) -
        // are the retail victory presentation's internal progress flags;
        // the engine has no victory sequencer, so "the won battle is
        // still on screen" stands in for them. Escapes also raise 0xFE
        // but never set the celebration flag, so they stay excluded.
        let victory_window =
            self.session.host.world.battle.end == Some(BattleEndCause::MonsterWipe);
        let mut changed = false;
        for mf in &mut self.battle_faces {
            let Some(actor) = self.session.host.world.actors.get(mf.actor_slot) else {
                continue;
            };
            // No player yet = the rest pose: behaves like clip frame 0 of
            // the (track-less) idle, i.e. the neutral face.
            let (action_id, frame) = actor
                .battle_animation
                .as_ref()
                .map(|p| (p.action_id(), p.current_frame()))
                .unwrap_or((0, 0));
            // Track source. Staged ids >= 0x10 are art-bank clips: retail
            // materializes bank record `id - 0x10` and installs its
            // embedded entry (record +0x24) as the action-table pointer
            // (FUN_8004AD80), so the animator reads THAT entry's tracks
            // (record +0xB0 eyes / +0xBC mouth). Below the art base, the
            // playing clip's action slot picks the record[0] / swing entry.
            let tracks = if action_id >= legaia_asset::battle_char_assembly::ART_ANIM_ID_BASE {
                mf.art_tracks
                    .get(
                        (action_id - legaia_asset::battle_char_assembly::ART_ANIM_ID_BASE) as usize,
                    )
                    .and_then(|t| t.as_ref())
            } else {
                mf.tracks.get(action_id as usize).and_then(|t| t.as_ref())
            };
            // Retail gate 4: the member's last-staged anim id
            // (`actor[+0x1DB]`; the engine's art-bank clips carry it as
            // their `action_id`) sits in the dynamic-art-slot band
            // `0x11..=0x18`. Open the override window, clocking the
            // member's `gp+0x9EA` mirror from 0; closed, the counter
            // resets.
            let art_mouth = if victory_window
                && (ART_BAND_FIRST..=ART_BAND_LAST).contains(&action_id)
                && let Some(track) = art_tables.and_then(|t| t.track(mf.char_index, action_id))
            {
                let counter = mf.art_counter.unwrap_or(0);
                mf.art_counter = Some(counter.saturating_add(1));
                Some(ArtMouthOverride { track, counter })
            } else {
                mf.art_counter = None;
                None
            };
            // The retail mouth-neutral gate: character-record word `+0xF8`
            // bit 0x2000 = ability bitfield (`+0xF4`) bit 45 (passive 0x2D
            // Rage, Evil Medallion). Read it off the occupying character's
            // rebuilt ability bytes (byte 5 bit 0x20).
            let world = &self.session.host.world;
            let force_neutral_mouth = world
                .party
                .roster
                .members
                .get(world.party_roster_slot(mf.actor_slot))
                .map(|m| m.ability_bits()[5] & 0x20 != 0)
                .unwrap_or(false);
            let stamps = tables.stamps_with_art_window(
                mf.char_index,
                mf.actor_slot,
                tracks,
                frame,
                art_mouth,
                force_neutral_mouth,
            );
            if mf.last_stamps.as_deref() != Some(&stamps) {
                for s in &stamps {
                    vram.move_image(s.src_x, s.src_y, s.w, s.h, s.dst_x, s.dst_y);
                }
                mf.last_stamps = Some(stamps);
                changed = true;
            }
        }
        if !changed {
            return;
        }
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    // A face re-stamp is a legitimate battle-VRAM refresh:
                    // move the expected resident generation along with it.
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: face-stamp VRAM re-upload: {e:#}"),
            }
        }
    }

    /// Status CLUT recolour - the fourth pass of `FUN_8004CE2C`. An actor
    /// whose Stone latch fired this frame has its party CLUT row
    /// (`481 + slot`) restaged grey from the pristine palette copy
    /// ([`legaia_engine_core::battle_status_clut`]); the latch itself is
    /// armed inside `BattleHud::sync_status`, which
    /// [`Self::drain_and_log_battle_events`] already runs once per slot per
    /// frame.
    ///
    /// Shares the mid-battle re-upload protocol with
    /// [`Self::tick_battle_face_stamps`]: mutate the stashed battle VRAM,
    /// re-upload, and move the expected resident generation with it so
    /// [`Self::check_battle_vram_residency`] does not read the refresh as a
    /// clobber.
    pub(super) fn tick_battle_status_clut(&mut self) {
        if !self.battle_hud.status_clut.armed() {
            return;
        }
        let Some(vram) = self.battle_vram.as_mut() else {
            return;
        };
        if !self.battle_hud.status_clut.step(vram) {
            return;
        }
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: status-CLUT VRAM re-upload: {e:#}"),
            }
        }
    }

    /// Effect **CLUT stages** - the palette arm of the action-effect script's
    /// table form (`FUN_801DEA50`, `0x801df0dc..0x801df134`). Each queued
    /// `0x801F6418` byte is a VRAM source x whose sixteen entries move onto
    /// `(224, 476)`, recolouring whatever the spawned move-FX prototype draws
    /// ([`legaia_engine_core::battle_effect_clut`]).
    ///
    /// Shares the mid-battle re-upload protocol with
    /// [`Self::tick_battle_status_clut`]: mutate the stashed battle VRAM,
    /// re-upload, and move the expected resident generation with it so
    /// [`Self::check_battle_vram_residency`] does not read the refresh as a
    /// clobber. Drains unconditionally so the queue cannot accumulate across
    /// a battle when no renderer is up.
    pub(super) fn tick_battle_effect_clut(&mut self) {
        let stages = self.session.host.world.drain_battle_clut_stages();
        if stages.is_empty() {
            return;
        }
        let Some(vram) = self.battle_vram.as_mut() else {
            return;
        };
        let mut dirty = false;
        for x in stages {
            dirty |= legaia_engine_core::battle_effect_clut::stage_effect_clut(vram, x);
        }
        if !dirty {
            return;
        }
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: effect-CLUT VRAM re-upload: {e:#}"),
            }
        }
    }

    /// Stage-module render edits, mid-fight: the battle `MoveImage`s a stage
    /// module queued (`World::apply_battle_vram_moves` - the evolved-Cort
    /// arrival blanks the ground tile) and a changed backdrop object list
    /// (`SceneHost::battle_stage_object_indices` - the same arrival's slot-0
    /// rebind), which re-builds the stage shell meshes in place.
    ///
    /// Shares the mid-battle re-upload protocol with
    /// [`Self::tick_battle_effect_clut`].
    pub(super) fn tick_battle_stage_shell(&mut self) {
        let moved = match self.battle_vram.as_mut() {
            Some(vram) => self.session.host.world.apply_battle_vram_moves(vram),
            None => {
                self.session.host.world.discard_battle_vram_edits();
                false
            }
        };
        if moved
            && let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref())
        {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: stage-module VRAM re-upload: {e:#}"),
            }
        }
        let (Some(idx), Some((built, second))) =
            (self.battle_stage_mesh, self.battle_stage_shell.clone())
        else {
            return;
        };
        let Some((tmd, raw)) = self.scene_tmd_data.get(idx).cloned() else {
            return;
        };
        let objects = self
            .session
            .host
            .battle_stage_object_indices(tmd.objects.len());
        if objects == built {
            return;
        }
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        // The split the entry build made (`SceneHost::battle_stage_layers`).
        // The one mid-fight edit, the slot-0 rebind, leaves slot 1 holding
        // the object it held, so the spun meshes stand; a list with no slot
        // 1 drops them.
        let layers = self.session.host.battle_stage_layers(tmd.objects.len());
        if layers.spun.is_empty() {
            self.battle_stage_spun_mesh = None;
            self.battle_stage_spun_color_mesh = None;
        }
        let (vmesh, cmesh) = stage_shell_meshes(&tmd, &raw, second, &layers.fixed);
        match r.upload_vram_mesh(
            &vmesh.positions,
            &vmesh.uvs,
            &vmesh.cba_tsb,
            &vmesh.normals,
            &vmesh.colors,
            &vmesh.indices,
        ) {
            Ok(m) => self.meshes[idx] = m,
            Err(e) => log::warn!("play-window: stage shell rebuild: {e:#}"),
        }
        let cm = (!cmesh.is_empty())
            .then(|| {
                r.upload_color_mesh_blended(
                    &cmesh.positions,
                    &cmesh.colors,
                    &cmesh.indices,
                    &cmesh.blend,
                )
                .ok()
            })
            .flatten();
        match (self.battle_stage_color_mesh, cm) {
            (Some(ci), Some(cm)) => self.color_meshes[ci] = cm,
            (Some(_), None) => self.battle_stage_color_mesh = None,
            (None, Some(cm)) => {
                self.battle_stage_color_mesh = Some(self.color_meshes.len());
                self.color_meshes.push(cm);
            }
            (None, None) => {}
        }
        log::info!("play-window: battle stage shell re-bound to objects {objects:?}");
        self.battle_stage_shell = Some((objects, second));
    }

    /// Follow the live battle ambient on the ground grid: the pre-cue vertex
    /// colour is the ambient `0x8007B7B0` (base `+ 0x404040`) and the cue's
    /// far colour `0x8007BB48` is derived from the same base, both of which a
    /// summon close-up ramps down (`World::battle_ambient_base`, docs:
    /// battle.md "A cast dims it"). The far colour is re-derived every frame;
    /// the grid mesh bakes the near colour into its vertices, so it is rebuilt
    /// only on the frames the ambient moves.
    pub(super) fn sync_battle_ground_ambient(&mut self) {
        use legaia_engine_vm::battle_ground_grid as grid;
        let Some(gi) = self.battle_ground_mesh else {
            return;
        };
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let base = self.session.host.world.battle_ambient_base();
        self.battle_ground_cue_far = Some(
            grid::grid_far_colour(base, self.battle_stage_outdoor).map(|c| f32::from(c) / 255.0),
        );
        let near = grid::battle_ambient_colour(base);
        if near == self.battle_ground_rgbc {
            return;
        }
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let g = build_battle_ground_grid(near);
        match r.upload_vram_mesh(
            &g.positions,
            &g.uvs,
            &g.cba_tsb,
            &g.normals,
            &g.colors,
            &g.indices,
        ) {
            Ok(m) => {
                self.meshes[gi] = m;
                self.battle_ground_rgbc = near;
            }
            Err(e) => log::warn!("play-window: battle ground grid re-colour: {e:#}"),
        }
    }

    /// Residency guard: while a battle texture is expected to be GPU-resident,
    /// verify no other path re-uploaded VRAM over it this frame. The
    /// white-speckle party bug (a background CLUT animator re-uploading the
    /// field snapshot mid-battle) was invisible to every CPU-side VRAM oracle
    /// because they never check *which* upload the draw samples. On a
    /// violation: log loudly, fail debug builds, and self-heal by re-uploading
    /// the stashed battle VRAM.
    pub(super) fn check_battle_vram_residency(&mut self) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(expected) = self.battle_vram_generation else {
            return;
        };
        let current = self.uploaded_vram.as_ref().map(|v| v.generation());
        if current == Some(expected) {
            return;
        }
        log::error!(
            "play-window: battle VRAM clobbered mid-battle (expected upload \
             generation {expected}, GPU holds {current:?}); re-uploading the \
             battle texture"
        );
        debug_assert!(
            false,
            "battle VRAM residency violated: expected generation {expected}, found {current:?}"
        );
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle VRAM re-upload (heal): {e:#}"),
            }
        }
    }
}

impl PlayWindowApp {
    /// Post-battle spoils panel draws, or empty when the panel is not up.
    ///
    /// Reads [`legaia_engine_core::world::World::battle_spoils_banner`] (the
    /// timer + the name resolution both live in engine-core) and feeds the
    /// shared `engine-ui` builder, so the browser play page draws the exact
    /// same panel from the exact same model.
    pub(super) fn battle_spoils_draws(&self, surface_w: u32, surface_h: u32) -> Vec<TextDraw> {
        if let Some(defeat) = self.session.host.world.battle_defeat_banner() {
            let windows =
                legaia_engine_render::battle_defeat_windows(defeat.line.as_deref(), defeat.slide_y);
            let (origin, scale) = self.save_select_stage(surface_w, surface_h);
            let mut draws =
                legaia_engine_render::battle_result_line_draws_for(&self.font, &windows);
            legaia_engine_render::scale_stage_text_draws(&mut draws, origin, scale);
            return draws;
        }
        let Some(banner) = self.session.host.world.battle_spoils_banner() else {
            return Vec::new();
        };
        let leader = self.session.host.world.battle_spoils_leader();
        let view = legaia_engine_render::BattleSpoilsView {
            xp: banner.xp,
            gold: banner.gold,
            level_ups: &banner.level_ups,
            drops: &banner.drops,
            leader: &leader,
            subject: banner.subject,
            slide: banner.slide,
        };
        let windows = legaia_engine_render::battle_spoils_windows(&view);
        let (origin, scale) = self.save_select_stage(surface_w, surface_h);
        let mut draws = legaia_engine_render::battle_spoils_draws_for(&self.font, &view, &windows);
        legaia_engine_render::scale_stage_text_draws(&mut draws, origin, scale);
        draws
    }

    /// The post-battle report's window chrome - the gold nine-slice over the
    /// blue fill, one frame per window the shared builder describes. Rides
    /// the system-UI atlas slot with the rest of the menu chrome.
    pub(super) fn battle_spoils_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(rects) = self.save_menu.as_ref().map(|a| &a.rects) else {
            return Vec::new();
        };
        // The loss window (screen element `0x42`) shares the report frame.
        if let Some(defeat) = self.session.host.world.battle_defeat_banner() {
            let (origin, scale) = self.save_select_stage(surface_w, surface_h);
            return legaia_engine_render::battle_defeat_windows(
                defeat.line.as_deref(),
                defeat.slide_y,
            )
            .iter()
            .flat_map(|w| {
                legaia_engine_render::menu_window_chrome_draws_for(rects, w.rect, origin, scale)
            })
            .collect();
        }
        let Some(banner) = self.session.host.world.battle_spoils_banner() else {
            return Vec::new();
        };
        let leader = self.session.host.world.battle_spoils_leader();
        let view = legaia_engine_render::BattleSpoilsView {
            xp: banner.xp,
            gold: banner.gold,
            level_ups: &banner.level_ups,
            drops: &banner.drops,
            leader: &leader,
            subject: banner.subject,
            slide: banner.slide,
        };
        let (origin, scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_render::battle_spoils_windows(&view)
            .iter()
            .flat_map(|w| {
                legaia_engine_render::menu_window_chrome_draws_for(rects, w.rect, origin, scale)
            })
            .collect()
    }

    /// One dim HUD line while the live loop is armed on a scene that cannot
    /// roll a random encounter.
    ///
    /// Several retail scenes - `town01`, the scene the binary boots into,
    /// among them - have every rollable encounter region shadowed by an
    /// earlier rate-0 row, so walking them forever produces nothing. That is
    /// scene data the port keeps faithfully; without a hint it reads as the
    /// engine being broken, which is exactly how it was reported.
    /// The field overlay's **passive-ability badge column** - the icons
    /// retail floats over the player's head while an accessory passive is
    /// active (`FUN_801d095c`).
    ///
    /// The engine's half is `World::passive_hud_points` (three head-relative
    /// world points, lifted by fractions of the player's `+0x72`) and
    /// `World::passive_hud_icons` (the icon list for a resolved anchor); the
    /// projection between them is this host's, because the camera is. Retail
    /// takes the **X of the first** projected point and the **Y of the
    /// third** - a mixed pair, not one point's.
    ///
    /// The icon ids `0x47..=0x4D` name cells of the field pictogram bank
    /// (`FUN_8002C488`), which this host has no sprite atlas for, so each
    /// draws as the same ASCII stand-in the menu painters' pictograms use.
    /// The browser play page draws the same list off the same World seat.
    pub(super) fn passive_hud_draws(
        &self,
        cam: glam::Mat4,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        // The badge column is not a screen of its own: retail reaches
        // `FUN_801d095c` only through `FUN_801D0D38`'s `jal` at `0x801D130C`,
        // below the `_DAT_8007B868` gate whose suppress arm jumps past it. So
        // the badges answer the party readout's suppression, and this host
        // asks the same shared kernel the readout does. Gating only on "no
        // boot-UI panel is up" left the badges painted over every dialog box,
        // every cutscene beat and every fight; the browser play page has
        // asked the full question since its own column landed.
        if self.field_party_hud_suppressed() {
            return Vec::new();
        }
        let world = &self.session.host.world;
        if !world.passive_hud_active() {
            return Vec::new();
        }
        let Some(points) = world.passive_hud_points() else {
            return Vec::new();
        };
        let project = |p: [f32; 3]| -> Option<(i32, i32)> {
            let clip = cam * glam::Vec4::new(p[0], p[1], p[2], 1.0);
            if clip.w <= 0.01 {
                return None;
            }
            let ndc = clip.truncate() / clip.w;
            Some((
                ((ndc.x * 0.5 + 0.5) * 320.0) as i32,
                ((0.5 - ndc.y * 0.5) * 240.0) as i32,
            ))
        };
        let (Some(first), Some(third)) = (project(points[0]), project(points[2])) else {
            return Vec::new();
        };
        let (origin, scale) = self.save_select_stage(surface_w, surface_h);
        let mut out = Vec::new();
        for i in world.passive_hud_icons((first.0, third.1)) {
            let mut draws = text_draws_for(
                &self.font.layout_ascii("*"),
                (i.x, i.y),
                legaia_engine_render::MENU_TEXT_GOLD,
            );
            legaia_engine_render::scale_stage_text_draws(&mut draws, origin, scale);
            out.extend(draws);
        }
        out
    }

    pub(super) fn encounter_hint_draws(&self, _w: u32, surface_h: u32) -> Vec<TextDraw> {
        // A diagnostic row like the shell's others, so it takes the same
        // toggle: retail prints nothing here, and a permanent caption over
        // the game is worse than the confusion it was written to prevent.
        if !self.diag_rows || !self.session.host.world.show_encounter_hint() {
            return Vec::new();
        }
        let dim = [0.7f32, 0.7, 0.7, 1.0];
        text_draws_for(
            &self
                .font
                .layout_ascii("no random encounters in this scene (retail)"),
            (8, surface_h as i32 - 20),
            dim,
        )
    }
}

/// Drop the `tmd_binding` of every actor slot the battle loader did **not**
/// just register, and report the slots dropped.
///
/// Retail's battle scene loader builds the fight its own actor set:
/// `FUN_800513F0` registers the backdrop, the assembled party blobs and the
/// monster meshes into `DAT_8007C018[]` and links those actors - and only
/// those - into the render OT. The field scene's actor list does not survive
/// the transition; nothing the town was drawing is still linked once the
/// arena comes up.
///
/// The port keeps ONE actor array across the transition (the world clones it
/// into `field_return` and restores it when the battle ends), so the field
/// slots arrive in the battle still carrying their scene-mesh bindings. Each
/// then draws at whatever battle-world coordinates its `move_state` happens
/// to hold - and for a scene actor that never moved that is the **origin**,
/// dead centre of the arena between the party row and the monster row. The
/// `!actor.active` gate at the draw site only catches the slots the scene
/// left inactive; an active field actor (rikuroa leaves two) walks straight
/// through it and plants a scene prop on top of the party member, which is
/// what "the party member is not visibly in the battle" turned out to be.
///
/// Un-registering here rather than filtering at the draw site keeps the rule
/// where the loader is: the battle registration decides what the battle
/// draws. Nothing is stashed for the restore - the bindings come back with
/// the field actor table (`World::end_battle`'s `field_return` restore), the
/// same channel `exit_battle_render` already relies on.
// REF: FUN_800513F0 (battle setup: the battle's own registration set)
/// The [`legaia_art::Character`] whose art tables a player-file character
/// slot (`0..=2` = Vahn / Noa / Gala) resolves against; `None` for Terra
/// (slot `3`), who has no arts catalog.
pub(super) fn unregister_non_battle_meshes(
    world: &mut legaia_engine_core::world::World,
    registered: &[usize],
) -> Vec<usize> {
    let mut dropped = Vec::new();
    for (i, a) in world.actors.iter_mut().enumerate() {
        if a.tmd_binding.is_some() && !registered.contains(&i) {
            a.tmd_binding = None;
            dropped.push(i);
        }
    }
    dropped
}

#[cfg(test)]
mod battle_display_list_tests {
    use super::unregister_non_battle_meshes;
    use legaia_engine_core::world::World;

    /// A field scene leaves live actors bound to its meshes and parked at the
    /// coordinates the scene gave them. After the battle registration only the
    /// combatant slots may still carry a binding - anything else draws inside
    /// the arena, in front of the party.
    #[test]
    fn only_the_registered_combatants_keep_a_battle_mesh() {
        let mut world = World::new();
        // Field state: every slot bound (the host's naive actor K -> mesh K
        // pre-bind) and a couple of them ACTIVE at the world origin, which is
        // exactly the shape that slipped through the draw site's
        // `!actor.active` gate.
        for (i, a) in world.actors.iter_mut().enumerate() {
            a.tmd_binding = Some(i);
        }
        world.actors[62].active = true;
        world.actors[63].active = true;
        world.enter_battle(1, 1);
        // What the loader registered this battle: party ordinal 0 + monster 1.
        let dropped = unregister_non_battle_meshes(&mut world, &[0, 1]);
        assert_eq!(world.actors[0].tmd_binding, Some(0));
        assert_eq!(world.actors[1].tmd_binding, Some(1));
        for (i, a) in world.actors.iter().enumerate().skip(2) {
            assert_eq!(a.tmd_binding, None, "slot {i} still draws in the battle");
        }
        assert!(dropped.contains(&62) && dropped.contains(&63));
        assert_eq!(dropped.len(), world.actors.len() - 2);
    }

    /// Non-vacuous: without the pass the same world keeps every field slot in
    /// the battle display list, including the two active ones at the origin.
    #[test]
    fn the_unfiltered_display_list_really_does_carry_the_strays() {
        let mut world = World::new();
        for (i, a) in world.actors.iter_mut().enumerate() {
            a.tmd_binding = Some(i);
        }
        world.actors[62].active = true;
        world.enter_battle(1, 1);
        let drawn = world
            .actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.tmd_binding.is_some() && a.active)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        assert!(
            drawn.contains(&62),
            "the pre-fix draw predicate must admit the stray, or the fix pins nothing"
        );
        assert_eq!(world.actors[62].move_state.world_x, 0);
        assert_eq!(world.actors[62].move_state.world_z, 0);
    }

    /// A party slot the assembly failed on is un-registered too: it must show
    /// nothing at its seat, not the field mesh that happened to share its
    /// index.
    #[test]
    fn an_unbound_party_slot_does_not_fall_back_to_its_field_mesh() {
        let mut world = World::new();
        for (i, a) in world.actors.iter_mut().enumerate() {
            a.tmd_binding = Some(i);
        }
        world.enter_battle(3, 1);
        // Ordinals 0 and 2 assembled; ordinal 1 failed. Monster is slot 3.
        unregister_non_battle_meshes(&mut world, &[0, 2, 3]);
        assert_eq!(world.actors[1].tmd_binding, None);
        assert!(world.actors[0].tmd_binding.is_some());
        assert!(world.actors[2].tmd_binding.is_some());
        assert!(world.actors[3].tmd_binding.is_some());
    }
}

#[cfg(test)]
mod encounter_banner_tests {
    use super::encounter_banner_label;
    use legaia_engine_core::world::World;

    #[test]
    fn label_names_unresolved_monsters_positionally() {
        let mut world = World::new();
        world.enter_battle(1, 2);
        // `enter_battle` seats actors but leaves monster HP unseeded; seed it
        // so the label counts the slots as live formation members. With no
        // catalog resolution the label falls back to positional M<n> names.
        for a in world.actors.iter_mut().skip(1).take(2) {
            a.battle.max_hp = 10;
            a.battle.hp = 10;
        }
        assert_eq!(encounter_banner_label(&world), "M1  M2");
    }

    #[test]
    fn unseeded_formation_yields_empty_label() {
        let mut world = World::new();
        world.enter_battle(1, 2);
        assert_eq!(encounter_banner_label(&world), "");
    }
}
