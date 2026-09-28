//! A party member's **battle form**, decoded once for both play hosts.
//!
//! Retail's battle loader assembles each member's mesh from the character's
//! player battle file (extraction PROT `863 + char_slot`): the equipment-id
//! sections are spliced into one TMD, relocated into the present-party
//! ordinal's runtime texture band, posed from the file's own idle stream, and
//! the band's pixels and palette come off the same file
//! ([`legaia_asset::battle_char_assembly`], `docs/formats/character-mesh.md`).
//! The port keeps the static PROT 1204 pack (posed from PROT 1203) as a
//! per-member fallback when the file does not assemble.
//!
//! The native window and the browser play page each carried that whole
//! ladder - about two hundred and fifty lines apiece - and the copies had
//! drifted: a missing idle stream fell back to PROT 1204 on the native
//! window and kept the assembled mesh (with no pose source) on the page, and
//! only the native window overlaid the character's battle palette on a
//! fallback mesh. A third reported difference - the native window installing
//! a fallback member's (empty) art-record list - changed nothing, because
//! installing an empty list writes no record. This module is the one copy,
//! with the native window's choices; the hosts keep only what differs by
//! renderer (the GPU mesh upload and the per-frame posing).
//!
//! The rule that decides content versus placement is retail's, live-verified
//! for all four characters: the **character** picks the content (player file,
//! palette, art bank), the party **ordinal** picks the runtime band
//! (`relocate_tsb_cba` x = `0x200 + i*0x80`, CLUT row `481 + i`).

use legaia_asset::battle_char_assembly as bca;
use legaia_asset::face_anim::FaceTracks;
use legaia_asset::monster_archive::MonsterAnimation;

use crate::scene::ProtIndex;

/// Extraction PROT of Vahn's player battle file; character `c` is `+ c`.
pub const PLAYER_BATTLE_FILE_BASE: u32 = 863;
/// `readef.DAT` (extraction PROT 894): the per-character `"ME"` art archives.
pub const READEF_PROT_INDEX: u32 = 894;
/// PROT 1203: the rest-pose banks for the PROT 1204 fallback meshes.
pub const FALLBACK_POSE_PROT_INDEX: u32 = 1203;
/// First PROT 1203 record of each character's bank (Vahn / Noa / Gala); the
/// first record is that character's idle rest pose, bone `i` driving 1204
/// object `i`. Terra has no bank.
const FALLBACK_POSE_BANKS: [usize; 3] = [0, 9, 18];

/// One rigid transform per TMD object: `(translation, rotation)`.
pub type BonePose = ([i16; 3], [i16; 3]);

/// The per-battle sources every member draws from: the PROT 1204 fallback
/// pack and the PROT 1203 rest-pose banks.
pub struct PartyFormSources {
    pub pack: legaia_asset::battle_char_pack::BattleCharPack,
    pub fallback_poses: Option<legaia_asset::player_anm::PlayerAnmBundle>,
}

impl PartyFormSources {
    /// Parse the fallback pack and its rest poses, and upload the eight
    /// Baka Fighter authoring atlases the fallback meshes sample at their
    /// declared rects. `None` when the pack does not parse.
    pub fn load(index: &ProtIndex, vram: &mut legaia_tim::Vram) -> Option<Self> {
        let mesh = index
            .entry_bytes(legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)
            .ok()?;
        let atlas = index
            .entry_bytes(legaia_asset::battle_char_pack::ATLAS_PROT_ENTRY_INDEX)
            .ok()?;
        let pack = legaia_asset::battle_char_pack::parse(&mesh, &atlas).ok()?;
        for a in &pack.atlases {
            if let Ok(tim) = legaia_tim::parse(&a.tim_bytes) {
                vram.upload_tim(&tim);
            }
        }
        let fallback_poses = index
            .entry_bytes_extended(FALLBACK_POSE_PROT_INDEX)
            .ok()
            .and_then(|raw| {
                [6usize, 3, 5, 7].iter().find_map(|&dc| {
                    legaia_asset::player_anm::find_in_entry(&raw, dc)
                        .into_iter()
                        .next()
                })
            });
        Some(Self {
            pack,
            fallback_poses,
        })
    }
}

