//! Disc-gated end-to-end test for the starting-bag `GIVE_ITEM` injection: splice a
//! guarded grant block (larger than the 7-slot direct-seed cap) into the opening
//! scene's entry script on a scratch copy of the disc, then re-decode the patched
//! MAN and confirm the edit is faithful - the block decodes at the entry-script
//! start as the guard test → the gives in order → the set, the guard skip lands
//! exactly past the block, the original entry-script bytes resume verbatim after it,
//! the MAN re-parses and recompresses within its footprint, the image is the same
//! size, and the patch is byte-deterministic. The *runtime* behaviour (granted once
//! at new game) needs a boot test. Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_asset::field_disasm::{self, FlagKind, InsnInfo};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::starting_bag::{self, SceneBagInject};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// A bag bigger than the 7-slot direct-seed cap: 5 convenience items + 4 random.
fn big_bag() -> Vec<(u8, u8)> {
    vec![
        (0x89, 10), // Door of Wind x10
        (0x8a, 10), // Incense x10
        (0xd1, 1),  // Speed Chain
        (0xf4, 1),  // Chicken Heart
        (0xfc, 1),  // Good Luck Bell
        (0x77, 5),  // Healing Leaf x5
        (0x7c, 3),  // Magic Leaf x3
        (0x88, 2),  // Door of Light x2
        (0x83, 4),  // Power Water x4
    ]
}

#[test]
fn starting_bag_injects_into_town01_and_round_trips() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let items = big_bag();
    let guard = starting_bag::DEFAULT_GUARD_BIT;
    let block = starting_bag::guarded_grant_block(&items, guard);
    let total_units: usize = items.iter().map(|&(_, c)| c as usize).sum();
    assert!(
        total_units > 7,
        "the test bag must exceed the 7-slot direct-seed cap"
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    // Capture the opening scene's ORIGINAL entry-script bytes (from pc0) so we can
    // prove the splice preserves them verbatim after the block.
    let report_dry = apply::apply_starting_bag(
        &mut DiscPatcher::open(original.clone()).expect("open"),
        &items,
        guard,
    )
    .expect("apply");
    let ext = report_dry.scene_entry.expect("opening scene located");
    let (orig_decoded, orig_pc0) = decode_entry_script(
        &DiscPatcher::open(original.clone())
            .unwrap()
            .read_entry(ext)
            .unwrap(),
        ext,
    );
    let orig_script_tail = orig_decoded[orig_pc0..].to_vec();

    let report = apply::apply_starting_bag(&mut patcher, &items, guard).expect("apply");
    assert!(
        report.applied,
        "the grant block injected into the opening scene"
    );
    assert_eq!(report.scene_entry, Some(ext));

    // Re-decode the patched scene: this validates the descriptor size word matches
    // the recompressed stream, and recomputes the entry-script pc0 - which is
    // unchanged (the record start didn't move) and now points at the injected block.
    let entry = patcher.read_entry(ext).expect("read patched entry");
    SceneBagInject::locate(&entry, ext).expect("re-locate patched scene");
    let (decoded, pc0) = decode_entry_script(&entry, ext);
    assert_eq!(pc0, orig_pc0, "entry-script pc0 unchanged by the prepend");

    // 1) guard TEST at pc0 → tests the guard bit, skip lands past the block.
    let test = field_disasm::decode(&decoded, pc0).expect("decode guard");
    match test.info {
        InsnInfo::SystemFlag {
            kind: FlagKind::Test,
            idx,
            target: Some(target),
            ..
        } => {
            assert_eq!(idx, guard, "guard tests the requested bit");
            assert_eq!(
                target,
                pc0 + block.len(),
                "guard skip lands exactly past the injected block"
            );
        }
        other => panic!("expected guard SystemFlag Test at pc0, got {other:?}"),
    }

    // 2) the gives, in slice order, one GiveItem per unit.
    let mut pc = pc0 + test.size;
    let expected: Vec<u8> = items
        .iter()
        .flat_map(|&(id, count)| std::iter::repeat_n(id, count as usize))
        .collect();
    for &id in &expected {
        let insn = field_disasm::decode(&decoded, pc).expect("decode give");
        match insn.info {
            InsnInfo::GiveItem { item_id } => assert_eq!(item_id, id, "give in order"),
            other => panic!("expected GiveItem at {pc:#x}, got {other:?}"),
        }
        pc += insn.size;
    }

    // 3) the closing SET, then the ORIGINAL entry-script first op resumes (the
    // scene's `Bgm`), proving the splice didn't clobber the original script.
    let set = field_disasm::decode(&decoded, pc).expect("decode set");
    assert!(
        matches!(
            set.info,
            InsnInfo::SystemFlag {
                kind: FlagKind::Set,
                idx,
                ..
            } if idx == guard
        ),
        "block ends with the guard SET"
    );
    pc += set.size;
    assert_eq!(pc, pc0 + block.len(), "block consumed exactly its length");
    // The original entry-script bytes resume verbatim after the block (the splice
    // prepended the block and left the rest of the record intact).
    assert_eq!(
        &decoded[pc..pc + orig_script_tail.len()],
        &orig_script_tail[..],
        "original entry script preserved verbatim after the injected block"
    );

    // 4) the patched image is the same size (an in-place PROT-entry edit).
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );

    // 5) determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::apply_starting_bag(&mut patcher2, &items, guard).expect("apply");
    assert!(patcher2.image() == patcher.image(), "deterministic");

    eprintln!(
        "starting-bag: injected {} item(s) ({} units, {}-byte block) into PROT entry {ext}",
        items.len(),
        total_units,
        block.len()
    );
}

/// An empty bag (all counts zero) injects nothing and leaves the disc untouched.
#[test]
fn empty_bag_is_a_no_op() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report =
        apply::apply_starting_bag(&mut patcher, &[(0x77, 0)], starting_bag::DEFAULT_GUARD_BIT)
            .expect("apply");
    assert!(!report.applied, "empty bag injects nothing");
    assert!(patcher.image() == original.as_slice(), "disc untouched");
}

/// Decode the opening scene's MAN out of a PROT entry and return `(decoded_man,
/// inject_offset)` - the offset `SceneBagInject` splices at (just past the entry
/// script's BGM op), used here to re-read the injected bytecode.
fn decode_entry_script(entry: &[u8], _ext: usize) -> (Vec<u8>, usize) {
    use legaia_asset::field_disasm;
    use legaia_asset::{man_section, scene_asset_table};
    let table = scene_asset_table::detect(entry).expect("scene table");
    let man_idx = table.descriptor_index(0x03).expect("MAN descriptor");
    let man = table.used()[man_idx];
    let body = &entry[man.data_offset as usize..];
    let (decoded, _) =
        legaia_lzs::decompress_tracked(body, man.size as usize).expect("decompress MAN");
    let mf = man_section::parse(&decoded).expect("parse MAN");
    let off = *mf.partitions[1].first().expect("partition-1 record 0");
    let rstart = mf.data_region_offset + off as usize;
    let locals = decoded[rstart] as usize;
    let pc0 = rstart + 1 + locals * 2 + 4;
    // Walk to just past the first BGM op (0x35) - the injection point.
    let mut pc = pc0;
    for _ in 0..64 {
        let insn = field_disasm::decode(&decoded, pc).expect("decode entry op");
        let next = pc + insn.size;
        if insn.opcode == 0x35 {
            return (decoded, next);
        }
        pc = next;
    }
    (decoded, pc0)
}
