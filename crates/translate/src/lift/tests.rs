use super::super::ui;
use super::chunk_align::{Chunk, align_chunks_dp, nul_chunks, pool_text};
use super::dialog::{crawl_page_map, crawl_pages, drop_baseline_text, has_prose};
use super::source_tables::{SourceBases, looks_like_name, source_build_for_exe};
use super::string_pools::pinned_pool_pairs;
use super::*;

#[test]
fn baseline_filter_blanks_shared_text_per_entry() {
    use super::super::pack::Entry;
    let entry = |key: &str, t: &str| Entry {
        key: key.to_string(),
        context: String::new(),
        source: String::new(),
        translation: t.to_string(),
        budget: 8,
    };
    let mut pack = LanguagePack::new("xx");
    pack.sections.scene_dialog = vec![
        entry("man:5:0x10", "same key"), // blanked: baseline has it at this key
        entry("man:5:0x20", "shifted line"), // blanked: baseline has it elsewhere in entry 5
        entry("man:5:0x30", "translated"), // kept
        entry("man:6:0x10", "shifted line"), // kept: entry 6 never carried it
        entry("man:5:0x40", "{c2:7c}."), // kept: no prose of its own
    ];
    pack.sections.items = vec![entry("scus:str:0x80011230", "Potion")];
    let mut base = LanguagePack::new("xx");
    base.sections.scene_dialog = vec![
        entry("man:5:0x10", "same key"),
        entry("man:5:0x28", "shifted line"),
        entry("man:5:0x30", "retail text"),
        entry("man:5:0x40", "{c2:7c}."),
    ];
    base.sections.items = vec![entry("scus:str:0x80011230", "Potion")];
    assert_eq!(drop_baseline_text(&mut pack, &base), 3);
    let t: Vec<&str> = pack
        .sections
        .scene_dialog
        .iter()
        .map(|e| e.translation.as_str())
        .collect();
    assert_eq!(t, ["", "", "translated", "shifted line", "{c2:7c}."]);
    assert_eq!(pack.sections.items[0].translation, "");
}

#[test]
fn nul_chunks_drop_a_run_the_window_cuts() {
    let buf = b"\0Alpha\0Bravo\0Charlie\0";
    // The window ends inside `Charlie`: that fragment is not a string.
    let got: Vec<Vec<u8>> = nul_chunks(buf, 0, 17).into_iter().map(|c| c.1).collect();
    assert_eq!(got, [b"Alpha".to_vec(), b"Bravo".to_vec()]);
    let got = nul_chunks(buf, 0, buf.len());
    assert_eq!(got.len(), 3);
}

#[test]
fn pool_text_accepts_labels_the_dialog_gate_refuses() {
    // An abbreviation's `/` and a space-less punctuated word behind a
    // control token are UI strings, not junk.
    assert!(pool_text(b"Nenhum item p/ equipar."));
    assert!(pool_text(&[
        0xCE, 0x13, b'P', b'e', b'r', b'n', b'a', b's', b'.'
    ]));
    assert!(pool_text(&[b'P', b'r', b'e', 0x87, b'o']));
    // Pointer bytes and box / shade tiles are not.
    assert!(!pool_text(&[0xC8, 0x02, 0x80, 0x7C]));
    assert!(!pool_text(&[b'L', 0xB4, b'a', 0xAC, 0x7F]));
    assert!(!pool_text(b"@."));
}

#[test]
fn dp_alignment_absorbs_a_surplus_chunk_mid_pool() {
    // The source build carries an extra label variant in the middle of
    // the pool (`Possui ` ahead of `@Possui `): a head alignment pairs
    // `@Have ` with the variant and slides every later pair by one.
    let mk = |v: &[&[u8]]| -> Vec<Chunk> {
        v.iter()
            .enumerate()
            .map(|(i, b)| (i * 0x20, b.to_vec()))
            .collect()
    };
    let u = mk(&[b"@Cannot Equip", b"@Have ", b"@.", b"@None", b"@Price"]);
    let s = mk(&[
        b"@Nao pode equipar",
        b"Possui ",
        b"@Possui ",
        b"@.",
        b"@Nada",
        b"@Preco",
    ]);
    let (ur, sr): (Vec<&Chunk>, Vec<&Chunk>) = (u.iter().collect(), s.iter().collect());
    let pairs = align_chunks_dp(&ur, &sr).expect("aligns");
    let got: Vec<(&[u8], &[u8])> = pairs
        .iter()
        .map(|(a, b)| (a.1.as_slice(), b.1.as_slice()))
        .collect();
    assert_eq!(
        got,
        [
            (&b"@Cannot Equip"[..], &b"@Nao pode equipar"[..]),
            (b"@Have ", b"@Possui "),
            (b"@.", b"@."),
            (b"@None", b"@Nada"),
            (b"@Price", b"@Preco"),
        ]
    );
    // The same list against itself is the identity.
    let pairs = align_chunks_dp(&ur, &ur).expect("identity");
    assert!(pairs.iter().all(|(a, b)| a.0 == b.0));
    assert_eq!(pairs.len(), u.len());
}