/// Everything one party member's battle form carries, once its pixels and
/// palette are in the battle VRAM.
pub struct PartyBattleForm {
    /// Present-party ordinal (the actor slot and the texture band).
    pub member: usize,
    /// Roster slot of the occupying character (the content).
    pub cslot: usize,
    /// `true` for the retail assembly, `false` for the PROT 1204 fallback.
    pub assembled: bool,
    pub tmd: legaia_tmd::Tmd,
    pub tmd_bytes: Vec<u8>,
    /// The per-object idle clip (assembled forms only), expanded so channel
    /// `i` drives TMD object `i`.
    pub idle: Option<MonsterAnimation>,
    /// The static rest pose: frame 0 of [`Self::idle`] for an assembled
    /// form, the PROT 1203 bank's idle record for a fallback. Empty when
    /// neither exists (a Terra fallback renders unposed).
    pub rest_pose: Vec<BonePose>,
    /// Every action-stream slot plus the equipment-spliced swings
    /// (`0xC..=0xF`); `None` on a fallback.
    pub action_clips: Option<Vec<Option<MonsterAnimation>>>,
    /// The art-animation bank (record[0] `+0x58`); empty on a fallback.
    pub art_bank: Vec<Option<MonsterAnimation>>,
    /// The bank's records - the arts the queue-builder matches. Empty on a
    /// fallback.
    pub art_records: Vec<bca::ArtAnimRecord>,
    /// Per-action face tracks, present only when the facial animator can
    /// run: an assembled form whose band holds the real texture-pool pixels
    /// (the face-frame strip the stamps copy from), for characters `0..=2`
    /// on bands `0..=2` - the retail animator skips Terra.
    pub face_tracks: Option<Vec<Option<FaceTracks>>>,
    /// The art bank records' embedded-entry face tracks.
    pub art_face_tracks: Vec<Option<FaceTracks>>,
}

/// Build member `member`'s battle form: the retail assembly out of the
/// occupying character's player file, or the PROT 1204 fallback. Writes the
/// band pixels (texture pools + record[0] image blocks at the pinned
/// `FUN_80052FA0` placement, or the 1204 atlas pair when that decode fails)
/// and the character's battle palette into `vram`. `None` when neither path
/// yields a mesh.
///
/// The assembled path requires the idle stream: the assembled TMD is a set
/// of object-local pieces, so without a pose source it would draw every
/// limb at the origin, and the fallback's known-correct rest pose is the
/// better picture.
pub fn build_party_battle_form(
    index: &ProtIndex,
    world: &crate::world::World,
    sources: &PartyFormSources,
    vram: &mut legaia_tim::Vram,
    member: usize,
) -> Option<PartyBattleForm> {
    let cslot = world.party_roster_slot(member);
    let raw = index
        .entry_bytes_extended(PLAYER_BATTLE_FILE_BASE + cslot as u32)
        .map_err(|e| log::warn!("battle party {cslot}: player file: {e:#}"))
        .ok();
    // Equipped item ids from the canonical roster record; an absent or
    // zeroed record assembles the all-default (unequipped) sections.
    let equipped: [u8; 5] = world
        .party
        .roster
        .members
        .get(cslot)
        .map(|rec| {
            let s = rec.equipment().slots;
            [s[0], s[1], s[2], s[3], s[4]]
        })
        .unwrap_or_default();
    let form = raw
        .as_deref()
        .and_then(|raw| assemble(index, sources, vram, raw, &equipped, member, cslot))
        .or_else(|| fallback(sources, member, cslot))?;
    if let Some(raw) = raw.as_deref() {
        overlay_battle_palette(vram, raw, cslot, &form);
    }
    Some(form)
}

