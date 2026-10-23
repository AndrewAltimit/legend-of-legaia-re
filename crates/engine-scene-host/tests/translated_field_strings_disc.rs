//! Disc-gated: a language pack that rewrites the field overlay's strings
//! reaches the port's own readers.
//!
//! The port reads the field overlay (PROT 0897) off the disc for the code
//! lock's header, the flag window's legend, the coin counter's labels, the
//! Incense wear-off notice and the tile board's quit prompt. This test
//! exports a pack from the retail disc, fills every string the field
//! overlay's reference table names ([`STRING_REFS`]) with a longer, accented
//! translation (so most of them **move**, with the code that forms their
//! addresses rewritten), imports it in memory, builds a [`SceneHost`] over
//! the patched `PROT.DAT`, and reads each screen through the `SceneHost`
//! method both play hosts draw from. Each must return the text the import
//! says it wrote - the translation when applied, the retail text when the
//! import kept it.
//!
//! It also pins the reference table against the translation pipeline's own
//! reference scan of the retail image.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_disc_patch::disc::DiscPatcher;
use legaia_engine_core::field_submode_flag_window::FLAG_WINDOW_SLOT;
use legaia_engine_field::field_overlay::{self, FIELD_OVERLAY_BASE, STRING_REFS};
use legaia_engine_field::field_submode_code_lock::{CODE_LOCK_HEADER_VA, CODE_LOCK_SLOT};
use legaia_engine_field::field_submode_flag_window::FLAG_WINDOW_LEGEND_VAS;
use legaia_engine_minigames::slot_machine::COIN_ENTRY_LABEL_VAS;
use legaia_engine_minigames::tile_board::PROMPT_STRING_VAS;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_overlay_images::string_refs::StrRef;
use legaia_translate::code_refs::{self, RefSite};
use legaia_translate::markup::{self, Target};
use legaia_translate::{export_pack, import_pack};

const FIELD_PROT: usize = 897;
/// The Incense notice's string (`legaia_engine_core::incense_notice`).
const NOTICE_VA: u32 = legaia_engine_core::incense_notice::NOTICE_STRING_VA;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn cstr(img: &[u8], va: u32) -> Vec<u8> {
    let off = (va - FIELD_OVERLAY_BASE) as usize;
    let end = img[off..].iter().position(|&b| b == 0).unwrap();
    img[off..off + end].to_vec()
}

/// Every retail VA the host readers under test take.
fn port_read_vas() -> Vec<u32> {
    let mut v = vec![CODE_LOCK_HEADER_VA, NOTICE_VA];
    v.extend(FLAG_WINDOW_LEGEND_VAS);
    v.extend(COIN_ENTRY_LABEL_VAS);
    v.extend(PROMPT_STRING_VAS);
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn the_field_reference_table_matches_the_retail_scan() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let p = DiscPatcher::open(disc).expect("open disc");
    let img = p.read_entry(FIELD_PROT).expect("PROT 0897");
    let scan = code_refs::scan(
        &img,
        FIELD_OVERLAY_BASE,
        Some(code_refs::RETAIL_GP),
        FIELD_OVERLAY_BASE,
        FIELD_OVERLAY_BASE + img.len() as u32,
    );
    let base = FIELD_OVERLAY_BASE as usize;
    for &(va, site) in STRING_REFS {
        assert_eq!(
            field_overlay::resolve_va(&img, va),
            va,
            "{va:#010x}: the retail site must form the retail address"
        );
        let sites = scan.get(&va).expect("the scan finds a reference");
        let listed = sites.iter().any(|s| match (*s, site) {
            (RefSite::Word { off }, StrRef::Word(w)) => base + off == w as usize,
            (RefSite::Pair { lui, lo, .. }, StrRef::Pair { lui: l, lo: c }) => {
                base + lui == l as usize && base + lo == c as usize
            }
            _ => false,
        });
        assert!(listed, "{va:#010x}: {site:?} is one of the scan's sites");
    }
    for va in port_read_vas() {
        assert!(
            legaia_overlay_images::string_refs::lookup(STRING_REFS, va).is_some(),
            "{va:#010x} has no reference site"
        );
    }
}

