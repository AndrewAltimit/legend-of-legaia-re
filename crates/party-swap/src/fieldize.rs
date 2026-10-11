//! Field-side of the party swap: rebuild PROT 0874 so the party's
//! walking-around (field) models depict the Delilas siblings.
//!
//! The preferred source ([`fieldize_pack_npc`]) is the siblings' **own
//! field NPC meshes** from the nilboa duel scene ([`NPC_PACK_ENTRY`] /
//! [`NPC_BUNDLE_ENTRY`]): retail-authored 10-part chibi rigs (duel
//! costume, matching the battle forms) whose geometry fits the §0
//! budget at full detail. The battle-monster conversion
//! ([`fieldize_pack`]) survives as a fallback; its high-entropy baked
//! geometry only fits through the decimation ladder.
//!
//! The party field rig is a 12-group TMD per character (10 posed bones
//! plus 2 unposed equipment-template groups), driven by the shared
//! locomotion ANM bundle whose records decode to the same flat per-part
//! pose model as the battle streams - so a rest-pose bake retargets
//! either source: each source part bakes into the party bone's local
//! frame anchored on the retail part's rest **bbox** with a per-axis
//! span (the clips' translations dictate joint spacing, so matching
//! retail extents is what keeps limb chains connected under every
//! clip); the head keeps its proportions under a uniform fit.
//!
//! Texture: the field atlas gives each character an 80x128-texel window
//! of texpage 0x1D plus four 16-colour CLUT columns on row 478. Retail
//! field models are mostly flat-shaded (bodies are vertex-coloured;
//! only faces carry texture) and the NPC meshes share that split. The
//! converter keeps the **head** textured (its islands re-lay into the
//! atlas window, palettes union-merged to four) and converts every
//! other textured face to a flat prim whose colour samples the source
//! texture.

use super::*;
use legaia_actor_formats::character_pack;
use legaia_container_formats::pack::extract_pack;
use legaia_container_formats::parse_player_lzs;

mod atlas;
mod decimate;
mod npc_source;
mod roles;
mod slot;

pub(super) use atlas::paint_vram;
use atlas::patch_atlas;
pub(super) use decimate::{decimate_object, flatten_prim};
pub(super) use npc_source::npc_coords;
use npc_source::npc_slot_source;
pub use npc_source::{NPC_BUNDLE_ENTRY, NPC_PACK_ENTRY};
pub(super) use roles::{FIELD_CHILD, FIELD_PARENT, derive_field_roles, group_world_stats};
use slot::{DoubleSideScope, FieldSlot, SlotSource, fieldize_slot, monster_slot_source};

/// PROT entry the field-form container lives in.
pub const PROT_ENTRY_INDEX: usize = 874;

/// Field bones per character (the locomotion clips' channel count).
pub const FIELD_BONES: usize = 10;

/// Groups per field character slot (bones + 2 equipment templates).
pub const FIELD_GROUPS: usize = 12;

/// Field atlas geometry: per-character U window width (texels), page
/// height, CLUT columns per character.
const ATLAS_WINDOW_W: usize = 80;
const ATLAS_WINDOW_H: usize = 128;
const ATLAS_COLS_PER_CHAR: usize = 4;
/// Every field-character prim authors texpage 0x1D, ABR 1.
const FIELD_TSB: u16 = 0x003D;
/// Field CLUT row.
const FIELD_CLUT_ROW: u16 = 478;

/// The rebuilt PROT 0874 entry.
#[derive(Debug, Clone)]
pub struct FieldizedPack {
    pub entry: Vec<u8>,
    pub warnings: Vec<String>,
}

/// Rebuild the PROT 0874 entry: slots 0..2 wear the mapped siblings'
/// models, the locomotion bundle survives verbatim, and each
/// character's atlas TIM entry repaints to the new head window.
/// `mapping[slot]` = the monster id whose model character `slot` wears.
pub fn fieldize_pack(
    prot_0874: &[u8],
    entry_len: usize,
    archive_entry: &[u8],
    mapping: [u16; 3],
) -> Result<FieldizedPack> {
    let sources = mapping
        .iter()
        .map(|&id| monster_slot_source(archive_entry, id))
        .collect::<Result<Vec<_>>>()?;
    fieldize_pack_laddered(prot_0874, entry_len, &sources, mapping)
}

/// Like [`fieldize_pack`], but sourcing each character's model from the
/// sibling's own field NPC mesh (nilboa scene, [`NPC_PACK_ENTRY`] /
/// [`NPC_BUNDLE_ENTRY`]) instead of converting the battle monster model.
/// Retail-authored field geometry fits the §0 budget at full detail, so
/// this is the quality-preserving path; the monster conversion stays the
/// fallback.
pub fn fieldize_pack_npc(
    prot_0874: &[u8],
    entry_len: usize,
    npc_pack: &[u8],
    npc_bundle: &[u8],
    mapping: [u16; 3],
) -> Result<FieldizedPack> {
    let sources = mapping
        .iter()
        .map(|&id| npc_slot_source(npc_pack, npc_bundle, id))
        .collect::<Result<Vec<_>>>()?;
    fieldize_pack_laddered(prot_0874, entry_len, &sources, mapping)
}

