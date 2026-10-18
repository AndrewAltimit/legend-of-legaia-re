//! Mid-battle VRAM re-upload channel for the play page.
//!
//! Both play hosts re-stamp the battle VRAM every battle tick through the
//! shared steps (`legaia_engine_session::battle_vram`: `stamp_battle_faces`,
//! `step_battle_vram_edits`); the native window re-uploads its texture
//! inline. The page uploads its battle VRAM once per fight
//! (`play_battle_generation`), so this module is the channel that carries a
//! re-stamp to the screen: the steps run in
//! [`LegaiaRuntime::tick_battle_vram_channel`] against the battle VRAM copy
//! [`crate::play_battle_render::BattleRender`] owns, and the page re-uploads
//! it when [`LegaiaRuntime::play_battle_vram_take_dirty`] reports a change.
//!
//! Three riders, one channel:
//!
//! * **Facial animation** (`FUN_80047430` -> `FUN_8004C7B4`): per visible
//!   party member the playing clip's face tracks pick an eye + mouth frame,
//!   and each is a VRAM-to-VRAM `MoveImage` from the band's face-frame strip
//!   onto its live face rows. The stamp selection is the shared
//!   `legaia_actor_formats::face_anim::FaceFrameTables::stamps_with_art_window`; the
//!   tracks come off the same player-file entries the native loader reads
//!   (record[0] entries, the equipment-spliced swings, the art bank's
//!   embedded entries), collected in
//!   [`crate::play_battle_render`]'s party build.
//! * **Status CLUT recolour** (`FUN_8004CE2C` pass 4): the Stone latch
//!   `BattleHud::sync_status` arms on this host too, drained through the
//!   shared `legaia_engine_battle::battle_status_clut::StatusClutState::step`.
//! * **Effect CLUT stage** (`FUN_801DEA50`'s palette arm): the
//!   `World::battle.clut_stages` queue the effect-script walk fills on every
//!   host, drained through the shared
//!   `legaia_engine_battle::battle_effect_clut::stage_effect_clut`.
//!
//! The native window's fourth per-tick battle-VRAM body,
//! `check_battle_vram_residency`, is a GPU-side invariant (which upload does
//! the draw sample?) that only the page can evaluate: the runtime never sees
//! the texture. What this host has instead is structural - every field VRAM
//! writer is battle-gated engine-side (`step_field_vram_fx`) and the page
//! only uploads `field_vram_bytes` on the non-battle branch of its frame -
//! so there is no clobber path to guard. [`BattleVramChannel::serial`]
//! counts the images the channel has published so a page-side twin of the
//! guard can be added without touching the engine.
//!
//! The channel's dirty flag is a host detail (the native window re-uploads
//! inline; the page re-reads the bytes next frame), not a retail fact.

use wasm_bindgen::prelude::*;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::battle_vram::BattleFaceTables;

use crate::runtime::LegaiaRuntime;

pub(crate) use legaia_engine_session::battle_vram::BattleMemberFace;

/// Per-fight state of the re-upload channel.
#[derive(Default)]
pub(crate) struct BattleVramChannel {
    /// Set when a re-stamp changed texels since the page last re-uploaded.
    pub(crate) dirty: bool,
    /// Count of distinct battle VRAM images the channel has published (the
    /// entry image plus one per dirty edge). Monotonic within a page load.
    pub(crate) serial: u32,
    /// The battle-render generation the channel last saw; the tick that
    /// first sees a new one is the entry tick, whose full upload the page
    /// already does on the generation edge.
    last_generation: Option<u32>,
    /// The static `SCUS_942.54` face-frame and victory-mouth tables, read
    /// once per page load off the runtime's kept executable bytes. Empty on a
    /// PROT.DAT-only load - faces stay neutral then, exactly like the native
    /// window without a readable SCUS.
    face_tables: BattleFaceTables,
}

impl BattleVramChannel {
    /// Lazily parse the SCUS face tables off the kept executable bytes.
    fn load_face_tables(&mut self, scus: Option<&[u8]>) {
        if self.face_tables.load(scus) {
            crate::console_log(
                "play battle: SCUS face-frame tables unavailable - battle faces stay neutral",
            );
        }
    }

    /// Mark the battle VRAM as changed since the page's last upload.
    pub(crate) fn mark_dirty(&mut self) {
        if !self.dirty {
            self.serial = self.serial.wrapping_add(1);
        }
        self.dirty = true;
    }
}

