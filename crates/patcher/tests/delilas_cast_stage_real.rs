//! Disc oracle for the cast route's staged CASTER rows: after
//! `--delilas-party`, every mapped slot's record[0] rows `0x0A`/`0x0B`
//! decode as real packed streams carrying its sibling's wind-up and
//! payoff (the Che host asserted duration-exact), the Block clip
//! survives on row `0x06` in all four player files, the party-init
//! Block-reaction literal follows it, and the PROT 959 stage step stays
//! retail (no pin). Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::battle_char_assembly as bca;
use legaia_asset::party_swap::cast_stage;
use legaia_patcher::delilas_party::{
    CastRoutePolicy, DelilasMoveMode, PartyMapping, Sibling, apply_delilas_party,
};
use legaia_patcher::delilas_voice_fx::ArtsVoiceMode;
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    std::fs::read(path).ok()
}

/// `(entry offset, parts, frames)` of one record[0] action-table row.
fn row_shape(file: &[u8], slot: usize) -> (usize, usize, usize) {
    let block = bca::decode_record0(file).expect("decode record0");
    let off = u32::from_le_bytes(block[slot * 4..slot * 4 + 4].try_into().unwrap()) as usize;
    let s = off + bca::PLAYER_ANIM_STREAM_OFFSET;
    (off, block[s] as usize, block[s + 1] as usize)
}

