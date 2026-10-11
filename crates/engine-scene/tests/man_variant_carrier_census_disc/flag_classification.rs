//! Flags classified by how the disc uses them: scratch-bank init guards, write-only cutscene
//! toggles, real local progress gates, and a doorway record with no variant.

use super::*;

/// `0x528` is NOT a story-progression flag - it is one bit of the
/// **`0x527..=0x52E` scratch-register bank** that field scripts bulk-reset.
/// The poll surfaced it as a churn=125 "[lead]", but it is scratch traffic.
///
/// The bank is reset as one contiguous eight-flag run
/// (`65 27 65 28 65 29 65 2A 65 2B 65 2C 65 2D 65 2E` = CLEAR `0x527..=0x52E`)
/// in ~70-90 scenes' scene-entry / per-actor setup - which is exactly the high
/// churn the poll sees on every scene transition.
///
/// CORRECTION (runtime-confirmed): an earlier static-only pass called `0x528`
/// "write-only to the VM" because the disc-wide opcode census reports ZERO
/// `0x70` TEST sites for it. **That was a census desync miss, not the truth.**
/// A read-watch capture (`autorun_flag_reader_watch.lua`, `LEGAIA_FLAG=0x528`,
/// balden) caught the field-VM TEST-opcode handler reading it 1951x in one idle
/// minute: `jal FUN_8003CE64` from `ra=0x801E35E8` (the op-`0x70` case; SET is
/// `ra=0x801E3598`, CLEAR `ra=0x801E35C0`). The consumer is balden's
/// scene-entry script, which TESTs `0x528` to guard its own scratch-bank reset
/// (`75 28 5D 00` immediately before the batch-clear run at balden MAN 0xAC54):
/// a per-scene init guard, not a story gate. The static census walker used to
/// desync past that `75 28` in balden's dialogue-heavy record (census TEST
/// count 0 - a floor, not the answer); the op-`0x4E` sub-op width fix (subs
/// 4..8 are the 7-byte compare-and-skip, not 3/5-byte jumps - see
/// `field_disasm::decode_subops`) resyncs the walker and the census now
/// surfaces the TEST family disc-wide, balden's guard included.
#[test]
fn flag_0x528_is_scratch_bank_tested_only_as_an_init_guard() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    let man_of = |name: &str| -> Vec<u8> {
        Scene::load(&index, name)
            .expect("load")
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let kinds = |flag: u16| -> (usize, usize, usize) {
        let sites = census.get(&flag).map(Vec::as_slice).unwrap_or(&[]);
        let n = |k: FlagKind| sites.iter().filter(|h| h.kind == k).count();
        (n(FlagKind::Set), n(FlagKind::Clear), n(FlagKind::Test))
    };
    let count_sub = |hay: &[u8], sub: &[u8]| hay.windows(sub.len()).filter(|w| *w == sub).count();
    // Genuine field-VM TEST: the op followed by a small `[lo, 0x00]` branch word.
    let genuine_tests = |hay: &[u8], op: [u8; 2]| -> usize {
        (0..hay.len().saturating_sub(3))
            .filter(|&o| hay[o] == op[0] && hay[o + 1] == op[1] && hay[o + 3] == 0x00)
            .count()
    };

    // 0x528 is written both ways (its 64 SETs cluster in balden).
    let (set, clr, _tst) = kinds(0x528);
    assert!(set > 0 && clr > 0, "0x528 is written (SET+CLEAR)");

    // The census used to UNDERCOUNT its TEST to zero (walker desync across
    // the mis-widthed 0x4E sub-ops); post-width-fix it surfaces the reader
    // family, including balden's `75 28` guard - the reader the runtime
    // capture confirmed (field-VM op-0x70 handler, ra 0x801E35E8).
    assert!(
        kinds(0x528).2 > 0,
        "census surfaces the 0x528 TEST family (the old zero was a desync floor)"
    );
    let balden = man_of("balden");
    assert!(
        genuine_tests(&balden, [0x75, 0x28]) >= 1,
        "balden genuinely TESTs 0x528 (byte-scan cross-check)"
    );

    // The bank is live scratch, not dead: sibling bits are TESTed too.
    for sib in [0x527u16, 0x52C, 0x52E] {
        assert!(kinds(sib).2 > 0, "sibling 0x{sib:X} is VM-tested scratch");
    }

    // Structural: the eight-flag batch-clear run resets the whole bank at once
    // (town01 P1[0]); in balden the TEST guards exactly this run.
    let batch: [u8; 16] = [
        0x65, 0x27, 0x65, 0x28, 0x65, 0x29, 0x65, 0x2A, 0x65, 0x2B, 0x65, 0x2C, 0x65, 0x2D, 0x65,
        0x2E,
    ];
    assert!(
        count_sub(&man_of("town01"), &batch) >= 1,
        "town01 bulk-clears the 0x527..=0x52E scratch bank"
    );
    assert!(
        count_sub(&balden, &batch) >= 1,
        "balden also carries the bank-reset run (guarded by the 0x528 TEST)"
    );
}