fn assemble(
    index: &ProtIndex,
    sources: &PartyFormSources,
    vram: &mut legaia_tim::Vram,
    raw: &[u8],
    equipped: &[u8; 5],
    member: usize,
    cslot: usize,
) -> Option<PartyBattleForm> {
    let warn = |what: &str, e: &dyn std::fmt::Display| {
        log::warn!("battle party {cslot}: {what}: {e:#} (PROT 1204 fallback)");
    };
    let file_pack = legaia_asset::battle_data_pack::parse(raw)
        .map_err(|e| warn("player-file pack", &e))
        .ok()?;
    let mut asm = bca::assemble_character(raw, &file_pack, equipped)
        .map_err(|e| warn("mesh assembly", &e))
        .ok()?;
    bca::relocate_tsb_cba(&mut asm.tmd, member as u8)
        .map_err(|e| warn("TSB/CBA relocation", &e))
        .ok()?;
    let tmd = legaia_tmd::parse(&asm.tmd)
        .map_err(|e| warn("assembled TMD parse", &e))
        .ok()?;
    let idle = match bca::idle_battle_animation(raw) {
        Ok(Some(anim)) => bca::expand_animation_for_objects(&anim, &asm.anm_bones),
        Ok(None) => {
            log::warn!("battle party {cslot}: no idle stream (PROT 1204 fallback)");
            return None;
        }
        Err(e) => {
            warn("idle-stream decode", &e);
            return None;
        }
    };

    // Band pixels. A failed pool decode degrades to the 1204 atlas pair
    // (content = the CHARACTER's pair, destination = the MEMBER's band).
    let uploads = bca::character_texture_uploads(raw, &file_pack, equipped, member as u8)
        .unwrap_or_else(|e| {
            log::warn!("battle party {cslot}: texture-pool decode: {e:#}");
            Vec::new()
        });
    let real_band = !uploads.is_empty();
    if real_band {
        for u in &uploads {
            vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
            if !u.clut.is_empty() {
                vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
            }
        }
    } else {
        for half in 0..2usize {
            if let Some(atlas) = sources.pack.atlases.get(cslot * 2 + half)
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
    }

    // Record[0] action streams, then the equipment-spliced swings on
    // `0xC..=0xF`; the face tracks follow the same keys.
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; bca::ACTION_SLOT_COUNT];
    let mut faces: Vec<Option<FaceTracks>> = vec![None; bca::ACTION_SLOT_COUNT];
    match legaia_asset::face_anim::battle_face_tracks(raw) {
        Ok(t) => faces = t,
        Err(e) => log::warn!("battle party {cslot}: face-track decode: {e:#}"),
    }
    match bca::battle_animations(raw) {
        Ok(anims) => {
            for a in &anims {
                if let Some(slot) = clips.get_mut(a.action_id as usize) {
                    *slot = Some(bca::expand_animation_for_objects(a, &asm.anm_bones));
                }
            }
        }
        Err(e) => log::warn!("battle party {cslot}: action-stream decode: {e:#}"),
    }
    match bca::swing_battle_animations(raw, &file_pack, equipped) {
        Ok(swings) => {
            for s in &swings {
                if let Some(slot) = clips.get_mut(s.slot as usize) {
                    *slot = Some(bca::expand_animation_for_objects(&s.anim, &asm.anm_bones));
                }
                if let Some(face) = faces.get_mut(s.slot as usize) {
                    *face = s.face;
                }
            }
        }
        Err(e) => log::warn!("battle party {cslot}: swing decode: {e:#}"),
    }
    let (art_bank, art_face_tracks, art_records) = art_bank(index, raw, cslot, &asm.anm_bones);

    let rest_pose = idle
        .frames
        .first()
        .map(|f0| {
            f0.iter()
                .map(|p| ([p.tx, p.ty, p.tz], [p.rx as i16, p.ry as i16, p.rz as i16]))
                .collect()
        })
        .unwrap_or_default();
    let face_tracks = (real_band
        && cslot < legaia_asset::face_anim::FACE_CHAR_COUNT
        && member < legaia_asset::face_anim::FACE_SLOT_COUNT)
        .then_some(faces);
    Some(PartyBattleForm {
        member,
        cslot,
        assembled: true,
        tmd,
        tmd_bytes: asm.tmd,
        idle: Some(idle),
        rest_pose,
        action_clips: Some(clips),
        art_bank,
        art_records,
        face_tracks,
        art_face_tracks,
    })
}

/// The static PROT 1204 slot, posed from its PROT 1203 bank.
fn fallback(sources: &PartyFormSources, member: usize, cslot: usize) -> Option<PartyBattleForm> {
    let slot = sources.pack.slot(cslot)?;
    let tmd = legaia_tmd::parse(&slot.tmd_bytes).ok()?;
    let rest_pose = match (&sources.fallback_poses, FALLBACK_POSE_BANKS.get(cslot)) {
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
    Some(PartyBattleForm {
        member,
        cslot,
        assembled: false,
        tmd,
        tmd_bytes: slot.tmd_bytes.clone(),
        idle: None,
        rest_pose,
        action_clips: None,
        art_bank: Vec::new(),
        art_records: Vec::new(),
        face_tracks: None,
        art_face_tracks: Vec::new(),
    })
}

/// Overlay the character's battle palette onto every CLUT row the form's
/// mesh samples: Vahn by the byte-exact record parse, the others through
/// the equipment-robust collector over the sampled columns.
fn overlay_battle_palette(
    vram: &mut legaia_tim::Vram,
    raw: &[u8],
    cslot: usize,
    form: &PartyBattleForm,
) {
    // Posing moves no CBA, so the unposed mesh names the same rows.
    let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&form.tmd, &form.tmd_bytes);
    let mut rows: Vec<u16> = vmesh.cba_tsb.iter().map(|c| (c[0] >> 6) & 0x1FF).collect();
    rows.sort_unstable();
    rows.dedup();
    let mut cols: Vec<u16> = vmesh.cba_tsb.iter().map(|c| (c[0] & 0x3F) * 16).collect();
    cols.sort_unstable();
    cols.dedup();
    let pal = match cslot {
        0 => legaia_asset::battle_char_palette::find_record0(raw)
            .and_then(|rec0| legaia_asset::battle_char_palette::parse_record(raw, rec0).ok()),
        1..=3 => legaia_asset::battle_char_palette::collect_palette(raw, 0, &cols).ok(),
        _ => None,
    };
    let Some(pal) = pal else { return };
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

/// One character's art-animation bank (commit-ready clips), the records'
/// embedded-entry face tracks, and the records themselves. Each record's
/// stream resolves through the character's `readef.DAT` `"ME"` archive
/// (main slot `3*char+1`, base slot `3*char+2` for `rate_alt == 0xFF`);
/// failures degrade per record or to an empty bank.
type ArtBank = (
    Vec<Option<MonsterAnimation>>,
    Vec<Option<FaceTracks>>,
    Vec<bca::ArtAnimRecord>,
);

fn art_bank(index: &ProtIndex, raw: &[u8], cslot: usize, anm_bones: &[u8]) -> ArtBank {
    let record0 = match bca::decode_record0(raw) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("battle party {cslot}: record[0] decode for art bank: {e:#}");
            return (Vec::new(), Vec::new(), Vec::new());
        }
    };
    let records = match bca::art_animation_bank(&record0) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("battle party {cslot}: art-bank parse: {e:#}");
            return (Vec::new(), Vec::new(), Vec::new());
        }
    };
    let faces: Vec<Option<FaceTracks>> = records.iter().map(|r| r.face).collect();
    let readef = match index.entry_bytes_extended(READEF_PROT_INDEX) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("battle party: readef.DAT (PROT {READEF_PROT_INDEX}) read: {e:#}");
            return (Vec::new(), faces, records);
        }
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
        match bca::art_animation(rec, archive) {
            Ok(anim) => bank[rec.index] = Some(bca::expand_animation_for_objects(&anim, anm_bones)),
            Err(e) => log::warn!(
                "battle party {cslot}: art record {} stream: {e:#}",
                rec.index
            ),
        }
    }
    (bank, faces, records)
}