impl LegaiaRuntime {
    /// Run the per-tick battle VRAM re-stamps. Cheap no-op outside battle.
    ///
    /// Order matches the native `redraw.rs` battle branch: faces, then the
    /// status recolour, then the effect CLUT stage. The world's own battle
    /// tick (clip advance, effect-script walk, status fold) has already run
    /// this frame, and so has `BattleHud::sync_status` (the latch arm).
    pub(crate) fn tick_battle_vram_channel(&mut self) {
        let in_battle = self
            .scene_host
            .host()
            .is_some_and(|h| h.world.mode == SceneMode::Battle)
            && self.battle_render.is_some();
        if !in_battle {
            // The battle boundary: retail rebuilds the battle context (and
            // with it the `+0x220` latches + the palette copies) per fight.
            // Neither the pristine palette copy nor the Stone edge state
            // may survive into the next fight's band assignment.
            if self.battle_hud.status_clut.armed() || self.battle_vram.last_generation.is_some() {
                self.battle_hud.status_clut.reset();
            }
            self.battle_vram.last_generation = None;
            return;
        }
        let generation = self.battle_render.as_ref().map(|b| b.generation);
        let entry_tick = self.battle_vram.last_generation != generation;
        if entry_tick {
            self.battle_vram.last_generation = generation;
            self.battle_vram.serial = self.battle_vram.serial.wrapping_add(1);
        }
        self.tick_battle_vram_restamps();
        self.tick_battle_stage_shell_web();
        if entry_tick {
            // The page uploads the whole battle VRAM on the generation edge
            // this same frame; the entry-tick stamps are already in those
            // bytes, so a second upload of the identical image is waste.
            self.battle_vram.dirty = false;
        }
    }

    /// The battle VRAM's re-stamps against the page's copy, through the steps
    /// the native window runs (`legaia_engine_session::battle_vram`): the
    /// facial animation, then the status CLUT recolour, the effect CLUT
    /// stages and the stage module's queued `MoveImage`s. A moved texel
    /// becomes the dirty flag.
    fn tick_battle_vram_restamps(&mut self) {
        use legaia_engine_session::battle_vram::{stamp_battle_faces, step_battle_vram_edits};
        self.battle_vram.load_face_tables(self.scus.as_deref());
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let mut dirty = false;
        if let Some(br) = self.battle_render.as_mut() {
            dirty |= stamp_battle_faces(
                &host.world,
                &self.battle_vram.face_tables,
                &mut br.faces,
                &mut br.vram,
            );
        }
        dirty |= step_battle_vram_edits(
            &mut host.world,
            &mut self.battle_hud.status_clut,
            self.battle_render.as_mut().map(|br| &mut br.vram),
        );
        if dirty {
            self.battle_vram.mark_dirty();
        }
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// `true` once per change: the battle VRAM texels moved since the page
    /// last re-uploaded them (re-read `play_battle_vram_bytes`).
    pub fn play_battle_vram_take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.battle_vram.dirty)
    }

    /// Count of distinct battle VRAM images the channel has published this
    /// page load: bumps on every battle entry and on every dirty edge. A
    /// page that records the serial it last uploaded can compare it against
    /// this to detect a stale texture - the browser twin of the native
    /// `check_battle_vram_residency`, evaluated where the texture is.
    pub fn play_battle_vram_serial(&self) -> u32 {
        self.battle_vram.serial
    }

    /// Number of party members the facial animator is registered for in the
    /// current fight (0 outside battle, or when no member assembled with a
    /// real texture band).
    pub fn play_battle_face_count(&self) -> u32 {
        self.battle_render
            .as_ref()
            .map(|b| b.faces.len() as u32)
            .unwrap_or(0)
    }
}

/// Test-facing hooks (native builds only; nothing here reaches the bundle).
#[cfg(not(target_arch = "wasm32"))]
impl LegaiaRuntime {
    /// Apply a status effect to a party slot through the world's tracker -
    /// what an enemy's inflicting strike does - so a driver can exercise
    /// the status CLUT drain without scripting a fight to the exact hit.
    /// `kind` is the `StatusKind` discriminant name (`"Stone"`, `"Toxic"`,
    /// ...). Returns `false` outside battle or for an unknown kind.
    pub fn debug_apply_battle_status(&mut self, slot: u8, kind: &str) -> bool {
        use legaia_engine_battle_vm::status_effects::StatusKind;
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        if host.world.mode != SceneMode::Battle {
            return false;
        }
        let kind = match kind {
            "Stone" => StatusKind::Stone,
            "Toxic" => StatusKind::Toxic,
            "Numb" => StatusKind::Numb,
            "Venom" => StatusKind::Venom,
            "Sleep" => StatusKind::Sleep,
            "Confuse" => StatusKind::Confuse,
            "Rot" => StatusKind::Rot,
            "Curse" => StatusKind::Curse,
            "Faint" => StatusKind::Faint,
            _ => return false,
        };
        host.world.battle.status_effects.apply(slot, kind);
        true
    }

