//! Disc-gated census: **which op-`0x49` sub-ops does any retail scene MAN
//! actually arm?**
//!
//! The field VM's op `0x49` parks on `_DAT_8007B450` with its *operand
//! pointer*, and every consumer dereferences that pointer's first byte - the
//! armed sub-op (`lbu v0,0x0(s6)` / `sw s6,-0x4bb0(s0)` at `0x801e0984` /
//! `0x801e09a8`, `ghidra/scripts/funcs/overlay_0897_801de840.txt`). The
//! menu overlay's outer dispatcher `FUN_801DC6B4` then routes on that byte:
//! `0` -> sub-screen `0x1A`, `1` -> `0x19`, `7` -> `0x20` (the casino prize
//! exchange), `0x0D` -> `4` (the notice panel whose root-menu cancel is the
//! ready check). See `0x801dc88c..0x801dc8e4`.
//!
//! So "which of those screens can retail's field scripts actually reach" is
//! a **disc** question, not a code question, and this is the measurement.
//!
//! ## Two tallies, because neither is the truth alone
//!
//! - **walk**: [`op49_window_census`] - the repo's own opcode-aware sweep,
//!   shared with `op49_window_census_disc.rs`. It walks every MAN carrier a
//!   scene has (bundle *and* standalone variants) and every record of every
//!   partition through `partition_record_span`, which knows that partition 2
//!   opens with a Shift-JIS name and three condition gates rather than the
//!   `[u8 N][N*2][4]` prefix partitions 0/1 use. Reusing it is deliberate: a
//!   second, cruder walker written for this file would report a different
//!   corpus than the pinned one, and two disagreeing measurements of the same
//!   bytes is worse than either.
//! - **bytes**: every `49 <n>` byte pair with `n <= 0x0D` anywhere in the
//!   decompressed MAN. A strict upper bound.
//!
//! The walk can under-count (a desync skips real sites) and the bytes can
//! over-count (a `49` inside an operand is not an opcode), so a sub-op
//! present in **both** is armed somewhere and a sub-op absent from the
//! **bytes** is armed nowhere. Skips (passes) without `LEGAIA_DISC_BIN` /
//! `extracted/`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::op49_window_census;
use legaia_engine_core::scene::{ProtIndex, Scene};

/// Highest sub-op the op's Idle arm accepts (`sltiu v0,v0,0xe` at
/// `0x801e098c` - a sub-op `>= 0x0E` never arms the park at all).
const MAX_SUB_OP: u8 = 0x0D;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Per-sub-op site count plus the scenes each sub-op appears in.
#[derive(Default)]
struct Census {
    sites: BTreeMap<u8, usize>,
    scenes: BTreeMap<u8, std::collections::BTreeSet<String>>,
}

impl Census {
    fn record(&mut self, sub_op: u8, scene: &str) {
        *self.sites.entry(sub_op).or_default() += 1;
        self.scenes
            .entry(sub_op)
            .or_default()
            .insert(scene.to_string());
    }

    fn report(&self, label: &str) {
        eprintln!("--- op-0x49 sub-op census ({label}) ---");
        for (sub_op, count) in &self.sites {
            let scenes = self.scenes.get(sub_op);
            let n = scenes.map(|s| s.len()).unwrap_or(0);
            let sample: Vec<&str> = scenes
                .map(|s| s.iter().take(6).map(String::as_str).collect())
                .unwrap_or_default();
            eprintln!(
                "  sub-op {sub_op:#04x}: {count:5} sites across {n:3} scenes  e.g. {sample:?}"
            );
        }
        for missing in 0..=MAX_SUB_OP {
            if !self.sites.contains_key(&missing) {
                eprintln!("  sub-op {missing:#04x}: ABSENT");
            }
        }
    }
}

#[test]
fn op49_sub_op_census_over_every_scene_man() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut names = index.cdname_scene_names();
    names.sort();
    names.dedup();

    // --- walk: the shared opcode-aware sweep ---
    let mut walk = Census::default();
    for site in op49_window_census(&index, &names) {
        walk.record(site.sub_op, &site.scene_name);
    }

    // --- bytes: the strict upper bound, over the same MAN payloads ---
    let mut bytes = Census::default();
    let mut scenes_with_man = 0usize;
    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        scenes_with_man += 1;
        for w in man.windows(2) {
            if w[0] == 0x49 && w[1] <= MAX_SUB_OP {
                bytes.record(w[1], name);
            }
        }
    }

    eprintln!(
        "scenes named in CDNAME: {}; scenes with a field MAN: {scenes_with_man}",
        names.len()
    );
    walk.report("op49_window_census - the shared opcode-aware sweep");
    bytes.report("raw `49 nn` byte pairs (upper bound)");

    assert!(
        scenes_with_man > 50,
        "the sweep must actually reach the scene corpus, got {scenes_with_man}"
    );
    // Non-vacuity: the inline gold shop is the sub-op a playable disc leans
    // on hardest, so a sweep that cannot find it is measuring wrong bytes
    // rather than reporting an empty corpus.
    assert!(
        walk.sites.get(&0).copied().unwrap_or(0) > 10,
        "the inline-shop sub-op must be all over the walk"
    );

    // The two findings this census exists to publish: the sub-ops that
    // select the menu overlay's entry-context screens are armed by real
    // scene scripts, so those screens are reachable rather than dead
    // dispatch arms.
    //
    // Each is asserted in **both** tallies. The walk alone could be a
    // desync artifact and the bytes alone could be an operand that merely
    // looks like an opcode; agreeing is what makes it a measurement. The
    // bounds are loose on purpose - this is a claim about the retail disc
    // holding such a site at all, not a count to ratchet.
    for (sub_op, screen) in [
        (0x07u8, "sub-screen 0x20, the casino prize exchange"),
        (0x0D, "sub-screen 4 + the root cancel's sub-screen 3"),
    ] {
        let w = walk.sites.get(&sub_op).copied().unwrap_or(0);
        let b = bytes.sites.get(&sub_op).copied().unwrap_or(0);
        eprintln!("FINDING: sub-op {sub_op:#04x} ({screen}): walk {w}, bytes {b}");
        assert!(
            w > 0,
            "sub-op {sub_op:#04x} must be armed somewhere on the disc"
        );
        assert!(
            b > 0,
            "sub-op {sub_op:#04x} must also appear in the byte upper bound"
        );
    }

    // Why the byte tally is reported at all: the walk is opcode-aware, but
    // a sub-op it does not decode can still be armed. Read an ABSENT walk
    // row as "not decoded here", never as "not on the disc".
    for sub_op in 0..=MAX_SUB_OP {
        if walk.sites.contains_key(&sub_op) {
            assert!(
                bytes.sites.contains_key(&sub_op),
                "sub-op {sub_op:#04x} decoded by the walk must be inside the byte bound"
            );
        }
    }
}
