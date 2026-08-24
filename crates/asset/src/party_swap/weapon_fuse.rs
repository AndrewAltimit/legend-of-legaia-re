//! Fuse the host character's own equipped **weapon** into the swapped hand.
//!
//! The swap drops every equipment-visual surplus object (see
//! `playerize::rewrite_section_record`), which also drops the held weapon -
//! but for most weapon records the weapon is not a surplus at all: the
//! held-item section **re-authors the hand (and sometimes forearm) bone
//! object with the weapon welded in** (`equip_isolate` module doc). The
//! swapped sibling therefore fought bare-handed (or, for Che, with his own
//! welded hammer-fist standing in for every weapon).
//!
//! This module puts the host's weapon back, per record: for each held-item
//! section record it runs the same curated item-alone cut the site's
//! equipment viewer uses ([`equip_isolate`]), takes the claimed primitives
//! of the **skeleton** objects (the welded weapon - surplus copies stay
//! dropped, the swap's variant aliasing covers those), flat-shades each
//! primitive from the mean texel it sampled on the retail band, and hands
//! the per-channel geometry to the record rewrite to merge into the baked
//! sibling part. Coordinates copy verbatim: the claimed primitives are
//! already in the attach bone's local frame, the same frame the baked fist
//! was seated into, so the weapon rides the host's clips exactly as retail
//! posed it (the pose-exact merge measured on the enemy-side swap).
//!
//! Flat shading is the deliberate v1: the band re-layout repaints every
//! section tile with the sibling's texels, so the weapon's retail texels
//! are gone from VRAM - per-prim baked colours (`F3`/`F4`) need no texture
//! space, no palette columns and no UV management, and a club or blade at
//! PSX resolution reads fine with its painted shading averaged per face.
//!
//! Ra-Seru records (item-table ids `0x01..=0x18`, the three Ra-Serus'
//! level forms - the living arm the section-3/section-2 sibling slot
//! carries) are left alone here: dressing the arm is a separate, larger
//! cut than "hold your weapon".

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use crate::battle_char_assembly::{self as bca, SECTION_COUNT};
use bca::{equip_isolate, equip_item};
use legaia_tim::Vram;
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::{ModelGroup, ModelObject, ModelPrim};

/// Item-table ids `0x01..=0x18` are the Ra-Seru level forms (Meta / Terra
/// / Ozma `$1..$8` - `asset item-tables` on `SCUS_942.54`), not weapons.
const RA_SERU_MAX_ID: u32 = 0x18;

/// The two held-item descriptor sections. Which one is the real weapon
/// varies per character (Vahn / Gala: section 2; Noa: section 3 - her
/// section 2 is Terra), so both are walked and the Ra-Seru id range does
/// the telling.
const HELD_SECTIONS: [usize; 2] = [2, 3];

/// VRAM band slot the extraction assembles into (any slot works - the cut
/// only compares texels against the same slot's bare assembly).
const BAND: u8 = 0;

/// Per-record weapon geometry to merge at record-rewrite time.
#[derive(Default)]
pub(crate) struct WeaponFusion {
    /// `(section, record id)` -> channel -> flat-shaded weapon geometry in
    /// that channel's local frame.
    pub per_record: BTreeMap<(usize, u32), BTreeMap<u8, ModelObject>>,
}

/// 5-bit BGR555 channel to 8-bit.
fn c5to8(v: u16) -> u32 {
    let v = v as u32;
    (v << 3) | (v >> 2)
}

/// Mean colour of the texels a primitive sampled, as the F-prim's baked
/// RGB. Falls back to mid-grey for a primitive with no opaque texels.
fn mean_colour(words: &[u16]) -> [u8; 3] {
    if words.is_empty() {
        return [128, 128, 128];
    }
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for &w in words {
        r += c5to8(w & 31);
        g += c5to8((w >> 5) & 31);
        b += c5to8((w >> 10) & 31);
    }
    let n = words.len() as u32;
    [(r / n) as u8, (g / n) as u8, (b / n) as u8]
}