fn fieldize_pack_laddered(
    prot_0874: &[u8],
    entry_len: usize,
    sources: &[SlotSource],
    mapping: [u16; 3],
) -> Result<FieldizedPack> {
    // Detail ladder: full detail DOUBLE-SIDED first (the NPC meshes are
    // not watertight from free angles - see `double_side`), then
    // single-sided, then progressively drop prims under a size
    // threshold (invisible at the chibi field scale) until the rebuilt
    // container fits its PROT entry.
    let mut last_err = None;
    use DoubleSideScope as Ds;
    for (decimate, double_sided) in [
        (0.0f32, Ds::All),
        (0.0, Ds::HeadTorso),
        (0.0, Ds::Head),
        (0.0, Ds::None),
        (2.0, Ds::None),
        (3.5, Ds::None),
        (5.0, Ds::None),
        (7.0, Ds::None),
        (9.0, Ds::None),
        (12.0, Ds::None),
    ] {
        match fieldize_pack_at(
            prot_0874,
            entry_len,
            sources,
            mapping,
            decimate,
            double_sided,
        ) {
            Ok(mut out) => {
                match double_sided {
                    Ds::All => {}
                    Ds::HeadTorso => out
                        .warnings
                        .push("field mesh double-sided on head+torso only (container size)".into()),
                    Ds::Head => out
                        .warnings
                        .push("field mesh double-sided on the head only (container size)".into()),
                    Ds::None => out
                        .warnings
                        .push("field mesh single-sided (container size)".into()),
                }
                if decimate > 0.0 {
                    out.warnings
                        .push(format!("field detail reduced (min prim size {decimate})"));
                }
                return Ok(out);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("field rebuild failed")))
        .context("rebuilt PROT 0874 does not fit its entry at any detail level")
}

/// Sibling-native / retail-host posed height ratio for one field slot,
/// measured over the role-mapped parts on both sides (the same stats the
/// bake's own radial uses, inverted). Clamped to a sane figure range.
fn slot_height_ratio(
    pack: &character_pack::CharacterPack,
    retail_anm: &legaia_actor_formats::player_anm::PlayerAnmBundle,
    slot: usize,
    source: &SlotSource,
) -> Result<f32> {
    let cs = pack
        .slot(slot)
        .ok_or_else(|| anyhow::anyhow!("field slot {slot} missing"))?;
    let field_tmd = legaia_tmd::parse(&cs.tmd_bytes).context("field TMD")?;
    let field_model = decode_model(&field_tmd, &cs.tmd_bytes)?;
    let idle_rec =
        character_pack::locomotion_record_index(slot, character_pack::LOCOMOTION_IDLE_SLOT);
    let idle = retail_anm
        .record_to_monster_animation(idle_rec)
        .ok_or_else(|| anyhow::anyhow!("field slot {slot}: idle clip missing"))?;
    let field_rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("field idle has no frames"))?;
    let roles = derive_field_roles(&field_model, field_rest)?;
    let dst_parts: Vec<(&ModelObject, &PartPose)> = roles
        .iter()
        .map(|&b| (&field_model[b], &field_rest[b]))
        .collect();
    let src_parts: Vec<(&ModelObject, &PartPose)> = source
        .role_sources
        .iter()
        .flatten()
        .map(|&c| (&source.model[c], &source.rest[c]))
        .collect();
    let (_, e_dst) = group_world_stats(&dst_parts);
    let (_, e_src) = group_world_stats(&src_parts);
    Ok((e_src[1] / e_dst[1]).clamp(0.5, 2.0))
}