/// The other two poll "high-churn" leads - `0x5A1` and `0x6C3` - are also
/// write-only cutscene toggles, NOT gates. Both are zero-TEST to the field VM.
///
/// - **`0x5A1` is half of a flip-flop pair with `0x5A0`**: the two are toggled
///   mutually exclusively (`CLEAR 0x5A0 ; SET 0x5A1` = `65 A0 55 A1`, and the
///   reverse `55 A0 65 A1`), a two-state binary variable held as two flags
///   inside rayman's Hari cutscene records (`P2[7]`/`P2[19]`). Confined to
///   `rayman`; engine-read display/sequence state, never an inline script test.
///
/// - **`0x6C3` is a stone SOLVED-cutscene toggle** (`P2[5]`), write-only.
///   Its census hit list names six scenes, but that is a DESYNC artifact: the
///   flag operand byte `0xC3` collides with a repeating `C3 CC nn` data table
///   in the town-family / rayman records, so `56 C3` matches across a table
///   record boundary. Filtering to genuine ops (a `56 C3` NOT adjacent to a
///   `0xCC`) leaves `stone` as the only real user. A sharper form of the
///   `0x528` note: even the opcode-boundary census's SET/CLEAR *attributions*
///   can be desync false positives when the operand byte aliases table data.
#[test]
fn flags_0x5a1_and_0x6c3_are_write_only_cutscene_toggles() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    let man_of = |name: &str| -> Vec<u8> {
        Scene::load(&index, name)
            .expect("load")
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let test_count = |flag: u16| -> usize {
        census
            .get(&flag)
            .map(|s| s.iter().filter(|h| h.kind == FlagKind::Test).count())
            .unwrap_or(0)
    };
    let census_scenes = |flag: u16| -> BTreeSet<String> {
        census
            .get(&flag)
            .map(|s| s.iter().map(|h| h.scene_name.clone()).collect())
            .unwrap_or_default()
    };
    let count_sub = |hay: &[u8], sub: &[u8]| hay.windows(sub.len()).filter(|w| *w == sub).count();

    // 0x5A0 / 0x5A1: both write-only; 0x5A1 confined to rayman; the two toggle
    // as a mutually-exclusive flip-flop.
    assert_eq!(test_count(0x5A0), 0, "0x5A0 write-only");
    assert_eq!(test_count(0x5A1), 0, "0x5A1 write-only");
    assert_eq!(
        census_scenes(0x5A1),
        BTreeSet::from(["rayman".to_string()]),
        "0x5A1 lives only in rayman"
    );
    let rayman = man_of("rayman");
    assert!(
        count_sub(&rayman, &[0x65, 0xA0, 0x55, 0xA1]) > 0
            && count_sub(&rayman, &[0x55, 0xA0, 0x65, 0xA1]) > 0,
        "0x5A0/0x5A1 toggle as a flip-flop (both directions present)"
    );

    // 0x6C3: write-only, and genuinely stone-local despite the inflated census
    // scene list. A genuine op is a `56 C3` (SET 0x6C3) not adjacent to a 0xCC
    // (which would make it a `C3 CC nn` data-table boundary match).
    assert_eq!(test_count(0x6C3), 0, "0x6C3 write-only");
    let genuine_6c3 = |hay: &[u8]| -> usize {
        (0..hay.len().saturating_sub(1))
            .filter(|&o| hay[o] == 0x56 && hay[o + 1] == 0xC3)
            .filter(|&o| !(o >= 1 && hay[o - 1] == 0xCC || o + 2 < hay.len() && hay[o + 2] == 0xCC))
            .count()
    };
    assert!(
        genuine_6c3(&man_of("stone")) > 0,
        "0x6C3 real user is stone"
    );
    // `town01` / `town0b` each carry `56 C3` byte pairs that are all
    // `C3 CC nn` table boundaries, so both cases are non-vacuous. The list
    // used to name `gameover_data` as a third; that block has no MAN of its
    // own (its window is a subset of town01's with no asset-table bundle -
    // see `flag_549_reader_is_the_rim_elm_p2_gate`), so the "third" case was
    // re-checking town01's bytes under another name.
    for other in ["town01", "town0b"] {
        let man = man_of(other);
        assert!(
            count_sub(&man, &[0x56, 0xC3]) > 0,
            "{other} must actually carry `56 C3` pairs, else the next assert is vacuous"
        );
        assert_eq!(
            genuine_6c3(&man),
            0,
            "0x6C3 in {other} is a C3-CC data-table desync, not a real op"
        );
    }
}

