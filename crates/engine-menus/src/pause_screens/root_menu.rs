//! Root pause-menu routing: row routes, entry / cancel subscreens and the context-locked overlay labels.

/// Number of rows the menu-overlay root command picker offers.
pub const ROOT_MENU_ROWS: u16 = 7;

/// The entry-context kind byte (`*_DAT_8007B450`) that both gates the root
/// menu's **Load** row and redirects its cancel into the Yes/No confirm.
pub const ROOT_MENU_CONTEXT_LOCKED: u8 = 0x0D;

/// The per-scene save-allow flag `_DAT_8007B6A8` gating the **Save** row.
///
/// Scene load seeds it from the MAN header's `[0x01] & 1`
/// ([`legaia_scene_formats::man_section::ManHeader::low_flag`]); a cleared flag is
/// what makes a scene a no-save scene. It is the same byte the
/// "Save Anywhere" cheat forces - see
/// [`docs/reference/memory-map.md`](../../../docs/reference/memory-map.md).
pub const ROOT_MENU_SAVE_ALLOW_FLAG: u32 = 0x8007_B6A8;

/// Sub-screen each root-menu row hands off to, in the retail draw order
/// **Items / Magic / Equip / Status / Options / Load / Save**. Rows `5`
/// (Load, `0x18`) and `6` (Save, `0x19`) are the two conditional ones -
/// see [`root_menu_confirm_route`].
///
/// The row labels are read off the menu overlay's own string pool - the
/// seven pointers `FUN_801CFD68` hands the string primitive are `@Items`,
/// `@Magic`, `@Equip`, `@Status`, `@Options`, `@Load`, `@Save` at
/// `0x801CE9D0`, `..9D8`, `..9E0`, `..9E8`, `..9F4`, `0x801CEA00`,
/// `..EA08` - so `0x18` is the **load** card driver and `0x19` the save
/// one, which is the direction retail's own op selector confirms
/// (`0x18` -> `FUN_801DD35C(1, 2)` skips the card-file erase, `0x19` ->
/// `(1, 1)` performs it).
///
/// REF: FUN_801dd35c (the card-driver body whose op selector fixes the
/// direction of the two gated rows)
pub const ROOT_MENU_ROUTES: [u8; ROOT_MENU_ROWS as usize] =
    [0x05, 0x0E, 0x12, 0x15, 0x17, 0x18, 0x19];

/// What confirming a root-menu row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootMenuRoute {
    /// Hand off to this sub-screen id (`DAT_801E46A4`).
    Sub(u8),
    /// Row is unavailable - retail plays the reject cue `0x23` and stays.
    Buzz,
    /// Row index outside `0..7`: nothing happens.
    None,
}

