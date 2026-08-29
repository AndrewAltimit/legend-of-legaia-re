//! Cross-character weapon transplant: give a character's player battle file
//! a section record for a weapon only another character's file carries, so
//! the weapon shows in their hand once the SCUS owner mask lets them equip
//! it.
//!
//! The retail loader (`FUN_80052770` case 4) picks each equipment section
//! by the equipped item id and falls through to the section's `id = 0`
//! default when nothing matches - which is why an owner-mask edit alone
//! leaves the new owner swinging bare-handed: their file has no record for
//! the id, and the default record is the bare hand. A held-item record is
//! the whole arm (three bone objects: upper arm, forearm, hand) re-authored
//! with the weapon welded in, plus the section's texture tile and CLUT run
//! ([`battle-data-pack.md`](../../../docs/formats/battle-data-pack.md)), so
//! a transplant is built, not copied:
//!
//! 1. the weapon's own primitives are cut out of the **donor** record with
//!    the same curated item-alone cut the equipment viewer and the Delilas
//!    swap use ([`party_swap::weapon_fuse`]): retail prims verbatim, CBA
//!    columns remapped onto the target section's CLUT columns;
//! 2. they are merged into the **target's** own weapon-section default
//!    record (its bare arm, its swing records, its attach list) channel by
//!    channel - donor bone `k` of the section maps to target bone `k`, the
//!    sections being the same three arm bones in the same order on every
//!    file. Coordinates do **not** copy verbatim: the three skeletons'
//!    arm-bone frames differ (the same Short Sword runs along `-Y` in
//!    Vahn's hand frame, `-Z` in Noa's), so each channel's geometry is
//!    re-seated through the rigid transform [`crate::equip_hand_frame`]
//!    calibrates from the weapons both files carry - of the transplanted
//!    weapon's own class, since a club and a blade are gripped differently;
//! 3. the donor's section tile rides along as the new record's pool,
//!    texels outside the weapon's UV box blanked, with the weapon's
//!    palettes installed on the target section's columns. Sections 2 and 3
//!    share a texture page and differ only by tile row, so a cross-section
//!    transplant shifts the fused prims' `v` by the tile offset;
//! 4. the record's arm cost (`swing + 0x74`) is the donor weapon's, so the
//!    weapon keeps its retail price on the new owner's Arts bar.
//!
//! The rebuilt record is appended to the target's weapon section and the
//! file re-packed through [`party_swap::playerize::rebuild_player_file`];
//! whether it fits the PROT entry is the caller's problem (the retail
//! player files tile their footprints exactly, so a transplant usually
//! needs room from a neighbouring entry).

use anyhow::{Context, Result, bail};
use legaia_tmd::encode::{ModelObject, decode_model, encode};

use crate::battle_char_assembly::{SECTION_COUNT, SECTION_TEXTURE_RECTS};
use crate::battle_data_pack::{self, BattleDataPack};
use crate::party_swap::playerize::{
    alias_variant_onto_bone, rebuild_player_file, splice_record_tmd, variant_object,
};
use crate::party_swap::weapon_fuse::{
    BareFrame, WEAPON_PALETTE_MAX, merge_into, weapon_fusion_record,
};

/// The two held-item sections of a player file.
const HELD_SECTIONS: [usize; 2] = [2, 3];
/// Item ids up to here are Ra-Seru level forms (the living arm), not weapons.
pub const RA_SERU_MAX_ID: u32 = 0x1A;
/// Arm cost byte inside a swing record.
const SWING_COST_OFF: usize = 0x74;

/// Per-record section index of a descriptor chain: the `id = 0` entries
/// close sections, so an entry's section is the number of terminators
/// before it.
pub fn record_sections(pack: &BattleDataPack) -> Vec<usize> {
    let mut out = Vec::with_capacity(pack.records.len());
    let mut sec = 0usize;
    for r in &pack.records {
        out.push(sec);
        if r.id == 0 {
            sec += 1;
        }
    }
    out
}

/// The held section that carries real weapons (ids past the Ra-Seru
/// range): 2 for Vahn and Gala, 3 for Noa. `None` when neither does.
pub fn weapon_section(pack: &BattleDataPack) -> Option<usize> {
    let secs = record_sections(pack);
    pack.records
        .iter()
        .zip(&secs)
        .find(|(r, s)| HELD_SECTIONS.contains(s) && r.id > RA_SERU_MAX_ID)
        .map(|(_, s)| *s)
}

