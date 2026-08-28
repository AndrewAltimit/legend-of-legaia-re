//! Event-scene mirrors of the Delilas party swap: rebuild each story
//! scene's Delilas NPC field meshes as the mapped **heroes'** field
//! models, so every event that shows the siblings shows Vahn / Noa /
//! Gala instead. The nilboa duel ground has its own pass
//! ([`super::nivora_field`]) because its members live in a separate
//! pack entry; the scenes here keep their NPC meshes **inside** the
//! scene bundle's compressed TMD section, so the rebuild recompresses
//! and re-lays the bundle instead of tiling a raw pack.
//!
//! Covered scenes ([`EVENT_SCENES`], coordinates read off the retail
//! disc via the MAN actor placements - `ActorPlacement::model_index` is
//! the TMD-section member and `anim_id - 1` the scene ANM record the
//! placement poses it with):
//!
//! | scene | bundle | siblings (member @ anchor record) |
//! |---|---|---|
//! | `stone` (map-stone confrontation) | 175 | Gi 33@37, Che 34@52, Lu 35@62 |
//! | `taiku2` (Zora's floating castle) | 426 | Gi 116..119@14, Che 120..123@20, Lu 124..127@26 |
//! | `conc2` (past Conkram, court outfits) | 624 (+TIM pack 625) | Gi 165@64, Che 166@66, Lu 167@68 |
//!
//! `stone` / `taiku2` reuse the nilboa chibi rigs byte-for-byte (same
//! CLUT rows on 481, same texpage), so the head-window discovery and
//! budget behave exactly like the ravine. `taiku2` stores **four
//! copies** of each sibling differing only in baked per-vertex shading
//! (the script swaps copies across event beats); all four get the same
//! hero bake, anchored on the placement's own record. `conc2`'s
//! siblings wear their past-Conkram court outfits - different meshes
//! (pack tail 165..167, CLUTs on row 476), with the scene textures in
//! the sibling `tim_pack` entry 625 rather than a bundle `TIM_LIST`
//! section, so its head repaint patches that entry in place.
//!
//! **Ordering constraint (load-bearing):** same as the nilboa pass -
//! `prot_0874` must be the **pre-fieldize** entry bytes, captured
//! before `apply_delilas_party` rewrites PROT 0874 with sibling
//! geometry.

use std::collections::BTreeMap;

use super::nivora_field::{
    SlotReport, hero_slot_source, heroize_slot, npc_target_at, rebuild_pack_body, repaint_head_tim,
};
use super::*;
use crate::pack::parse_pack;
use crate::parse_player_lzs;

/// One sibling's coordinates inside an event scene.
#[derive(Debug, Clone, Copy)]
pub struct EventSlotSpec {
    /// Sibling monster id (162 Gi / 163 Che / 164 Lu).
    pub monster_id: u16,
    /// TMD-section member indices the sibling's mesh occupies (taiku2
    /// keeps four shading copies; every copy gets the same bake).
    pub members: &'static [usize],
    /// Scene ANM record the placement binds (`anim_id - 1`) - the rest
    /// anchor the bake re-expresses the hero rig against.
    pub anchor_record: usize,
    /// Pinned object-index -> body-role assignment, for scenes whose
    /// anchor stance fools the geometric role splitter (taiku2's kneel
    /// derives a "clean" split that puts the torso on a leg bone). The
    /// values are the split derived from a trusted neutral stance of the
    /// byte-identical rig (stone records 37/52/62). `None` = derive from
    /// the anchor record's frames.
    pub roles: Option<[usize; 10]>,
}

/// One event scene's mirror coordinates.
#[derive(Debug, Clone, Copy)]
pub struct EventSceneSpec {
    /// CDNAME scene label (report tag).
    pub scene: &'static str,
    /// The scene-asset-table bundle PROT entry (extraction index).
    pub bundle_entry: usize,
    /// Separate raw-TIM carrier entry when the bundle has no `TIM_LIST`
    /// section (conc2's `tim_pack` sibling); `None` = bundle section.
    pub tim_entry: Option<usize>,
    /// Gi / Che / Lu coordinates.
    pub slots: [EventSlotSpec; 3],
}