/// Confirm routing for the menu-overlay **root command picker**
/// (sub-screen `0x01`).
///
/// The picker runs `FUN_801D688C(&DAT_801E46BC, 7, 1)` - seven rows - and
/// routes the confirmed row through [`ROOT_MENU_ROUTES`]. Two rows are
/// conditional and buzz instead of advancing:
///
/// * **Load** (row `5`) is blocked when an entry context is installed at
///   `_DAT_8007B450` **and** its kind byte is
///   [`ROOT_MENU_CONTEXT_LOCKED`]. A null context pointer allows the row -
///   the test is on the kind, not on the pointer's presence. The same
///   context makes cancel ask first ([`root_menu_cancel_route`]), which is
///   coherent: a parked field script must not be replaced by a loaded game
///   nor abandoned without a confirm.
/// * **Save** (row `6`) is blocked when the per-scene save-allow byte
///   [`ROOT_MENU_SAVE_ALLOW_FLAG`] is zero.
///
/// Both gates are re-read by the list renderer `FUN_801CFD68`, which greys
/// the same two rows to ink `0` from the same two globals - so the confirm
/// arm never buzzes a row that drew white.
///
/// Every accepted row first clears the shared list globals
/// `_DAT_8007BB98` / `_DAT_8007BB90` / `_DAT_8007BB88`, and the Magic row
/// additionally stages `DAT_801E46C8 = DAT_801E46C4 & 0xFFF`; both are host
/// state the caller mirrors.
///
/// PORT: FUN_801d6b20 (menu-overlay sub-screen `0x01`, phase-1 confirm arm
/// `0x801D6BCC..0x801D6CF4`)
/// REF: FUN_801d688c (the cursor navigator this screen drives; ported as
/// `legaia_engine_system::menu_input`)
/// REF: FUN_801cfd68 (the row renderer whose grey arms read the same two
/// globals in the same order)
///
/// Live on the pause-menu path. [`crate::field_menu::FieldMenuSession`] calls
/// this once per row to ink the list ([`crate::field_menu::FieldMenuSession::row_is_available`],
/// which the renderer greys on) and once more on Cross to decide advance vs
/// buzz - the same double read, in the same order, that keeps retail's row
/// renderer and confirm arm agreeing. The `Sub(id)` payload is consumed rather
/// than dropped: the session resolves the confirmed row back **through** the
/// id ([`crate::field_menu::FieldMenuRow::from_retail_subscreen`]), so
/// [`ROOT_MENU_ROUTES`] decides which sub-session the shell pushes.
///
/// Both gate inputs come from the world at menu-open
/// (`BootSession::open_field_menu`): `save_allowed` from
/// `crate::party_state::PartyState::scene_save_allowed`, which scene load seeds from
/// [`legaia_scene_formats::man_section::ManHeader::low_flag`], and
/// `entry_context_kind` from
/// `legaia_engine_core::world::World::menu_entry_context_kind`. The save gate is the one
/// that bites on real data - the MAN bit is set on the three kingdom world
/// maps and clear on every field scene, so Save greys everywhere but the
/// overworld. The Load gate is plumbed and live but cannot yet reach its
/// blocking value: the port tags each op-`0x49` park with its owning context
/// instead of keeping retail's single pointer, and no path records the armed
/// sub-op, so the kind resolves to `0`, `5` or `None` - all allow branches.
pub fn root_menu_confirm_route(
    row: u16,
    entry_context_kind: Option<u8>,
    save_allowed: bool,
) -> RootMenuRoute {
    match row {
        5 => {
            if entry_context_kind == Some(ROOT_MENU_CONTEXT_LOCKED) {
                RootMenuRoute::Buzz
            } else {
                RootMenuRoute::Sub(ROOT_MENU_ROUTES[5])
            }
        }
        6 => {
            if save_allowed {
                RootMenuRoute::Sub(ROOT_MENU_ROUTES[6])
            } else {
                RootMenuRoute::Buzz
            }
        }
        r if r < ROOT_MENU_ROWS => RootMenuRoute::Sub(ROOT_MENU_ROUTES[r as usize]),
        _ => RootMenuRoute::None,
    }
}

