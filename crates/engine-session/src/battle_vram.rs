//! The battle VRAM's per-tick re-stamps both play hosts run: the party's
//! facial animation, the status CLUT recolour, the effect CLUT stages and the
//! stage module's queued `MoveImage`s.
//!
//! Each host keeps its own copy of the fight's battle VRAM - the native
//! window a CPU image it re-uploads to wgpu, the browser page a copy it hands
//! to WebGL on a dirty edge - and used to carry its own copy of these steps
//! over it, the face pass line for line. The steps live here; a host calls
//! [`stamp_battle_faces`] then [`step_battle_vram_edits`] once per tick and
//! does only its own half on a `true` (upload, or mark its channel dirty).

use legaia_actor_formats::battle_char_assembly::ART_ANIM_ID_BASE;
use legaia_actor_formats::face_anim::{
    ART_BAND_FIRST, ART_BAND_LAST, ArtMouthOverride, ArtMouthTables, FaceFrameTables, FaceStamp,
    FaceTracks,
};
use legaia_engine_battle::battle_status_clut::StatusClutState;
use legaia_engine_battle_vm::battle_action::BattleEndCause;
use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_tim::Vram;

/// One party member's facial-animation state for the current fight.
#[derive(Debug, Clone)]
pub struct BattleMemberFace {
    /// World actor slot (= present-party band ordinal, 0..3).
    pub actor_slot: usize,
    /// Character index (0 Vahn / 1 Noa / 2 Gala; Terra never gets an entry -
    /// the retail animator skips char 3).
    pub char_index: usize,
    /// Face tracks indexed by action slot (record[0] entries + the
    /// equipment-spliced swing slots 0xC..0xF).
    pub tracks: Vec<Option<FaceTracks>>,
    /// Face tracks of the art-bank records' embedded entries, indexed by
    /// bank record (= playing clip `action_id - 0x10`). Retail reads these
    /// through the `FUN_8004AD80`-installed entry pointer (bank record
    /// `+0x24`), i.e. record `+0xB0` eyes / `+0xBC` mouth - the mid-battle
    /// art-strike faces.
    pub art_tracks: Vec<Option<FaceTracks>>,
    /// The stamp set the last pass issued (`None` = nothing applied yet,
    /// force the first stamp). A frame whose set is identical re-issues
    /// nothing: retail re-issues identical `MoveImage`s every frame, and the
    /// visible result is the same.
    pub last_stamps: Option<Vec<FaceStamp>>,
    /// Victory-window frame counter - the engine's per-member equivalent of
    /// the retail global `gp+0x9EA` (reset to 0 when the win pose is staged,
    /// advanced per frame; the stamp pass halves it). `Some` while the
    /// member's mouth-override window is open, `None` outside it.
    pub art_counter: Option<u16>,
}

impl BattleMemberFace {
    /// A member registered at the battle build, nothing stamped yet.
    pub fn new(
        actor_slot: usize,
        char_index: usize,
        tracks: Vec<Option<FaceTracks>>,
        art_tracks: Vec<Option<FaceTracks>>,
    ) -> Self {
        Self {
            actor_slot,
            char_index,
            tracks,
            art_tracks,
            last_stamps: None,
            art_counter: None,
        }
    }
}

/// The static `SCUS_942.54` tables the face pass reads, parsed once.
#[derive(Debug, Default)]
pub struct BattleFaceTables {
    face: Option<FaceFrameTables>,
    art_mouth: Option<ArtMouthTables>,
    attempted: bool,
}

impl BattleFaceTables {
    /// Whether [`Self::load`] already ran. A host whose executable read is
    /// itself costly asks this before reading it.
    pub fn attempted(&self) -> bool {
        self.attempted
    }

