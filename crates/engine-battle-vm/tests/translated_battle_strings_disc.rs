//! Disc-gated: a language pack that rewrites the battle screen's labels, the
//! victory and defeat lines and the name-entry prompts reaches the port's
//! readers.
//!
//! The readers under test are the ones the hosts install: the battle labels
//! (`legaia_ui_formats::battle_ui_strings`, `SCUS_942.54` + PROT 0898), the
//! result window's pieces (`battle_party_panel::{VictoryText, DefeatText}`,
//! PROT 0898) and the name-entry prompts
//! (`legaia_ui_formats::name_entry_prompts`, PROT 0897). The test exports a
//! pack from the retail disc, fills each of those strings with a longer,
//! accented translation (so most of them move, with the code or pointer word
//! that forms their address rewritten), imports it in memory, and reads the
//! patched images back: every reader returns what the import wrote.
//!
//! It also pins each reader's reference table against the translation
//! pipeline's own reference scan of the retail images.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_disc_patch::disc::DiscPatcher;
use legaia_engine_battle_vm::battle_party_panel::{
    self as bpp, DefeatText, OVERLAY_0898_LINK_BASE, ResultSubject, VictoryText,
};
use legaia_overlay_images::string_refs::StrRef;
use legaia_translate::code_refs::{self, RefSite};
use legaia_translate::markup::{self, Target};
use legaia_translate::{export_pack, import_pack};
use legaia_ui_formats::battle_ui_strings::{self as bus, BattleUiStrings};
use legaia_ui_formats::name_entry_prompts::{self as nep, NameEntryPrompts, NamePrompt};

const SCUS_BASE: u32 = 0x8000_F800;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn images(p: &DiscPatcher) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    (
        p.read_named_file("SCUS_942.54").expect("SCUS"),
        p.read_entry(898).expect("PROT 0898"),
        p.read_entry(897).expect("PROT 0897"),
    )
}

/// The string at `va` in an image loaded at `base`, to its NUL, each
/// `0xC0..=0xCF` token kept with its operand.
fn token_cstr(img: &[u8], base: u32, va: u32) -> Vec<u8> {
    let mut i = (va - base) as usize;
    let mut out = Vec::new();
    while img[i] != 0 {
        let n = if img[i] & 0xF0 == 0xC0 { 2 } else { 1 };
        out.extend_from_slice(&img[i..i + n]);
        i += n;
    }
    out
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

/// `true` when `site` is one of the scan's sites for its string.
fn listed(scan: &BTreeMap<u32, Vec<RefSite>>, base: u32, va: u32, site: StrRef) -> bool {
    let base = base as usize;
    scan.get(&va).is_some_and(|sites| {
        sites.iter().any(|s| match (*s, site) {
            (RefSite::Word { off }, StrRef::Word(w)) => base + off == w as usize,
            (RefSite::Pair { lui, lo, .. }, StrRef::Pair { lui: l, lo: c }) => {
                base + lui == l as usize && base + lo == c as usize
            }
            _ => false,
        })
    })
}

#[test]
fn the_reference_tables_match_the_retail_scan() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let p = DiscPatcher::open(disc).expect("open disc");
    let (scus, o898, _) = images(&p);
    let gp = Some(code_refs::RETAIL_GP);
    let scan_scus = code_refs::scan(&scus, SCUS_BASE, gp, 0x8001_0000, 0x8008_0000);
    let base = OVERLAY_0898_LINK_BASE;
    let scan_898 = code_refs::scan(&o898, base, gp, base, base + o898.len() as u32);
    for (va, site) in bus::SCUS_REFS {
        assert!(listed(&scan_scus, SCUS_BASE, va, site), "SCUS {va:#x}");
    }
    for (va, site) in bus::OVERLAY_REFS.iter().chain(&bpp::RESULT_TEXT_REFS) {
        assert!(listed(&scan_898, base, *va, *site), "0898 {va:#x}");
    }
    // The name-entry prompts: their table word is each one's only reference.
    let p897 = p.read_entry(897).expect("PROT 0897");
    let fb = nep::FIELD_OVERLAY_BASE_VA;
    let scan_897 = code_refs::scan(&p897, fb, gp, fb, fb + p897.len() as u32);
    for slot in 0..7u32 {
        let word_va = nep::PROMPT_TABLE_VA + slot * 4;
        let o = (word_va - fb) as usize;
        let va = u32::from_le_bytes(p897[o..o + 4].try_into().unwrap());
        assert!(
            listed(&scan_897, fb, va, StrRef::Word(word_va)),
            "0897 slot {slot}"
        );
    }
    // On retail the readers read every label at its own coordinate.
    let s = BattleUiStrings::from_images(&scus, &o898, base);
    assert_eq!(s.len(), bus::SCUS_LABELS.len() + bus::OVERLAY_LABELS.len());
    assert!(VictoryText::parse(&o898).is_some() && DefeatText::parse(&o898).is_some());
}

