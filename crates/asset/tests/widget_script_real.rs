//! Disc-gated: the window-widget scripts ([`legaia_asset::widget_script`])
//! resolve from the real menu overlay (PROT 0899).
//!
//! Asserts the two shop programs pinned from `FUN_801DAFD4`'s disassembly
//! (docs/subsystems/shop.md) parse at their VAs with the documented window
//! sets, and that the `jal`-site scan recovers a substantial program table
//! whose every entry re-parses. Skips + passes when `extracted/PROT.DAT` or
//! `LEGAIA_DISC_BIN` is missing.

use legaia_asset::widget_script as ws;
use legaia_prot::archive::Archive;
use std::path::PathBuf;

fn menu_overlay_bytes() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let prot = [
        PathBuf::from("extracted/PROT.DAT"),
        PathBuf::from("../../extracted/PROT.DAT"),
    ]
    .into_iter()
    .find(|p| p.is_file())?;
    let mut archive = Archive::open(&prot).ok()?;
    let entry = archive
        .entries
        .get(legaia_asset::menu_windows::MENU_OVERLAY_PROT_INDEX)?
        .clone();
    let mut buf = Vec::new();
    archive.read_entry(&entry, &mut buf).ok()?;
    Some(buf)
}

#[test]
fn shop_scripts_parse_at_pinned_vas() {
    let Some(overlay) = menu_overlay_bytes() else {
        eprintln!("[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing");
        return;
    };

    // Open script: global update, then open the five shop windows
    // (vendor plate 0x21, picker 0x2A, gold 0x20, 0x28, 0x22).
    let open = ws::parse_at(&overlay, ws::SHOP_OPEN_SCRIPT_VA).expect("open script parses");
    assert_eq!(open.insns[0].opcode, 0x05);
    let opened: Vec<u8> = open.insns[1..].iter().map(|i| i.window).collect();
    assert_eq!(opened, vec![0x21, 0x2A, 0x20, 0x28, 0x22]);
    assert!(open.insns[1..].iter().all(|i| i.opcode == 0x01));

    // Sell-transition slide-away: close 0x28 / 0x2A / 0x22, keeping the
    // gold + vendor plates.
    let away = ws::parse_at(&overlay, ws::SHOP_SELL_AWAY_SCRIPT_VA).expect("away script parses");
    let closed: Vec<u8> = away.insns.iter().map(|i| i.window).collect();
    assert_eq!(closed, vec![0x28, 0x2A, 0x22]);
    assert!(away.insns.iter().all(|i| i.opcode == 0x04));

    // The raw byte views the interpreter consumes are terminator-inclusive.
    let open_bytes = ws::script_bytes_at(&overlay, ws::SHOP_OPEN_SCRIPT_VA).unwrap();
    assert_eq!(open_bytes.len(), open.byte_len());
    assert_eq!(open_bytes[open_bytes.len() - 4], 0x00);
}

#[test]
fn scan_recovers_program_table() {
    let Some(overlay) = menu_overlay_bytes() else {
        eprintln!("[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing");
        return;
    };
    let refs = ws::scan(&overlay);
    // Non-vacuity: the scan must find a real program table, not an empty
    // sweep. (40 distinct programs resolve on the USA disc; the floor
    // leaves room for scanner-conservatism changes, not for zero.)
    assert!(refs.len() >= 30, "only {} programs recovered", refs.len());
    // The menu-open staging program is among the lui+addiu-resolvable ones.
    assert!(refs.iter().any(|r| r.script.va == ws::MENU_OPEN_SCRIPT_VA));
    for r in &refs {
        assert!(!r.call_sites.is_empty());
        // Every recovered program re-parses and re-slices.
        let bytes = ws::script_bytes_at(&overlay, r.script.va).expect("script bytes");
        assert_eq!(bytes.len(), r.script.byte_len());
    }
}