/// Extract every held-section record's weapon as per-channel flat-shaded
/// geometry. `char_slot` keys the committed isolation-rule table (0 Vahn,
/// 1 Noa, 2 Gala). Records whose cut claims nothing (defaults, records
/// identical to bare) simply get no entry.
pub(crate) fn weapon_fusions(player_file: &[u8], char_slot: usize) -> Result<WeaponFusion> {
    let pack = crate::battle_data_pack::parse(player_file).context("parse player file")?;
    let mut fusion = WeaponFusion::default();

    // The bare assembly + its VRAM, shared across every record's diff.
    let bare_ids = [0u8; SECTION_COUNT];
    let mut bare = bca::assemble_character(player_file, &pack, &bare_ids)?;
    bca::relocate_tsb_cba(&mut bare.tmd, BAND)?;
    let bare_tmd = legaia_tmd::parse(&bare.tmd).context("bare TMD")?;
    let bare_vram = vram_for(player_file, &pack, &bare_ids)?;

    // Walk the descriptor chain with the same section tracking the record
    // rewrite uses (`id == 0` closes a section).
    let mut section = 0usize;
    for rec in &pack.records {
        let id = rec.id;
        if id == 0 {
            section += 1;
            continue;
        }
        if HELD_SECTIONS.contains(&section) && id > RA_SERU_MAX_ID {
            let mut equipped = [0u8; SECTION_COUNT];
            equipped[section] = id as u8;
            let Ok(mut asm) = bca::assemble_character(player_file, &pack, &equipped) else {
                continue;
            };
            if asm.sections[section].id != id {
                continue;
            }
            bca::relocate_tsb_cba(&mut asm.tmd, BAND)?;
            let tmd = legaia_tmd::parse(&asm.tmd).context("equipped TMD")?;
            let Some(partition) = equip_item::item_partition(section, &bare, &bare_tmd, &asm, &tmd)
            else {
                continue;
            };
            let vram = vram_for(player_file, &pack, &equipped)?;
            let iso = equip_isolate::isolate_item(
                &equip_isolate::IsolationInputs {
                    section,
                    bare: &bare,
                    bare_tmd: &bare_tmd,
                    bare_vram: &bare_vram,
                    equipped: &asm,
                    equipped_tmd: &tmd,
                    vram: &vram,
                    partition: &partition,
                },
                equip_isolate::rules().rule_for(char_slot, id),
            );
            let mut per_channel: BTreeMap<u8, ModelObject> = BTreeMap::new();
            for &oi in &iso.objects {
                // Skeleton objects only: the weapon welded into the drawn
                // bone. Surplus copies (tags 100+/200+) stay dropped - the
                // rewrite's variant aliasing already covers the 0xFF slot,
                // and fusing an extra's copy would double the blade.
                if asm.bone_tags.get(oi).copied().unwrap_or(255) >= 100 {
                    continue;
                }
                let channel = asm.anm_bones[oi];
                let obj = &tmd.objects[oi];
                let mut prims: Vec<(Vec<u16>, [u8; 3])> = Vec::new();
                for pr in equip_isolate::object_prim_refs(&tmd, &asm.tmd, oi) {
                    if !iso.claims(oi, pr.ordinal) {
                        continue;
                    }
                    let colour = mean_colour(&equip_isolate::prim_texels(&pr, &vram));
                    prims.push((pr.corners.iter().map(|&c| c as u16).collect(), colour));
                }
                if prims.is_empty() {
                    continue;
                }
                // Compact the claimed corners into a fresh vertex list.
                let mut remap: BTreeMap<u16, u16> = BTreeMap::new();
                let mut vertices: Vec<[i16; 3]> = Vec::new();
                let mut tris = ModelGroup {
                    shape: PacketShape::F3,
                    semi_transparent: false,
                    prims: Vec::new(),
                };
                let mut quads = ModelGroup {
                    shape: PacketShape::F4,
                    semi_transparent: false,
                    prims: Vec::new(),
                };
                for (corners, colour) in prims {
                    let mapped: Vec<u16> = corners
                        .iter()
                        .map(|&c| {
                            *remap.entry(c).or_insert_with(|| {
                                let v = obj.vertices[c as usize];
                                vertices.push([v.x, v.y, v.z]);
                                (vertices.len() - 1) as u16
                            })
                        })
                        .collect();
                    let prim = ModelPrim {
                        vertices: mapped,
                        uvs: Vec::new(),
                        cba: 0,
                        tsb: 0,
                        colors: vec![colour],
                    };
                    match prim.vertices.len() {
                        3 => tris.prims.push(prim),
                        4 => quads.prims.push(prim),
                        _ => {}
                    }
                }
                let groups: Vec<ModelGroup> = [tris, quads]
                    .into_iter()
                    .filter(|g| !g.prims.is_empty())
                    .collect();
                if groups.is_empty() {
                    continue;
                }
                per_channel.insert(
                    channel,
                    ModelObject {
                        vertices,
                        groups,
                        scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
                    },
                );
            }
            if !per_channel.is_empty() {
                fusion.per_record.insert((section, id), per_channel);
            }
        }
    }
    Ok(fusion)
}

/// Append `add`'s geometry to `dst` (indices rebased).
pub(crate) fn merge_into(dst: &mut ModelObject, add: &ModelObject) {
    let base = dst.vertices.len() as u16;
    dst.vertices.extend_from_slice(&add.vertices);
    for g in &add.groups {
        let mut g = g.clone();
        for p in &mut g.prims {
            for v in &mut p.vertices {
                *v += base;
            }
        }
        dst.groups.push(g);
    }
}

/// Assemble a loadout's VRAM band (texture pools + CLUT rows), the same
/// recipe the equipment viewer uses.
fn vram_for(
    player_file: &[u8],
    pack: &crate::battle_data_pack::BattleDataPack,
    equipped: &[u8; SECTION_COUNT],
) -> Result<Vram> {
    let mut vram = Vram::new();
    for u in &bca::character_texture_uploads(player_file, pack, equipped, BAND)? {
        vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
        if !u.clut.is_empty() {
            vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
        }
    }
    Ok(vram)
}
