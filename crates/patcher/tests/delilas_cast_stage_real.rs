//! Disc oracle for the cast route's staged CASTER rows: after
//! `--delilas-party`, the Che-mapped slot's record[0] rows `0x0A`/`0x0B`
//! decode as real packed streams carrying Che's wind-up and smash, the
//! Block clip survives on row `0x06` in all four player files, the
//! party-init Block-reaction literal follows it, and the PROT 959 stage
//! step stays retail (no pin). Skips (and passes) when `LEGAIA_DISC_BIN`
//! is unset.

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
    let mapping = PartyMapping::default(); // Che on the Gala slot (865)
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
            .any(|n| n.contains("caster rows authored")),
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
    // The Che host gave rows 0x0A/0x0B to the module; the other files
    // keep them retail.
    for (entry, retail_file) in &retail_files {
        if *entry == che_entry {
            continue;
        }
        let live = reopened.read_entry(*entry).expect("read patched file");
        assert_eq!(
            row_shape(&live, cast_stage::STAGE_ROW_WINDUP),
            row_shape(retail_file, cast_stage::STAGE_ROW_WINDUP),
            "PROT {entry}: row 0x0A untouched"
        );
        assert_eq!(
            row_shape(&live, cast_stage::BLOCK_ROW_RETAIL),
            row_shape(retail_file, cast_stage::BLOCK_ROW_RETAIL),
            "PROT {entry}: row 0x0B untouched"
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
