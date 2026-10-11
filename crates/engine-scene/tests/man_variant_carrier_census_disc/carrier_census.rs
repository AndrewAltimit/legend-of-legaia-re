//! The carrier set itself: how many streaming variant carriers the disc has, that no two share
//! bytes, and the cross-check that revalidates every mined flag against it.

use super::*;

/// The thirteen streaming variant carriers, pinned by `(scene, entry_idx)`.
#[test]
fn thirteen_streaming_variant_carriers_disc_wide() {
    let Some(index) = open_index() else { return };
    let mut found: BTreeSet<(String, u32)> = BTreeSet::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for c in scene_man_carriers(&index, &scene) {
            if c.is_variant() {
                found.insert((name.clone(), c.entry_idx));
            }
        }
    }
    eprintln!("[carriers] {found:?}");
    let want: BTreeSet<(String, u32)> = [
        ("dolk2", 70u32),
        ("rikuroa2", 122),
        ("rikuroa", 157),
        ("rayman", 201),
        ("station", 228),
        ("balden2", 320),
        ("ropeway2", 339),
        ("taiku", 373),
        ("doman", 401),
        ("taiku2", 427),
        ("nilboa2", 648),
        ("edbalden", 792),
        ("eddoman", 817),
    ]
    .into_iter()
    .map(|(s, i)| (s.to_string(), i))
    .collect();
    assert_eq!(
        found, want,
        "streaming variant MAN carrier set (scene, entry_idx)"
    );
}

/// No two MAN carriers anywhere on the disc share bytes.
///
/// This is the general form of the `gameover_data` correction above, and the
/// property every MAN-keyed census silently depends on: if one MAN can be
/// reached through two scenes' windows, every per-scene count over it is
/// inflated by an amount no assertion in that census can see. It is the
/// failure mode the old declared-span PROT entry size produced across the
/// corpus - a "second copy" of a MAN that was the same sectors under another
/// block's name.
///
/// Each carrier being byte-unique means a per-scene census is a partition of
/// the corpus, so `N scenes carry X` and `N carriers carry X` are the same
/// statement. If this ever fails, re-read every count in this file before
/// trusting it.
#[test]
fn no_two_man_carriers_share_bytes_disc_wide() {
    let Some(index) = open_index() else { return };
    // payload -> the (scene, entry) it was first seen under.
    let mut seen: std::collections::HashMap<Vec<u8>, (String, u32)> =
        std::collections::HashMap::new();
    let mut carriers = 0usize;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            carriers += 1;
            if let Some((prev_scene, prev_entry)) =
                seen.insert(carrier.payload.clone(), (name.clone(), carrier.entry_idx))
            {
                panic!(
                    "MAN payload shared by {prev_scene} (PROT {prev_entry}) and {name} \
                     (PROT {}) - one MAN reached through two windows inflates every \
                     per-scene census over it",
                    carrier.entry_idx
                );
            }
        }
    }
    eprintln!("[disc] {carriers} MAN carriers, {} distinct", seen.len());
    assert_eq!(carriers, seen.len(), "carriers must be byte-unique");
    assert!(carriers > 90, "sanity: the corpus has ~100 MAN carriers");
}

