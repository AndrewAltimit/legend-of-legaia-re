//! Disc-gated regression test for the site ROM patcher's "language + full
//! randomizer" combination (the "chaos" preset). Reproduces the shipped-site
//! failure `equipment drops: equipment id table is empty`: the bonus-drop
//! pass classifies gear by matching the disc's item names against curated
//! English names, so a language pack that translated the whole item table
//! before it ran emptied the pool and aborted the patch. The fix is the
//! two-phase language import the WASM `patch_rom` now uses - dialog sections
//! before the randomizer (their `man:` offsets predate record relocation),
//! SCUS name sections after every pass - which this test mirrors exactly.
//!
//! Asserts:
//!
//! - the whole combination succeeds (no pass errors out) with EVERY SCUS
//!   name entry translated (the shipped-pack shape that reproduced the bug);
//! - every touched sector stays EDC/ECC-valid;
//! - the translated strings are present on the patched image - re-exported
//!   through the same parsers (dialog lines that door / starting-bag
//!   relocation moved are found by value at their new offsets);
//! - the randomized doors still resolve (the door enumerator parses every
//!   scene on the patched image and finds the same site count).
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_iso::raw::SECTOR_SIZE;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::items::valid_item_pool;
use legaia_rando::translation::{ImportPhase, import_pack_phase};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// A same-length, in-charset, reversible transform of a source line: swap
/// vowels within their own case class. Keeps `{..}` markup tokens
/// byte-identical so the encoded length never changes.
fn vowel_swap(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_brace = false;
    for c in src.chars() {
        let mapped = match c {
            '{' => {
                in_brace = true;
                c
            }
            '}' => {
                in_brace = false;
                c
            }
            _ if in_brace => c,
            'a' => 'e',
            'e' => 'a',
            'i' => 'o',
            'o' => 'i',
            'A' => 'E',
            'E' => 'A',
            'I' => 'O',
            'O' => 'I',
            _ => c,
        };
        out.push(mapped);
    }
    out
}

/// Run every randomizer pass in the exact order (and with the exact settings)
/// the site's chaos preset uses in `patch_rom`.
fn run_chaos_passes(patcher: &mut DiscPatcher, seed: u64) {
    let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .expect("SCUS in image");
    let mut pool = valid_item_pool(&scus).expect("item pool");
    legaia_rando::unused::extend_pool(&mut pool, legaia_rando::unused::UNUSED_ITEM_IDS);
    apply::inject_seru_bell_name(patcher).expect("seru bell name");

    apply::randomize_drops(patcher, &pool, seed, DropMode::Random).expect("drops");
    apply::inject_equipment_bonus_drop(patcher, legaia_rando::bonus_drop::DEFAULT_CHANCE_PCT)
        .expect("equipment drops");
    apply::randomize_encounters_full(
        patcher,
        seed,
        DropMode::Random,
        apply::EncounterScope::World,
        legaia_rando::unused::UNUSED_ENEMY_IDS,
        Some(apply::SoloStrongConfig::default()),
    )
    .expect("encounters");
    apply::inject_flee_exp(patcher, legaia_rando::flee_exp::DEFAULT_PCT).expect("flee exp");
    apply::inject_enemy_ally(patcher, legaia_rando::enemy_ally::DEFAULT_PCT).expect("enemy ally");
    apply::inject_shiny_seru(patcher, legaia_rando::shiny_seru::DEFAULT_PCT).expect("shiny seru");
    apply::inject_trade_full(patcher, seed).expect("seru trade");
    let keep_static = legaia_rando::items::default_static_chest_items(&scus);
    apply::randomize_chests(patcher, &pool, seed, DropMode::Random, &keep_static).expect("chests");
    apply::randomize_shops(patcher, seed, DropMode::Random).expect("shops");
    apply::randomize_casino(patcher, seed, DropMode::Random).expect("casino");
    apply::randomize_monster_stats(patcher, seed, DropMode::Random).expect("monster stats");
    apply::randomize_move_powers(patcher, seed, DropMode::Random).expect("move power");
    apply::randomize_element_affinity(patcher, seed, DropMode::Random).expect("element affinity");
    apply::randomize_spell_costs(patcher, seed, DropMode::Random).expect("spell costs");
    apply::randomize_equip_bonuses(patcher, seed, DropMode::Random).expect("equip bonus");
    apply::randomize_weapon_specialty(patcher, seed).expect("weapon specialty");
    apply::randomize_steals(patcher, &pool, seed, DropMode::Random).expect("steals");
    apply::randomize_arts(patcher, seed, legaia_rando::arts::ArtsMode::Random).expect("arts");
    apply::randomize_doors(
        patcher,
        seed,
        DropMode::Random,
        apply::DoorCoupling::Coupled,
    )
    .expect("doors");
    apply::randomize_house_doors(patcher, seed, DropMode::Shuffle).expect("house doors");

    let seed_opts = legaia_rando::starting_items::StartingSeedOptions {
        random_items: 5,
        door_of_wind: 10,
        incense: 10,
        speed_chain: 1,
        chicken_heart: 1,
        good_luck_bell: 1,
        all_warps: true,
        extra_items: Vec::new(),
    };
    apply::randomize_starting_items(patcher, seed, &seed_opts).expect("starting items");
    let overflow = legaia_rando::starting_items::overflow_bag(seed, &seed_opts);
    if !overflow.is_empty() {
        apply::apply_starting_bag(
            patcher,
            &overflow,
            legaia_rando::starting_bag::DEFAULT_GUARD_BIT,
        )
        .expect("starting bag overflow");
    }
    apply::apply_starting_level(patcher, 10).expect("starting level");
}

