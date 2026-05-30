//! Disc-gated: the field-VM inline option-picker decoder
//! ([`legaia_mes::scan_pickers`]) recovers real menus across the whole field
//! corpus, and every decoded option's relative jump lands inside its own
//! interaction script.
//!
//! The picker control region is `[0x27/0x28/0x29 open][N*2-byte i16 jump
//! table][continuation][N * 0x1F label segments]`; each 2-byte entry is a
//! signed relative jump the inline-script control handler `FUN_80038050`
//! applies (`new_pc = (open + 1 + index*2) + rel_jump`). See
//! `docs/formats/mes.md` § "Dialog window pager" and `crates/mes/src/picker.rs`.
//!
//! Skips (passes) when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::man_field_scripts::{PlacementKind, classify_placements};
use legaia_engine_core::scene::{ProtIndex, Scene};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn label_str(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&c| {
            if (0x20..0x7f).contains(&c) {
                c as char
            } else {
                '.'
            }
        })
        .collect()
}

#[test]
fn field_inline_pickers_decode_and_jump_in_bounds() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let mut scenes = index.cdname_scene_names();
    scenes.sort();
    scenes.dedup();

    let mut total_pickers = 0usize;
    let mut total_options = 0usize;
    let mut options_with_reply = 0usize;
    // (scene, slot) -> sorted option label strings, for spot-checks.
    let mut by_scene: Vec<(String, usize, usize, Vec<String>)> = Vec::new();

    for name in &scenes {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Some(man) = scene.field_man_payload(&index).ok().flatten() else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        for (p, kind) in classify_placements(&mf, &man) {
            let PlacementKind::Npc {
                dialog_inline: Some(inline),
                ..
            } = kind
            else {
                continue;
            };
            for pk in legaia_mes::scan_pickers(&inline) {
                // Option count matches the open byte's implied arity.
                assert_eq!(
                    pk.options.len(),
                    pk.n,
                    "[{name}] slot {} picker option count mismatch",
                    p.index
                );
                assert!(
                    (2..=4).contains(&pk.n),
                    "[{name}] slot {} bad arity {}",
                    p.index,
                    pk.n
                );
                // Every option's relative jump must resolve to a byte INSIDE
                // the same interaction script — the decisive correctness
                // signal for the "2-byte entry = relative jump" reading. A
                // garbage match would jump out of bounds.
                for i in 0..pk.n {
                    let t = pk.jump_target(i).unwrap_or(usize::MAX);
                    assert!(
                        t < inline.len(),
                        "[{name}] slot {} option {i} jump target {t} out of \
                         bounds (len {}); rel_jump={}",
                        p.index,
                        inline.len(),
                        pk.options[i].rel_jump
                    );
                    // `OwnedDialogPanel::confirm_menu` resumes by scanning from
                    // the jump target for the branch's first `0x1F` reply
                    // segment. Count how many real option branches actually
                    // have downstream reply text reachable that way.
                    if inline[t..].contains(&0x1F) {
                        options_with_reply += 1;
                    }
                    total_options += 1;
                }
                let labels: Vec<String> = pk.options.iter().map(|o| label_str(&o.label)).collect();
                by_scene.push((name.clone(), p.index, pk.n, labels));
                total_pickers += 1;
            }
        }
    }

    eprintln!("[pickers] {total_pickers} decoded across the field corpus");
    eprintln!(
        "[pickers] {options_with_reply}/{total_options} option branches reach a reply segment"
    );
    for (s, slot, n, labels) in &by_scene {
        eprintln!("  [{s}] slot {slot} N={n} {labels:?}");
    }

    // confirm_menu's resume premise: the vast majority of real option branches
    // have downstream reply text the jump lands before (the rest are
    // conversation-end branches with nothing after). A high ratio confirms the
    // relative-jump targets point into live script, not noise.
    assert!(
        total_options > 0 && options_with_reply * 10 >= total_options * 8,
        "expected >=80% of option branches to reach a reply, got {options_with_reply}/{total_options}"
    );

    // Coverage floor: the corpus carries dozens of genuine menus (config
    // On/Off/Exit, shop haggling, the Genesis Tree quiz, story prompts).
    assert!(
        total_pickers >= 20,
        "expected >=20 genuine pickers corpus-wide, found {total_pickers}"
    );

    // Spot-check a few stable, recognizable menus by their decoded labels.
    let has = |scene: &str, want: &[&str]| -> bool {
        by_scene.iter().any(|(s, _, _, labels)| {
            s == scene && want.iter().all(|w| labels.iter().any(|l| l == w))
        })
    };
    assert!(
        has("retona", &["On", "Off", "Exit"]),
        "retona config menu On/Off/Exit should decode"
    );
    assert!(
        by_scene.iter().any(|(s, _, n, labels)| s == "cave01"
            && *n == 4
            && labels.iter().any(|l| l.contains("Seru"))),
        "cave01 Genesis-Tree quiz (4-option, mentions Seru) should decode"
    );
}