#[test]
fn a_translated_field_overlay_reaches_the_port_readers() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = DiscPatcher::open(disc.clone()).expect("open disc");
    let retail = original.read_entry(FIELD_PROT).expect("PROT 0897");
    let mut pack = export_pack(&original).expect("export");

    // Every string the table names is in the pack.
    let vas: Vec<u32> = STRING_REFS.iter().map(|&(va, _)| va).collect();
    let key = |va: u32| format!("ui:{FIELD_PROT}:0x{va:08x}");
    for &va in &vas {
        assert!(
            pack.sections.ui_menu.iter().any(|e| e.key == key(va)),
            "{} is exported",
            key(va)
        );
    }

    // The field overlay has no translation region of its own, so a longer
    // string moves only into room shorter ones give up in its pools'
    // compaction. Every string the port reads gets a longer, accented
    // translation (`{82}` is a high glyph cell, so the readers must carry a
    // byte outside ASCII through); every other field-overlay string gets a
    // one-letter one, which frees that room.
    let read_vas = port_read_vas();
    let prefix = format!("ui:{FIELD_PROT}:0x");
    let mut want_tr: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for e in pack.sections.ui_menu.iter_mut() {
        let Some(va) = e
            .key
            .strip_prefix(&prefix)
            .and_then(|h| u32::from_str_radix(h, 16).ok())
        else {
            continue;
        };
        e.translation = if read_vas.contains(&va) {
            format!("{}{{82}}t", e.source)
        } else {
            "x".to_string()
        };
        want_tr.insert(va, markup::encode(&e.translation, Target::CString).unwrap());
    }

    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    let img = patcher.read_entry(FIELD_PROT).expect("patched PROT 0897");
    let applied = |va: u32| report.applied_keys.contains(&key(va));
    let want = |va: u32| -> Vec<u8> {
        if applied(va) {
            want_tr[&va].clone()
        } else {
            cstr(&retail, va)
        }
    };
    let mut moved = 0;
    for &va in &vas {
        if let Some(&(from, to)) = report.trace.moved.get(&key(va)) {
            assert_eq!(from, va);
            assert_eq!(field_overlay::resolve_va(&img, va), to, "{}", key(va));
            moved += 1;
        }
        assert_eq!(
            field_overlay::field_overlay_cstr(&img, va),
            Some(want(va).as_slice()),
            "{}",
            key(va)
        );
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
    assert!(
        read_vas.iter().all(|&va| applied(va)),
        "every string the port reads finds room"
    );
    let read_moved = read_vas
        .iter()
        .filter(|va| report.trace.moved.contains_key(&key(**va)))
        .count();
    assert!(read_moved > 0, "the test must exercise a moved string");

    // The engine path: a host over the patched PROT.DAT.
    let prot = patcher.read_named_file("PROT.DAT").expect("PROT.DAT");
    let cdname = patcher
        .read_named_file("CDNAME.TXT")
        .and_then(|b| String::from_utf8(b).ok());
    let mut host = SceneHost::from_prot_bytes(prot, cdname.as_deref()).expect("host");
    let has = |lines: &[Vec<u8>], va: u32| {
        let w = want(va);
        assert!(
            lines.contains(&w),
            "{}: {:?} among {lines:?}",
            key(va),
            String::from_utf8_lossy(&w)
        );
    };

    // The code lock's header.
    let _ = host.world.man_load_actor_reset();
    host.world.open_field_submode_screen(CODE_LOCK_SLOT, None);
    host.world.tick_submode_screen(1);
    let lines: Vec<_> = host.code_lock_lines().into_iter().map(|l| l.text).collect();
    has(&lines, CODE_LOCK_HEADER_VA);
    host.world.close_field_submode_screen();

    // The flag window's legend, with the kor warp pad's operand parked.
    host.world.open_field_submode_screen(FLAG_WINDOW_SLOT, None);
    host.world
        .set_submode_board_entries(&[0x49, 0x04, 0x08, 0x00, 0x08, 0x38, 0x01]);
    host.world.tick_submode_screen(1);
    let lines: Vec<_> = host
        .flag_window_lines()
        .into_iter()
        .map(|l| l.text)
        .collect();
    for va in FLAG_WINDOW_LEGEND_VAS {
        has(&lines, va);
    }
    host.world.close_field_submode_screen();

    // The coin counter's entry panel.
    host.world.party.money = 1_000;
    host.world.minigames.casino_coins = 7;
    host.world.open_coin_counter();
    host.world.tick_submode_screen(1);
    let lines: Vec<_> = host
        .coin_counter_lines()
        .into_iter()
        .map(|l| l.text)
        .collect();
    for va in COIN_ENTRY_LABEL_VAS {
        has(&lines, va);
    }
    host.world.close_field_submode_screen();

    // The tile board's quit prompt.
    let prompt = host.tile_board_prompt_lines().expect("prompt lines");
    for (got, va) in prompt.iter().zip(PROMPT_STRING_VAS) {
        assert_eq!(*got, want(va), "{}", key(va));
    }

    // The Incense notice: its item-name escape is expanded, so compare the
    // text that follows it.
    host.world.raise_incense_notice();
    host.world.tick_incense_notice();
    let line = host.incense_notice_line().expect("the notice is up");
    let w = want(NOTICE_VA);
    let text = host.world.menu.text.as_ref();
    let expanded = legaia_engine_core::incense_notice::expand_item_names(&w, |id| {
        text.and_then(|t| t.item_name(id)).map(str::to_string)
    });
    assert_eq!(&w[..2], &[0xC2, 0x8A], "the notice opens on the Incense");
    assert_eq!(line, expanded, "{:?}", String::from_utf8_lossy(&line));
}