/// Descriptor index + section of the weapon record for `id` in a held
/// section, if the file carries one.
pub fn find_weapon_record(pack: &BattleDataPack, id: u32) -> Option<(usize, usize)> {
    let secs = record_sections(pack);
    pack.records
        .iter()
        .zip(&secs)
        .find(|(r, s)| r.id == id && HELD_SECTIONS.contains(s))
        .map(|(r, s)| (r.index, *s))
}

/// The bones a held section attaches, in channel order - read off the
/// section's `id = 0` default record's loader frame.
pub fn section_bones(file: &[u8], pack: &BattleDataPack, section: usize) -> Result<Vec<u8>> {
    let secs = record_sections(pack);
    let Some(def) = pack
        .records
        .iter()
        .zip(&secs)
        .find(|(r, s)| **s == section && r.id == 0)
        .map(|(r, _)| r.index)
    else {
        bail!("section {section} has no default record");
    };
    let dec = battle_data_pack::decode_record(file, pack, def)?.bytes;
    Ok(frame_bones(&dec)?.1)
}

/// A file's records as `(id, decoded bytes)` in chain order - what
/// [`rebuild_player_file`] repacks.
pub type RecordList = Vec<(u32, Vec<u8>)>;

/// Decode every record of a file into the `(id, decoded)` list
/// [`rebuild_player_file`] repacks.
pub fn file_records(buf: &[u8], pack: &BattleDataPack) -> Result<Vec<(u32, Vec<u8>)>> {
    let mut out = Vec::with_capacity(pack.records.len());
    for r in &pack.records {
        let d = battle_data_pack::decode_record(buf, pack, r.index)
            .with_context(|| format!("decode record {} (id {:#x})", r.index, r.id))?;
        out.push((r.id, d.bytes));
    }
    Ok(out)
}

/// Insert `(id, decoded)` into `section` of a record list (before the
/// section's `id = 0` terminator). An existing record with that id in the
/// section is replaced instead.
pub fn insert_record(
    records: &mut Vec<(u32, Vec<u8>)>,
    section: usize,
    id: u32,
    decoded: Vec<u8>,
) -> Result<()> {
    let mut sec = 0usize;
    for i in 0..records.len() {
        if sec == section && records[i].0 == id {
            records[i].1 = decoded;
            return Ok(());
        }
        if records[i].0 == 0 {
            if sec == section {
                records.insert(i, (id, decoded));
                return Ok(());
            }
            sec += 1;
        }
    }
    bail!("record list has no section {section}")
}

/// Bytes the packed record list needs from the data base, `0x800`-aligned
/// per record, with the optimal LZS parse - the smallest footprint a
/// rebuild can reach. Expensive (an optimal parse per record).
pub fn packed_len(records: &[(u32, Vec<u8>)]) -> usize {
    records
        .iter()
        .map(|(_, d)| (4 + legaia_lzs::compress_optimal(d).len() + 0x7FF) & !0x7FF)
        .sum()
}

/// One transplanted weapon record. `hand_frame` is the calibration that
/// re-seated the weapon in the new owner's arm frames
/// ([`crate::equip_hand_frame`]); `dropped_channels` are donor bones whose
/// geometry had no calibration and was left out rather than guessed.
#[derive(Debug, Clone)]
pub struct Transplant {
    /// The weapon's item id.
    pub item_id: u32,
    /// The target file's weapon section the record belongs in.
    pub section: usize,
    /// The rebuilt, decoded record.
    pub decoded: Vec<u8>,
    /// The arm cost the record carries (the donor weapon's).
    pub cost: u8,
    /// Prims the cut claimed as the weapon.
    pub weapon_prims: usize,
    /// The hand and its per-clip variant share one copy of the geometry.
    pub aliased: bool,
    /// Per donor bone the weapon occupied: the calibrated re-seat (degrees
    /// of rotation, residual in GTE units) that was applied.
    pub reseated: Vec<(u8, f64, f64)>,
    /// Donor bones whose weapon geometry had no calibration and was dropped.
    pub dropped_channels: Vec<u8>,
}

/// Loader-frame attach list (bone ids) of a decoded record.
fn frame_bones(decoded: &[u8]) -> Result<(usize, Vec<u8>)> {
    let frame = u32_at(decoded, 0)? as usize;
    let n = *decoded
        .get(frame)
        .ok_or_else(|| anyhow::anyhow!("loader frame past record end"))? as usize;
    let bones = decoded
        .get(frame + 1..frame + 1 + n)
        .ok_or_else(|| anyhow::anyhow!("attach list past record end"))?
        .to_vec();
    Ok((frame, bones))
}

fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    legaia_bytes::u32_le(b, o).ok_or_else(|| anyhow::anyhow!("short record at +{o:#x}"))
}

/// A record's pool, split: `(clut_x, CLUT run, pixel bytes)`.
type PoolParts<'a> = (u16, Vec<u16>, &'a [u8]);

/// The `[clut_x][clut_n][cluts][pixels]` pool of a decoded record, split.
fn pool_of(decoded: &[u8]) -> Result<Option<PoolParts<'_>>> {
    let body_end = u32_at(decoded, 0xC)? as usize;
    let flag = u16::from_le_bytes([decoded[0x12], decoded[0x13]]);
    if flag == 0 || body_end + 4 > decoded.len() {
        return Ok(None);
    }
    let clut_x = u16::from_le_bytes([decoded[body_end], decoded[body_end + 1]]);
    let clut_n = u16::from_le_bytes([decoded[body_end + 2], decoded[body_end + 3]]) as usize;
    let run_end = body_end + 4 + clut_n * 2;
    if run_end > decoded.len() {
        bail!("pool CLUT run past record end");
    }
    let cluts: Vec<u16> = decoded[body_end + 4..run_end]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok(Some((clut_x, cluts, &decoded[run_end..])))
}

/// The CLUT columns a file's weapon section installs its palettes on: the
/// run every weapon record of the section uploads (`clut_x / 16`, one
/// column per 16 entries), capped at [`WEAPON_PALETTE_MAX`].
pub fn section_clut_cols(buf: &[u8], pack: &BattleDataPack, section: usize) -> Result<Vec<u16>> {
    let secs = record_sections(pack);
    for (r, s) in pack.records.iter().zip(&secs) {
        if *s != section || r.id == 0 {
            continue;
        }
        let d = battle_data_pack::decode_record(buf, pack, r.index)?;
        if let Some((clut_x, cluts, _)) = pool_of(&d.bytes)? {
            let first = clut_x / 16;
            let n = (cluts.len() as u16)
                .div_ceil(16)
                .min(WEAPON_PALETTE_MAX as u16);
            if n > 0 {
                return Ok((first..first + n).collect());
            }
        }
    }
    bail!("section {section} has no record with a CLUT run")
}

