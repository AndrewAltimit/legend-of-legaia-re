//! Disc-gated reproducibility for the Baka Fighter **presentation** assets -
//! the pieces the duel is drawn with, as opposed to the rules tables covered
//! by `baka_opponents_real.rs`:
//!
//! * the HUD widget descriptor table (`DAT_801d7160`, 51 records) inside the
//!   overlay (PROT 0976), with the cells traced from `FUN_801d2afc` /
//!   `FUN_801d5ed0` anchored structurally;
//! * the HUD / banner art entry (PROT 1203): 9-TIM `TIM_LIST`, the 4-TMD
//!   stage pack, and the 30-record battle-form ANM bank;
//! * the fourteen per-rung fighter packs (PROT 1206..=1219): each a
//!   `[TIM][TMD][anim]` chunk chain whose anim bank is canonical ANM data
//!   with `bone_count` == the TMD's `nobj`.
//!
//! No Sony bytes are asserted - only structure. Skips + passes when
//! `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::baka_opponents::{self as baka};
use legaia_asset::static_overlay;
use legaia_asset::{DecodeMode, decode, minigame_art, pack, parse_player_lzs, player_anm};
use legaia_prot::archive::Archive;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn entry_bytes(archive: &mut Archive, index: u32) -> Vec<u8> {
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == index)
        .cloned()
        .unwrap_or_else(|| panic!("PROT entry {index} present"));
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    raw
}

#[test]
fn hud_widget_table_reproduces() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(baka::BAKA_OVERLAY_PROT_INDEX as u32)
        .expect("baka overlay in static map");
    let raw = entry_bytes(&mut archive, rec.prot_index);
    let overlay = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");

    let widgets = baka::parse_baka_hud(&overlay).expect("widget table parses");
    assert_eq!(widgets.len(), baka::HUD_WIDGET_COUNT);

    // Every record is drawable: a non-empty cell on a resolvable page.
    for (i, w) in widgets.iter().enumerate() {
        assert!(w.w > 0 && w.h > 0, "widget {i} has an empty cell");
        assert!(w.scale > 0, "widget {i} has a non-positive scale");
        // CLUT rows sit in the framebuffer's palette band.
        let clut_y = w.clut >> 6;
        assert!(
            (448..512).contains(&clut_y),
            "widget {i} CLUT row {clut_y} outside the palette band"
        );
    }

    // Anchors traced from the renderer dumps:
    // FUN_801d2afc's round-timer label is widget 0x12 and the stage digits
    // patch widget 5's `u` in 24px steps (`DAT_801d71cc = stage * 0x18`).
    assert_eq!(
        (widgets[5].w as usize, widgets[5].h as usize),
        (24, 32),
        "widget 5 is the 24px stage digit cell"
    );
    // The "PRESS START" strip (widget 0) is the 112x16 cell at (48, 48).
    let w0 = widgets[0];
    assert_eq!((w0.u, w0.v, w0.w, w0.h), (48, 48, 112, 16));

    // Every widget's texpage resolves to a page of the PROT 1203 TIM pack.
    let art_entry = entry_bytes(&mut archive, baka::BAKA_HUD_ART_PROT_INDEX as u32);
    let art = minigame_art::parse_art_pack(&art_entry).expect("PROT 1203 art pack");
    for (i, w) in widgets.iter().enumerate() {
        assert!(
            art.iter()
                .any(|t| t.image.fb_x == w.page_x() && t.image.fb_y == w.page_y()),
            "widget {i} texpage ({}, {}) has no art page",
            w.page_x(),
            w.page_y()
        );
    }
}

