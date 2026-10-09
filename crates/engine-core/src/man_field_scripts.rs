//! The MAN field-script decoders' scene seam.
//!
//! The decoders - partition walks, placement classification, carrier
//! derivation, NPC motion, scene triggers, the flag / window / motion
//! censuses over a set of MAN carriers - live in
//! [`legaia_engine_field::man_field_scripts`] and are re-exported here at
//! their old paths. What stays is what needs a loaded [`Scene`]: resolving a
//! scene's MAN carriers out of its CDNAME block, and the three disc-wide
//! censuses' scene-name entry points, which load each scene and hand its
//! carriers to the carrier-level census.

pub use legaia_engine_field::man_field_scripts::*;

use crate::scene::{ProtIndex, Scene};
use std::collections::BTreeMap;

/// Every walkable MAN payload in `scene`'s CDNAME block: the asset-table
/// bundle MAN first (when present), then each **variant** MAN found as a
/// type-3 chunk of a `DataFieldStreaming` / `DataFieldTruncated` entry whose
/// chunk payload parses as a MAN. Payload-identical duplicates are dropped,
/// so a variant that merely re-ships the bundle MAN's bytes appears once.
pub fn scene_man_carriers(index: &ProtIndex, scene: &Scene) -> Vec<ManCarrier> {
    let mut out: Vec<ManCarrier> = Vec::new();
    if let Some(bundle) = crate::scene_bundle::find_bundle(scene)
        && let Ok(entry_bytes) = index.entry_bytes_extended(bundle.entry_idx())
        && let Ok(Some(payload)) = crate::scene_bundle::extract_man_payload(&bundle, &entry_bytes)
    {
        out.push(ManCarrier {
            entry_idx: bundle.entry_idx(),
            chunk_offset: None,
            payload,
        });
    }
    for (entry_idx, chunk_offset, payload) in crate::scene_bundle::streaming_man_payloads(scene) {
        if out.iter().any(|c| c.payload == payload) {
            continue;
        }
        out.push(ManCarrier {
            entry_idx,
            chunk_offset: Some(chunk_offset),
            payload,
        });
    }
    out
}

/// Each named scene's MAN carriers, loading the scene off `index`; a scene
/// that fails to load is skipped, as every census below documents.
fn scene_carriers<'a, I, S>(
    index: &'a ProtIndex,
    scenes: I,
) -> impl Iterator<Item = (S, Vec<ManCarrier>)> + 'a
where
    I: IntoIterator<Item = S>,
    I::IntoIter: 'a,
    S: AsRef<str>,
{
    scenes.into_iter().filter_map(move |name| {
        let scene = Scene::load(index, name.as_ref()).ok()?;
        let carriers = scene_man_carriers(index, &scene);
        Some((name, carriers))
    })
}

/// Disc-wide SYSTEM-flag census over the named scenes: loads each one and
/// walks its MAN carriers ([`scene_man_carriers`]) through
/// [`system_flag_census_of`], which documents the result.
pub fn system_flag_census<I, S>(index: &ProtIndex, scenes: I) -> BTreeMap<u16, Vec<FlagCensusSite>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    system_flag_census_of(scene_carriers(index, scenes))
}

/// Disc-wide op-`0x49` window census over the named scenes: loads each one
/// and walks its MAN carriers through [`op49_window_census_of`].
// REF: FUN_801EF014
pub fn op49_window_census<I, S>(index: &ProtIndex, scenes: I) -> Vec<Op49WindowSite>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    op49_window_census_of(scene_carriers(index, scenes))
}

/// Disc-wide motion-VM story-flag census over the named scenes: loads each
/// one and walks its MAN carriers through [`motion_flag_census_of`].
pub fn motion_flag_census<I, S>(
    index: &ProtIndex,
    scenes: I,
) -> BTreeMap<u16, Vec<MotionCensusSite>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    motion_flag_census_of(scene_carriers(index, scenes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::man_section::{ManFile, ManHeader};

    /// Build a MAN with two partition-1 records: record 0 (the scene
    /// controller, skipped by `actor_placements`) and record 1 (a placed actor
    /// whose `[N=0][model][actions][tx][tz]` header is followed by `script`).
    fn man_with_placement_script(script: &[u8]) -> (ManFile, Vec<u8>) {
        let data_region_offset = 0x40usize;
        // Record 0: a minimal controller (`N=0`, header, halt).
        let rec0: &[u8] = &[0x00, 0, 0, 0, 0, 0x21];
        // Record 1: N=0, model=5, actions=0, tile (3,4), then the script.
        let mut rec1 = vec![0x00, 0x05, 0x00, 0x03, 0x04];
        rec1.extend_from_slice(script);

        let off0 = 0u32;
        let off1 = rec0.len() as u32;
        let mut man = vec![0u8; data_region_offset];
        man.extend_from_slice(rec0);
        man.extend_from_slice(&rec1);

        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 2, 0],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![off0, off1], vec![]],
            data_region_offset,
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        (man_file, man)
    }

    #[test]
    fn classify_inline_text_with_phantom_warp_byte_is_an_npc() {
        // A talk-NPC record whose message contains a literal '>' (0x3E, the
        // warp/interact opcode). The structural pass finds the 0x1F text block;
        // the desync gate ignores the '>' byte because it sits inside the text,
        // so the actor classifies as an Npc carrying the inline message - NOT a
        // phantom portal.
        let mut script = vec![0x25u8]; // a benign leading op
        script.extend_from_slice(&[0x1F]); // text-segment lead
        script.extend_from_slice(b"<Go north>"); // contains 0x3E ('>')
        script.push(0x00); // terminator
        let (mf, man) = man_with_placement_script(&script);
        let placements = mf.actor_placements(&man);
        let kind = classify_placement(&mf, &man, &placements[0]);
        match kind {
            PlacementKind::Npc { dialog_inline, .. } => {
                let inline = dialog_inline.expect("inline text captured");
                // Renders the segment text (after the 0x1F lead).
                let panel = crate::dialog::OwnedDialogPanel::from_inline_dialog(&inline);
                assert!(panel.is_some(), "inline buffer is renderable");
            }
            other => panic!("expected Npc, got {other:?}"),
        }
    }
}
