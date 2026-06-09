//! Pin the battle effect-model-library additive base `gp[0x754]` against the
//! retail save-state corpus.
//!
//! `gp[0x754]` (the plain SDA global at PSX address `0x8007BA6C`) is the base
//! the summon / move-FX stager `FUN_80021B04` adds to a part record's
//! `model_sel` to index the runtime TMD pool `DAT_8007C018`
//! (`mesh = DAT_8007C018[model_sel + gp[0x754]]`; see
//! `docs/formats/move-power.md` and `docs/subsystems/effect-vm.md`). A single
//! live capture had pinned it as `3` "in battle", which the engine adopted as a
//! fixed library base (`scene::EFFECT_MODEL_LIBRARY_BASE = 3`).
//!
//! Read across the whole corpus, the value is **not** a constant `3`: it is `0`
//! whenever no battle effect-model library is resident (every field / town /
//! menu / minigame / cutscene / battle-loading frame), and **`party_count + 2`**
//! whenever a battle has installed the library — `3` for the 1-member training
//! party (Vahn alone) and `5` for the 3-member party (Vahn / Noa / Gala). So the
//! base tracks party size: the two fixed pool slots plus the live party-character
//! meshes precede the effect-model library, and `gp[0x754]` is where that library
//! starts.
//!
//! This does *not* make the engine's fixed base-3 wrong: the engine registers
//! the PROT 0871 effect-model library at a fixed `DAT_8007C018[3..]` and a part's
//! `model_sel` is library-relative, so `model_sel + 3` lands on the same library
//! model retail reaches via `model_sel + gp[0x754]` (the library content is the
//! same; only its pool offset shifts with party size). The two layouts are
//! equivalent. This test pins the retail *relationship* so the observation
//! survives — and so a future capture with a 2-member party can refine the `+2`
//! prefix if it ever turns out to be party-size-dependent rather than fixed.
//!
//! Library-gated (not disc-gated): the capture states live as immutable,
//! content-hashed backups under `saves/library/mednafen/` (gitignored Sony RAM)
//! and resolve via each scenario's `backup_fingerprint`. The test skip-passes
//! when the manifest or the backups are absent, so CI stays green without the
//! save corpus.

use std::path::{Path, PathBuf};

use legaia_mednafen::{SaveState, ScenarioManifest, extract::ram_slice, scenarios};

/// PSX address of `gp[0x754]` — the battle effect-model-library additive base
/// (`gp` base `0x8007B318` + `0x754`). See `docs/formats/move-power.md`.
const GP_754_MODEL_BASE: u32 = 0x8007_BA6C;

/// PSX address of the party member count (`u8`). See
/// `docs/reference/memory-map.md`.
const PARTY_COUNT: u32 = 0x8008_4594;

/// The fixed pool prefix that precedes the live party-character meshes in
/// `DAT_8007C018` at battle init: `gp[0x754] == party_count + MODEL_BASE_PREFIX`
/// whenever the library is resident.
const MODEL_BASE_PREFIX: u8 = 2;

fn read_u32(ram: &[u8], addr: u32) -> u32 {
    let s = ram_slice(ram, addr, addr + 4)
        .unwrap_or_else(|e| panic!("u32 slice @ {addr:#010x}: {e:#}"));
    u32::from_le_bytes([s[0], s[1], s[2], s[3]])
}

fn read_u8(ram: &[u8], addr: u32) -> u8 {
    let s =
        ram_slice(ram, addr, addr + 1).unwrap_or_else(|e| panic!("u8 slice @ {addr:#010x}: {e:#}"));
    s[0]
}

fn manifest_path() -> Option<PathBuf> {
    for c in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

/// `(gp[0x754], party_count)` for a scenario's mednafen library backup, or
/// `None` (skip) when the scenario, its `backup_fingerprint`, or the backup
/// file is missing.
fn model_base_and_party(manifest: &ScenarioManifest, lib: &Path, label: &str) -> Option<(u32, u8)> {
    let scn = manifest.scenarios.iter().find(|s| s.label == label)?;
    let fp = scn.backup_fingerprint.as_deref()?;
    let path = scenarios::library_backup_for("mednafen", lib, fp)?;
    let save = SaveState::from_path(&path)
        .unwrap_or_else(|e| panic!("parse save {label} ({}): {e:#}", path.display()));
    let ram = save
        .main_ram()
        .unwrap_or_else(|e| panic!("main RAM for {label}: {e:#}"));
    Some((read_u32(ram, GP_754_MODEL_BASE), read_u8(ram, PARTY_COUNT)))
}

#[test]
fn model_library_base_tracks_party_size() {
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/scenarios.toml not found");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let Some(lib) = library_dir() else {
        eprintln!("[skip] saves/library not present (gitignored save corpus)");
        return;
    };

    // For every scenario with a mednafen backup, the model-library base is
    // either 0 (no library resident) or exactly party_count + 2 (resident). We
    // also require having seen the relationship hold for at least two *distinct*
    // party sizes, so the test demonstrably pins a party-size-tracking base and
    // not a constant 3.
    let mut resident_party_sizes: Vec<u8> = Vec::new();
    let mut checked = 0usize;

    for scn in &manifest.scenarios {
        let Some((base, party)) = model_base_and_party(&manifest, &lib, &scn.label) else {
            continue;
        };
        checked += 1;
        let expected_resident = u32::from(party) + u32::from(MODEL_BASE_PREFIX);
        assert!(
            base == 0 || base == expected_resident,
            "{}: gp[0x754]={base} must be 0 (no library) or party_count+2={expected_resident} \
             (party_count={party})",
            scn.label
        );
        if base != 0 {
            resident_party_sizes.push(party);
        }
    }

    if checked == 0 {
        eprintln!("[skip] no mednafen library backups present");
        return;
    }

    resident_party_sizes.sort_unstable();
    resident_party_sizes.dedup();
    // Non-vacuous: the corpus must exercise the 1-member training party
    // (base 3) and the full 3-member party (base 5), so the test actually shows
    // the base shifting by the party-size delta rather than being a constant.
    if resident_party_sizes.len() < 2 {
        eprintln!(
            "[skip] need >=2 distinct battle party sizes in the corpus to pin the \
             tracking relationship; saw {resident_party_sizes:?}"
        );
        return;
    }
    assert!(
        resident_party_sizes.contains(&1) && resident_party_sizes.contains(&3),
        "expected the corpus to cover the 1-member (base 3) and 3-member (base 5) \
         party battles; saw resident party sizes {resident_party_sizes:?}"
    );
}
