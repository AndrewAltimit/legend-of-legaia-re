//! The flag-window actor: panel geometry, row flip and tick.

use super::*;

// ---------------------------------------------------------------------------
// FUN_801EF014 - flag-window picker
// ---------------------------------------------------------------------------

/// Descriptor the flag-window picker reads through `_DAT_8007B450`, laid out
/// by the MAN's op-`0x49` operands.
///
/// The engine supplies it from `PanelActorHost::flag_desc` rather than from a
/// MAN, because no field script installs a flag window on the overworld.
///
/// PORT: FUN_801ef014 (`lbu 1/2/3(desc)`, `FUN_8003CE9C(desc + 4)`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlagWindowDescriptor {
    /// `desc[+1]` - how many consecutive story flags the window covers.
    pub count: u8,
    /// `desc[+2]` - the first visible row, and the fallback selection when no
    /// flag in the range is set.
    pub first_visible: u8,
    /// `desc[+3]` - how many rows are visible at once.
    pub rows: u8,
    /// `desc[+4..6]` - the base story-flag id, read as an unaligned `u16`.
    pub base_flag: i32,
}

/// Panel-descriptor index the picker sizes (`0x801F2B98 + 0x192/0x196`, i.e.
/// `14*28 + 10` and `14*28 + 14`).
pub const FLAG_WINDOW_PANEL_INDEX: usize = 14;

/// Rows the panel geometry is measured against (`8 - rows`).
pub const FLAG_WINDOW_PANEL_ROWS: i32 = 8;

/// Bottom anchor the panel's `y` is offset from.
pub const FLAG_WINDOW_PANEL_Y_BASE: i32 = 0x48;

/// Sentinel the cancel and no-op-confirm arms write into `_DAT_8007BB88`.
pub const FLAG_WINDOW_CANCEL_SENTINEL: i32 = 0x64;

/// Panel script the picker opens with.
pub const FLAG_WINDOW_SCRIPT: u32 = 0x801F_3304;

/// The panel `(y, height)` the picker writes into descriptor
/// [`FLAG_WINDOW_PANEL_INDEX`].
///
/// `height = rows * 16` and `y = (8 - rows) * 16 + 0x48`, so the window grows
/// upward from a fixed bottom edge at `0x48 + 8*16 = 0xC8`.
///
/// PORT: FUN_801ef014 (`sll v0,v0,4` / `subu` / `addiu v0,v0,0x48`)
pub fn flag_window_panel_geometry(rows: u8) -> (i32, i32) {
    let rows = i32::from(rows);
    let height = rows << 4;
    let y = ((FLAG_WINDOW_PANEL_ROWS - rows) << 4) + FLAG_WINDOW_PANEL_Y_BASE;
    (y, height)
}

/// Screen row for an absolute selection, and its inverse.
///
/// The list draws bottom-up, so retail converts in both directions with the
/// same expression: `row = rows - (sel - first_visible) - 1`. Applying it
/// twice is the identity, which is exactly how the confirm arm recovers the
/// selection from the cursor the shared kernel moved.
///
/// PORT: FUN_801ef014 (`subu v1,v1,a2; subu v0,v0,v1; addiu v0,v0,-1`)
pub fn flag_window_row_flip(value: i32, desc: FlagWindowDescriptor) -> i32 {
    i32::from(desc.rows) - (value - i32::from(desc.first_visible)) - 1
}

/// What one tick of the flag-window picker asks the host to do.
///
/// PORT: FUN_801ef014
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagWindowEffect {
    /// Clear every flag in `[base_flag, base_flag + count)` (`FUN_8003CE34`).
    ClearRange { base: i32, count: u8 },
    /// Size panel [`FLAG_WINDOW_PANEL_INDEX`].
    SizePanel { y: i32, height: i32 },
    /// `FUN_801E9B3C(script)`.
    RunPanelScript(u32),
    /// `FUN_8003CE08(base_flag + selection)` - commit the pick.
    SetFlag(i32),
    /// The terminal arm.
    Exit(ActorExit),
    /// `FUN_80031D00()`, run on every path.
    TickTextActors,
}