/// Chunks laid out one after another the way `nul_chunks` frames them: each
/// starts one past the previous one's terminator. A chunk that ends in a
/// name token whose `0x00` argument also ends its string (`@Equipar
/// {c1:00}`) is followed by that second zero.
fn laid_out(v: &[&[u8]]) -> Vec<Chunk> {
    let mut off = 0x10;
    let mut prev_head = false;
    v.iter()
        .map(|b| {
            // A string that opens right after a head's argument byte is that
            // head's tail; one listed after a string-ending head is not.
            let tail = b.first() == Some(&b' ');
            if prev_head && !tail {
                off += 1;
            }
            let c = (off, b.to_vec());
            off += b.len() + 1;
            prev_head = b.last().is_some_and(|&x| x & 0xF0 == 0xC0);
            c
        })
        .collect()
}

#[test]
fn a_token_cut_fragment_never_anchors_a_pool() {
    use super::chunk_align::pair_chunk_lists;
    // `@{c1:00} will equip` scans as a two-byte head and a tail; the source
    // build puts the name last, and an unrelated later line opens with the
    // same `@{c1:00}` head. Anchoring on it would pair the labels between
    // with the source's later lines.
    let u = laid_out(&[
        b"@Have ",
        b"@.",
        b"@None",
        b"@How many?",
        b"@\xC1",
        b" will equip",
        b"@Price",
        b"@Swap",
    ]);
    let s = laid_out(&[
        b"@Tienes ",
        b"@.",
        b"@Nada",
        b"@Cuantos?",
        b"@Equipar \xC1",
        b"@Precio",
        b"@Cambiar",
        b"@El combate va a empezar.",
        b"@Prepara a tus luchadores:",
        b"@\xC1",
        b", y.",
    ]);
    let map = pair_chunk_lists(&u, &s);
    let got = |i: usize| map.get(&u[i].0).map(|b| b.as_slice());
    assert_eq!(got(2), Some(&b"@Nada"[..]));
    assert_eq!(got(3), Some(&b"@Cuantos?"[..]));
    assert_eq!(got(6), Some(&b"@Precio"[..]));
    assert_eq!(got(7), Some(&b"@Cambiar"[..]));
    // The fragments pair with nothing of another shape.
    assert_eq!(got(4), None);
    assert_eq!(got(5), None);
}

#[test]
fn a_surplus_string_mid_pool_does_not_slide_the_tail() {
    use super::chunk_align::pair_chunk_lists;
    // The source carries one extra string mid-pool (a label without its `@`
    // ahead of the same label with it) and runs on past the pool's end. Every
    // order-preserving alignment of these lengths scores alike pair by pair;
    // only the one that skips the extra string pairs each label with its own.
    let u = laid_out(&[
        b"@How many to sell?",
        b"@Price",
        b"@Cannot sell",
        b"@Exchange",
        b"@The battle begins.",
        b"@Are you ready?",
    ]);
    let s = laid_out(&[
        b"@Quanti ne vendi?",
        b"@Prezzo",
        b"Non vendibile",
        b"@Non vendibile",
        b"@Scambio",
        b"@Inizia lo scontro.",
        b"@Sei pronto?",
        b"@Lo scontro contro i fratelli",
        b"@inizia ora con tre incontri",
    ]);
    let map = pair_chunk_lists(&u, &s);
    let got: Vec<Option<&[u8]>> = u
        .iter()
        .map(|c| map.get(&c.0).map(|b| b.as_slice()))
        .collect();
    assert_eq!(
        got,
        [
            Some(&b"@Quanti ne vendi?"[..]),
            Some(b"@Prezzo"),
            Some(b"@Non vendibile"),
            Some(b"@Scambio"),
            Some(b"@Inizia lo scontro."),
            Some(b"@Sei pronto?"),
        ]
    );
}

#[test]
fn a_slot_filling_string_splits_at_the_table_stride() {
    use super::chunk_align::{pair_chunk_lists, split_at_stride, table_stride};
    // A four-slot table at stride 0x10; the source's second string fills
    // its slot exactly, so the NUL scan reads it and the third as one chunk.
    let u: Vec<Chunk> = vec![
        (0x00, b"Curse".to_vec()),
        (0x10, b"Recover all".to_vec()),
        (0x20, b"MP zero".to_vec()),
        (0x30, b"Items lost".to_vec()),
    ];
    assert_eq!(table_stride(&[0x00, 0x10, 0x20, 0x30]), Some(0x10));
    assert_eq!(table_stride(&[0x00, 0x10, 0x28]), None);
    assert_eq!(table_stride(&[0x00, 0x10]), None);
    let merged = b"Alles zurueckhol".to_vec();
    assert_eq!(merged.len(), 0x10);
    let mut joined = merged.clone();
    joined.extend_from_slice(b"MP Null");
    let s: Vec<Chunk> = vec![
        (0x00, b"Fluch".to_vec()),
        (0x10, joined),
        (0x30, b"Objekte weg".to_vec()),
    ];
    let s = split_at_stride(s, 0x10);
    assert_eq!(s[1], (0x10, merged.clone()));
    assert_eq!(s[2], (0x20, b"MP Null".to_vec()));
    let map = pair_chunk_lists(&u, &s);
    assert_eq!(map.get(&0x10), Some(&merged));
    assert_eq!(map.get(&0x20).map(Vec::as_slice), Some(&b"MP Null"[..]));
    assert_eq!(map.get(&0x30).map(Vec::as_slice), Some(&b"Objekte weg"[..]));
}