/// Build the target's record for `item_id` from the donor file's own record
/// of it. `source_slot` / `target_slot` are the party-band slots (0 Vahn,
/// 1 Noa, 2 Gala), which key the committed isolation-rule table and the
/// hand-frame calibration.
pub fn transplant_weapon(
    target_file: &[u8],
    source_file: &[u8],
    source_slot: usize,
    target_slot: usize,
    item_id: u32,
) -> Result<Transplant> {
    let sp = battle_data_pack::parse(source_file).context("parse donor player file")?;
    let tp = battle_data_pack::parse(target_file).context("parse target player file")?;
    let Some((sidx, ssec)) = find_weapon_record(&sp, item_id) else {
        bail!("donor file carries no held-item record for {item_id:#04x}");
    };
    let Some(tsec) = weapon_section(&tp) else {
        bail!("target file has no weapon section");
    };
    let tsecs = record_sections(&tp);
    let Some(tdef) = tp
        .records
        .iter()
        .zip(&tsecs)
        .find(|(r, s)| **s == tsec && r.id == 0)
        .map(|(r, _)| r.index)
    else {
        bail!("target weapon section has no default record");
    };
    if ssec >= SECTION_COUNT || tsec >= SECTION_COUNT {
        bail!("held section out of range");
    }
    let (srect, trect) = (SECTION_TEXTURE_RECTS[ssec], SECTION_TEXTURE_RECTS[tsec]);
    if srect.x0 != trect.x0 || srect.w != trect.w || srect.h != trect.h {
        bail!("donor section {ssec} and target section {tsec} tiles differ in shape");
    }
    let dv = trect.y0 as i32 - srect.y0 as i32;

    // The weapon cut, palettes remapped onto the target section's columns.
    let cols = section_clut_cols(target_file, &tp, tsec)?;
    let bare = BareFrame::new(source_file, &sp).context("donor bare assembly")?;
    let Some((per_channel, pals)) =
        weapon_fusion_record(source_file, &sp, &bare, source_slot, ssec, item_id, &cols)?
    else {
        bail!(
            "the item-alone cut claims nothing of {item_id:#04x} in the donor file (or it needs more than {} palettes)",
            cols.len()
        );
    };

    let src = battle_data_pack::decode_record(source_file, &sp, sidx)?.bytes;
    let def = battle_data_pack::decode_record(target_file, &tp, tdef)?.bytes;
    let (_, sbones) = frame_bones(&src)?;
    let (tframe, tbones) = frame_bones(&def)?;
    if sbones.len() != tbones.len() {
        bail!(
            "donor section attaches {} bones, target section {} - not the same arm",
            sbones.len(),
            tbones.len()
        );
    }
    let tmd_off = tframe + 0xC;
    let tmd = legaia_tmd::parse(&def[tmd_off..]).context("target default TMD")?;
    let mut objects = decode_model(&tmd, &def[tmd_off..]).context("target default model")?;
    let attach_count = tbones.len();
    // The section's `0xFF` variant (the hand's per-clip swap) - on every
    // retail WEAPON record of all three files it is a byte copy of the
    // armed hand (only the bare defaults ship a differently posed / shaded
    // variant), so the transplant follows the weapon records: one copy of
    // the armed hand, the variant's object-table entry aliased onto it.
    let variant = variant_object(attach_count, objects.len());
    let alias = variant.is_some();

    // The donor's arm frames are not the target's: re-seat the weapon
    // through the calibration the shared weapons give (`equip_hand_frame`).
    let hand_frame = crate::equip_hand_frame::fit_for(
        source_file,
        &sp,
        source_slot,
        target_file,
        &tp,
        target_slot,
        item_id,
    )
    .context("hand-frame calibration")?;
    let mut reseated = Vec::new();
    let mut dropped_channels = Vec::new();

    // Merge channel by channel; UV rows shifted onto the target tile;
    // track each prim's UV box in donor-tile space for the pool blank.
    let mut boxes: Vec<(u8, u8, u8, u8)> = Vec::new();
    let mut weapon_prims = 0usize;
    for (sbone, geom) in &per_channel {
        let Some(k) = sbones.iter().position(|b| b == sbone) else {
            bail!("donor channel {sbone} is not one of the section's bones");
        };
        let mut g = geom.clone();
        match hand_frame.channels.get(k).and_then(|c| c.as_ref()) {
            Some(cf) => {
                cf.xf.apply_object(&mut g);
                reseated.push((*sbone, cf.xf.angle_deg(), cf.rms));
            }
            None => {
                dropped_channels.push(*sbone);
                continue;
            }
        }
        for grp in &mut g.groups {
            for p in &mut grp.prims {
                weapon_prims += 1;
                let mut bbox = (u8::MAX, u8::MAX, 0u8, 0u8);
                for uv in &p.uvs {
                    bbox.0 = bbox.0.min(uv.0);
                    bbox.1 = bbox.1.min(uv.1);
                    bbox.2 = bbox.2.max(uv.0);
                    bbox.3 = bbox.3.max(uv.1);
                }
                boxes.push(bbox);
                for uv in &mut p.uvs {
                    let v = uv.1 as i32 + dv;
                    if !(0..=255).contains(&v) {
                        bail!(
                            "weapon texel row {} leaves the page after the tile shift",
                            uv.1
                        );
                    }
                    uv.1 = v as u8;
                }
            }
        }
        merge_into(&mut objects[k], &g);
    }
    if let Some(v) = variant {
        // Geometry lives in the variant's slot; the bone's entry is aliased
        // onto it after encoding (`alias_variant_onto_bone` reads the TMD
        // length off the last object, so the alias points forward).
        objects[v] = objects[attach_count - 1].clone();
        objects[attach_count - 1] = ModelObject {
            vertices: Vec::new(),
            groups: Vec::new(),
            scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
        };
    }
    let mut new_tmd = encode(&objects).context("encode transplanted TMD")?;
    if let Some(v) = variant {
        alias_variant_onto_bone(&mut new_tmd, v)?;
    }

    // Pool: the donor tile, blanked outside the weapon's UV box, under the
    // weapon palettes on the target section's columns.
    let Some((_, _, spixels)) = pool_of(&src)? else {
        bail!("donor record uploads no texture tile");
    };
    let row_bytes = srect.w as usize * 2;
    let tile_len = row_bytes * srect.h as usize;
    if spixels.len() < tile_len {
        bail!(
            "donor tile is {} bytes, the section rect needs {tile_len}",
            spixels.len()
        );
    }
    // Keep only the texels some weapon prim samples (the union of the
    // prims' UV boxes, one texel of slack each way for bilinear-free PSX
    // sampling); everything else - the donor's fist skin, unused page -
    // zeroes, which the LZS parse folds down to almost nothing.
    let mut keep = vec![false; tile_len * 2];
    let tile_w = row_bytes * 2;
    for &(u0, v0, u1, v1) in &boxes {
        for row in v0.saturating_sub(1) as usize..=(v1 as usize + 1).min(srect.h as usize - 1) {
            for u in u0.saturating_sub(1) as usize..=(u1 as usize + 1).min(tile_w - 1) {
                keep[row * tile_w + u] = true;
            }
        }
    }
    let mut pixels = spixels[..tile_len].to_vec();
    for (byte, px) in pixels.iter_mut().enumerate() {
        // 4bpp: two texels per byte, low nibble first.
        let (a, b) = (keep[byte * 2], keep[byte * 2 + 1]);
        *px &= (if a { 0x0F } else { 0 }) | (if b { 0xF0 } else { 0 });
    }
    let lo = cols[0];
    let hi = *cols.last().unwrap();
    let mut block = Vec::with_capacity(4 + tile_len + 64);
    block.extend_from_slice(&(lo * 16).to_le_bytes());
    block.extend_from_slice(&((hi - lo + 1) * 16).to_le_bytes());
    let mut run = vec![0u16; ((hi - lo + 1) * 16) as usize];
    for (i, pal) in pals.iter().enumerate() {
        let base = ((cols[i] - lo) * 16) as usize;
        run[base..base + 16].copy_from_slice(pal);
    }
    for w in run {
        block.extend_from_slice(&w.to_le_bytes());
    }
    block.extend_from_slice(&pixels);

    let mut decoded = splice_record_tmd(&def, &new_tmd, &block)?;

    // The donor weapon's arm cost.
    let scost = u32_at(&src, 4)? as usize + SWING_COST_OFF;
    let cost = *src
        .get(scost)
        .ok_or_else(|| anyhow::anyhow!("donor swing record past record end"))?;
    let tcost = u32_at(&decoded, 4)? as usize + SWING_COST_OFF;
    if tcost >= decoded.len() {
        bail!("target swing record past record end");
    }
    decoded[tcost] = cost;

    Ok(Transplant {
        item_id,
        section: tsec,
        decoded,
        cost,
        weapon_prims,
        aliased: alias,
        reseated,
        dropped_channels,
    })
}