/// Per-frame inputs the flag-window picker reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlagWindowInput {
    /// The op-`0x49` descriptor at `_DAT_8007B450`.
    pub desc: FlagWindowDescriptor,
    /// `_DAT_8007BB80`.
    pub input_locked: bool,
    /// `_DAT_8007BB88`, the picker's selection.
    pub selection: i32,
    /// `_DAT_8007BB9C`, the row the scan remembered on entry.
    pub remembered: i32,
    /// The pad banks the shared cursor kernel reads.
    pub pad: CursorPad,
    /// `ctx[+0x50]`.
    pub handler_id: u16,
}

/// Scan result for phase 0: the first index in `[0, count)` whose flag is set,
/// or `count` when none is.
///
/// PORT: FUN_801ef014 (the `FUN_8003CE64` scan loop)
pub fn flag_window_initial_row(flag_set: impl Fn(i32) -> bool, desc: FlagWindowDescriptor) -> i32 {
    let count = i32::from(desc.count);
    let mut i = 0;
    while i < count {
        if flag_set(desc.base_flag + i) {
            return i;
        }
        i += 1;
    }
    count
}

/// One tick of `FUN_801EF014`.
///
/// Returns `(phase, selection, remembered, effects)`.
///
/// `flag_set` answers `FUN_8003CE64` for phase 0's scan; it is never consulted
/// in any other phase.
///
/// PORT: FUN_801ef014
///
/// Wired: `PanelActorKind::FlagWindow` in
/// `legaia_engine_world_views::world_map_panel_host`, over the world's shared system
/// flag bank, and - the path retail takes - handler slot `0x23` of the field
/// submode dispatcher, `legaia_engine_core::field_submode_flag_window`, which
/// the `kor` lift's `49 04` opens. The routine is a field-overlay (PROT 0897)
/// function; the world-map host is an engine affordance.
pub fn flag_window_tick(
    phase: i16,
    input: FlagWindowInput,
    flag_set: impl Fn(i32) -> bool,
) -> (i16, i32, i32, Vec<FlagWindowEffect>) {
    let desc = input.desc;
    let mut phase = phase;
    let mut selection = input.selection;
    let mut remembered = input.remembered;
    let mut out = Vec::new();
    match phase {
        0 => {
            let found = flag_window_initial_row(flag_set, desc);
            remembered = found;
            selection = if found == i32::from(desc.count) {
                i32::from(desc.first_visible)
            } else {
                found
            };
            out.push(FlagWindowEffect::ClearRange {
                base: desc.base_flag,
                count: desc.count,
            });
            let (y, height) = flag_window_panel_geometry(desc.rows);
            out.push(FlagWindowEffect::SizePanel { y, height });
            out.push(FlagWindowEffect::RunPanelScript(FLAG_WINDOW_SCRIPT));
            phase += 1;
        }
        1 => {
            if !input.input_locked {
                let mut row = flag_window_row_flip(selection, desc);
                let (outcome, _) =
                    list_cursor_input(&mut row, i32::from(desc.rows), false, &input.pad);
                let picked = flag_window_row_flip(row, desc);
                selection = picked;
                match outcome {
                    CursorOutcome::ActionA => {
                        if picked == remembered {
                            selection = FLAG_WINDOW_CANCEL_SENTINEL;
                            phase += 2;
                        } else {
                            out.push(FlagWindowEffect::SetFlag(desc.base_flag + picked));
                            phase += 1;
                        }
                    }
                    CursorOutcome::ActionB => {
                        selection = FLAG_WINDOW_CANCEL_SENTINEL;
                        phase += 2;
                    }
                    _ => {}
                }
            }
        }
        2 => phase += 1,
        3 => {
            out.push(FlagWindowEffect::Exit(ActorExit {
                saved_handler: input.handler_id,
                next_handler: 0x1A,
            }));
            phase = 0;
        }
        _ => {}
    }
    out.push(FlagWindowEffect::TickTextActors);
    (phase, selection, remembered, out)
}