/// Desync cross-check over every mined flag: the systemic guard the `0x528`
/// correction earned. [`system_flag_census`]'s opcode walker is NOT
/// authoritative for TEST in dialogue-heavy records - it desyncs into
/// Shift-JIS text and silently drops real ops. The walker-independent
/// [`flag_test_bytescan`] (the `[0x70|hi][lo][blo][0x00]` branch idiom)
/// cross-checks it. This test re-validates all seven poll minings against that
/// side channel; it FAILS if any flag we asserted "write-only" elsewhere turns
/// out to carry a hidden TEST reader (a second `0x528`).
///
/// Key results (see [`flag_test_bytescan`] for the noise-floor model):
/// - **Write-only is robust, not a desync floor**: `0x5A0` / `0x5A1` / `0x6C3`
///   have `raw == 0` - the TEST byte-pair is *absent disc-wide*, so no reader
///   can hide. This is the strongest possible confirmation of the cutscene-
///   toggle finding, far beyond "census TEST == 0".
/// - **`0x528` is the lone census-zero-but-real case** (already corrected +
///   firehose-confirmed): census TEST == 0 yet `genuine` sits ~67 vs a `0.4`
///   noise floor. The scratch bank `0x527..=0x52E` is where both static tools
///   disagree wildly - inherently runtime-only, consistent with scratch churn.
/// - **The real gates confirm**: the stone puzzle (`0x1D8..=0x1E8`), rayman
///   Haris hub (`0x1FD..=0x1FF`), and stone-local `0x32A` / `0x590` all show
///   `genuine` far above their (~0) noise floors; `0x1E8` even surfaces its
///   `map02` cross-scene reader.
#[test]
fn desync_crosscheck_revalidates_every_mined_flag() {
    let Some(index) = open_index() else { return };
    let scenes: Vec<String> = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    // Cache every scene's concatenated MAN-carrier payloads once (a 0xFF
    // separator keeps a cross-carrier window from matching a spurious pair).
    let mut man_cache: Vec<(String, Vec<u8>)> = Vec::new();
    for name in &scenes {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let mut buf: Vec<u8> = Vec::new();
        for c in scene_man_carriers(&index, &scene) {
            buf.extend_from_slice(&c.payload);
            buf.push(0xFF);
        }
        if !buf.is_empty() {
            man_cache.push((name.clone(), buf));
        }
    }
    assert!(!man_cache.is_empty(), "no MAN carriers decoded");

    // Disc-wide (raw, genuine) for a flag's TEST branch idiom.
    let bytescan = |flag: u16| -> (usize, usize) {
        man_cache.iter().fold((0, 0), |(r, g), (_, hay)| {
            let (dr, dg) = flag_test_bytescan(hay, flag);
            (r + dr, g + dg)
        })
    };
    let census_test = |flag: u16| -> usize {
        census
            .get(&flag)
            .map(|s| s.iter().filter(|h| h.kind == FlagKind::Test).count())
            .unwrap_or(0)
    };

    // (1) THE GUARD: every flag asserted "write-only" elsewhere must have a
    // truly absent TEST byte-pair disc-wide (raw == 0). A future mining that
    // mislabels a real gate as write-only trips this.
    for wo in [0x5A0u16, 0x5A1, 0x6C3] {
        let (raw, genuine) = bytescan(wo);
        assert_eq!(
            (raw, genuine),
            (0, 0),
            "0x{wo:X} claimed write-only but its TEST byte-pair appears disc-wide (possible hidden reader)"
        );
    }

    // (2) 0x528: the census used to report zero TEST here (a desync floor
    // from the mis-widthed 0x4E sub-ops 4..8); the width fix resyncs the
    // walker and the census now agrees with the byte-scan that the reader
    // family is real. Locks the correction both ways: the census surfaces
    // the family AND the branch-idiom scan confirms it above the noise floor.
    assert!(
        census_test(0x528) > 0,
        "0x528 census TEST surfaced by the 0x4E width fix (old zero = desync floor)"
    );
    let (raw_528, gen_528) = bytescan(0x528);
    assert!(
        gen_528 >= 10 && gen_528 as f64 > (raw_528 as f64 / 256.0) * 6.0,
        "0x528 carries a real TEST reader (genuine {gen_528} vs raw {raw_528})"
    );

    // (3) The real gates: genuine TEST far above a ~0 noise floor. Each has a
    // tiny raw pool (rare op-pair), so the branch-idiom count is decisive.
    let real = |flag: u16, min: usize| {
        let (raw, genuine) = bytescan(flag);
        assert!(
            genuine >= min && genuine as f64 > (raw as f64 / 256.0) * 3.0 + 0.5,
            "0x{flag:X} is a real TEST gate (genuine {genuine} >= {min}, raw {raw})"
        );
    };
    for chest in 0x96u16..=0x9C {
        real(chest, 2); // tunnela treasure-chest gates
    }
    for sym in 0x1D8u16..=0x1E8 {
        real(sym, 4); // stone symbol-puzzle selectors + solved gate
    }
    for hari in [0x1FDu16, 0x1FE, 0x1FF] {
        real(hari, 4); // rayman three-Haris hub
    }
    real(0x32A, 3); // stone code-clue marker
    real(0x590, 1); // stone scene-entry progress gate

    // 0x1E8 (stone SOLVED) is read cross-scene in map02 - the byte-scan sees
    // it even though it is a different scene than the setter.
    let (_, gen_1e8) = bytescan(0x1E8);
    assert!(gen_1e8 >= 15, "0x1E8 solved gate is heavily TESTed");
}