/// Unlike the write-only churn flags, the last two poll leads - `0x32A` and
/// `0x590` - ARE real stone-LOCAL progress gates: both are SET and TESTed by
/// the field VM (a story gate is SET + TESTed; a scratch/toggle is
/// SET/CLEAR-many + zero-TEST). Checking for TEST sites first is what
/// separates them from `0x528`/`0x5A1`/`0x6C3`.
///
/// - **`0x32A` = the stone "code clue" marker**: SET (`53 2A`) at the
///   "Some sort of code is..." hint, TESTed three times in the SOLVED cutscene
///   (`P2[5]`) via the clean `73 2A 05 00 26` test-and-branch idiom. Distinct
///   from the 16 selector flags (`0x1D8..=0x1E7`) and the solved gate
///   (`0x1E8`). The census misses the SET (its walker desyncs in the
///   dialogue-heavy record) but decodes the three TESTs.
/// - **`0x590` = a stone scene-entry progress gate**: SET in a stone cutscene
///   (`P2[6]`), TESTed at scene-entry (`P1[0]`, right before the scratch-bank
///   clear) to branch opened-state behavior. Stone-LOCAL: the census's
///   `other7 P1[15]` "cross-scene TEST" is FALSIFIED - those `75 90` bytes sit
///   inside Shift-JIS dialogue (`75 90 B6 82 ...` = きてる), so the branch word
///   high byte is `0x82`, not `0x00`. A real field-VM TEST is followed by a
///   small `[lo, 0x00]` branch offset; that high-byte==0 test is the
///   real-op-vs-text-desync discriminator.
#[test]
fn stone_0x32a_and_0x590_are_real_local_progress_gates() {
    let Some(index) = open_index() else { return };
    let man_of = |name: &str| -> Vec<u8> {
        Scene::load(&index, name)
            .expect("load")
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let count_sub = |hay: &[u8], sub: &[u8]| hay.windows(sub.len()).filter(|w| *w == sub).count();
    // A genuine field-VM TEST: the two-byte TEST op followed by a `[lo, 0x00]`
    // branch word (real jump offsets are small; text desync gives a high byte).
    let genuine_tests = |hay: &[u8], op: [u8; 2]| -> usize {
        (0..hay.len().saturating_sub(3))
            .filter(|&o| hay[o] == op[0] && hay[o + 1] == op[1] && hay[o + 3] == 0x00)
            .count()
    };

    let stone = man_of("stone");
    // 0x32A: real SET (53 2A) + real TESTs (73 2A .. 00) in stone.
    assert!(count_sub(&stone, &[0x53, 0x2A]) >= 1, "stone SETs 0x32A");
    assert!(
        genuine_tests(&stone, [0x73, 0x2A]) >= 3,
        "stone TESTs 0x32A in the solved cutscene"
    );
    // 0x590: real SET (55 90) + real TEST (75 90 .. 00) in stone.
    assert!(count_sub(&stone, &[0x55, 0x90]) >= 1, "stone SETs 0x590");
    assert!(
        genuine_tests(&stone, [0x75, 0x90]) >= 1,
        "stone TESTs 0x590 at scene entry"
    );

    // Cross-scene reader FALSIFIED: other7's census 0x590 TESTs are Shift-JIS
    // text - `75 90` occurs, but never as a genuine `[lo, 0x00]` branch.
    let other7 = man_of("other7");
    assert!(
        count_sub(&other7, &[0x75, 0x90]) > 0,
        "other7 has the 75 90 byte pair (the census hit)"
    );
    assert_eq!(
        genuine_tests(&other7, [0x75, 0x90]),
        0,
        "but none are real TESTs - 0x590 is stone-local, not cross-scene"
    );
}

/// No later `keikoku` MAN replaces `P2[7]`, the Ravine's inner-doorway
/// record, and nothing outside it touches the record's own flags.
///
/// The Ravine block carries one MAN carrier - its bundle MAN - so the record
/// retail runs on every visit is this one. `P2[7]` has no header gate and
/// dispatches on its own flags: `0x142` (the Mt. Rikuroa post-Caruban latch)
/// splits it into a before half and an after half, and each half counts
/// visits on its own latch trio (`0x2BB..=0x2BD` / `0x2BE..=0x2C0`). Every
/// arm ends by walking the player back out of the doorway, so the doorway is
/// a story wall in every flag state the Ravine can be entered in (its `map01`
/// portals close on `0x193`). The latch trios have no writer anywhere else on
/// the disc.
#[test]
fn keikoku_inner_doorway_record_has_no_variant_and_no_foreign_writer() {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "keikoku").expect("load keikoku");
    let carriers = scene_man_carriers(&index, &scene);
    assert_eq!(carriers.len(), 1, "keikoku ships exactly one MAN carrier");
    assert!(!carriers[0].is_variant(), "and it is the bundle MAN");
    let man = &carriers[0].payload;
    let mf = legaia_scene_formats::man_section::parse(man).expect("parse keikoku MAN");
    let (c1, c2) = partition2_record_gates(&mf, man, 7).expect("P2[7] gates");
    assert!(
        c1.is_empty() && c2.is_empty(),
        "P2[7] carries no header gate"
    );

    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    let clean = |flag: u16, kind: FlagKind| -> BTreeSet<(String, usize, usize)> {
        census
            .get(&flag)
            .map(|v| {
                v.iter()
                    .filter(|h| h.kind == kind && h.clean && !h.text_alias && !h.debug_menu)
                    .map(|h| (h.scene_name.clone(), h.partition, h.record))
                    .collect()
            })
            .unwrap_or_default()
    };
    let p2_7: BTreeSet<(String, usize, usize)> = [("keikoku".to_string(), 2usize, 7usize)].into();
    assert!(
        clean(0x142, FlagKind::Test).contains(&("keikoku".to_string(), 2, 7)),
        "P2[7] dispatches on 0x142"
    );
    for flag in 0x2BBu16..=0x2C0 {
        assert_eq!(
            clean(flag, FlagKind::Set),
            p2_7,
            "0x{flag:X} is set by P2[7] only"
        );
        assert!(
            clean(flag, FlagKind::Clear).is_empty(),
            "0x{flag:X} has no clean clearer anywhere"
        );
    }
    eprintln!("[ran] keikoku P2[7]: one carrier, no gate, latches 0x2BB..=0x2C0 P2[7]-only");
}
