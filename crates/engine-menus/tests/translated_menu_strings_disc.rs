//! Disc-gated: a language pack that rewrites the menu overlay's strings
//! reaches the port's own readers.
//!
//! The port reads the menu overlay's string pool (PROT 0899) off the disc for
//! the pause menu's labels, the Throw Out / Door of Light / Incense confirms,
//! the Yes / No choices and the two notice templates (window 7's spell
//! level-up sentence, window 8's art-learned line). This test exports a pack
//! from the retail disc, fills every one of those strings with a longer,
//! accented translation (so most of them **move**, with the code that forms
//! their addresses rewritten), imports it in memory, and reads the patched
//! overlay back through the same readers the hosts call. Each must return
//! the text the import says it wrote - the translation when applied, the
//! retail text when the import kept it.
//!
//! It also pins the reference table the readers follow
//! (`legaia_game_tables::menu_string_refs`) against the translation
//! pipeline's own reference scan of the retail image.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_disc_patch::disc::DiscPatcher;
use legaia_engine_menus::pause_screens::{
    self, MENU_OVERLAY_BASE_VA, SPELL_LEVEL_NOTICE_TEMPLATE_VA, ThrowConfirmLabels, UseRoutePrompts,
};
use legaia_game_tables::menu_pool::{MenuLabel, MenuPoolLabels};
use legaia_game_tables::menu_string_refs::{self, POOL_REFS, PoolRef};
use legaia_translate::code_refs::{self, RefSite};
use legaia_translate::markup::{self, Target};
use legaia_translate::{export_pack, import_pack};

const MENU_PROT: usize = 899;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Every retail VA the readers under test take, by what reads it.
fn port_read_vas() -> Vec<u32> {
    let mut v: Vec<u32> = MenuLabel::ALL.iter().map(|l| l.va()).collect();
    v.extend(pause_screens::THROW_CONFIRM_PROMPT_VAS);
    v.extend([
        pause_screens::CHOICE_YES_VA,
        pause_screens::CHOICE_NO_VA,
        pause_screens::DOOR_OF_LIGHT_PROMPT_VA,
    ]);
    v.extend(pause_screens::INCENSE_PROMPT_VAS);
    v.extend([
        SPELL_LEVEL_NOTICE_TEMPLATE_VA,
        pause_screens::NOTIFY_TEMPLATE_VA,
    ]);
    v.sort_unstable();
    v.dedup();
    v
}

/// The string at `va` the way the template readers walk it: to the NUL,
/// each `0xC0..=0xCF` token kept with its operand.
fn token_cstr(img: &[u8], va: u32) -> Vec<u8> {
    let off = (va - MENU_OVERLAY_BASE_VA) as usize;
    let mut out = Vec::new();
    let mut i = off;
    while img[i] != 0 {
        if img[i] & 0xF0 == 0xC0 {
            out.extend_from_slice(&img[i..i + 2]);
            i += 2;
        } else {
            out.push(img[i]);
            i += 1;
        }
    }
    out
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

#[test]
fn the_reference_table_matches_the_retail_scan() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let p = DiscPatcher::open(disc).expect("open disc");
    let img = p.read_entry(MENU_PROT).expect("PROT 0899");
    let scan = code_refs::scan(
        &img,
        MENU_OVERLAY_BASE_VA,
        Some(code_refs::RETAIL_GP),
        MENU_OVERLAY_BASE_VA,
        MENU_OVERLAY_BASE_VA + img.len() as u32,
    );
    for &(va, site) in POOL_REFS {
        assert_eq!(
            menu_string_refs::resolve_va(&img, va),
            va,
            "{va:#010x}: the retail site must form the retail address"
        );
        let sites = scan.get(&va).expect("the scan finds a reference");
        let base = MENU_OVERLAY_BASE_VA as usize;
        let listed = sites.iter().any(|s| match (*s, site) {
            (RefSite::Word { off }, PoolRef::Word(w)) => base + off == w as usize,
            (RefSite::Pair { lui, lo, .. }, PoolRef::Pair { lui: l, lo: c }) => {
                base + lui == l as usize && base + lo == c as usize
            }
            _ => false,
        });
        assert!(listed, "{va:#010x}: {site:?} is one of the scan's sites");
    }
    // Every VA the readers take has a site, so none of them reads stale
    // bytes after a move.
    for va in port_read_vas() {
        assert!(
            menu_string_refs::pool_ref(va).is_some(),
            "{va:#010x} has no reference site"
        );
    }
}

