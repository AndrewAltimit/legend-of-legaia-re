//! Fuse the host character's own equipped **weapon** into the swapped hand,
//! keeping its **real texture**.
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
//! dropped, the swap's variant aliasing covers those), and hands the
//! per-channel geometry to the record rewrite to merge into the baked
//! sibling part. Coordinates copy verbatim: the claimed primitives are
//! already in the attach bone's local frame, the same frame the baked fist
//! was seated into, so the weapon rides the host's clips exactly as retail
//! posed it (the pose-exact merge measured on the enemy-side swap).
//!
//! **Texturing** rides three measured facts (`weapon_tex_census`): every
//! fusable weapon's UVs stay inside its own section's band tile, every
//! weapon uses at most [`WEAPON_PALETTE_MAX`] distinct CLUT columns (the
//! apparent 4-way splits are ABR variants of one palette), and a
//! held-section record's pool upload is a fixed-size tile no matter what
//! it contains. So the record rewrite keeps the weapon record's **retail
//! tile pixels verbatim** (each record repaints the tile with its own
//! weapon's texels at exactly the UVs its prims already carry -
//! per-record texture at zero extra pixel cost), the band relayout keeps
//! the sibling's islands *out* of that one tile, and the weapon's
//! palettes ride the record's own CLUT run at reserved columns. The fused
//! prims are the retail prims verbatim - shape, UVs, colour words, ABR
//! bits - with only the CBA column remapped onto the reserved columns.
//!
//! Ra-Seru records (item-table ids `0x01..=0x1A`, the three Ra-Serus'
//! level forms - the living arm the section-3/section-2 sibling slot
//! carries) are left alone here: dressing the arm is a separate, larger
//! cut than "hold your weapon". The ra-seru-flagged held weapons
//! (`0x1B` Ra-Seru Blade, `0x1F` Ra-Seru Fangs, `0x21` Ra-Seru Club)
//! sit past the bound and fuse like any other weapon.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use crate::battle_char_assembly::{self as bca, SECTION_COUNT};
use bca::{equip_isolate, equip_item};
use legaia_tim::Vram;
use legaia_tmd::encode::{ModelGroup, ModelObject, ModelPrim};
use legaia_tmd::legaia_prims;

/// Item-table ids `0x01..=0x1A` are the Ra-Seru level forms (`asset
/// item-tables` on `SCUS_942.54`): Meta `$1..$9` = `0x01..=0x09`, Terra
/// `$1..$9` = `0x0A..=0x12`, Ozma `$1..` = `0x13..` with `Ozma $7` at
/// `0x19` and the blank-named `0x1A` closing the run - NOT weapons. The
/// first real weapon id is `0x1B` (Ra-Seru Blade, a held living weapon
/// that DOES fuse). An earlier `0x18` bound misread `Ozma $7`/`$8` as
/// weapons.
const RA_SERU_MAX_ID: u32 = 0x1A;

/// The two held-item descriptor sections. Which one is the real weapon
/// varies per character (Vahn / Gala: section 2; Noa: section 3 - her
/// section 2 is Terra), so both are walked and the Ra-Seru id range does
/// the telling.
const HELD_SECTIONS: [usize; 2] = [2, 3];

/// VRAM band slot the extraction assembles into (any slot works - the cut
/// only compares texels against the same slot's bare assembly).
const BAND: u8 = 0;

/// Most distinct CLUT columns any fusable weapon samples (census over all
/// three player files' weapon records: every weapon uses 1 or 2).
pub(crate) const WEAPON_PALETTE_MAX: usize = 2;

/// Authoring CLUT row of the player band (the loader's CBA relocation
/// retargets it to `0x1E1 + slot` - see `relocate_tsb_cba`).
const AUTHORING_CLUT_ROW: u16 = 480;

/// Per-record weapon data to merge at record-rewrite time.
#[derive(Default)]
pub(crate) struct WeaponFusion {
    /// `(section, record id)` -> channel -> textured weapon geometry in
    /// that channel's local frame (retail prims, CBA column remapped).
    pub per_record: BTreeMap<(usize, u32), BTreeMap<u8, ModelObject>>,
    /// `(section, record id)` -> the weapon's palettes, index-aligned
    /// with the reserved columns handed to [`weapon_fusions`]. Disc form
    /// (STP bit stripped - the loader re-applies it on upload).
    pub palettes: BTreeMap<(usize, u32), Vec<[u16; 16]>>,
    /// The held section whose records are real weapons for this slot
    /// (the other held section is the Ra-Seru arm). `None` when nothing
    /// fused.
    pub weapon_section: Option<usize>,
}

