//! Disc-gated: the slot-B band's **DATA** verdict, checked against the disc.
//!
//! `docs/subsystems/cast-module.md`'s worklist grades most of the band's
//! routines **DATA** - "an arm switch whose arms do nothing but call
//! `FUN_80021B04` / `FUN_80050ED4` / `FUN_801DFDF0` / `FUN_80024E80` with a
//! module-resident record pointer" - and files them out of the port worklist
//! on that basis, as `[slot_b_spawn_stagers]` in
//! `scripts/ci/port-catalog-ignore.toml`. This test is what makes the filing
//! checkable: for each row it asserts, off the disc,
//!
//! 1. the routine's **owning image** is where the doc says it is, by finding
//!    the routine's own frame-matched extent in that image (a band image ends
//!    in a byte-identical copy of a sibling's tail, so the wrong image often
//!    carries the same bytes - the owner is the one PROT 0898's tables name,
//!    and it is the one this table records);
//! 2. the routine really is a spawn stager - it calls one of the four spawn
//!    primitives exactly as many times as the doc's "N spawn calls" cell says;
//! 3. it calls **no** damage wrapper (`FUN_801DD0AC` / `FUN_801DD4B0` /
//!    `FUN_801DD6B4`), which is the line between DATA and PORT; and
//! 4. `legaia_asset::cast_effect_pool` recovers the module's spawn records, so
//!    the DATA layer the verdict promises is actually staged.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::CastEffectPool;
use std::path::PathBuf;

/// Slot-B link base: every band image loads here.
const LINK_BASE: u32 = 0x801F_69D8;

/// The four spawn primitives a DATA arm may call.
const SPAWN: [u32; 4] = [0x8002_1B04, 0x8005_0ED4, 0x801D_FDF0, 0x8002_4E80];
/// The three damage wrappers a DATA arm may **not** call.
const WRAPPERS: [u32; 3] = [0x801D_D0AC, 0x801D_D4B0, 0x801D_D6B4];

/// `(routine VA, owning PROT entry, `jal`s into a spawn primitive)` - the
/// **DATA** rows of `docs/subsystems/cast-module.md`'s verdict table.
///
/// The count is `jal` sites, not records: PROT 0915's stager reaches one
/// shared call from two arms with two different record pointers.
const DATA_ROWS: [(u32, u32, usize); 47] = [
    (0x801F_8EAC, 904, 1),
    (0x801F_8078, 905, 2),
    (0x801F_7FA8, 907, 2),
    (0x801F_8310, 908, 5),
    (0x801F_89D4, 910, 1),
    (0x801F_7FE8, 911, 1),
    (0x801F_835C, 912, 3),
    (0x801F_864C, 913, 3),
    (0x801F_7A80, 914, 2),
    (0x801F_7F34, 915, 1),
    (0x801F_88F8, 916, 7),
    (0x801F_82D8, 917, 3),
    (0x801F_8578, 919, 4),
    (0x801F_800C, 921, 2),
    (0x801F_7820, 924, 1),
    (0x801F_7AE8, 925, 3),
    (0x801F_8E68, 928, 12),
    (0x801F_8C30, 929, 10),
    (0x801F_7EA4, 930, 8),
    (0x801F_8ADC, 931, 1),
    (0x801F_84A4, 932, 1),
    (0x801F_8748, 933, 2),
    (0x801F_92AC, 934, 11),
    (0x801F_7FF0, 935, 1),
    (0x801F_7BD0, 936, 4),
    (0x801F_7850, 937, 5),
    (0x801F_7AB8, 938, 3),
    (0x801F_7DB0, 941, 2),
    (0x801F_8118, 942, 2),
    (0x801F_769C, 943, 3),
    (0x801F_7F2C, 944, 3),
    (0x801F_776C, 945, 2),
    (0x801F_76C4, 946, 5),
    (0x801F_8504, 948, 1),
    (0x801F_8208, 950, 1),
    (0x801F_81DC, 951, 4),
    (0x801F_9370, 955, 1),
    (0x801F_7EC4, 956, 5),
    (0x801F_99F4, 957, 3),
    (0x801F_8D30, 958, 7),
    (0x801F_8250, 959, 11),
    (0x801F_86B0, 960, 2),
    (0x801F_78A4, 961, 6),
    (0x801F_813C, 962, 3),
    (0x801F_81A0, 963, 4),
    (0x801F_8BF8, 964, 2),
    (0x801F_7B74, 965, 2),
];