/// Every Delilas event appearance outside the nilboa duel ground.
pub const EVENT_SCENES: &[EventSceneSpec] = &[
    EventSceneSpec {
        scene: "stone",
        bundle_entry: 175,
        tim_entry: None,
        slots: [
            EventSlotSpec {
                monster_id: 162,
                members: &[33],
                anchor_record: 37,
                roles: None,
            },
            EventSlotSpec {
                monster_id: 163,
                members: &[34],
                anchor_record: 52,
                roles: None,
            },
            EventSlotSpec {
                monster_id: 164,
                members: &[35],
                anchor_record: 62,
                roles: None,
            },
        ],
    },
    EventSceneSpec {
        scene: "taiku2",
        bundle_entry: 426,
        tim_entry: None,
        slots: [
            EventSlotSpec {
                monster_id: 162,
                members: &[116, 117, 118, 119],
                anchor_record: 14,
                roles: Some([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
            },
            EventSlotSpec {
                monster_id: 163,
                members: &[120, 121, 122, 123],
                anchor_record: 20,
                roles: Some([0, 1, 3, 2, 4, 5, 6, 7, 8, 9]),
            },
            EventSlotSpec {
                monster_id: 164,
                members: &[124, 125, 126, 127],
                anchor_record: 26,
                roles: Some([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
            },
        ],
    },
    EventSceneSpec {
        scene: "conc2",
        bundle_entry: 624,
        tim_entry: Some(625),
        slots: [
            EventSlotSpec {
                monster_id: 162,
                members: &[165],
                anchor_record: 64,
                roles: None,
            },
            EventSlotSpec {
                monster_id: 163,
                members: &[166],
                anchor_record: 66,
                roles: None,
            },
            EventSlotSpec {
                monster_id: 164,
                members: &[167],
                anchor_record: 68,
                roles: None,
            },
        ],
    },
];

/// Scene-bundle descriptor count (`07 00 00 00` lead on every event
/// bundle here).
const BUNDLE_DESCRIPTORS: usize = 7;

/// The rebuilt entries for one event scene.
#[derive(Debug, Clone)]
pub struct EventFieldPatch {
    /// Rebuilt bundle entry bytes (same length as the input).
    pub bundle_entry: Vec<u8>,
    /// Rebuilt external TIM entry (present iff the spec names one).
    pub tim_entry: Option<Vec<u8>>,
    /// Non-fatal notes (decimation level, texture downscales).
    pub warnings: Vec<String>,
}

/// Rebuild one event scene so its Delilas NPC meshes wear the mapped
/// heroes' field models.
///
/// `mapping[slot]` = the sibling monster id character `slot` wears in
/// battle (the `fieldize_pack_npc` array); the hero shown for sibling S
/// is the slot whose entry is S. `prot_0874` MUST be the pre-fieldize
/// entry bytes (see the module doc). `tim_entry` carries the external
/// TIM entry's bytes iff `spec.tim_entry` names one.
pub fn heroize_event_scene(
    spec: &EventSceneSpec,
    bundle: &[u8],
    tim_entry: Option<&[u8]>,
    prot_0874: &[u8],
    mapping: [u16; 3],
) -> Result<(EventFieldPatch, Vec<SlotReport>)> {
    let mut warnings = Vec::new();
    let container = parse_player_lzs(bundle, BUNDLE_DESCRIPTORS)
        .with_context(|| format!("{} bundle container", spec.scene))?;
    let tmd_idx = container
        .descriptors
        .iter()
        .position(|d| d.type_byte == 0x02)
        .ok_or_else(|| anyhow::anyhow!("{} bundle has no TMD section", spec.scene))?;
    let sec_tmd = crate::decode(
        bundle,
        &container.descriptors[tmd_idx],
        crate::DecodeMode::Lzs,
    )
    .with_context(|| format!("{} TMD section", spec.scene))?;
    let (mut tims, tim_sec_idx) = match spec.tim_entry {
        Some(_) => {
            let bytes = tim_entry.ok_or_else(|| {
                anyhow::anyhow!("{}: external TIM entry not supplied", spec.scene)
            })?;
            (bytes.to_vec(), None)
        }
        None => {
            let idx = container
                .descriptors
                .iter()
                .position(|d| d.type_byte == 0x01)
                .ok_or_else(|| anyhow::anyhow!("{} bundle has no TIM_LIST section", spec.scene))?;
            (
                crate::decode(bundle, &container.descriptors[idx], crate::DecodeMode::Lzs)
                    .with_context(|| format!("{} TIM list", spec.scene))?,
                Some(idx),
            )
        }
    };
    let anm = crate::player_anm::find_in_entry(bundle, BUNDLE_DESCRIPTORS)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("{} bundle carries no ANM bundle", spec.scene))?;

    // (sibling spec, hero slot) in ascending first-member order.
    let mut coords: Vec<(&EventSlotSpec, usize)> = Vec::new();
    for (hero_slot, &id) in mapping.iter().enumerate() {
        let slot_spec = spec
            .slots
            .iter()
            .find(|s| s.monster_id == id)
            .ok_or_else(|| anyhow::anyhow!("monster id {id} has no {} coordinates", spec.scene))?;
        coords.push((slot_spec, hero_slot));
    }
    coords.sort_by_key(|(s, _)| s.members[0]);

    // Budget: the contiguous span of every replaced member.
    let pack_entries = parse_pack(&sec_tmd)?;
    let all_members: Vec<usize> = {
        let mut m: Vec<usize> = coords
            .iter()
            .flat_map(|(s, _)| s.members.iter().copied())
            .collect();
        m.sort_unstable();
        m
    };
    let first = all_members[0];
    let last = *all_members.last().unwrap();
    let budget = pack_entries
        .get(last + 1)
        .map(|e| e.byte_offset)
        .unwrap_or_else(|| pack_entries[last].byte_offset + pack_entries[last].size)
        - pack_entries[first].byte_offset;

    // Detail ladder: full detail first, then progressively drop prims
    // under a size threshold until every replaced member fits the span.
    for decimate in [0.0f32, 1.5, 2.5, 3.5, 5.0, 7.0, 9.0, 12.0] {
        let mut trial_warnings = Vec::new();
        let mut slots = Vec::with_capacity(3);
        for &(slot_spec, hero_slot) in &coords {
            let hero = hero_slot_source(prot_0874, hero_slot)
                .with_context(|| format!("hero slot {hero_slot}"))?;
            let npc = npc_target_at(
                &sec_tmd,
                &tims,
                &anm,
                slot_spec.members[0],
                slot_spec.anchor_record,
                slot_spec.roles,
            )
            .with_context(|| format!("{} sibling {}", spec.scene, slot_spec.monster_id))?;
            slots.push((
                heroize_slot(&hero, &npc, decimate, &mut trial_warnings).with_context(|| {
                    format!(
                        "bake hero {hero_slot} onto {} sibling {}",
                        spec.scene, slot_spec.monster_id
                    )
                })?,
                npc,
            ));
        }
        let need: usize = slots
            .iter()
            .zip(&coords)
            .map(|((s, _), (spec_slot, _))| s.tmd.len().div_ceil(4) * 4 * spec_slot.members.len())
            .sum();
        if need > budget {
            continue;
        }
        if decimate > 0.0 {
            trial_warnings.push(format!("field detail reduced (min prim size {decimate})"));
        }
        warnings.extend(trial_warnings);
        let mut reports = Vec::with_capacity(3);
        let mut replacements: BTreeMap<usize, &[u8]> = BTreeMap::new();
        for ((s, npc), &(slot_spec, hero_slot)) in slots.iter().zip(&coords) {
            repaint_head_tim(&mut tims, &npc.head_tim, s).with_context(|| {
                format!(
                    "repaint {} sibling {} head TIM",
                    spec.scene, slot_spec.monster_id
                )
            })?;
            for &m in slot_spec.members {
                replacements.insert(m, s.tmd.as_slice());
            }
            let tmd = legaia_tmd::parse(&s.tmd)?;
            let model = decode_model(&tmd, &s.tmd)?;
            reports.push(SlotReport {
                monster_id: slot_spec.monster_id,
                hero_slot,
                member: slot_spec.members[0],
                tmd_len: s.tmd.len(),
                part_verts: model.iter().map(|o| o.vertices.len()).collect(),
            });
        }
        let new_tmd = rebuild_pack_body(&sec_tmd, &replacements)?;
        let mut section_updates: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
        section_updates.insert(tmd_idx, new_tmd);
        if let Some(idx) = tim_sec_idx {
            section_updates.insert(idx, tims.clone());
        }
        let bundle_entry = rebuild_bundle_sections(bundle, &section_updates)
            .with_context(|| format!("re-lay {} bundle", spec.scene))?;
        let tim_out = spec.tim_entry.map(|_| tims);
        return Ok((
            EventFieldPatch {
                bundle_entry,
                tim_entry: tim_out,
                warnings,
            },
            reports,
        ));
    }
    bail!(
        "hero field meshes do not fit the {} member span at any detail level (budget {budget})",
        spec.scene
    )
}

/// Rebuild a scene-asset-table bundle with some decoded sections
/// replaced (same decoded size each). Untouched sections keep their raw
/// compressed byte spans; replaced sections recompress (greedy, then
/// optimal on overflow). In-place when every replaced stream fits its
/// retail span; otherwise every section re-lays inside the entry (all
/// [`BUNDLE_DESCRIPTORS`] descriptor offsets patched), which is valid
/// because each descriptor addresses its stream independently.
fn rebuild_bundle_sections(bundle: &[u8], updates: &BTreeMap<usize, Vec<u8>>) -> Result<Vec<u8>> {
    let container = parse_player_lzs(bundle, BUNDLE_DESCRIPTORS)?;
    let offs: Vec<usize> = container
        .descriptors
        .iter()
        .map(|d| d.data_offset as usize)
        .collect();
    for w in offs.windows(2) {
        if w[1] < w[0] {
            bail!("bundle descriptor offsets are not ascending");
        }
    }
    let entry_len = bundle.len();
    let span_end = |i: usize| {
        if i + 1 < offs.len() {
            offs[i + 1]
        } else {
            entry_len
        }
    };
    let mut compressed: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    for (&i, decoded) in updates {
        let d = &container.descriptors[i];
        if d.size as usize != decoded.len() {
            bail!(
                "section {i} decoded size changed ({} vs {})",
                decoded.len(),
                d.size
            );
        }
        let mut c = legaia_lzs::compress(decoded);
        if offs[i] + c.len() > span_end(i) {
            c = legaia_lzs::compress_optimal(decoded);
        }
        compressed.insert(i, c);
    }
    // In-place when every replaced stream fits its retail span.
    if compressed
        .iter()
        .all(|(&i, c)| offs[i] + c.len() <= span_end(i))
    {
        let mut out = bundle.to_vec();
        for (&i, c) in &compressed {
            out[offs[i]..offs[i] + c.len()].copy_from_slice(c);
            out[offs[i] + c.len()..span_end(i)].fill(0);
        }
        return Ok(out);
    }
    // Re-lay every section back to back from the first descriptor's
    // retail offset, 4-aligned, and patch all descriptor offsets.
    let mut out = bundle[..offs[0]].to_vec();
    let mut new_offs = vec![0u32; offs.len()];
    for i in 0..offs.len() {
        let cursor = out.len().div_ceil(4) * 4;
        out.resize(cursor, 0);
        new_offs[i] = cursor as u32;
        match compressed.get(&i) {
            Some(c) => out.extend_from_slice(c),
            None => out.extend_from_slice(&bundle[offs[i]..span_end(i)]),
        }
    }
    if out.len() > entry_len {
        bail!(
            "rebuilt bundle is {} bytes, entry holds {entry_len}",
            out.len()
        );
    }
    out.resize(entry_len, 0);
    for (i, off) in new_offs.iter().enumerate() {
        let p = 8 + i * 8 + 4;
        out[p..p + 4].copy_from_slice(&off.to_le_bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_scene_table_is_well_formed() {
        for spec in EVENT_SCENES {
            let mut ids: Vec<u16> = spec.slots.iter().map(|s| s.monster_id).collect();
            ids.sort_unstable();
            assert_eq!(ids, vec![162, 163, 164], "{}: sibling ids", spec.scene);
            let mut members: Vec<usize> = spec
                .slots
                .iter()
                .flat_map(|s| s.members.iter().copied())
                .collect();
            assert!(!members.is_empty());
            members.sort_unstable();
            // One contiguous run - the budget span the rebuild reflows.
            for w in members.windows(2) {
                assert_eq!(w[1], w[0] + 1, "{}: members contiguous", spec.scene);
            }
            for s in &spec.slots {
                assert!(!s.members.is_empty(), "{}: member list", spec.scene);
            }
        }
    }
}