fn fieldize_pack_at(
    prot_0874: &[u8],
    entry_len: usize,
    sources: &[SlotSource],
    mapping: [u16; 3],
    decimate: f32,
    double_sided: DoubleSideScope,
) -> Result<FieldizedPack> {
    let mut warnings = Vec::new();
    let pack = character_pack::parse(prot_0874).context("parse PROT 0874")?;
    let container = parse_player_lzs(prot_0874, character_pack::CONTAINER_DESCRIPTORS)?;

    // Height-preserving locomotion retarget: the shared §1 clips pose
    // each slot at the retail HERO's joint spacing, so a sibling baked
    // onto them walks at the hero's stature (Che loses ~40% of his
    // nilboa height, and the compressed skeleton swallows the head into
    // the chest). The per-(bone, frame) translations are flat
    // actor-space joint positions with the origin on the floor plane,
    // so scaling every translation of a slot's 7-record bank by the
    // sibling/host height ratio rescales the whole walking skeleton in
    // place - same byte size, rotations untouched - and the §0 bake
    // below then anchors on the SCALED rest, keeping the chains closed.
    // The two shared records (21 savepoint / 22 aux) stay retail: they
    // already pose three differently-proportioned heroes in retail, and
    // scaling them for one slot would break the other two.
    let sec1_desc = &container.descriptors[character_pack::LOCOMOTION_SECTION];
    let mut sec1_decoded = legaia_container_formats::decode(
        prot_0874,
        sec1_desc,
        legaia_container_formats::DecodeMode::Lzs,
    )
    .context("LZS-decode PROT 0874 section 1 (party locomotion ANM)")?;
    let retail_anm = legaia_actor_formats::player_anm::parse(&sec1_decoded)
        .context("parse retail locomotion bundle")?;
    let mut sec1_scaled = false;
    for (slot, source) in sources.iter().enumerate().take(3) {
        let r = slot_height_ratio(&pack, &retail_anm, slot, source)
            .with_context(|| format!("field slot {slot} height ratio"))?;
        if (r - 1.0).abs() < 0.02 {
            continue;
        }
        for bank in 0..character_pack::LOCOMOTION_BANK_STRIDE {
            let rec = character_pack::locomotion_record_index(slot, bank);
            legaia_actor_formats::player_anm::scale_record_translations(&mut sec1_decoded, rec, r)
                .with_context(|| format!("scale slot {slot} locomotion record {rec}"))?;
        }
        sec1_scaled = true;
        warnings.push(format!(
            "field slot {slot}: walking skeleton scaled x{r:.2} to the sibling's own height"
        ));
    }
    let anm = if sec1_scaled {
        legaia_actor_formats::player_anm::parse(&sec1_decoded)
            .context("re-parse scaled locomotion bundle")?
    } else {
        retail_anm
    };

    let mut slots: Vec<FieldSlot> = Vec::with_capacity(3);
    for (slot, source) in sources.iter().enumerate() {
        slots.push(
            fieldize_slot(
                &pack,
                &anm,
                slot,
                source,
                decimate,
                double_sided,
                &mut warnings,
            )
            .with_context(|| format!("field slot {slot} <- monster {}", mapping[slot]))?,
        );
    }

    // New §0: the 5-body pack with slots 0..2 replaced. Its DECODED size
    // stays EXACTLY retail's, and the container's first four header words
    // stay byte-exact. `meta[1]` is the sum of the descriptors' decoded
    // sizes (docs/formats/scene-bundles.md), not a pointer: the battle
    // loader's VDF registration walks a DIFFERENT entry (raw TOC 874 =
    // extraction 872, `vdf`), so the old "meta[1] addresses a VDF tail
    // inside this entry" reading is falsified - see
    // docs/formats/character-mesh.md "Not a dual consumer". A rebuild
    // that changed §0's decoded size was still observed to hang the next
    // battle load, with no mechanism yet, so the size stays pinned.
    // Retail's own §0 carries ~19 KB of trailing pack padding, so padding
    // to the exact size is the retail shape.
    let sec0 = &container.descriptors[character_pack::CONTAINER_SECTION];
    let sec0_decoded = legaia_container_formats::decode(
        prot_0874,
        sec0,
        legaia_container_formats::DecodeMode::Lzs,
    )?;
    let bodies = extract_pack(&sec0_decoded)?;
    let mut new_bodies: Vec<Vec<u8>> = Vec::with_capacity(bodies.len());
    for (i, b) in bodies.iter().enumerate() {
        if i < 3 {
            new_bodies.push(slots[i].tmd.clone());
        } else {
            new_bodies.push(b.to_vec());
        }
    }
    let mut sec0_new = Vec::new();
    sec0_new.extend_from_slice(&(new_bodies.len() as u32).to_le_bytes());
    let table_words = 1 + new_bodies.len();
    let mut cursor = table_words;
    let mut offsets = Vec::with_capacity(new_bodies.len());
    for b in &new_bodies {
        offsets.push(cursor as u32);
        cursor += b.len().div_ceil(4);
    }
    for off in &offsets {
        sec0_new.extend_from_slice(&off.to_le_bytes());
    }
    for b in &new_bodies {
        sec0_new.extend_from_slice(b);
        while sec0_new.len() % 4 != 0 {
            sec0_new.push(0);
        }
    }
    if sec0_new.len() > sec0_decoded.len() {
        bail!(
            "rebuilt §0 pack decodes to {} bytes, retail's {} is a hard cap \
             (the container header words must stay byte-exact)",
            sec0_new.len(),
            sec0_decoded.len()
        );
    }
    sec0_new.resize(sec0_decoded.len(), 0);

    // New §2: the atlas with each character entry's pixels + CLUT
    // repainted in place (same TIM sizes).
    let sec2 = &container.descriptors[2];
    let mut sec2_new = legaia_container_formats::decode(
        prot_0874,
        sec2,
        legaia_container_formats::DecodeMode::Lzs,
    )?;
    patch_atlas(&mut sec2_new, &slots)?;

    // §1 (locomotion): the original compressed byte span when untouched,
    // a re-compress of the scaled bundle otherwise.
    let sec1 = &container.descriptors[1];
    let spans: Vec<(usize, usize)> = {
        let mut offs: Vec<usize> = container
            .descriptors
            .iter()
            .map(|d| d.data_offset as usize)
            .collect();
        offs.push(entry_len);
        (0..container.descriptors.len())
            .map(|i| (offs[i], offs[i + 1]))
            .collect()
    };
    let sec1_raw = if sec1_scaled {
        legaia_lzs::compress(&sec1_decoded)
    } else {
        prot_0874
            .get(spans[1].0..spans[1].1.min(prot_0874.len()))
            .ok_or_else(|| anyhow::anyhow!("section 1 span out of range"))?
            .to_vec()
    };

    // Reassemble the container: header + pairs + section data. Greedy
    // LZS first; the optimal parse only when the entry budget misses.
    let mut s0 = legaia_lzs::compress(&sec0_new);
    let mut s2 = legaia_lzs::compress(&sec2_new);
    let mut sec1_raw = sec1_raw;
    let header_and_slack = 0x20 + 8;
    if header_and_slack + s0.len() + sec1_raw.len() + s2.len() > entry_len {
        s0 = legaia_lzs::compress_optimal(&sec0_new);
        s2 = legaia_lzs::compress_optimal(&sec2_new);
        if sec1_scaled {
            sec1_raw = legaia_lzs::compress_optimal(&sec1_decoded);
        }
    }
    if std::env::var("LEGAIA_FIELDIZE_DEBUG").is_ok() {
        eprintln!(
            "[fieldize] decimate {decimate}: sec0 {} (lzs {}), sec1 raw {}, sec2 {} (lzs {}), entry {}",
            sec0_new.len(),
            s0.len(),
            sec1_raw.len(),
            sec2_new.len(),
            s2.len(),
            entry_len
        );
    }
    let streams = [s0, sec1_raw, s2];
    let sizes = [sec0_new.len() as u32, sec1.size, sec2_new.len() as u32];
    let types = [sec0.type_byte, sec1.type_byte, sec2.type_byte];
    let mut out = Vec::with_capacity(entry_len);
    out.extend_from_slice(&container.meta[0].to_le_bytes());
    // meta[1] = total decompressed budget (sum of the three sizes).
    let meta1: u32 = sizes.iter().sum();
    out.extend_from_slice(&meta1.to_le_bytes());
    let header_len = 8 + 3 * 8;
    let mut offset = header_len.max(container.descriptors[0].data_offset as usize);
    let mut pairs = Vec::new();
    for i in 0..3 {
        pairs.push(((types[i] as u32) << 24 | sizes[i], offset as u32));
        offset += streams[i].len();
        offset = offset.div_ceil(4) * 4;
    }
    for (ts, off) in &pairs {
        out.extend_from_slice(&ts.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
    }
    for (i, s) in streams.iter().enumerate() {
        let start = pairs[i].1 as usize;
        if out.len() > start {
            bail!("container section {i} overlaps the previous one");
        }
        out.resize(start, 0);
        out.extend_from_slice(s);
    }
    if out.len() > entry_len {
        bail!(
            "rebuilt PROT 0874 is {} bytes, entry holds {}",
            out.len(),
            entry_len
        );
    }
    // Preserve the RETAIL bytes past the rebuilt payload instead of
    // zero-filling. What reads that band is not settled - it is NOT the
    // battle VDF table, which is registered off a different entry (see
    // the note above sec0) - but zero-filling it was measured live as
    // thin garbage streaks over the Spirit charge (the
    // `delilas_gi_spirit_artifact` catalogue state), and statically as
    // ~4.5 KB of retail-nonzero bytes at the entry tail
    // (0x186DB..0x197FF on the USA disc) that the rebuild wiped. The
    // copy keeps every retained byte at its ORIGINAL offset (the tail
    // is addressed relative to the raw entry base); the stretch between
    // the shorter rebuilt streams and retail's stream end is stale
    // compressed data nothing reads.
    let payload_end = out.len();
    let retail_rest = prot_0874
        .get(payload_end..entry_len)
        .ok_or_else(|| anyhow::anyhow!("retail entry shorter than its own length"))?;
    out.extend_from_slice(retail_rest);
    Ok(FieldizedPack {
        entry: out,
        warnings,
    })
}