#[test]
fn a_translated_menu_pool_reaches_the_port_readers() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = DiscPatcher::open(disc.clone()).expect("open disc");
    let retail = original.read_entry(MENU_PROT).expect("PROT 0899");
    let mut pack = export_pack(&original).expect("export");

    // Every string the readers take is in the pack.
    let vas = port_read_vas();
    let key = |va: u32| format!("ui:{MENU_PROT}:0x{va:08x}");
    for &va in &vas {
        assert!(
            pack.sections.ui_menu.iter().any(|e| e.key == key(va)),
            "{} is exported",
            key(va)
        );
    }

    // A longer, accented translation for each: `{82}` is a high glyph cell,
    // so the readers must carry a byte outside ASCII through. The two
    // templates keep the tokens their renderers patch.
    let mut want_tr: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for e in pack.sections.ui_menu.iter_mut() {
        let Some(va) = vas.iter().copied().find(|&va| e.key == key(va)) else {
            continue;
        };
        e.translation = if va == SPELL_LEVEL_NOTICE_TEMPLATE_VA {
            "{c3:01} gagne un niveau de magie{82} !".to_string()
        } else if va == pause_screens::NOTIFY_TEMPLATE_VA {
            "{c1:00} conna{8c}t d{82}sormais|le coup {cf:06}{c5:00}{cf:07} !".to_string()
        } else {
            format!("{}{{82}}x", e.source)
        };
        want_tr.insert(va, markup::encode(&e.translation, Target::CString).unwrap());
    }

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    let img = patcher.read_entry(MENU_PROT).expect("patched PROT 0899");
    let applied = |va: u32| report.applied_keys.contains(&key(va));
    let want = |va: u32| -> Vec<u8> {
        if applied(va) {
            want_tr[&va].clone()
        } else {
            token_cstr(&retail, va)
        }
    };
    // The moves the import reports, each checked against the address the
    // readers resolve.
    let mut moved = 0;
    for &va in &vas {
        if let Some(&(from, to)) = report.trace.moved.get(&key(va)) {
            assert_eq!(from, va);
            assert_eq!(menu_string_refs::resolve_va(&img, va), to, "{}", key(va));
            moved += 1;
        }
    }
    let n_applied = vas.iter().filter(|&&va| applied(va)).count();
    eprintln!(
        "{n_applied} / {} applied, {moved} moved; issues on these keys: {:?}",
        vas.len(),
        report
            .issues
            .iter()
            .filter(|(k, _)| vas.iter().any(|&va| *k == key(va)))
            .collect::<Vec<_>>()
    );
    assert!(n_applied * 2 > vas.len(), "most of the strings apply");
    assert!(moved > 0, "the test must exercise a moved string");

    // The pause menu's pool labels.
    let labels = MenuPoolLabels::from_menu_overlay(&img);
    for l in MenuLabel::ALL {
        assert_eq!(
            labels.get(l),
            Some(latin1(&want(l.va())).as_str()),
            "{l:?} ({:#010x})",
            l.va()
        );
    }
    // The Throw Out confirm and the shared Yes / No.
    let throw = ThrowConfirmLabels::from_menu_overlay(&img);
    for (got, va) in throw
        .prompt
        .iter()
        .zip(pause_screens::THROW_CONFIRM_PROMPT_VAS)
        .chain(
            throw
                .choices
                .iter()
                .zip([pause_screens::CHOICE_YES_VA, pause_screens::CHOICE_NO_VA]),
        )
    {
        assert_eq!(*got, latin1(&want(va)), "{va:#010x}");
    }
    // The Door of Light and Incense confirms, raw (the Incense line keeps
    // its item-name token).
    let use_routes = UseRoutePrompts::from_menu_overlay(&img);
    assert_eq!(
        use_routes.door_light,
        want(pause_screens::DOOR_OF_LIGHT_PROMPT_VA)
    );
    for (got, va) in use_routes
        .incense
        .iter()
        .zip(pause_screens::INCENSE_PROMPT_VAS)
    {
        assert_eq!(*got, want(va), "{va:#010x}");
    }
    // Window 7's sentence and window 8's template.
    assert_eq!(
        pause_screens::menu_overlay_bytes(&img, SPELL_LEVEL_NOTICE_TEMPLATE_VA),
        Some(want(SPELL_LEVEL_NOTICE_TEMPLATE_VA))
    );
    assert_eq!(
        pause_screens::notify_template_from_menu_overlay(&img),
        Some(want(pause_screens::NOTIFY_TEMPLATE_VA))
    );
    assert!(
        applied(SPELL_LEVEL_NOTICE_TEMPLATE_VA) && applied(pause_screens::NOTIFY_TEMPLATE_VA),
        "both templates apply"
    );

    // A spell sentence that moves its token off byte 0 is refused: retail
    // writes the spell id into byte 1.
    let mut bad = pack.clone();
    for e in bad.sections.ui_menu.iter_mut() {
        if e.key == key(SPELL_LEVEL_NOTICE_TEMPLATE_VA) {
            e.translation = "Niveau de magie de {c3:01} !".to_string();
        }
    }
    let mut p2 = DiscPatcher::open(disc).expect("open disc");
    let r2 = import_pack(&mut p2, &bad).expect("import");
    assert!(
        r2.issues
            .iter()
            .any(|(k, _)| *k == key(SPELL_LEVEL_NOTICE_TEMPLATE_VA)),
        "a reordered spell sentence is refused"
    );
    assert_eq!(
        pause_screens::menu_overlay_bytes(
            &p2.read_entry(MENU_PROT).unwrap(),
            SPELL_LEVEL_NOTICE_TEMPLATE_VA
        ),
        Some(token_cstr(&retail, SPELL_LEVEL_NOTICE_TEMPLATE_VA)),
        "the refused sentence stays retail"
    );
}