/// Extract every held-section record's weapon as per-channel textured
/// geometry plus its palettes. `char_slot` keys the committed
/// isolation-rule table (0 Vahn, 1 Noa, 2 Gala); `weapon_cols` are the
/// reserved band CLUT columns the prims are remapped onto (the record
/// rewrite uploads each record's palettes there). Records whose cut
/// claims nothing (defaults, records identical to bare) simply get no
/// entry.
pub(crate) fn weapon_fusions(
    player_file: &[u8],
    char_slot: usize,
    weapon_cols: &[u16],
) -> Result<WeaponFusion> {
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
            // Two frames of one assembly: the isolation compares texels
            // in the RELOCATED frame (band VRAM); the harvested prims
            // keep their AUTHORING words (the frame a stored record
            // uses). Ordinals are identical - relocation rewrites CBA /
            // TSB words in place without touching structure.
            let authoring_tmd_bytes = asm.tmd.clone();
            bca::relocate_tsb_cba(&mut asm.tmd, BAND)?;
            let tmd = legaia_tmd::parse(&asm.tmd).context("equipped TMD")?;
            let authoring_tmd =
                legaia_tmd::parse(&authoring_tmd_bytes).context("equipped TMD (authoring)")?;
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
            // Distinct source columns among claimed textured prims.
            let mut src_cols: Vec<u16> = Vec::new();
            for &oi in &iso.objects {
                if asm.bone_tags.get(oi).copied().unwrap_or(255) >= 100 {
                    continue;
                }
                for pr in equip_isolate::object_prim_refs(&authoring_tmd, &authoring_tmd_bytes, oi)
                {
                    if !iso.claims(oi, pr.ordinal) || pr.uvs.is_empty() {
                        continue;
                    }
                    if !src_cols.contains(&pr.column) {
                        src_cols.push(pr.column);
                    }
                }
            }
            if src_cols.is_empty() {
                continue;
            }
            if src_cols.len() > weapon_cols.len() {
                // Outside the measured envelope - leave this record
                // bare-handed rather than mis-palette it.
                continue;
            }
            src_cols.sort_unstable();
            // The weapon's palettes, read off the equipped band CLUT row
            // (whoever installed them - the record's own run or a
            // sibling block). Stored disc-form: STP bit stripped, the
            // loader re-applies it on every non-zero entry.
            let clut_row = 0x1E1usize + BAND as usize;
            let pals: Vec<[u16; 16]> = src_cols
                .iter()
                .map(|&c| {
                    let mut p = [0u16; 16];
                    for (i, e) in p.iter_mut().enumerate() {
                        *e = vram.pixel(c as usize * 16 + i, clut_row) & 0x7FFF;
                    }
                    p
                })
                .collect();
            let col_map: BTreeMap<u16, u16> = src_cols
                .iter()
                .enumerate()
                .map(|(i, &c)| (c, weapon_cols[i]))
                .collect();
            // Harvest the claimed prims verbatim from the authoring
            // frame: shape + semi-transparency from the group header,
            // UVs / TSB / colour words untouched, CBA column remapped.
            let mut per_channel: BTreeMap<u8, ModelObject> = BTreeMap::new();
            for &oi in &iso.objects {
                if asm.bone_tags.get(oi).copied().unwrap_or(255) >= 100 {
                    continue;
                }
                let channel = asm.anm_bones[oi];
                let obj = &authoring_tmd.objects[oi];
                let mut groups: BTreeMap<(u8, bool), ModelGroup> = BTreeMap::new();
                let mut remap: BTreeMap<usize, u16> = BTreeMap::new();
                let mut vertices: Vec<[i16; 3]> = Vec::new();
                let mut ordinal = 0u32;
                for g in legaia_prims::iter_groups_lenient(
                    &authoring_tmd_bytes,
                    obj.primitives_byte_offset,
                    obj.primitives_byte_size,
                ) {
                    let Some((shape, semi)) = legaia_tmd::encode::shape_for_flags(g.header.flags)
                    else {
                        ordinal += g.prims.len() as u32;
                        continue;
                    };
                    for p in &g.prims {
                        let this = ordinal;
                        ordinal += 1;
                        if !iso.claims(oi, this) || p.uvs.is_empty() {
                            continue;
                        }
                        let corners = p.vertex_indices();
                        if corners.len() != shape.n_vertices() {
                            continue;
                        }
                        let mapped: Vec<u16> = corners
                            .iter()
                            .map(|&c| {
                                *remap.entry(c as usize).or_insert_with(|| {
                                    let v = obj.vertices[c as usize];
                                    vertices.push([v.x, v.y, v.z]);
                                    (vertices.len() - 1) as u16
                                })
                            })
                            .collect();
                        let new_col = col_map[&(p.cba & 0x3F)];
                        let tex_shape = shape.textured_variant();
                        let colors = if tex_shape.is_gouraud() {
                            p.colors.clone()
                        } else {
                            vec![*p.colors.first().unwrap_or(&[128, 128, 128])]
                        };
                        let e = groups.entry((tex_shape as u8, semi)).or_insert(ModelGroup {
                            shape: tex_shape,
                            semi_transparent: semi,
                            prims: Vec::new(),
                        });
                        e.prims.push(ModelPrim {
                            vertices: mapped,
                            uvs: p.uvs.clone(),
                            cba: (AUTHORING_CLUT_ROW << 6) | new_col,
                            tsb: p.tsb,
                            colors,
                        });
                    }
                }
                let groups: Vec<ModelGroup> = groups.into_values().collect();
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
                match fusion.weapon_section {
                    None => fusion.weapon_section = Some(section),
                    Some(s) if s != section => {
                        bail!(
                            "fusable weapon records in both held sections ({s} and {section}) - \
                             the one-weapon-tile texturing scheme cannot host that"
                        );
                    }
                    Some(_) => {}
                }
                fusion.per_record.insert((section, id), per_channel);
                fusion.palettes.insert((section, id), pals);
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
