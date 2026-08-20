//! Disc oracle for the enemy-side animation mirror
//! (`enemy_anim_mirror::apply_enemy_anim_mirror`): apply the party swap
//! plus the mirror on a scratch copy under a NON-default mapping, then
//! prove the staged special entries changed and still decode through the
//! retail stream reading at the block's part count, every module-staged
//! entry keeps the 23-keyframe floor, the rewritten idle is rigidly
//! anchored on the block's own rest, the block fits its budgets, the
//! pass is deterministic + idempotent, and the whole thing is
//! non-vacuous against the unpatched image. Also holds the bake-parity
//! bound: the per-part exact affine fit of the enemy-side mesh bake
//! (the instrument the historical roll defects were invisible without).
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::monster_archive as ma;
use legaia_asset::party_swap;
use legaia_asset::party_swap::enemy_anim;
use legaia_patcher::delilas_party::{PartyMapping, apply_delilas_party};
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_patcher::enemy_anim_mirror::{RetailSources, apply_enemy_anim_mirror, staged_entries};

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    std::fs::read(path).ok()
}

/// Pack one part pose back into the raw 9-byte stream record - an
/// independent transcription of the `FUN_8004998C` bit layout, used to
/// prove the monster streams are the raw packed family (byte-identical
/// re-encode), not a delta codec.
fn pack_raw(p: &ma::PartPose) -> [u8; 9] {
    let f = [
        (p.tx as u16) & 0xFFF,
        (p.ty as u16) & 0xFFF,
        (p.tz as u16) & 0xFFF,
        p.rx & 0xFFF,
        p.ry & 0xFFF,
        p.rz & 0xFFF,
    ];
    let mut out = [0u8; 9];
    for pair in 0..3 {
        let (a, b) = (f[pair * 2], f[pair * 2 + 1]);
        out[pair * 3] = a as u8;
        out[pair * 3 + 1] = b as u8;
        out[pair * 3 + 2] = ((a >> 8) as u8 & 0x0F) | ((b >> 4) as u8 & 0xF0);
    }
    out
}

/// Entry offsets + spans of a decoded monster block under the raw
/// 9-byte stream reading.
fn entry_spans(block: &[u8]) -> Vec<(usize, usize, u8, usize)> {
    let count = block[0x4A] as usize;
    (0..count)
        .map(|i| {
            let off =
                u32::from_le_bytes(block[0x4C + i * 4..0x50 + i * 4].try_into().unwrap()) as usize;
            let parts = block[off + 0x8C] as usize;
            let frames = block[off + 0x8D] as usize;
            (off, 0x8E + parts * frames * 9, block[off], frames)
        })
        .collect()
}

#[test]
fn monster_streams_are_the_raw_packed_family() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(original).expect("open disc");
    let archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    for id in [162u16, 163, 164] {
        let block = ma::decode_block(&archive, id)
            .expect("decode")
            .expect("populated");
        let spans = entry_spans(&block);
        let anims = ma::animations(&archive, id)
            .expect("anims")
            .expect("populated");
        assert_eq!(
            spans.len(),
            anims.len(),
            "monster {id}: every entry decodes"
        );
        // Ascending-contiguous raw layout (0..=3 byte alignment gaps).
        for w in spans.windows(2) {
            let gap = w[1].0 as i64 - (w[0].0 + w[0].1) as i64;
            assert!(
                (0..=3).contains(&gap),
                "monster {id}: raw spans not contiguous (gap {gap})"
            );
        }
        // Byte-exact re-encode: decode -> pack_raw == the on-disc bytes.
        for (i, ((off, _, _, frames), anim)) in spans.iter().zip(&anims).enumerate() {
            assert_eq!(anim.frame_count, *frames);
            let mut repacked = Vec::new();
            for row in &anim.frames {
                for p in row {
                    repacked.extend_from_slice(&pack_raw(p));
                }
            }
            let raw = &block[off + 0x8E..off + 0x8E + repacked.len()];
            assert_eq!(
                raw,
                &repacked[..],
                "monster {id} entry {i}: stream is not the raw 9-byte packed encoding"
            );
        }
    }
}