#[test]
fn crawl_pages_pair_across_paragraph_gaps() {
    // Equal counts pair one for one.
    let t: [&[u8]; 2] = [b"a", b"b"];
    assert_eq!(crawl_page_map(&t, &[b"x", b"y"]), vec![Some(0), Some(1)]);
    // The source adds blank paragraph pages: text pages pair in order.
    let s: [&[u8]; 5] = [b"one", b" ", b"two", b"three", b" "];
    let t: [&[u8]; 3] = [b"uno", b"dos", b"tres"];
    assert_eq!(crawl_page_map(&t, &s), vec![Some(0), Some(2), Some(3)]);
    // A reflow that changes the text-page count pairs nothing.
    let s: [&[u8]; 4] = [b"one", b" ", b"two", b" "];
    assert_eq!(crawl_page_map(&t, &s), vec![None, None, None]);
}

#[test]
fn crawl_opener_is_the_nibble_8_sub_0_form() {
    assert_eq!(crawl_pages(&[0xCC, 0xF8, 0x80, 0x0E], 0), Some(14));
    assert_eq!(crawl_pages(&[0xCC, 0xF8, 0x89, 0x01], 0), None);
    assert_eq!(crawl_pages(&[0xCC, 0xF8, 0x80], 0), None);
}

#[test]
fn pinned_pool_pairs_are_build_specific() {
    assert!(pinned_pool_pairs("SCES_019.45").is_empty());
    let es = pinned_pool_pairs("SCES_019.47");
    // Every USA coordinate lands in a strict pool of its overlay.
    for (prot, usa_off, _) in &es {
        if *prot == usize::MAX {
            assert!(ui::pool_for(*prot, *usa_off).is_some_and(|p| p.strict));
            continue;
        }
        let va = ui::overlay_base_va(*prot).unwrap() + usa_off;
        assert!(
            ui::pool_for(*prot, va).is_some_and(|p| p.strict),
            "{prot}:{usa_off:#x}"
        );
    }
    // One coordinate per USA string.
    let keys: std::collections::BTreeSet<_> = es.iter().map(|(p, u, _)| (p, u)).collect();
    assert_eq!(keys.len(), es.len());
}

#[test]
fn prose_test_ignores_control_tokens() {
    assert!(!has_prose("{c2:7c}."));
    assert!(!has_prose("{c1:00}, {c1:01}!"));
    assert!(has_prose("{c1:00} no"));
    assert!(has_prose("S{a1}"));
}

#[test]
fn source_build_map_covers_every_latin_build() {
    let lang = |exe| source_build_for_exe(exe).map(|b| b.lang);
    assert_eq!(lang("SCES_019.44"), Some("fr"));
    assert_eq!(lang("SCES_019.45"), Some("de"));
    assert_eq!(lang("SCES_019.46"), Some("it"));
    assert_eq!(lang("SCES_019.47"), Some("es"));
    assert_eq!(lang("SCES_017.52"), Some("en"));
    assert_eq!(lang("SCUS_942.54"), Some("en"));
    // The measured three are pinned; the rest are located from the USA VAs.
    for (exe, pinned) in [
        ("SCES_019.44", true),
        ("SCES_019.46", true),
        ("SCES_019.47", false),
        ("SCUS_942.54", false),
    ] {
        let b = source_build_for_exe(exe).unwrap();
        assert_eq!(matches!(b.bases, SourceBases::Pinned(_)), pinned, "{exe}");
    }
    // Not Latin-script: the JP original and the demos.
    assert!(source_build_for_exe("SCPS_100.59").is_none());
    assert!(source_build_for_exe("SCUS_943.66").is_none());
}

#[test]
fn key_parsers() {
    assert_eq!(key_scus_va("scus:str:0x80011230"), Some(0x8001_1230));
    assert_eq!(key_scus_va("man:31:0xe7"), None);
    assert_eq!(key_party_slot("scus:party:2"), Some(2));
    assert_eq!(key_entry_index("man:874:0x1a"), Some(874));
    assert_eq!(key_entry_index("raw:12:0x0"), Some(12));
}

#[test]
fn name_shape_gate() {
    assert!(looks_like_name(b"Gl\x81cksglocke")); // German u-umlaut byte
    assert!(looks_like_name(&[0x01, b'K', b'e', b'y'])); // icon prefix
    assert!(!looks_like_name(b"")); // empty
    assert!(!looks_like_name(&[0x00, 0x03, 0x1f])); // control bytes / code
}