/// The band's two record-less entries (`cast_effect_pool_disc.rs` names the
/// same pair): PROT 0926 is the 1-sector null stub, and PROT 0952's two spawn
/// sites load `a2` out of a saved register no static window can see. Neither
/// is a DATA row, so neither appears above - this is only here so a reader
/// does not go looking for them.
const RECORDLESS: [u32; 2] = [926, 952];

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    None
}

fn read_entry(dir: &std::path::Path, idx: u32) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(idx as usize)
        .cloned()
        .unwrap_or_else(|| panic!("PROT {idx} entry"));
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .unwrap_or_else(|e| panic!("read PROT {idx}: {e:#}"));
    bytes
}

fn word_at(bytes: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(off..off + 4)?.try_into().ok()?,
    ))
}

/// Frame-match one routine's extent: `addiu sp, sp, -F` at `va`, running to
/// the first `jr ra` whose delay slot is `addiu sp, sp, +F`.
///
/// The Puera stager (`0x801F90E4`) is the one band routine whose prologue
/// opens in a branch delay slot, so the scan also accepts a frame opened one
/// instruction late - it is not in the DATA set, but the helper stays honest.
fn frame_extent(bytes: &[u8], va: u32) -> Option<(usize, usize, u32)> {
    let start = (va - LINK_BASE) as usize;
    for lead in [0usize, 4] {
        let head = word_at(bytes, start + lead)?;
        if head >> 16 != 0x27BD {
            continue;
        }
        let imm = head & 0xFFFF;
        if imm & 0x8000 == 0 {
            continue;
        }
        let frame = 0x1_0000 - imm;
        let mut off = start + lead + 4;
        while off + 8 <= bytes.len() {
            if word_at(bytes, off)? == 0x03E0_0008
                && word_at(bytes, off + 4)? == (0x27BD_0000 | frame)
            {
                return Some((start, off + 8, frame));
            }
            off += 4;
        }
    }
    None
}

/// `jal` targets inside `[start, end)`.
fn jal_targets(bytes: &[u8], start: usize, end: usize) -> Vec<u32> {
    let mut out = Vec::new();
    let mut off = start;
    while off + 4 <= end.min(bytes.len()) {
        let w = word_at(bytes, off).unwrap_or(0);
        if w >> 26 == 3 {
            out.push(((w & 0x03FF_FFFF) << 2) | 0x8000_0000);
        }
        off += 4;
    }
    out
}

#[test]
fn every_data_row_is_a_spawn_stager_the_pool_covers() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] slot-B DATA rows: no LEGAIA_DISC_BIN / extracted/");
        return;
    };

    assert_eq!(DATA_ROWS.len(), 47, "the doc's DATA verdict count");
    let mut pool = CastEffectPool::new();
    let mut total_spawn = 0usize;

    for (va, owner, doc_spawn) in DATA_ROWS {
        let bytes = read_entry(&dir, owner);
        let (start, end, frame) = frame_extent(&bytes, va).unwrap_or_else(|| {
            panic!("{va:#010X}: no frame-matched routine in its owning PROT {owner}")
        });
        assert!(frame > 0 && end > start);

        let jals = jal_targets(&bytes, start, end);
        let spawn = jals.iter().filter(|t| SPAWN.contains(t)).count();
        assert_eq!(
            spawn, doc_spawn,
            "{va:#010X} (PROT {owner}): spawn-call count in the bytes vs the \
             verdict table's cell"
        );
        total_spawn += spawn;

        // The DATA/PORT line: a DATA arm rolls no damage.
        for w in WRAPPERS {
            assert!(
                !jals.contains(&w),
                "{va:#010X} (PROT {owner}) calls the damage wrapper {w:#010X} - \
                 that makes it a PORT row, not a DATA row"
            );
        }

        // ... and the pool really stages the module's records.
        assert!(pool.insert(owner, &bytes), "PROT {owner} is in the band");
        let module = pool.module(owner).expect("just inserted");
        assert!(
            module.spawn_sites >= spawn,
            "PROT {owner}: the pool found {} spawn sites across the whole \
             image, fewer than the {spawn} in this one routine",
            module.spawn_sites
        );
        assert!(
            !module.parts.is_empty(),
            "PROT {owner}: a DATA verdict with no recovered record is a real \
             parser gap"
        );
    }

    for entry in RECORDLESS {
        assert!(
            !DATA_ROWS.iter().any(|(_, o, _)| *o == entry),
            "PROT {entry} carries no record, so it cannot be a DATA row"
        );
    }

    println!(
        "[ok] slot-B DATA rows: {} routines across {} modules, {total_spawn} spawn calls, \
         0 damage wrappers",
        DATA_ROWS.len(),
        pool.len()
    );
}
