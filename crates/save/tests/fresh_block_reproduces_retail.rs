//! A retail save lifted and written into a **free** block of a blank card
//! must come out as the block retail wrote: the live-state window rides on
//! the save ([`legaia_save::SaveExtV2::retail_window`]) and the summary is
//! derived as retail's composer derives it, so the engine's card Save of a
//! retail-origin session does not zero the words it does not model.
//!
//! Runs over every Legaia save on the library cards
//! (`$LEGAIA_SAVES_LIBRARY/cards`, default `saves/library/cards`); skips and
//! passes without them. No Sony bytes are asserted - only equality between
//! the retail block and the rewrite of its own lift.

use legaia_save::emu::{self, MountedCard};
use legaia_save::{SaveFile, SaveResume, card};
use std::path::PathBuf;

fn library_cards() -> Vec<PathBuf> {
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../saves/library"));
    let Ok(rd) = std::fs::read_dir(lib.join("cards")) else {
        return Vec::new();
    };
    let mut cards: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    cards.sort();
    cards
}

#[test]
fn a_lifted_save_rewrites_into_a_free_block_as_retail_wrote_it() {
    let cards = library_cards();
    if cards.is_empty() {
        eprintln!("[skip] no library memory cards");
        return;
    }
    let mut checked = 0;
    for path in cards {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(mounted) = MountedCard::from_bytes(bytes, "library") else {
            continue;
        };
        for cell in 0..15u8 {
            let block = cell + 1;
            let Some(sc) = mounted.sc_block(block) else {
                continue;
            };
            let Ok(sf) = SaveFile::from_retail_sc_block(sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
            else {
                continue;
            };
            if sf.party.members.is_empty() {
                continue;
            }
            let resume = SaveResume::from_retail_sc_block(sc);
            let mut blank = card::formatted_card_image();
            let view = emu::detect(&blank).unwrap();
            let out = view.sc_block_mut(&mut blank, block).unwrap();
            sf.write_into_retail_sc_block(out).unwrap();
            resume.write_into_retail_sc_block(out).unwrap();
            card::write_retail_block_identity(out, u32::from(sc[0x22A]), None).unwrap();
            let window = card::RETAIL_LIVE_WINDOW_OFFSET..card::RETAIL_LIVE_STATE_SIZE;
            let stray: Vec<usize> = window.filter(|&i| sc[i] != out[i]).collect();
            assert!(
                stray.is_empty(),
                "{} block {block}: the rewrite differs from retail's block at {stray:x?}",
                path.display()
            );
            assert!(card::sc_block_checksum_valid(out));
            checked += 1;
        }
    }
    eprintln!("checked {checked} library saves");
    assert!(checked > 0, "the library cards held no Legaia save");
}

/// The same save with its window withheld - what a save the engine never
/// lifted from a block looks like - still gets the summary the load screen's
/// preview reads, derived from its own records.
#[test]
fn a_save_without_a_window_still_carries_the_load_screen_summary() {
    let cards = library_cards();
    let Some((sc, mut sf)) = cards.iter().find_map(|p| {
        let m = MountedCard::from_bytes(std::fs::read(p).ok()?, "library").ok()?;
        (1..=15u8).find_map(|b| {
            let sc = m.sc_block(b)?.to_vec();
            let sf = SaveFile::from_retail_sc_block(&sc, 4).ok()?;
            (!sf.party.members.is_empty()).then_some((sc, sf))
        })
    }) else {
        eprintln!("[skip] no library memory cards");
        return;
    };
    sf.ext_v2.retail_window = None;
    let mut blank = card::formatted_card_image();
    let view = emu::detect(&blank).unwrap();
    let out = view.sc_block_mut(&mut blank, 1).unwrap();
    sf.write_into_retail_sc_block(out).unwrap();
    let names = card::RETAIL_PARTY_NAME_TABLE_OFFSET
        ..card::RETAIL_PARTY_NAME_TABLE_OFFSET + 3 * card::RETAIL_PARTY_NAME_STRIDE;
    let summary = card::RETAIL_SUMMARY_COUNT_OFFSET..card::RETAIL_SUMMARY_SLOT_OFFSET;
    let rows = card::RETAIL_SUMMARY_IDS_OFFSET..card::RETAIL_PARTY_NAME_TABLE_OFFSET;
    let n = usize::from(sc[card::RETAIL_PARTY_COUNT_OFFSET]);
    assert_eq!(&out[summary.clone()], &sc[summary]);
    assert_eq!(
        &out[names.start..names.start + n * 0xC],
        &sc[names.start..names.start + n * 0xC]
    );
    for k in 0..n {
        assert_eq!(out[rows.start + k], sc[rows.start + k], "member {k} id");
        assert_eq!(
            out[card::RETAIL_SUMMARY_LEVELS_OFFSET + k],
            sc[card::RETAIL_SUMMARY_LEVELS_OFFSET + k],
            "member {k} level"
        );
        for (row, _) in card::RETAIL_SUMMARY_STAT_ROWS {
            assert_eq!(
                &out[row + 2 * k..row + 2 * k + 2],
                &sc[row + 2 * k..row + 2 * k + 2]
            );
        }
    }
}