/// Sub-screen a cancel out of the root command picker lands on: `0` (the
/// terminal exit screen) normally, and `3` - the Yes/No confirm - when the
/// installed entry context's kind byte is [`ROOT_MENU_CONTEXT_LOCKED`]. So
/// the same context that hides the Load row is the one that makes leaving
/// the menu ask first.
///
/// PORT: FUN_801d6b20 (cancel arm `0x801D6CF8..0x801D6D18`)
///
/// WIRED: [`crate::field_menu::FieldMenuSession`]'s Browsing cancel arm
/// calls this with the gate's entry-context kind and, on the locked route
/// (`3`, [`CONTEXT_LOCKED_CANCEL_SUBSCREEN`]), opens its `ReadyConfirm`
/// phase - the Yes/No leave confirm this note used to name as the missing
/// prerequisite - instead of closing. The plain route (`0`) closes the
/// menu, retail's terminal exit screen.
pub fn root_menu_cancel_route(entry_context_kind: Option<u8>) -> u8 {
    if entry_context_kind == Some(ROOT_MENU_CONTEXT_LOCKED) {
        3
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// The kind-`0x0D` entry screens: sub-screens 4 and 3
// ---------------------------------------------------------------------------

/// Sub-screen the save/menu driver **opens on** for entry-context kind
/// [`ROOT_MENU_CONTEXT_LOCKED`] - the notice panel that draws window 6.
///
/// `FUN_801DC6B4`'s entry decode writes the sub-screen id four ways, one per
/// kind, and `4` is exactly one of them:
///
/// ```text
/// 801dc8d0  lbu  v1,0x0(a0)          ; the kind byte
/// 801dc8d4  li   v0,0xd
/// 801dc8d8  bne  v1,v0,0x801dc8ec
/// 801dc8e0  li   v0,0x4
/// 801dc8e4  sw   v0,0x46a4(a1)       ; DAT_801E46A4 = 4
/// ```
///
/// Nothing else in the overlay writes `4` there, and nothing else writes
/// `0x20` (the prize exchange) either - a sweep of every
/// `sw rt,0x46a4(rs)` in PROT 0899 finds 66 writers and exactly one for
/// each of those two ids, both inside this decode. So these screens hang
/// off the entry-context kind and off nothing else.
///
/// PORT: FUN_801DC6B4 (`0x801dc8d0..0x801dc8e4`)
///
/// Live: [`menu_entry_subscreen`] answers this id for kind `0x0D`, and
/// [`crate::field_menu::FieldMenuSession::open_entry_screen`] - which both
/// play hosts call at menu-open - opens the notice panel exactly when the
/// decode lands here.
pub const CONTEXT_LOCKED_ENTRY_SUBSCREEN: u8 = 4;

/// Sub-screen the menu driver opens on when no entry context is installed:
/// the root command picker (`sw s1,0x46a4(a1)` with `s1 = 1` at `0x801DC86C`).
pub const ROOT_PICKER_SUBSCREEN: u8 = 1;
/// Entry-context kind `0` (an inline shop, op-`0x49` sub-op `0`) opens `0x1A`.
pub const CONTEXT_SHOP_ENTRY_SUBSCREEN: u8 = 0x1A;
/// Entry-context kind `1` (a field save point) opens `0x19`, the save card
/// driver - the same id the root picker's Save row routes to.
pub const CONTEXT_SAVE_ENTRY_SUBSCREEN: u8 = 0x19;
/// Entry-context kind `7` opens `0x20`, the casino prize exchange.
pub const CONTEXT_PRIZE_ENTRY_SUBSCREEN: u8 = 0x20;

/// The menu driver's **entry decode**: which sub-screen a menu opens on,
/// from the entry-context kind byte `*_DAT_8007B450`.
///
/// `FUN_801DC6B4` state 0 (`0x801DC85C..0x801DC8E4`) stores the root picker
/// (`1`) first, then overwrites it once per matching kind with four
/// independent `lbu (a0)` compares - `0` -> `0x1A`, `1` -> `0x19`, `7` ->
/// `0x20`, `0x0D` -> `4` - so any other kind, and a null context pointer,
/// keeps the picker.
///
/// One arm is outside this function's domain: a context pointer equal to
/// the literal `1` (not a record) opens sub-screen `2` and clears the
/// pointer (`0x801DC868..0x801DC878`). The port's context is a kind byte, not
/// a pointer, and nothing in it produces that sentinel.
///
/// Only the `0x0D` arm has a consumer that routes on the decoded id -
/// [`crate::field_menu::FieldMenuSession::open_entry_screen`]. The shop, save
/// point and prize exchange reach their screens through dedicated host paths
/// (the shop session, the save flow, the prize-exchange session) that open
/// them directly rather than through the pause menu.
///
/// PORT: FUN_801DC6B4 (`0x801DC85C..0x801DC8E4`, the entry decode)
pub fn menu_entry_subscreen(entry_context_kind: Option<u8>) -> u8 {
    match entry_context_kind {
        Some(0) => CONTEXT_SHOP_ENTRY_SUBSCREEN,
        Some(1) => CONTEXT_SAVE_ENTRY_SUBSCREEN,
        Some(7) => CONTEXT_PRIZE_ENTRY_SUBSCREEN,
        Some(ROOT_MENU_CONTEXT_LOCKED) => CONTEXT_LOCKED_ENTRY_SUBSCREEN,
        _ => ROOT_PICKER_SUBSCREEN,
    }
}

/// Sub-screen the root picker's **cancel** hands to under the same kind -
/// the ready check that draws window 5. See [`root_menu_cancel_route`].
pub const CONTEXT_LOCKED_CANCEL_SUBSCREEN: u8 = 3;

/// Load base of the menu overlay's string pool - the image
/// [`menu_overlay_string`] slices.
pub const MENU_OVERLAY_BASE_VA: u32 = legaia_game_tables::menu_windows::MENU_OVERLAY_BASE_VA;

/// The six label VAs `FUN_801D6360` loads into the string primitive, in
/// draw order (`lui a0,0x801d` + `addiu a0,a0,-0x1358` and its five
/// siblings at `0x801d636c..0x801d6448`).
///
/// Coordinates only - the text is read from the caller's own image, the
/// same rule `legaia_ui_formats::battle_ui_strings` follows. The sixth entry is
/// a one-byte control string rather than a line, which is why the panel
/// reads as five lines plus the advance hand.
pub const NOTICE_PANEL_LABEL_VAS: [u32; 6] = [
    0x801C_ECA8,
    0x801C_ECD4,
    0x801C_ECFC,
    0x801C_ED20,
    0x801C_ED38,
    0x801C_ED58,
];

/// The two heading VAs `FUN_801D61B0` loads above its choice group.
pub const READY_CONFIRM_HEADING_VAS: [u32; 2] = [0x801C_EC78, 0x801C_EC94];

/// The one heading VA `FUN_801D603C` loads above its choice group (window
/// 46, the prize-exchange redeem confirm).
pub const CHOICE_PANEL_HEADING_VA: u32 = 0x801C_EAC8;

/// The shared Yes / No choice labels both choice painters load.
pub const CHOICE_YES_VA: u32 = 0x801C_EA84;
/// See [`CHOICE_YES_VA`].
pub const CHOICE_NO_VA: u32 = 0x801C_EA8C;

/// Read one NUL-terminated menu-overlay string at `va` out of a PROT 0899
/// image, keeping the pool's leading `@`.
///
/// The `@` is not a marker the string primitive strips: neither the
/// expander `FUN_80036514` nor the renderer `FUN_80036888` tests for it, and
/// it is a blank glyph of width `0`, so retail draws it as a one-pixel
/// advance (`widths[c] + pad + 1`) - every pool string's first visible glyph
/// sits one pixel past its pen (the root menu's rows ink from `WX + 0x15`
/// on a retail capture, against the `WX + 0x14` pen). Keeping it makes the
/// host font place the line the same way.
///
/// Stops at the first byte that is not a glyph
/// ([`legaia_game_tables::menu_pool::is_glyph`]) as well as at the NUL, so
/// an entry that is really a one-byte control code comes back empty rather
/// than as mojibake (a lone `@` counts as empty). A high glyph byte (an
/// accent cell) comes back as the Latin-1 `char` of its value. `None` means
/// the VA is outside the image.
///
/// `va` is the retail coordinate; the string is read where the overlay's own
/// code finds it ([`legaia_game_tables::menu_string_refs::resolve_va`]), so a
/// label a language pack moved still reads.
///
/// No text is committed anywhere in this crate: the VAs above are the
/// coordinates and this reads the bytes from the image the user supplied.
pub fn menu_overlay_string(overlay: &[u8], va: u32) -> Option<String> {
    let va = legaia_game_tables::menu_string_refs::resolve_va(overlay, va);
    let off = va.checked_sub(MENU_OVERLAY_BASE_VA)? as usize;
    let rest = overlay.get(off..)?;
    let end = rest
        .iter()
        .position(|&b| !legaia_game_tables::menu_pool::is_glyph(b))
        .unwrap_or(rest.len());
    let text = &rest[..end];
    if text.strip_prefix(b"@").unwrap_or(text).is_empty() {
        return Some(String::new());
    }
    Some(text.iter().map(|&b| char::from(b)).collect())
}

/// The two prompt VAs the Throw Out confirm (`FUN_801D1B20`, window 9)
/// draws: the line after the item name and count (`0x801D1C38`) and the
/// question at `(WX + 6, WY + 0xE)` (`0x801D1C58`).
pub const THROW_CONFIRM_PROMPT_VAS: [u32; 2] = [0x801C_EA60, 0x801C_EA74];

/// The Throw Out confirm's labels, read off one PROT 0899 image: its two
/// prompt lines and the shared Yes / No. Empty until a host installs the
/// overlay - the window then draws no label text rather than invented text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThrowConfirmLabels {
    pub prompt: [String; 2],
    pub choices: [String; 2],
}

impl ThrowConfirmLabels {
    /// Read every label out of a PROT 0899 image.
    pub fn from_menu_overlay(overlay: &[u8]) -> Self {
        let s = |va: u32| menu_overlay_string(overlay, va).unwrap_or_default();
        Self {
            prompt: [
                s(THROW_CONFIRM_PROMPT_VAS[0]),
                s(THROW_CONFIRM_PROMPT_VAS[1]),
            ],
            choices: [s(CHOICE_YES_VA), s(CHOICE_NO_VA)],
        }
    }

    /// `true` once a host has installed real disc text.
    pub fn is_installed(&self) -> bool {
        !self.prompt[0].is_empty()
    }
}

/// Every label the kind-`0x0D` pair needs, read off one PROT 0899 image.
///
/// A host installs this on its session at menu-open; a session without it
/// draws the panels with no text rather than with invented text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextLockedLabels {
    /// The notice panel's lines, empty entries dropped (window 6).
    pub notice_lines: Vec<String>,
    /// The ready check's two heading lines (window 5).
    pub ready_headings: [String; 2],
    /// Yes / No, shared by both choice painters.
    pub choices: [String; 2],
}

impl ContextLockedLabels {
    /// Read every label out of a PROT 0899 image.
    pub fn from_menu_overlay(overlay: &[u8]) -> Self {
        let s = |va: u32| menu_overlay_string(overlay, va).unwrap_or_default();
        Self {
            notice_lines: NOTICE_PANEL_LABEL_VAS
                .iter()
                .map(|&va| s(va))
                .filter(|l| !l.is_empty())
                .collect(),
            ready_headings: [
                s(READY_CONFIRM_HEADING_VAS[0]),
                s(READY_CONFIRM_HEADING_VAS[1]),
            ],
            choices: [s(CHOICE_YES_VA), s(CHOICE_NO_VA)],
        }
    }

    /// `true` once a host has installed real disc text.
    pub fn is_installed(&self) -> bool {
        !self.notice_lines.is_empty() || !self.ready_headings[0].is_empty()
    }
}