#[test]
fn mirror_on_an_unswapped_disc_is_refused() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(original).expect("open disc");
    let archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    let players: Vec<Vec<u8>> = (863..=865)
        .map(|e| patcher.read_entry_footprint(e).expect("player"))
        .collect();
    let readef = patcher.read_entry_footprint(894).expect("readef");
    let retail = RetailSources {
        archive: &archive,
        players: [&players[0], &players[1], &players[2]],
        readef: &readef,
    };
    let mapping = PartyMapping::parse("lu,gi,che").expect("mapping");
    let err = apply_enemy_anim_mirror(&mut patcher, &mapping, &retail)
        .expect_err("mirror before the model loop must be refused");
    assert!(
        format!("{err:#}").contains("must run after"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn mirrored_duel_blocks_fight_with_the_heroes_clips() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The user tests non-default arrangements; never key anything on the
    // default (Vahn<-Gi) permutation.
    let mapping = PartyMapping::parse("lu,gi,che").expect("mapping");

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let retail_archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    let players: Vec<Vec<u8>> = (863..=865)
        .map(|e| patcher.read_entry_footprint(e).expect("player"))
        .collect();
    let readef = patcher.read_entry_footprint(894).expect("readef");
    let retail = RetailSources {
        archive: &retail_archive,
        players: [&players[0], &players[1], &players[2]],
        readef: &readef,
    };

    apply_delilas_party(
        &mut patcher,
        &mapping,
        Default::default(),
        Default::default(),
    )
    .expect("apply party");
    // The swapped-but-unmirrored blocks: the baseline the mirror must
    // visibly change.
    let swapped_archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("swapped archive");

    apply_enemy_anim_mirror(&mut patcher, &mapping, &retail).expect("mirror");
    let mirrored_archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("mirrored archive");

    for (_, _, _, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let (staged, close) = staged_entries(sibling);
        let staged_all: Vec<usize> = staged.iter().copied().chain(close).collect();

        let retail_block = ma::decode_block(&retail_archive, id).unwrap().unwrap();
        let swapped_block = ma::decode_block(&swapped_archive, id).unwrap().unwrap();
        let block = ma::decode_block(&mirrored_archive, id).unwrap().unwrap();

        // Budget: the mirrored block stays inside the +16K decoded-growth
        // envelope over retail, and its slot re-encoded (or the patch
        // above would have failed the fixed 0x14000 stride).
        assert!(
            block.len() <= retail_block.len() + 0x4000,
            "{who} (monster {id}): decoded block grew too far ({} > {} + 16K)",
            block.len(),
            retail_block.len()
        );

        // Every entry decodes through the retail stream reading at the
        // block's 15-part count, and the entry count / index space is
        // untouched (the cast module stages raw indices).
        let anims = ma::animations(&mirrored_archive, id).unwrap().unwrap();
        let spans = entry_spans(&block);
        assert_eq!(spans.len(), entry_spans(&retail_block).len());
        assert_eq!(anims.len(), spans.len(), "{who}: every entry decodes");
        for (i, a) in anims.iter().enumerate() {
            assert_eq!(
                a.part_count,
                party_swap::CANONICAL_PARTS,
                "{who} entry {i}: part count"
            );
        }

        // Staged entries: streams CHANGED vs the swapped baseline, tags
        // preserved, and every module-staged entry holds the 23-frame
        // floor.
        let swapped_spans = entry_spans(&swapped_block);
        for &i in &staged_all {
            let (off, span, tag, frames) = spans[i];
            let (soff, sspan, stag, _) = swapped_spans[i];
            assert_eq!(tag, stag, "{who} staged entry {i}: tag preserved");
            assert!(
                frames >= 23,
                "{who} staged entry {i}: {frames} frames < the 23-keyframe module floor"
            );
            let new_stream = &block[off + 0x8C..off + span];
            let old_stream = &swapped_block[soff + 0x8C..soff + sspan];
            assert_ne!(
                new_stream, old_stream,
                "{who} staged entry {i}: stream did not change"
            );
        }
        // Duration preserved per staged chain stage: frames * 8 / rate
        // equals the hero Hyper segment's authored duration; assert the
        // coarser invariant that the whole chain's tick duration matches
        // the hero clip's within one tick per stage.
        let hyper = enemy_anim::hero_hyper_clip(
            &players[mapping
                .pairs()
                .iter()
                .find(|p| p.4 == sibling)
                .map(|p| p.2)
                .unwrap()],
            &readef,
            mapping
                .pairs()
                .iter()
                .find(|p| p.4 == sibling)
                .map(|p| p.2)
                .unwrap(),
        )
        .expect("hero hyper");
        let hero_ticks = hyper.frame_count * 8 / hyper.rate.max(1) as usize;
        let chain_ticks: usize = staged
            .iter()
            .map(|&i| {
                let (off, _, _, frames) = spans[i];
                let rate = block[off + 0x78].max(1) as usize;
                frames * 8 / rate
            })
            .sum();
        assert!(
            chain_ticks.abs_diff(hero_ticks) <= staged.len() * 8,
            "{who}: staged chain {chain_ticks} ticks vs hero Hyper {hero_ticks}"
        );

        // Idle (entry 0): stream changed, and frame 0 is rigidly
        // anchored on the block's own rest - torso x/z exactly on the
        // retail rest torso, and the deepest-ankle floor over the cycle
        // exactly on the retail floor (GTE y-down).
        let (off, span, _, _) = spans[0];
        let (soff, sspan, ..) = swapped_spans[0];
        assert_ne!(
            &block[off + 0x8C..off + span],
            &swapped_block[soff + 0x8C..soff + sspan],
            "{who}: idle stream did not change"
        );
        let retail_idle = ma::idle_animation(&retail_archive, id).unwrap().unwrap();
        let idle = &anims[0];
        let rest0 = &retail_idle.frames[0];
        let new0 = &idle.frames[0];
        assert_eq!(new0[1].tx, rest0[1].tx, "{who}: idle torso x anchored");
        assert_eq!(new0[1].tz, rest0[1].tz, "{who}: idle torso z anchored");
        let floor = |frames: &[Vec<ma::PartPose>]| {
            frames
                .iter()
                .flat_map(|f| [f[11].ty, f[14].ty])
                .max()
                .unwrap()
        };
        assert_eq!(
            floor(&idle.frames),
            floor(&retail_idle.frames),
            "{who}: idle floor anchored"
        );

        // Non-vacuous against retail too: the staged streams differ from
        // the retail Delilas choreography.
        let retail_spans = entry_spans(&retail_block);
        for &i in &staged_all {
            let (off, span, ..) = spans[i];
            let (roff, rspan, ..) = retail_spans[i];
            assert_ne!(
                &block[off + 0x8C..off + span],
                &retail_block[roff + 0x8C..roff + rspan],
                "{who} staged entry {i}: identical to retail (vacuous)"
            );
        }
    }

    // Determinism: the whole pipeline from the same image reproduces the
    // mirrored archive byte for byte; a second mirror pass on the
    // already-mirrored image is a no-op (idempotence).
    let mut patcher2 = DiscPatcher::open(original).expect("open disc 2");
    apply_delilas_party(
        &mut patcher2,
        &mapping,
        Default::default(),
        Default::default(),
    )
    .expect("apply party 2");
    apply_enemy_anim_mirror(&mut patcher2, &mapping, &retail).expect("mirror 2");
    let mirrored2 = patcher2
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("mirrored archive 2");
    assert_eq!(mirrored_archive, mirrored2, "pipeline determinism");
    apply_enemy_anim_mirror(&mut patcher2, &mapping, &retail).expect("mirror idempotent");
    let mirrored3 = patcher2
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("mirrored archive 3");
    assert_eq!(mirrored2, mirrored3, "mirror idempotence");
}

/// Bake parity: the enemy-side mesh bake holds the same whole-rig
/// alignment bound the player-side bake holds. Measured with a per-part
/// exact affine fit of source→baked geometry (rotation vs the pivot-only
/// whole-rig ideal + principal scales + non-affine residual) - the
/// instrument class that catches roll defects gap metrics are blind to.
/// Pre-fix this read up to 175 degrees of excess roll; the bound below
/// is an order of magnitude under the smallest defect ever observed.
#[test]
fn enemy_bake_holds_the_whole_rig_alignment_bound() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(original).expect("open disc");
    let archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    // Every pairing (the mapping is a free permutation).
    for (entry, who, rig) in [
        (863usize, "Vahn", &party_swap::RIG_VAHN_GALA),
        (864, "Noa", &party_swap::RIG_NOA),
        (865, "Gala", &party_swap::RIG_VAHN_GALA),
    ] {
        let file = patcher.read_entry_footprint(entry).expect("player file");
        for id in [162u16, 163, 164] {
            let report =
                enemy_anim::monsterize_fit_report(&file, rig, &archive, id).expect("fit report");
            assert!(!report.is_empty(), "{who} -> {id}: empty fit report");
            for f in &report {
                assert!(
                    f.excess_deg <= 5.0,
                    "{who} -> {id} part {}: {:.1} deg excess rotation \
                     (roll-defect class; whole-rig alignment broken)",
                    f.part,
                    f.excess_deg
                );
                assert!(
                    f.residual <= 0.06,
                    "{who} -> {id} part {}: non-affine residual {:.3}",
                    f.part,
                    f.residual
                );
                assert!(
                    f.principal_scales[0] <= 5.0 && f.principal_scales[2] >= 0.15,
                    "{who} -> {id} part {}: implausible principal scales {:?}",
                    f.part,
                    f.principal_scales
                );
            }
        }
    }
}
