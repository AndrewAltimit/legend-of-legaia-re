//! Disc-gated: the three op-`0x49` submode tables the port now carries as
//! constants are re-read off PROT 0897 and asserted against the bytes.
//!
//! Each of the three is a table the engine *decides* something with, so a
//! silent drift would be a behaviour change with a green suite behind it:
//!
//! 1. `OP49_SUBOP_SLOTS` (`0x801F33A4`) - which handler a script's
//!    `49 <sub_op>` opens. Read as 14 signed bytes.
//! 2. `PTR_FUN_801F33B4` - the handler table those ids index. Only the
//!    named [`slot`] constants are checked, each against the routine the
//!    port claims to have ported.
//! 3. The panel-window record table (`0x801F2B98`) - which painter a window
//!    index selects, plus the descriptor programs that name those indices.
//!
//! No Sony bytes are asserted: every assertion is over an *address* the
//! entry's own words encode, or over a small-integer index derived from one.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (CLAUDE.md disc-gated
//! convention).

use legaia_engine_vm::baka_hub_actors as hub;

/// The field overlay that owns the `0x801F0000+` submode band. PROT 0897 at
/// base `0x801CE818` (`0x4F800` bytes) is the image that reaches it; the
/// statically extracted Baka Fighter overlay (PROT 976) stops short of it.
const FIELD_OVERLAY_PROT_INDEX: u32 = 897;
const FIELD_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// `PTR_FUN_801F33B4` - the 52-entry handler table.
const HANDLER_TABLE_VA: u32 = 0x801F_33B4;

fn overlay() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match legaia_engine_core::scene::SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    match host.index.entry_bytes_extended(FIELD_OVERLAY_PROT_INDEX) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("[skip] PROT {FIELD_OVERLAY_PROT_INDEX} unreadable: {e:#}");
            None
        }
    }
}

fn at(entry: &[u8], va: u32) -> Option<usize> {
    let off = va.checked_sub(FIELD_OVERLAY_BASE_VA)? as usize;
    (off + 4 <= entry.len()).then_some(off)
}

fn word(entry: &[u8], va: u32) -> Option<u32> {
    let off = at(entry, va)?;
    Some(u32::from_le_bytes(entry[off..off + 4].try_into().ok()?))
}

fn byte(entry: &[u8], va: u32) -> Option<i8> {
    let off = va.checked_sub(FIELD_OVERLAY_BASE_VA)? as usize;
    entry.get(off).map(|b| *b as i8)
}

#[test]
fn the_sub_op_slot_table_is_the_disc_table() {
    let Some(entry) = overlay() else { return };
    for (sub_op, want) in hub::OP49_SUBOP_SLOTS.iter().copied().enumerate() {
        let got = byte(&entry, hub::OP49_SUBOP_SLOT_TABLE + sub_op as u32)
            .unwrap_or_else(|| panic!("sub-op {sub_op} row out of range"));
        assert_eq!(
            got, want,
            "sub-op {sub_op}: the disc says {got}, the port says {want}"
        );
    }
    // The VM rejects `sub_op > 0xD`, so the table is exactly as long as the
    // sub-op space and the entry after it is the handler pointer table.
    assert_eq!(
        hub::OP49_SUBOP_SLOTS.len(),
        14,
        "the table is the VM's own `sltiu v0,v0,0xe` bound"
    );
    assert_eq!(
        word(&entry, HANDLER_TABLE_VA),
        Some(0x801F_2134),
        "the handler table follows the sub-op table and opens with the close tick"
    );
}