    /// Parse the face-frame tables (`FaceFrameTables`) and the
    /// victory-window mouth override (`0x80077E80`, `ArtMouthTables`) off the
    /// executable, once: a failed attempt (a disc-free run, an unparsable
    /// executable) is remembered and the faces stay neutral. Returns `true`
    /// only on the attempt that left the face tables unavailable, so the
    /// host logs it once.
    pub fn load(&mut self, scus: Option<&[u8]>) -> bool {
        if self.attempted {
            return false;
        }
        self.attempted = true;
        self.face = scus.and_then(FaceFrameTables::from_scus);
        self.art_mouth = scus.and_then(ArtMouthTables::from_scus);
        self.face.is_none()
    }

    /// The face-frame tables, when the executable carried them.
    pub fn face(&self) -> Option<&FaceFrameTables> {
        self.face.as_ref()
    }
}

/// Per-tick battle facial animation: re-stamp each registered party
/// member's current eye + mouth face frame onto the band's live face rows,
/// as the retail per-frame animator does. The playing clip's `action_id`
/// selects the member's face tracks, its integer keyframe cursor is the frame
/// counter, and the selected frames are VRAM-to-VRAM copies from the band's
/// face-frame strip (`Vram::move_image`, the `MoveImage` stamp). During the
/// victory window - the battle ended in a monster wipe while a member still
/// plays a dynamic-art-slot clip (staged id `0x11..=0x18`, e.g. the killing
/// art) - the mouth records come from the static `0x80077E80` override table
/// and the animator clocks on the halved victory counter instead (the
/// win-quote mouth flap).
///
/// Returns whether any texel moved. A no-op outside battle, with no member
/// registered, or without the face tables.
// PORT: FUN_80047430 (facial-animator dispatch): per visible party
// node, call the stamp pass with (band slot, char index, cursor
// keyframes, playing action entry), skipping char 3 (Terra) and bands
// >= 3. The stamp-selection half is `FaceFrameTables::stamps_with_art_window`
// (PORT: FUN_8004C7B4 in legaia_actor_formats::face_anim).
pub fn stamp_battle_faces(
    world: &World,
    tables: &BattleFaceTables,
    faces: &mut [BattleMemberFace],
    vram: &mut Vram,
) -> bool {
    if world.mode != SceneMode::Battle || faces.is_empty() {
        return false;
    }
    let Some(face_tables) = tables.face() else {
        return false;
    };
    let art_tables = tables.art_mouth.as_ref();
    // Retail gate 1 of the victory-window mouth override: the battle-end
    // signal `DAT_8007BD71 == 0xFE` raised by a monster wipe (the SM `0x5A`
    // arm; the engine mirror is the `BattleActionHost::battle_end` latch).
    // Gates 2/3 - the victory sequencer's phase halfword `ctx+0x6CE != 0`
    // and the celebration flag `DAT_8007BD60 & 0x80` (which the party-wipe
    // path clears) - are the retail victory presentation's internal progress
    // flags; the engine has no victory sequencer, so "the won battle is
    // still on screen" stands in for them. Escapes also raise 0xFE but never
    // set the celebration flag, so they stay excluded.
    let victory_window = world.battle.end == Some(BattleEndCause::MonsterWipe);
    let mut changed = false;
    for mf in faces {
        let Some(actor) = world.actors.get(mf.actor_slot) else {
            continue;
        };
        // No player yet = the rest pose: behaves like clip frame 0 of the
        // (track-less) idle, i.e. the neutral face.
        let (action_id, frame) = actor
            .battle_animation
            .as_ref()
            .map(|p| (p.action_id(), p.current_frame()))
            .unwrap_or((0, 0));
        // Track source. Staged ids >= 0x10 are art-bank clips: retail
        // materializes bank record `id - 0x10` and installs its embedded
        // entry (record +0x24) as the action-table pointer (FUN_8004AD80), so
        // the animator reads THAT entry's tracks (record +0xB0 eyes / +0xBC
        // mouth). Below the art base, the playing clip's action slot picks
        // the record[0] / swing entry.
        let tracks = if action_id >= ART_ANIM_ID_BASE {
            mf.art_tracks
                .get((action_id - ART_ANIM_ID_BASE) as usize)
                .and_then(|t| t.as_ref())
        } else {
            mf.tracks.get(action_id as usize).and_then(|t| t.as_ref())
        };
        // Retail gate 4: the member's last-staged anim id (`actor[+0x1DB]`;
        // the engine's art-bank clips carry it as their `action_id`) sits in
        // the dynamic-art-slot band `0x11..=0x18`. Open the override window,
        // clocking the member's `gp+0x9EA` mirror from 0; closed, the counter
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
        // The retail mouth-neutral gate: character-record word `+0xF8` bit
        // 0x2000 = ability bitfield (`+0xF4`) bit 45 (passive 0x2D Rage,
        // Evil Medallion). Read it off the occupying character's rebuilt
        // ability bytes (byte 5 bit 0x20).
        let force_neutral_mouth = world
            .party
            .roster
            .members
            .get(world.party_roster_slot(mf.actor_slot))
            .map(|m| m.ability_bits()[5] & 0x20 != 0)
            .unwrap_or(false);
        let stamps = face_tables.stamps_with_art_window(
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
    changed
}

/// The battle VRAM's other per-tick edits, in the order both hosts run them
/// after [`stamp_battle_faces`]:
///
/// * **Status CLUT recolour** - the fourth pass of `FUN_8004CE2C`: an actor
///   whose Stone latch fired has its party CLUT row (`481 + slot`) restaged
///   grey from the pristine palette copy
///   ([`legaia_engine_battle::battle_status_clut`]). The latch is armed by
///   `BattleHud::sync_status`, which each host's battle-event step runs.
/// * **Effect CLUT stages** - the palette arm of the action-effect script's
///   table form (`FUN_801DEA50`, `0x801df0dc..0x801df134`): each queued
///   `0x801F6418` byte is a VRAM source x whose sixteen entries move onto
///   `(224, 476)` ([`legaia_engine_battle::battle_effect_clut`]). Drained
///   whether or not a VRAM is held, so the queue cannot accumulate.
/// * **Stage-module moves** - the battle `MoveImage`s a stage module queued
///   (`World::apply_battle_vram_moves`: the evolved-Cort arrival blanks the
///   ground tile); dropped when no VRAM is held.
///
/// `vram` is `None` when the host holds no battle VRAM image. Returns whether
/// any texel moved. The backdrop object-list rebuild that can follow a stage
/// move is mesh work, and stays the host's.
pub fn step_battle_vram_edits(
    world: &mut World,
    status_clut: &mut StatusClutState,
    vram: Option<&mut Vram>,
) -> bool {
    let stages = world.drain_battle_clut_stages();
    let Some(vram) = vram else {
        world.discard_battle_vram_edits();
        return false;
    };
    let mut dirty = status_clut.armed() && status_clut.step(vram);
    for x in stages {
        dirty |= legaia_engine_battle::battle_effect_clut::stage_effect_clut(vram, x);
    }
    dirty |= world.apply_battle_vram_moves(vram);
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Outside battle, or with no tables, the face pass touches nothing; the
    /// edit step drains the CLUT queue even without a VRAM.
    #[test]
    fn the_steps_gate_on_battle_and_drain_without_vram() {
        let mut world = World::new();
        let mut vram = Vram::new();
        let mut faces = vec![BattleMemberFace::new(0, 0, Vec::new(), Vec::new())];
        let mut tables = BattleFaceTables::default();
        assert!(tables.load(None), "no executable: reported once");
        assert!(!tables.load(None) && tables.attempted());
        world.mode = SceneMode::Battle;
        assert!(!stamp_battle_faces(&world, &tables, &mut faces, &mut vram));
        let mut clut = StatusClutState::default();
        assert!(!step_battle_vram_edits(&mut world, &mut clut, None));
        assert!(world.drain_battle_clut_stages().is_empty());
    }
}
