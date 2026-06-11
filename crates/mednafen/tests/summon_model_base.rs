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

/// PSX address of the 5-slot present-party character-id list the battle
/// loader (`FUN_80052FA0`) walks at init, loading one model per nonzero slot.
/// Slots 0..=2 are the main battle members, slot 3 a guest slot, slot 4 an
/// extra/special slot. (The story party-count global at `0x80084594` counts
/// main members only - the Terra-in-party capture showed it excludes guests,
/// so the model-base invariant keys on this list instead.)
const PRESENT_PARTY_LIST: u32 = 0x8007_BD10;

/// The fixed pool prefix that precedes the live party-character meshes in
/// `DAT_8007C018` at battle init: `gp[0x754]` snapshots the running
/// model-register counter after the per-slot loads, i.e.
/// `MODEL_BASE_PREFIX + loaded_model_count` whenever the library is resident.
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

/// `(gp[0x754], member_count, extra_slot)` for a scenario's mednafen library
/// backup, or `None` (skip) when the scenario, its `backup_fingerprint`, or
/// the backup file is missing. `member_count` counts nonzero entries in
/// present-party slots 0..=3; `extra_slot` is slot 4's id byte.
fn model_base_and_party(
    manifest: &ScenarioManifest,
    lib: &Path,
    label: &str,
) -> Option<(u32, u8, u8)> {
    let scn = manifest.scenarios.iter().find(|s| s.label == label)?;
    let fp = scn.backup_fingerprint.as_deref()?;
    let path = scenarios::library_backup_for("mednafen", lib, fp)?;
    let save = SaveState::from_path(&path)
        .unwrap_or_else(|e| panic!("parse save {label} ({}): {e:#}", path.display()));
    let ram = save
        .main_ram()
        .unwrap_or_else(|e| panic!("main RAM for {label}: {e:#}"));
    let members = (0..4)
        .filter(|i| read_u8(ram, PRESENT_PARTY_LIST + i) != 0)
        .count() as u8;
    let extra = read_u8(ram, PRESENT_PARTY_LIST + 4);
    Some((read_u32(ram, GP_754_MODEL_BASE), members, extra))
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
    // either 0 (no library resident) or 2 + the number of party-side models
    // the loader registered. Slots 0..=3 of the present-party list are stable
    // member ids; slot 4 (the extra/special slot) mutates at runtime, so when
    // it is nonzero in a late-battle state its model may or may not have been
    // resident at the snapshot point - both counts are accepted for it. We
    // also require having seen the relationship hold for at least two
    // *distinct* member counts, so the test demonstrably pins a
    // party-size-tracking base and not a constant.
    let mut resident_party_sizes: Vec<u8> = Vec::new();
    let mut checked = 0usize;

    for scn in &manifest.scenarios {
        let Some((base, members, extra)) = model_base_and_party(&manifest, &lib, &scn.label) else {
            continue;
        };
        checked += 1;
        let resident = u32::from(members) + u32::from(MODEL_BASE_PREFIX);
        let resident_with_extra = resident + u32::from(extra != 0);
        assert!(
            base == 0 || base == resident || base == resident_with_extra,
            "{}: gp[0x754]={base} must be 0 (no library), members+2={resident}, or \
             members+extra+2={resident_with_extra} (members={members}, extra slot={extra:#x})",
            scn.label
        );
        if base == resident && base != 0 {
            resident_party_sizes.push(members);
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
         party battles; saw resident member counts {resident_party_sizes:?}"
    );
}
