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
use legaia_patcher::delilas_party::{CastRoutePolicy, PartyMapping, apply_delilas_party};
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_patcher::enemy_anim_mirror::{
    RetailSources, apply_enemy_anim_mirror, staged_entries, staged_plan,
};

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
        CastRoutePolicy::Install,
    )
    .expect("apply party");

    // `apply_delilas_party` runs the enemy-anim mirror itself, so the
    // patcher image is already mirrored here - reading it back as a
    // "swapped but unmirrored" baseline would hand the assertions their
    // own output (the mirror is a pure function of the retail sources,
    // so the standalone pass below reproduces it byte for byte). The
    // true pre-mirror baseline needs no capture at all: the model swap
    // replaces only the mesh + texture pool and leaves the entry region
    // byte-identical to retail, so the RETAIL block's entries are the
    // pre-mirror entries. The standalone pass below is the idempotence
    // half of the oracle.
    apply_enemy_anim_mirror(&mut patcher, &mapping, &retail).expect("mirror");
    let mirrored_archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("mirrored archive");

    for (_, _, _, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let (staged, close) = staged_entries(sibling);
        let staged_all: Vec<usize> = staged.iter().copied().chain(close).collect();

        let retail_block = ma::decode_block(&retail_archive, id).unwrap().unwrap();
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

        // Staged entries: streams CHANGED vs the pre-mirror (= retail)
        // entry region, tags preserved, and every module-staged entry
        // holds its plan floor (the 0960 payoff cursor gate at 23
        // frames; retail's own 11-frame smallest staged entry
        // elsewhere).
        let plan = staged_plan(sibling);
        let retail_spans = entry_spans(&retail_block);
        for &i in &staged_all {
            let (off, span, tag, frames) = spans[i];
            let (soff, sspan, stag, _) = retail_spans[i];
            assert_eq!(tag, stag, "{who} staged entry {i}: tag preserved");
            let floor = plan
                .chain
                .iter()
                .position(|&c| c == i)
                .map(|k| plan.chain_floors[k])
                .unwrap_or(plan.close_floor);
            assert!(
                frames >= floor,
                "{who} staged entry {i}: {frames} frames < the {floor}-keyframe floor"
            );
            let new_stream = &block[off + 0x8C..off + span];
            let old_stream = &retail_block[soff + 0x8C..soff + sspan];
            assert_ne!(
                new_stream, old_stream,
                "{who} staged entry {i}: stream did not change"
            );
        }
        // The measured cursor gate: Lu's module (0960) damage tick needs
        // the playing clip's cursor to reach keyframe 22, whichever hero
        // wears her block. Under the folded stage walk that tick rides
        // the restaged wind-up row (chain[0]).
        if sibling == legaia_patcher::delilas_party::Sibling::Lu {
            let gated = plan.chain[0];
            assert!(
                spans[gated].3 >= 23,
                "{who}: Lu-block wind-up entry {gated} under the 0960 cursor gate"
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
        let (soff, sspan, ..) = retail_spans[0];
        assert_ne!(
            &block[off + 0x8C..off + span],
            &retail_block[soff + 0x8C..soff + sspan],
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
        CastRoutePolicy::Install,
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

/// Every legal mapping permutation fits: the mirror must land (no
/// budget skip) on all six assignments of the three siblings to the
/// three hero slots. Stages the post-swap state directly (swap, rename,
/// slot patch - what `apply_delilas_party`'s model loop leaves behind)
/// without paying for the full apply per permutation.
#[test]
fn every_mapping_permutation_fits_the_slot_budget() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open disc");
    let retail_archive = base
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    let players: Vec<Vec<u8>> = (863..=865)
        .map(|e| base.read_entry_footprint(e).expect("player"))
        .collect();
    let readef = base.read_entry_footprint(894).expect("readef");
    let retail = RetailSources {
        archive: &retail_archive,
        players: [&players[0], &players[1], &players[2]],
        readef: &readef,
    };

    // The nine (hero slot, block) swapped+renamed slots, cached once.
    let rigs = [
        &party_swap::RIG_VAHN_GALA,
        &party_swap::RIG_NOA,
        &party_swap::RIG_VAHN_GALA,
    ];
    let names = ["Vahn", "Noa", "Gala"];
    let mut swapped: std::collections::BTreeMap<(usize, u16), Vec<u8>> = Default::default();
    for (slot, name) in names.iter().enumerate() {
        for id in [162u16, 163, 164] {
            let out = party_swap::swap_into_block(&players[slot], rigs[slot], &retail_archive, id)
                .unwrap_or_else(|e| panic!("{name} -> {id}: {e:#}"));
            let mut block = out.block;
            legaia_patcher::delilas_party::rename_block(&mut block, name)
                .unwrap_or_else(|e| panic!("{name} -> {id}: rename: {e:#}"));
            let slot_bytes =
                ma::encode_slot(&block).unwrap_or_else(|e| panic!("{name} -> {id}: encode: {e:#}"));
            swapped.insert((slot, id), slot_bytes);
        }
    }

    let mut patcher = DiscPatcher::open(original).expect("open disc for perms");
    for perm in [
        "gi,lu,che",
        "gi,che,lu",
        "lu,gi,che",
        "lu,che,gi",
        "che,gi,lu",
        "che,lu,gi",
    ] {
        let mapping = PartyMapping::parse(perm).expect("mapping");
        // Stage the post-swap state for this permutation.
        for (_, _, slot, _, sibling) in mapping.pairs() {
            let id = sibling.monster_id();
            patcher
                .patch_monster_slot(id, &swapped[&(slot, id)])
                .expect("stage swapped slot");
        }
        let notes = apply_enemy_anim_mirror(&mut patcher, &mapping, &retail)
            .unwrap_or_else(|e| panic!("{perm}: mirror: {e:#}"));
        assert!(
            !notes.iter().any(|n| n.contains("(budget)")),
            "{perm}: a block kept the sibling's clips: {notes:?}"
        );
        // Report the fit per block.
        let cur = patcher
            .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
            .expect("archive after");
        for (_, _, _, who, sibling) in mapping.pairs() {
            let id = sibling.monster_id();
            let slot_off = (id as usize - 1) * ma::SLOT_STRIDE;
            let slot = &cur[slot_off..slot_off + ma::SLOT_STRIDE];
            let used = slot.iter().rposition(|&b| b != 0).unwrap_or(0) + 1;
            let ladder: Vec<&String> = notes
                .iter()
                .filter(|n| n.starts_with(&format!("{who} (monster {id})")))
                .filter(|n| n.contains("ladder step") || n.contains("density"))
                .collect();
            eprintln!(
                "[fit] {perm}: {who} on monster {id}: slot used {used:#x} \
                 (headroom {}){}",
                ma::SLOT_STRIDE - used,
                if ladder.is_empty() {
                    String::new()
                } else {
                    format!(" ladder: {ladder:?}")
                }
            );
            // Floors hold under every permutation.
            let block = ma::decode_block(&cur, id).unwrap().unwrap();
            let spans = entry_spans(&block);
            let plan = staged_plan(sibling);
            for (k, &i) in plan.chain.iter().enumerate() {
                assert!(
                    spans[i].3 >= plan.chain_floors[k],
                    "{perm}: {who} on {id} staged entry {i} under floor"
                );
            }
            if let Some(c) = plan.close {
                assert!(spans[c].3 >= plan.close_floor);
            }
        }
    }
}

/// The signature special runs as a PHYSICAL attack of the staged chain,
/// not the capture-class cast: the AI picker's Delilas case body
/// (overlay 0898) is rewritten IN PLACE - no injection arena - so it no
/// longer writes `+0x1DE = 2` / `+0x1DF = monster_id - 0x29` but a
/// category-3 strike queue of exactly the staged chains, and every
/// chain stage of every mirrored block carries a grafted contact head
/// (event beat + impact-spawn effect record) where retail shipped none.
/// Baselines pin where the cast decision really lives: the RETAIL
/// blocks carry no capture spell id anywhere in their action data - the
/// id is overlay code - so the retail arm words are the non-vacuity
/// anchor.
#[test]
fn signature_special_becomes_a_physical_attack_of_the_staged_chain() {
    use legaia_patcher::delilas_party::Sibling;
    use legaia_patcher::delilas_signature_attack as sig;

    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mapping = PartyMapping::parse("che,lu,gi").expect("mapping");
    let mut patcher = DiscPatcher::open(original).expect("open disc");

    let word_at =
        |bytes: &[u8], off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());

    // --- Retail baselines -------------------------------------------------
    // The cast decision lives in the OVERLAY: the retail case body still
    // holds the cast write, including the `monster_id - 0x29` literal.
    let overlay = patcher
        .read_entry(sig::BATTLE_OVERLAY_PROT)
        .expect("battle overlay");
    let arm_off = (sig::ARM_VA - sig::BATTLE_OVERLAY_BASE) as usize;
    let retail_words: Vec<u32> = (0..sig::ARM_WORDS)
        .map(|i| word_at(&overlay, arm_off + i * 4))
        .collect();
    assert_eq!(
        retail_words,
        sig::RETAIL_ARM.to_vec(),
        "retail Delilas case body carries the spell-cast write"
    );
    assert_eq!(
        word_at(&overlay, arm_off + 21 * 4),
        0x2442_FFD7,
        "the id literal"
    );
    // And the BLOCKS carry no capture spell id in their action data:
    // neither a `+0x4C` entry id nor a `+0x21..+0x23` magic-attack slot.
    let retail_archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    let assert_no_capture_ids = |archive: &[u8], label: &str| {
        for sib in [Sibling::Gi, Sibling::Che, Sibling::Lu] {
            let id = sib.monster_id();
            let r = ma::record(archive, id).unwrap().unwrap();
            assert!(
                r.spells.iter().all(|s| !(0x79..=0x7B).contains(&s.id)),
                "{label} block {id}: a spell entry carries a capture id"
            );
            assert!(
                r.magic_attacks.is_empty(),
                "{label} block {id}: unexpected global magic-attack slots"
            );
        }
    };
    assert_no_capture_ids(&retail_archive, "retail");

    // --- Apply the full mod ----------------------------------------------
    apply_delilas_party(
        &mut patcher,
        &mapping,
        Default::default(),
        Default::default(),
        CastRoutePolicy::Install,
    )
    .expect("apply party");

    // --- The case body is the in-place physical-attack arm, queueing
    // exactly the module-staged chains ------------------------------------
    let overlay2 = patcher
        .read_entry(sig::BATTLE_OVERLAY_PROT)
        .expect("patched overlay");
    let patched_words: Vec<u32> = (0..sig::ARM_WORDS)
        .map(|i| word_at(&overlay2, arm_off + i * 4))
        .collect();
    let queues = sig::strike_queues([
        staged_plan(Sibling::Gi).chain,
        staged_plan(Sibling::Che).chain,
        staged_plan(Sibling::Lu).chain,
    ])
    .expect("queues from the staged plans");
    assert_eq!(
        patched_words,
        sig::arm_replacement(&queues).to_vec(),
        "the case body is the physical-attack arm built from the staged plans"
    );
    assert!(
        !patched_words.contains(&0x2442_FFD7),
        "the cast id literal is gone"
    );

    // --- The mirrored blocks: still no capture ids, and every chain
    // stage carries a contact head where retail shipped none ---------------
    let mirrored = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("mirrored archive");
    assert_no_capture_ids(&mirrored, "patched");
    for (_, _, _, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let block = ma::decode_block(&mirrored, id).unwrap().unwrap();
        let retail_block = ma::decode_block(&retail_archive, id).unwrap().unwrap();
        let spans = entry_spans(&block);
        let retail_spans = entry_spans(&retail_block);
        let plan = staged_plan(sibling);
        for &stage in plan.chain {
            let (off, _, _, frames) = spans[stage];
            // Baseline: retail gives the tag-0x23 stages no contact beat
            // at all (the cast module did its own damage ticks), so a
            // populated beat + impact record there can only be the
            // graft. Non-0x23 staged rows (Lu's folded wind-up row is a
            // retail tag-0x12 castable) legitimately carry their own
            // retail beat/effect records - for those the head-changed
            // assertion below is the graft evidence.
            let (roff, _, rtag, _) = retail_spans[stage];
            if rtag == 0x23 {
                assert_eq!(
                    retail_block[roff + 0x10],
                    0,
                    "{who} stage {stage}: retail tag-0x23 staged entry unexpectedly has a beat"
                );
                assert_eq!(
                    retail_block[roff + 0x15],
                    0,
                    "{who} stage {stage}: retail staged entry unexpectedly has an effect record"
                );
            }
            assert_ne!(
                &block[off + 0x10..off + 0x54],
                &retail_block[roff + 0x10..roff + 0x54],
                "{who} stage {stage}: head unchanged from retail (graft vacuous)"
            );
            let ev = block[off + 0x10] as usize;
            assert!(
                ev >= 1 && ev < frames,
                "{who} stage {stage}: contact beat {ev} outside 1..{frames}"
            );
            let gate = block[off + 0x14] as usize;
            let effect = block[off + 0x15];
            assert!(
                gate >= 1 && gate < frames,
                "{who} stage {stage}: effect gate {gate} outside 1..{frames}"
            );
            assert_ne!(effect, 0, "{who} stage {stage}: impact record missing");
        }
    }

    // --- Idempotence + EDC validity ---------------------------------------
    // A second standalone mirror pass accepts its own overlay write and
    // reproduces every touched region byte for byte. Its retail sources
    // come from a fresh retail image (the patcher's own copies are
    // patched by now).
    let fresh = load_disc().expect("disc still readable");
    let retail_patcher = DiscPatcher::open(fresh).expect("open retail");
    let players: Vec<Vec<u8>> = (863..=865)
        .map(|e| retail_patcher.read_entry_footprint(e).expect("player"))
        .collect();
    let readef = retail_patcher.read_entry_footprint(894).expect("readef");
    let retail_sources = RetailSources {
        archive: &retail_archive,
        players: [&players[0], &players[1], &players[2]],
        readef: &readef,
    };
    apply_enemy_anim_mirror(&mut patcher, &mapping, &retail_sources).expect("mirror rerun");
    assert_eq!(
        patcher.read_entry(sig::BATTLE_OVERLAY_PROT).unwrap(),
        overlay2,
        "overlay idempotence"
    );
    assert_eq!(
        patcher.read_entry_footprint(MONSTER_ARCHIVE_ENTRY).unwrap(),
        mirrored,
        "archive idempotence"
    );

    // The touched overlay sector stays EDC/ECC-valid (the arm rewrite is
    // the only code edit this feature makes).
    use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
    let img = patcher.image();
    assert_eq!(img.len() % SECTOR_SIZE, 0);
    let ov_lba = patcher
        .entry_disc_lba(sig::BATTLE_OVERLAY_PROT)
        .expect("overlay lba") as usize;
    let s = (ov_lba + arm_off / USER_DATA_SIZE) * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[s..s + SECTOR_SIZE]),
        "overlay arm sector EDC/ECC"
    );
}