/// The Tactical-Arts character whose records a roster slot's bank carries
/// (Vahn / Noa / Gala; Terra has no arts).
pub fn art_character(cslot: usize) -> Option<legaia_art::Character> {
    [
        legaia_art::Character::Vahn,
        legaia_art::Character::Noa,
        legaia_art::Character::Gala,
    ]
    .get(cslot)
    .copied()
}

impl crate::world::World {
    /// Install a party member's battle form on its actor: the idle clip,
    /// the action-clip set, the art bank, and the art records the live arts
    /// input tokenizes (a fallback form has none).
    ///
    /// Both play hosts call this after uploading the form's mesh; the facial
    /// animator's registration ([`PartyBattleForm::face_tracks`]) stays with
    /// the host that owns the band's live VRAM.
    pub fn install_party_battle_form(&mut self, form: &mut PartyBattleForm) {
        let member = form.member;
        if let Some(anim) = &form.idle
            && let Some(player) = crate::battle_anim::MonsterAnimPlayer::new(anim)
        {
            self.set_actor_battle_animation(member, player);
        }
        if let Some(clips) = form.action_clips.take() {
            self.set_actor_battle_action_clips(member, std::sync::Arc::new(clips));
        }
        let bank = std::mem::take(&mut form.art_bank);
        if !bank.is_empty() {
            self.set_actor_battle_art_bank(member, std::sync::Arc::new(bank));
        }
        if let Some(character) = art_character(form.cslot) {
            self.install_art_bank_records(character, &form.art_records);
        }
    }

    /// Install a monster's battle form on its actor: the texture slot its
    /// mesh was relocated into (the posed rebuild re-applies it) and the
    /// archive idle clip. The archive-order action clips are the engine's own
    /// to stage (`SceneHost::tick`), so none is installed here.
    pub fn install_monster_battle_form(
        &mut self,
        actor: usize,
        tex_slot: u8,
        idle: Option<&MonsterAnimation>,
    ) {
        if let Some(a) = self.actors.get_mut(actor) {
            a.battle_tex_slot = Some(tex_slot);
        }
        if let Some(anim) = idle
            && let Some(player) = crate::battle_anim::MonsterAnimPlayer::new(anim)
        {
            self.set_actor_battle_animation(actor, player);
        }
    }
}

/// How many of the battle texture slots a set of monster binds consumed:
/// one past the highest slot bound. A mid-battle summon's creature texture
/// goes into this slot next. Repeated species share a slot, so this is not
/// the number of monsters.
pub fn monster_tex_slots_used(bound_slots: impl IntoIterator<Item = u8>) -> u8 {
    bound_slots
        .into_iter()
        .map(|s| s.saturating_add(1))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_species_share_their_slot() {
        assert_eq!(monster_tex_slots_used([0, 1, 1, 1]), 2);
        assert_eq!(monster_tex_slots_used([]), 0);
        assert_eq!(monster_tex_slots_used([3]), 4);
    }
}