#[test]
fn every_named_handler_slot_points_at_the_routine_the_port_ported() {
    let Some(entry) = overlay() else { return };
    let named = [
        (hub::slot::CLOSE_TICK, 0x801F_2134u32),
        (hub::slot::DEACTIVATE, 0x801F_1D90),
        (hub::slot::DRAW_TICK, 0x801F_20B0),
        (hub::slot::COIN_COUNTER, 0x801F_0ADC),
        (hub::slot::START_MENU, 0x801F_1138),
        (hub::slot::PROMPT, 0x801F_1FDC),
        (hub::slot::SUBMENU, 0x801F_1E48),
    ];
    for (slot, want) in named {
        assert_eq!(
            word(&entry, HANDLER_TABLE_VA + u32::from(slot) * 4),
            Some(want),
            "handler slot {slot:#x}"
        );
    }
    // Slots `0x14..=0x18` alias the close tick, which is why `run_slot`
    // routes the range there.
    for slot in 0x14u32..=0x18 {
        assert_eq!(
            word(&entry, HANDLER_TABLE_VA + slot * 4),
            Some(0x801F_2134),
            "slot {slot:#x} is a close-tick alias"
        );
    }
    // The two sub-ops the engine resolves through dedicated host paths select
    // the routines those paths implement - the cross-check that the sub-op
    // table was read in the right frame.
    let slot_of = |sub_op: usize| hub::OP49_SUBOP_SLOTS[sub_op] as u32;
    assert_eq!(
        word(&entry, HANDLER_TABLE_VA + slot_of(3) * 4),
        Some(0x801F_03F0),
        "sub-op 3 is the name-entry overlay"
    );
    assert_eq!(
        word(&entry, HANDLER_TABLE_VA + slot_of(5) * 4),
        Some(0x801E_F2B0),
        "sub-op 5 is the tile-board walk state machine"
    );
}

#[test]
fn the_panel_window_records_select_the_painters_the_port_names() {
    let Some(entry) = overlay() else { return };
    let painter = |index: usize| {
        word(
            &entry,
            hub::PANEL_WINDOW_TABLE
                + index as u32 * hub::PANEL_WINDOW_STRIDE
                + hub::PANEL_WINDOW_PAINTER,
        )
    };
    let named = [
        (hub::window::TWO_OPTION, 0x801F_1950u32),
        (hub::window::COUNT_GATED_LABEL, 0x801F_1A1C),
        (hub::window::ENTRY_LIST, 0x801F_16C0),
        (hub::window::THREE_LINE, 0x801F_1890),
        (hub::window::COLUMN_ROW, 0x801F_17D8),
        (hub::window::TWO_LINE, 0x801F_1AB0),
        (hub::window::SINGLE_LABEL, 0x801F_1B64),
    ];
    for (index, want) in named {
        assert_eq!(painter(index), Some(want), "window record {index}");
        assert!(
            hub::HubPainter::for_window(index).is_some(),
            "window record {index} has a ported painter"
        );
    }
    // The table's own extent: `PANEL_WINDOW_COUNT` records, then zero fill.
    assert!(
        painter(hub::PANEL_WINDOW_COUNT - 1).is_some_and(|p| p != 0),
        "the last record carries a painter"
    );
    assert_eq!(
        painter(hub::PANEL_WINDOW_COUNT),
        Some(0),
        "the record after the table is zero fill"
    );
    // Records `0..=3` are the `0x2A` kind with no painter - the four the old
    // reading's base skipped, which is what shifted every index by four.
    for index in 0..4 {
        assert_eq!(painter(index), Some(0), "record {index} has no painter");
    }
}

#[test]
fn the_panel_descriptors_name_the_windows_the_port_maps() {
    let Some(entry) = overlay() else { return };
    // A descriptor is 8-byte entries `[i16 op][i16 window][u32]`, zero op
    // terminating - `FUN_801E9B3C`'s own walk.
    let windows_of = |va: u32| -> Vec<usize> {
        let mut out = Vec::new();
        for i in 0..16u32 {
            let Some(off) = at(&entry, va + i * 8) else {
                break;
            };
            let op = i16::from_le_bytes(entry[off..off + 2].try_into().unwrap());
            let win = i16::from_le_bytes(entry[off + 2..off + 4].try_into().unwrap());
            if op == 0 {
                break;
            }
            out.push(win as usize);
        }
        out
    };
    for desc in [
        hub::PANEL_COIN_IDLE,
        hub::PANEL_COIN_CONFIRM,
        hub::PANEL_START,
        hub::PANEL_SUBMENU_IDLE,
        hub::PANEL_SUBMENU_CONFIRM,
        hub::PANEL_PROMPT,
    ] {
        assert_eq!(
            hub::panel_windows(desc),
            windows_of(desc).as_slice(),
            "descriptor {desc:#010x}"
        );
    }
    // The draw tick's descriptor names window `0`, which has no painter -
    // so an installed panel is not the same thing as a drawn one.
    assert_eq!(hub::panel_windows(hub::PANEL_DRAW_TICK), &[0]);
    assert_eq!(windows_of(hub::PANEL_DRAW_TICK), vec![0]);
    assert!(hub::HubPainter::for_window(0).is_none());
}