#[test]
fn a_translated_battle_and_name_entry_pack_reaches_the_port_readers() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(disc.clone()).expect("open disc");
    let (r_scus, r898, r897) = images(&retail);
    let mut pack = export_pack(&retail).expect("export");

    // Every string these readers take, by pack key, with its fixed
    // translation where the default `<source>{82}x` would break the line's
    // shape.
    let scus_key = |va: u32| format!("scus:str:0x{va:08x}");
    let ui_key = |prot: usize, va: u32| format!("ui:{prot}:0x{va:08x}");
    let mut wanted: BTreeMap<String, Option<&str>> = BTreeMap::new();
    for (va, _) in bus::SCUS_LABELS {
        wanted.insert(scus_key(va), None);
    }
    for (va, _) in bus::OVERLAY_LABELS {
        wanted.insert(ui_key(898, va), None);
    }
    wanted.insert(
        ui_key(898, bpp::VICTORY_TEAM_VA),
        Some("{c1:01} et son {82}quipe"),
    );
    wanted.insert(
        ui_key(898, bpp::VICTORY_REPORT_VA),
        Some(" gagnent !|Obtenu           points d'exp{82}rience et           Or."),
    );
    wanted.insert(
        ui_key(898, bpp::DEFEAT_TEAM_VA),
        Some("{c1:01} et son groupe sont vaincus{82}."),
    );
    wanted.insert(
        ui_key(898, bpp::DEFEAT_SOLO_SUFFIX_VA),
        Some(" n'a plus de force{82}."),
    );
    let prompt_vas: Vec<(NamePrompt, u32)> = [
        NamePrompt::Select,
        NamePrompt::Confirm,
        NamePrompt::Yes,
        NamePrompt::No,
    ]
    .into_iter()
    .map(|p| {
        let o = (nep::PROMPT_TABLE_VA + p as u32 * 4 - nep::FIELD_OVERLAY_BASE_VA) as usize;
        (p, u32::from_le_bytes(r897[o..o + 4].try_into().unwrap()))
    })
    .collect();
    for &(_, va) in &prompt_vas {
        wanted.insert(ui_key(897, va), None);
    }

    let mut filled: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let s = &mut pack.sections;
    for e in s.ui_menu.iter_mut().chain(s.system_text.iter_mut()) {
        let Some(fixed) = wanted.get(&e.key) else {
            // The battle overlay and the executable have no spare region a
            // longer string can take: the room comes from the pools'
            // compaction, so every other string in these images shrinks.
            if e.key.starts_with("ui:898:")
                || e.key.starts_with("ui:897:")
                || e.key.starts_with("scus:")
            {
                e.translation = "x".to_string();
            }
            continue;
        };
        e.translation = match fixed {
            Some(t) => t.to_string(),
            None => format!("{}{{82}}x", e.source),
        };
        filled.insert(
            e.key.clone(),
            markup::encode(&e.translation, Target::CString).unwrap(),
        );
    }
    // Every reader's string is a pack key (the steal captions' two-byte
    // tails carry no letter, so the pools do not export them).
    let missing: Vec<&String> = wanted.keys().filter(|k| !filled.contains_key(*k)).collect();
    for k in &missing {
        assert!(
            [
                bus::SCUS_STEAL_TOOK_TAIL,
                bus::SCUS_STEAL_RECOVERED_TAIL,
                bus::SCUS_STEAL_STOLE_TAIL
            ]
            .iter()
            .any(|&va| **k == scus_key(va)),
            "{k} is not exported"
        );
    }

    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    let (scus, o898, o897) = images(&patcher);
    let applied = |k: &str| report.applied_keys.iter().any(|a| a == k);
    let moved = |prefix: &str| {
        report
            .trace
            .moved
            .keys()
            .filter(|k| k.starts_with(prefix) && filled.contains_key(*k))
            .count()
    };
    eprintln!(
        "{} / {} applied; moved: SCUS {}, 0898 {}, 0897 {}; issues: {:?}",
        filled.keys().filter(|k| applied(k)).count(),
        filled.len(),
        moved("scus:"),
        moved("ui:898:"),
        moved("ui:897:"),
        report
            .issues
            .iter()
            .filter(|(k, _)| filled.contains_key(k))
            .collect::<Vec<_>>()
    );
    assert!(moved("scus:") > 0 && moved("ui:898:") > 0 && moved("ui:897:") > 0);
    let want = |key: String, img: &[u8], base: u32, va: u32| -> Vec<u8> {
        if applied(&key) {
            filled[&key].clone()
        } else {
            token_cstr(img, base, va)
        }
    };

    // The battle labels.
    let labels = BattleUiStrings::from_images(&scus, &o898, OVERLAY_0898_LINK_BASE);
    for (va, l) in bus::SCUS_LABELS {
        let w = want(scus_key(va), &r_scus, SCUS_BASE, va);
        assert_eq!(labels.get(l), Some(latin1(&w).as_str()), "{l:?}");
    }
    for (va, l) in bus::OVERLAY_LABELS {
        let w = want(ui_key(898, va), &r898, OVERLAY_0898_LINK_BASE, va);
        assert_eq!(labels.get(l), Some(latin1(&w).as_str()), "{l:?}");
    }

    // The result window.
    let lead = ResultSubject::Lead(1);
    let team = ResultSubject::LeadsTeam { escape_operand: 0 };
    let v = VictoryText::parse(&o898).expect("victory pieces parse");
    let d = DefeatText::parse(&o898).expect("defeat pieces parse");
    for k in [
        bpp::VICTORY_TEAM_VA,
        bpp::VICTORY_REPORT_VA,
        bpp::DEFEAT_TEAM_VA,
        bpp::DEFEAT_SOLO_SUFFIX_VA,
    ] {
        assert!(applied(&ui_key(898, k)), "{k:#x} applies");
    }
    assert_eq!(v.opening(lead, "Vahn"), "Vahn gagnent !");
    assert_eq!(v.opening(team, "Vahn"), "Vahn et son \u{82}quipe gagnent !");
    assert_eq!(
        v.report_runs(),
        ["Obtenu", "points d'exp\u{82}rience et", "Or."]
    );
    assert_eq!(d.compose(lead, "Vahn"), "Vahn n'a plus de force\u{82}.");
    assert_eq!(
        d.compose(team, "Vahn"),
        "Vahn et son groupe sont vaincus\u{82}."
    );

    // The name-entry prompts.
    let prompts = NameEntryPrompts::from_field_overlay(&o897, nep::FIELD_OVERLAY_BASE_VA);
    for (p, va) in prompt_vas {
        let w = want(ui_key(897, va), &r897, nep::FIELD_OVERLAY_BASE_VA, va);
        assert_eq!(prompts.get(p), Some(latin1(&w).as_str()), "{p:?}");
    }
}