#[test]
fn staged_cast_rows_carry_ches_clips_and_block_survives() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Retail row shapes, per player file, before the apply.
    let retail = DiscPatcher::open(original.clone()).expect("open retail");
    let retail_files: Vec<(usize, Vec<u8>)> = [863, 864, 865, 866]
        .into_iter()
        .map(|e| (e, retail.read_entry(e).expect("read player file")))
        .collect();
    for (entry, file) in &retail_files {
        let (_, p6, f6) = row_shape(file, cast_stage::BLOCK_ROW_RELOCATED);
        let (_, pa, fa) = row_shape(file, cast_stage::STAGE_ROW_WINDUP);
        let (_, pb, fb) = row_shape(file, cast_stage::BLOCK_ROW_RETAIL);
        assert_eq!(
            (p6, f6, pa, fa),
            (0, 0, 0, 0),
            "PROT {entry}: retail placeholders"
        );
        assert!(pb > 0 && fb > 0, "PROT {entry}: retail Block clip present");
    }

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    // The shipped mapping (`--delilas-party lu,gi,che`): Lu on the Vahn
    // file, Gi on Noa's, Che on Gala's. The full retail chains fit
    // these hosts' LZS budgets; the archetype-default mapping
    // (`gi,lu,che`) puts Lu's five-clip chain on Noa's tighter file,
    // where it legitimately degrades to the folded pair - so the hard
    // full-chain assertions below are pinned to THIS mapping.
    let mapping = PartyMapping {
        vahn: Sibling::Lu,
        noa: Sibling::Gi,
        gala: Sibling::Che,
    };
    let report = apply_delilas_party(
        &mut patcher,
        &mapping,
        ArtsVoiceMode::default(),
        DelilasMoveMode::default(),
        CastRoutePolicy::Install,
    )
    .expect("apply");
    assert!(report.changed);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("caster rows inserted")),
        "staged-row note missing: {:#?}",
        report.notes
    );

    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len(), "image length preserved");
    // Re-open validates EDC/ECC on every touched sector.
    let reopened = DiscPatcher::open(patched).expect("re-open patched");

    // Che's staged source clips: monster 163 entries 10/11, both 50
    // frames at rate 2 (= 200 ticks each). The duration-exact rung is
    // 25 frames at rate 1.
    let che_entry = mapping
        .pairs()
        .into_iter()
        .find(|&(_, _, _, _, s)| s == Sibling::Che)
        .map(|(e, _, _, _, _)| e)
        .unwrap();
    let live = reopened.read_entry(che_entry).expect("read Che host file");
    // The descriptor-table reclaim must leave the pack loader-shaped:
    // same record chain, same data base, table found through header
    // word 0 alone.
    let retail_che = &retail_files
        .iter()
        .find(|(e, _)| *e == che_entry)
        .unwrap()
        .1;
    let pack = legaia_asset::battle_data_pack::parse(&live).expect("patched pack parses");
    let retail_pack = legaia_asset::battle_data_pack::parse(retail_che).expect("retail pack");
    assert_eq!(
        pack.records.len(),
        retail_pack.records.len(),
        "record chain survives"
    );
    assert_eq!(pack.data_base, retail_pack.data_base, "data base survives");
    let anims = bca::battle_animations(&live).expect("decode patched animations");
    let bones = anims
        .iter()
        .find(|a| a.action_id == 0)
        .expect("idle")
        .part_count;
    for row in [cast_stage::STAGE_ROW_WINDUP, cast_stage::STAGE_ROW_PAYOFF] {
        let a = anims
            .iter()
            .find(|a| a.action_id == row as u8)
            .unwrap_or_else(|| panic!("row {row:#x} does not decode as a packed stream"));
        assert_eq!(a.part_count, bones, "row {row:#x} poses the whole skeleton");
        assert_eq!(
            (a.frame_count, a.rate),
            (25, 1),
            "row {row:#x} carries the duration-exact resample of Che's 50f clip"
        );
    }
    // The loader-stability law this layout exists for: everything from
    // `clut_a_off` on is battle-load scratch (CLUT/pixel upload, then
    // the five equip sub-records decode over it sequentially), so BOTH
    // staged rows must end at or below the patched `clut_a_off` - rows
    // any higher are destroyed before the first turn.
    {
        let (clut_a, clut_b) = cast_stage::record0_clut_offsets(&live).expect("patched header");
        let (retail_a, retail_b) =
            cast_stage::record0_clut_offsets(retail_che).expect("retail header");
        assert!(clut_a > retail_a, "clut_a_off shifted up by the insertion");
        assert_eq!(
            clut_b - clut_a,
            retail_b - retail_a,
            "payload A extent kept"
        );
        let block = bca::decode_record0(&live).unwrap();
        for row in [cast_stage::STAGE_ROW_WINDUP, cast_stage::STAGE_ROW_PAYOFF] {
            let (off, p, f) = row_shape(&live, row);
            let end = off + bca::PLAYER_ANIM_STREAM_OFFSET + 2 + p * f * 9;
            assert!(
                end <= clut_a,
                "row {row:#x} [{off:#x}..{end:#x}) must sit below the sub-record \
                 scratch base clut_a_off {clut_a:#x}"
            );
        }
        // The paired +0x5C sibling word follows the shift (retail
        // invariant: == clut_a_off - 4).
        let sib = u32::from_le_bytes(block[0x5C..0x60].try_into().unwrap()) as usize;
        assert_eq!(sib, clut_a - 4, "+0x5C sibling word tracks clut_a_off");
        assert_eq!(
            cast_stage::staged_state(&block, clut_a).expect("state"),
            cast_stage::StagedState::Applied,
            "patched file classifies as the loader-stable layout"
        );
        // The battle-load member init reads CLUT A / CLUT B out of
        // record0's decode at `clut_a_off` / `clut_b_off` before the
        // sub-records reuse that region as scratch. Both structs must
        // survive the insertion byte-identical at their shifted homes.
        // (The full five-sub palette walk of `battle_char_palette` is
        // NOT asserted here: its sub-offset derivation only models the
        // retail Vahn layout - retail Noa/Gala and any repacked file sit
        // outside the model. See the module docs' "Model scope" note.)
        let retail_block = bca::decode_record0(retail_che).unwrap();
        let clut_bytes = |b: &[u8], off: usize| -> Vec<u8> {
            let n = u16::from_le_bytes(b[off + 2..off + 4].try_into().unwrap()) as usize;
            b[off..off + 4 + n * 2].to_vec()
        };
        assert_eq!(
            clut_bytes(&block, clut_a),
            clut_bytes(&retail_block, retail_a),
            "CLUT A survives the shift byte-identical"
        );
        assert_eq!(
            clut_bytes(&block, clut_b),
            clut_bytes(&retail_block, retail_b),
            "CLUT B survives the shift byte-identical"
        );
    }

    // Block survives byte-identical on row 0x06 of every player file
    // (retail row 0x0B's entry, unmoved). On the Che host, row 0x0B now
    // belongs to the cast module.
    for (entry, retail_file) in &retail_files {
        let live = reopened.read_entry(*entry).expect("read patched file");
        let (retail_off, pb, fb) = row_shape(retail_file, cast_stage::BLOCK_ROW_RETAIL);
        let (relocated_off, p6, f6) = row_shape(&live, cast_stage::BLOCK_ROW_RELOCATED);
        assert_eq!(
            relocated_off, retail_off,
            "PROT {entry}: row 0x06 points at the retail Block entry"
        );
        assert_eq!(
            (p6, f6),
            (pb, fb),
            "PROT {entry}: Block clip shape survives"
        );
        let retail_block = bca::decode_record0(retail_file).unwrap();
        let live_block = bca::decode_record0(&live).unwrap();
        let len = bca::PLAYER_ANIM_STREAM_OFFSET + 2 + pb * fb * 9;
        assert_eq!(
            &retail_block[retail_off..retail_off + len],
            &live_block[relocated_off..relocated_off + len],
            "PROT {entry}: Block entry bytes survive unmoved"
        );
    }
    // Every routed host (all three mapped slots) gave rows 0x0A/0x0B to
    // its module: real packed streams over the full skeleton, kept
    // strictly below the sub-record scratch base, and the file
    // classifies as the loader-stable layout. Terra (no route) keeps
    // the retail placeholders.
    for (entry, _, _, _, sibling) in mapping.pairs() {
        if entry == che_entry {
            continue; // asserted in detail above
        }
        let live = reopened.read_entry(entry).expect("read patched file");
        let (clut_a, _) = cast_stage::record0_clut_offsets(&live).expect("patched header");
        let anims = bca::battle_animations(&live).expect("decode patched animations");
        let bones = anims
            .iter()
            .find(|a| a.action_id == 0)
            .expect("idle")
            .part_count;
        for row in [cast_stage::STAGE_ROW_WINDUP, cast_stage::STAGE_ROW_PAYOFF] {
            let a = anims
                .iter()
                .find(|a| a.action_id == row as u8)
                .unwrap_or_else(|| panic!("PROT {entry} row {row:#x} does not decode"));
            assert_eq!(
                a.part_count, bones,
                "PROT {entry} row {row:#x} poses the whole skeleton"
            );
            assert!(
                a.frame_count >= 1 && a.rate >= 1,
                "PROT {entry} row {row:#x} carries a real clip"
            );
            let (off, p, f) = row_shape(&live, row);
            let end = off + bca::PLAYER_ANIM_STREAM_OFFSET + 2 + p * f * 9;
            assert!(
                end <= clut_a,
                "PROT {entry} row {row:#x} sits below the scratch base"
            );
        }
        let block = bca::decode_record0(&live).unwrap();
        assert_eq!(
            cast_stage::staged_state(&block, clut_a).expect("state"),
            cast_stage::StagedState::Applied,
            "PROT {entry} classifies as the loader-stable layout"
        );
        // The FULL retail chain must be hosted (Gi 4 clips, Lu 5): the
        // un-folded module walks depend on the mid-chain entries the
        // head table does not bind.
        let want = match sibling {
            Sibling::Gi => 4,
            Sibling::Lu => 5,
            Sibling::Che => 2,
        };
        let offs = cast_stage::recover_entry_offsets(&block, clut_a, want)
            .unwrap_or_else(|e| panic!("PROT {entry}: full chain not authored: {e:#}"));
        for (i, &off) in offs.iter().enumerate() {
            let s = off + bca::PLAYER_ANIM_STREAM_OFFSET;
            let (p, f) = (block[s] as usize, block[s + 1] as usize);
            assert_eq!(
                p, bones,
                "PROT {entry} chain entry {i} poses the whole skeleton"
            );
            assert!(f >= 1, "PROT {entry} chain entry {i} carries frames");
        }
        // Lu's strike stage rides module 0960's keyframe-22 damage
        // cursor gate: it must carry at least 23 keyframes.
        if sibling == Sibling::Lu {
            let s = offs[3] + bca::PLAYER_ANIM_STREAM_OFFSET;
            assert!(
                block[s + 1] as usize >= 23,
                "Lu's strike stage under the cursor-gate floor"
            );
        }
    }
    // The un-folded modules' staging stores are cave calls (`jal` -
    // opcode 0x03 in the top six bits).
    for (prot, off) in [(958usize, 0x05B0usize), (960, 0x01C0)] {
        let module = reopened.read_entry(prot).expect("read module");
        let w = u32::from_le_bytes(module[off..off + 4].try_into().unwrap());
        assert_eq!(
            w >> 26,
            0x03,
            "PROT {prot} +{off:#x}: staging store hooked into a stage cave"
        );
    }
    {
        let live = reopened.read_entry(866).expect("read patched Terra file");
        let retail_file = &retail_files.iter().find(|(e, _)| *e == 866).unwrap().1;
        assert_eq!(
            row_shape(&live, cast_stage::STAGE_ROW_WINDUP),
            row_shape(retail_file, cast_stage::STAGE_ROW_WINDUP),
            "Terra row 0x0A untouched"
        );
        assert_eq!(
            row_shape(&live, cast_stage::BLOCK_ROW_RETAIL),
            row_shape(retail_file, cast_stage::BLOCK_ROW_RETAIL),
            "Terra row 0x0B untouched"
        );
    }

    // The party-init Block-reaction literal now stages row 0x06.
    let scus = reopened.read_named_file("SCUS_942.54").expect("SCUS");
    let off = legaia_asset::item_names::file_offset_for_va(&scus, 0x8005_4008).expect("VA map");
    assert_eq!(
        u32::from_le_bytes(scus[off..off + 4].try_into().unwrap()),
        0x2402_0006,
        "Block-reaction literal repointed to row 0x06"
    );

    // PROT 959 keeps its retail two-stage step (no pin): the lift arm's
    // staged-index increment at +0x0CAC.
    let module = reopened.read_entry(959).expect("read PROT 959");
    assert_eq!(
        u32::from_le_bytes(module[0x0CAC..0x0CB0].try_into().unwrap()),
        0x2442_0001,
        "the stage increment stays retail once real rows exist"
    );
}