#[test]
fn language_pack_plus_chaos_preset_composes() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0xC7A05_u64;

    // Build a language pack from the disc itself, filled like a real shipped
    // pack: EVERY entry translated (the all-names shape is what emptied the
    // name-keyed equipment pool on the shipped site; whole-scene dialog fills
    // are also the compression-friendly shape - a partially translated scene
    // loses cross-line LZS matches and overflows scenes a full fill fits).
    let src_patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let mut pack = legaia_rando::translation::export_pack(&src_patcher).expect("export skeleton");
    let mut expect: BTreeMap<String, String> = BTreeMap::new();
    for entries in pack.sections.each_mut() {
        for e in entries.iter_mut() {
            let t = vowel_swap(&e.source);
            if t == e.source {
                continue; // vowel-less line: no observable change
            }
            e.translation = t.clone();
            expect.insert(e.key.clone(), t);
        }
    }
    assert!(
        expect.len() > 25_000,
        "synthetic pack too small ({})",
        expect.len()
    );

    // patch_rom order: dialog sections, every randomizer pass, name sections.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let mut report =
        import_pack_phase(&mut patcher, &pack, ImportPhase::DialogOnly).expect("dialog import");
    run_chaos_passes(&mut patcher, seed);
    report.merge(
        import_pack_phase(&mut patcher, &pack, ImportPhase::NamesOnly).expect("names import"),
    );
    assert!(
        report.applied + report.already_applied > expect.len() * 95 / 100,
        "language import dropped too much: {} + {} of {} (first issues: {:?})",
        report.applied,
        report.already_applied,
        expect.len(),
        &report.issues[..report.issues.len().min(5)]
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
                "sector {i} invalid after combined patch"
            );
        }
    }
    assert!(touched > 0, "the combined patch must touch sectors");

    // The translated text survives the randomizer. Re-export from the patched
    // image: the SCUS keys (never relocated) must read back exactly; dialog
    // lines may have been relocated by the door / starting-bag passes (their
    // key offset moves with the record), so they are matched by value - the
    // overwhelming majority must read back somewhere in the corpus.
    let post_patcher = DiscPatcher::open(patched.clone()).expect("open patched");
    let re = legaia_rando::translation::export_pack(&post_patcher).expect("re-export");
    let mut translated_back = 0usize;
    let mut scus_missing: Vec<&str> = Vec::new();
    let expected_values: std::collections::BTreeSet<&str> =
        expect.values().map(|s| s.trim_end_matches(' ')).collect();
    for (_, entries) in re.sections.iter() {
        for e in entries {
            if expected_values.contains(e.source.trim_end_matches(' ')) {
                translated_back += 1;
            }
        }
    }
    for (key, want) in &expect {
        if !key.starts_with("scus:") {
            continue;
        }
        let found = re.sections.iter().any(|(_, entries)| {
            entries.iter().any(|e| {
                e.key == *key && e.source.trim_end_matches(' ') == want.trim_end_matches(' ')
            })
        });
        if !found {
            scus_missing.push(key);
        }
    }
    assert!(
        scus_missing.is_empty(),
        "SCUS translations lost under chaos: {scus_missing:?}"
    );
    assert!(
        translated_back * 100 >= expect.len() * 95,
        "translated text lost under chaos: {} of {} read back",
        translated_back,
        expect.len()
    );

    // Doors still resolve: the door enumerator parses every scene on the
    // patched image without error and finds the same number of sites.
    let base_patcher = DiscPatcher::open(original).expect("open original");
    let base_doors = apply::current_doors(&base_patcher).expect("base doors");
    let post_doors = apply::current_doors(&post_patcher).expect("post doors");
    assert_eq!(
        base_doors.len(),
        post_doors.len(),
        "door sites must survive the combined patch"
    );
}