#[test]
fn hud_art_stage_and_anm_bank_reproduce() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = entry_bytes(&mut archive, baka::BAKA_HUD_ART_PROT_INDEX as u32);

    // Descriptor 0: TIM_LIST of 9 pages.
    let art = minigame_art::parse_art_pack(&entry).expect("PROT 1203 art pack");
    assert_eq!(art.len(), 9, "PROT 1203 carries 9 TIMs");
    // The widget pages named by the table: (320,0) (384,0) (448,0) (384,256)
    // (832,256).
    for (x, y) in [(320u16, 0u16), (384, 0), (448, 0), (384, 256), (832, 256)] {
        assert!(
            art.iter().any(|t| t.image.fb_x == x && t.image.fb_y == y),
            "no art page at ({x}, {y})"
        );
    }

    // Descriptor 1 (type 0x02): a pack of 4 Legaia TMDs - the stage set.
    let container = parse_player_lzs(&entry, 4).expect("PROT 1203 container");
    let tmd_desc = container
        .descriptors
        .iter()
        .find(|d| d.type_byte == 0x02)
        .expect("TMD descriptor");
    let tmd_pack = decode(&entry, tmd_desc, DecodeMode::Lzs).expect("stage TMD pack decodes");
    let bodies = pack::extract_pack(&tmd_pack).expect("stage pack");
    assert_eq!(bodies.len(), 4, "stage pack holds 4 TMDs");
    for (i, b) in bodies.iter().enumerate() {
        let tmd = legaia_tmd::parse(b).unwrap_or_else(|e| panic!("stage TMD {i}: {e}"));
        assert!(
            tmd.objects[0].primitives_byte_size > 0,
            "stage TMD {i} draws nothing"
        );
    }

    // Descriptor 2 (type 0x05): the 30-record battle-form ANM bank.
    let bundles = player_anm::find_in_entry(&entry, 4);
    assert_eq!(bundles.len(), 1, "one ANM bundle in PROT 1203");
    assert_eq!(bundles[0].record_count, 30, "30 battle-form bank records");
    // The three character banks lead with a 15/16/15-bone idle.
    for (record, bones) in [(0usize, 15u16), (9, 16), (18, 15)] {
        let r = bundles[0].record(record).expect("bank record parses");
        assert_eq!(r.marker_1, 0x080C);
        assert_eq!(r.bone_count, bones, "bank record {record} bone count");
    }
}

#[test]
fn fighter_packs_reproduce() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");

    for roster in baka::FIGHTER_PACK_FIRST_ROSTER_ID
        ..baka::FIGHTER_PACK_FIRST_ROSTER_ID + baka::FIGHTER_PACK_COUNT
    {
        let prot_index = baka::fighter_pack_prot_index(roster).expect("in range");
        let entry = entry_bytes(&mut archive, prot_index as u32);
        let pack = baka::parse_fighter_pack(&entry)
            .unwrap_or_else(|| panic!("fighter pack {prot_index} (roster {roster}) parses"));

        // The atlas: a 256x256 4bpp TIM with a 256x1 CLUT strip.
        let tim = legaia_tim::parse(&pack.tim_bytes).expect("fighter TIM parses");
        assert_eq!((tim.pixel_width(), tim.pixel_height()), (256, 256));
        let clut = tim.clut.as_ref().expect("fighter TIM carries a CLUT");
        assert!(clut.entries.len() >= 256, "256-entry CLUT strip");

        // The mesh: a Legaia TMD whose objects all draw.
        let tmd = legaia_tmd::parse(&pack.tmd_bytes).expect("fighter TMD parses");
        let nobj = tmd.objects.len();
        assert!(nobj > 0);

        // The animation bank: canonical ANM records rigged for this TMD.
        let bank = player_anm::parse(&pack.anim_bytes).expect("fighter anim bank parses");
        assert!(bank.record_count >= 1, "at least an idle record");
        let idle = bank.record(0).expect("idle record parses");
        assert_eq!(idle.marker_1, 0x080C);
        assert_eq!(
            idle.bone_count as usize, nobj,
            "roster {roster}: idle rig covers every TMD object"
        );
    }

    // Entry 1220 breaks the pattern - the chain walk must refuse it.
    let past = entry_bytes(&mut archive, 1220);
    assert!(
        baka::parse_fighter_pack(&past).is_none(),
        "entry 1220 is not a fighter pack"
    );
}