    /// Queue one effect CLUT stage (`0x801F6418` byte = VRAM source x) on the
    /// world, exactly as the effect-script walk's table arm does, so a
    /// driver can exercise the drain without scripting a cast. Returns
    /// `false` outside battle.
    pub fn debug_stage_battle_effect_clut(&mut self, src_x: u8) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        if host.world.mode != SceneMode::Battle {
            return false;
        }
        host.world.battle.clut_stages.push(src_x);
        true
    }

    /// Stage battle anim `id` on party actor `slot` through the world's own
    /// staged-anim byte (`actor[+0x1DA]`) and commit it the way the SM's
    /// clip boundary does (`FUN_8004AD80`), so a driver can play a clip
    /// whose face tracks carry active records without scripting a fight to
    /// that exact swing. Returns `false` outside battle or for an unknown
    /// slot.
    pub fn debug_stage_battle_anim(&mut self, slot: usize, id: u8) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        if host.world.mode != SceneMode::Battle || slot >= host.world.actors.len() {
            return false;
        }
        host.world.actors[slot].battle.queued_anim = id;
        host.world.commit_staged_battle_anim(slot);
        host.world.actors[slot]
            .battle_animation
            .as_ref()
            .is_some_and(|p| p.action_id() == id)
    }

    /// The anim ids of registered member `face_index` whose face tracks
    /// carry at least one active record (record slots below `0x10`, art-bank
    /// records as `0x10 + index`) - what a driver stages to make the
    /// animator issue a non-neutral stamp. Empty outside battle.
    pub fn debug_battle_face_tracked_ids(&self, face_index: usize) -> Vec<u16> {
        let Some(mf) = self
            .battle_render
            .as_ref()
            .and_then(|b| b.faces.get(face_index))
        else {
            return Vec::new();
        };
        let has_active = |t: &legaia_actor_formats::face_anim::FaceTracks| {
            t.eyes.iter().any(|r| r.end != 0) || t.mouth.iter().any(|r| r.end != 0)
        };
        let base = legaia_actor_formats::battle_char_assembly::ART_ANIM_ID_BASE as usize;
        let mut out: Vec<u16> = mf
            .tracks
            .iter()
            .enumerate()
            .filter(|(i, t)| *i < base && t.as_ref().is_some_and(has_active))
            .map(|(i, _)| i as u16)
            .collect();
        out.extend(
            mf.art_tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.as_ref().is_some_and(has_active))
                .map(|(i, _)| (base + i) as u16),
        );
        out
    }

    /// One line per registered member describing the animator's inputs this
    /// frame - playing clip id + frame, whether that clip has any active
    /// face record at all, the last stamp count - plus the battle-end
    /// latch. A driver's diagnostic, not a model.
    pub fn debug_battle_face_state(&self) -> String {
        let Some(host) = self.scene_host.host() else {
            return "no host".into();
        };
        let Some(br) = self.battle_render.as_ref() else {
            return "no battle render".into();
        };
        let mut out = format!("end={:?}", host.world.battle.end);
        for mf in &br.faces {
            let (id, frame, has_player) = host
                .world
                .actors
                .get(mf.actor_slot)
                .and_then(|a| a.battle_animation.as_ref())
                .map(|p| (p.action_id(), p.current_frame(), true))
                .unwrap_or((0, 0, false));
            let tracks = if id >= legaia_actor_formats::battle_char_assembly::ART_ANIM_ID_BASE {
                mf.art_tracks
                    .get(
                        (id - legaia_actor_formats::battle_char_assembly::ART_ANIM_ID_BASE)
                            as usize,
                    )
                    .and_then(|t| t.as_ref())
            } else {
                mf.tracks.get(id as usize).and_then(|t| t.as_ref())
            };
            let active: usize = tracks
                .map(|t| {
                    t.eyes.iter().filter(|r| r.end != 0).count()
                        + t.mouth.iter().filter(|r| r.end != 0).count()
                })
                .unwrap_or(0);
            let slots_with_tracks = mf.tracks.iter().flatten().filter(|t| !t.is_empty()).count();
            out.push_str(&format!(
                " | slot{} char{} player={has_player} id={id:#04x} frame={frame} \
                 active_records={active} slots_with_tracks={slots_with_tracks}/{} \
                 art_slots={} last_stamps={:?}",
                mf.actor_slot,
                mf.char_index,
                mf.tracks.len(),
                mf.art_tracks.iter().flatten().count(),
                mf.last_stamps.as_ref().map(|s| s.len())
            ));
        }
        out
    }

    /// The face-stamp destination rects the animator would issue for every
    /// registered member's neutral face, as `[dst_x, dst_y, w, h]` quads -
    /// the region a driver compares across frames. Empty when the SCUS face
    /// tables are unavailable or no member is registered.
    pub fn debug_battle_face_regions(&mut self) -> Vec<u16> {
        self.battle_vram.load_face_tables(self.scus.as_deref());
        let Some(tables) = self.battle_vram.face_tables.face() else {
            return Vec::new();
        };
        let Some(br) = self.battle_render.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for mf in &br.faces {
            for s in tables.stamps(mf.char_index, mf.actor_slot, None, 0, false) {
                out.extend_from_slice(&[s.dst_x, s.dst_y, s.w, s.h]);
            }
        }
        out
    }
}