/// Rebuild `target_file` with `transplants` added to their sections, into
/// an entry of `entry_len` bytes. Errors when the records do not fit.
pub fn rebuild_with_transplants(
    target_file: &[u8],
    transplants: &[Transplant],
    entry_len: usize,
) -> Result<Vec<u8>> {
    let pack = battle_data_pack::parse(target_file).context("parse target player file")?;
    let mut records = file_records(target_file, &pack)?;
    for t in transplants {
        insert_record(&mut records, t.section, t.item_id, t.decoded.clone())?;
    }
    rebuild_player_file(
        target_file,
        pack.table_offset,
        pack.data_base,
        records,
        entry_len,
    )
}

/// The record list of `target_file` with `transplants` added - the input
/// [`packed_len`] sizes.
pub fn records_with_transplants(
    target_file: &[u8],
    transplants: &[Transplant],
) -> Result<(BattleDataPack, RecordList)> {
    let pack = battle_data_pack::parse(target_file).context("parse target player file")?;
    let mut records = file_records(target_file, &pack)?;
    for t in transplants {
        insert_record(&mut records, t.section, t.item_id, t.decoded.clone())?;
    }
    Ok((pack, records))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_record_lands_before_the_section_terminator() {
        let mut recs = vec![
            (0x51, vec![1]),
            (0, vec![2]),
            (0x3E, vec![3]),
            (0, vec![4]),
            (0x22, vec![5]),
            (0, vec![6]),
        ];
        insert_record(&mut recs, 2, 0xBA, vec![9]).unwrap();
        let ids: Vec<u32> = recs.iter().map(|r| r.0).collect();
        assert_eq!(ids, [0x51, 0, 0x3E, 0, 0x22, 0xBA, 0]);
        // Same id again replaces in place.
        insert_record(&mut recs, 2, 0xBA, vec![7]).unwrap();
        assert_eq!(recs[5], (0xBA, vec![7]));
        assert!(insert_record(&mut recs, 5, 1, vec![]).is_err());
    }

    #[test]
    fn empty_pack_has_no_weapon_section() {
        let pack = BattleDataPack {
            table_offset: 0,
            records: Vec::new(),
            data_base: 0,
        };
        assert_eq!(weapon_section(&pack), None);
        assert!(record_sections(&pack).is_empty());
    }
}
