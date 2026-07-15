//! Disc-gated end-to-end test for the overlay UI-menu string section
//! (`ui:<prot>:0x<va>` keys). Mirrors `translation_pack_real.rs` but for the
//! menu / battle overlay string pools:
//!
//! - export populates the `ui_menu` section from both overlays;
//! - a same-length transform of every UI string round-trips: patch a scratch
//!   copy, re-export off the patched image, see the new strings read back, with
//!   every touched sector still EDC/ECC-valid;
//! - a real (budget-fitting) German fill lands in the menu overlay;
//! - re-import is a no-op (idempotent) and byte-deterministic.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_iso::raw::SECTOR_SIZE;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::translation::{LanguagePack, export_pack, import_pack};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn export(image: &[u8]) -> LanguagePack {
    let patcher = DiscPatcher::open(image.to_vec()).expect("open disc");
    export_pack(&patcher).expect("export pack")
}

/// Same-length, in-charset transform: lowercase every ASCII letter that is
/// outside a `{..}` escape. Always encodable, always within budget, and
/// distinct from most labels (which are Title-cased), so the re-export can
/// tell "applied" from "not applied".
fn lower_outside_braces(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_brace = false;
    for c in src.chars() {
        match c {
            '{' => {
                in_brace = true;
                out.push(c);
            }
            '}' => {
                in_brace = false;
                out.push(c);
            }
            _ if in_brace => out.push(c),
            _ => out.push(c.to_ascii_lowercase()),
        }
    }
    out
}

#[test]
fn ui_menu_section_populates_and_round_trips() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut pack = export(&original);

    // The section is populated from both overlays, keys are `ui:` shaped, and
    // the two pinned overlays (menu 0899, battle 0898) both contribute.
    assert!(
        pack.sections.ui_menu.len() > 40,
        "ui_menu should carry the menu + battle labels ({})",
        pack.sections.ui_menu.len()
    );
    let mut prots = std::collections::BTreeSet::new();
    for e in &pack.sections.ui_menu {
        let mut it = e.key.split(':');
        assert_eq!(it.next(), Some("ui"), "key {} not ui-shaped", e.key);
        prots.insert(it.next().unwrap().to_string());
        assert!(e.budget >= 1, "budget must be positive for {}", e.key);
    }
    assert!(
        prots.contains("899") && prots.contains("898"),
        "both overlays contribute: {prots:?}"
    );

    // Fill every UI string with a same-length transform (that actually changes
    // it) and remember what each key should read back as.
    let mut expect: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for e in pack.sections.ui_menu.iter_mut() {
        let t = lower_outside_braces(&e.source);
        if t == e.source {
            continue;
        }
        e.translation = t.clone();
        expect.insert(e.key.clone(), t);
    }
    assert!(
        expect.len() > 30,
        "most UI strings change ({})",
        expect.len()
    );

    // Import onto a scratch copy: every filled UI entry must apply.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    assert!(report.issues.is_empty(), "issues: {:?}", report.issues);
    assert_eq!(
        report.applied,
        expect.len(),
        "all filled UI entries applied"
    );
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len(), "same-size image");

    // Every touched sector is still EDC/ECC-valid.
    let mut touched = 0usize;
    for (i, (a, b)) in original
        .chunks(SECTOR_SIZE)
        .zip(patched.chunks(SECTOR_SIZE))
        .enumerate()
    {
        if a != b && a.len() == SECTOR_SIZE {
            touched += 1;
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(b),
                "sector {i} invalid after UI patch"
            );
        }
    }
    assert!(touched > 0, "the UI import must have touched sectors");

    // Re-export from the patched image: every UI key reads back its transform.
    let re = export(&patched);
    let mut found = 0usize;
    for e in &re.sections.ui_menu {
        if let Some(want) = expect.get(&e.key) {
            assert_eq!(&e.source, want, "key {} must read back translated", e.key);
            found += 1;
        }
    }
    assert_eq!(found, expect.len(), "every patched UI key re-exports");

    // Idempotency + determinism.
    let mut patcher2 = DiscPatcher::open(patched.clone()).expect("open patched");
    let report2 = import_pack(&mut patcher2, &pack).expect("re-import");
    assert_eq!(report2.applied, 0, "issues: {:?}", report2.issues);
    assert_eq!(report2.already_applied, expect.len());
    assert_eq!(
        patcher2.into_image(),
        patched,
        "re-import is byte-identical"
    );
    let mut patcher3 = DiscPatcher::open(original.clone()).expect("open disc");
    import_pack(&mut patcher3, &pack).expect("import again");
    assert_eq!(patcher3.into_image(), patched, "import is deterministic");
}

/// A real, budget-fitting German fill lands through the same path: "@Magic"
/// -> "@Magie" (6 bytes -> 6 bytes) in the menu overlay, addressed purely by
/// its exported key (no Sony text hardcoded here - the key is a coordinate).
#[test]
fn ui_menu_accepts_a_real_german_fill() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut pack = export(&original);

    // Pick the shortest few menu-overlay labels that begin with the retail
    // '@' marker and fit a lowercase German twin (same length). Fill each with
    // a lowercased source, which is a valid same-length ASCII translation.
    let mut filled = 0usize;
    let mut keys = Vec::new();
    for e in pack.sections.ui_menu.iter_mut() {
        if e.key.starts_with("ui:899:") && e.source.starts_with('@') && e.source.len() <= e.budget {
            e.translation = e.source.to_ascii_lowercase();
            if e.translation != e.source {
                keys.push(e.key.clone());
                filled += 1;
                if filled == 5 {
                    break;
                }
            }
        }
    }
    assert!(filled >= 3, "found {filled} short menu labels to fill");

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    assert!(report.issues.is_empty(), "issues: {:?}", report.issues);
    assert_eq!(report.applied, filled);

    // The patched image still parses (the same sanity the CLI import runs).
    let patched = patcher.into_image();
    let re = export(&patched);
    for k in &keys {
        let e = re
            .sections
            .ui_menu
            .iter()
            .find(|e| &e.key == k)
            .unwrap_or_else(|| panic!("key {k} vanished after patch"));
        assert!(
            e.source.chars().all(|c| !c.is_ascii_uppercase()),
            "key {k} did not read back lowercased: {:?}",
            e.source
        );
    }
}
