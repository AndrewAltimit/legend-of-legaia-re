//! The notify window: operand layout, the menu-overlay template and its line expansion.

use super::*;

/// The staged notify-window message after its two markup operands are
/// patched, plus the window's two pens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyWindow {
    /// Operand byte written after the first `0xC1` markup token.
    pub c1_operand: u8,
    /// Operand byte written after the first `0xC5` markup token.
    pub c5_operand: u8,
    /// Message pen (ink `7`) at the window content origin.
    pub text_pen: (i16, i16),
    /// Hand-sprite pen at `(WX + 0xE6, WY + 0xD)`; kind and mode are both `1`.
    pub cursor_pen: (i16, i16),
}

/// Patch the two markup operands of the **notify window** (menu-overlay
/// window `8`, the panel an item-use result opens) and resolve its pens.
///
/// The message is not formatted at draw time: the window renderer takes the
/// template resident at `DAT_801E4700` in the menu overlay's own data
/// segment, finds the first `0xC1` and the first `0xC5` markup token in it
/// (`FUN_8003CBF8`, the same `0xC0`-class lead-byte scan the dialog
/// strcpy/strcat use) and overwrites **the byte following each token** in
/// place. So the template's operand slots are placeholders the renderer
/// refills every frame, not values baked when the message was staged.
///
/// The arithmetic is what the disassembly pins: the `0xC1` operand is the
/// low byte of `selector` (`_DAT_8007BB70`) and the `0xC5` operand is
/// `base + selector * 0x40` (`_DAT_8007BB78` plus the **halfword** at
/// `_DAT_8007BB70` scaled by `0x40`), both truncated to a byte by the `sb`.
///
/// What the two globals hold is pinned by their writers. The Items use
/// sub-screen seeds both to `0xFF` before the applier runs
/// (`0x801D850C` / `0x801D8510`) and opens this window only when `BB78`
/// changed (`0x801D8548..0x801D8564`, script `0x801E4C60` = `01 08`, open
/// window 8). The only writer that changes it on that path is the
/// applier's Hyper-Art-book arm, `jal 0x80035C00` at `0x8004208C` with
/// `a0 = class - 0xB` (the roster slot) and `a1` = the art id it inserted:
/// `FUN_80035C00` is two stores, `sh a0,0x858(gp)` / `sh a1,0x860(gp)`. So
/// `selector` is the learning character and `base` the art id, the `0xC1`
/// operand names the character and the `0xC5` operand `slot * 0x40 + art`
/// is exactly the arts-name token's `[character, art]` key. The window is
/// the "learned a new art" notice.
///
/// PORT: FUN_801dcd58 (menu-overlay notify-window content renderer)
/// REF: FUN_8003cbf8 (the markup-token scan whose offset the operand write
/// is relative to)
///
/// WIRED: [`patch_notify_template`] runs this over the disc template, and
/// `field_menu_dispatch::apply_inventory_outcome` composes the notice from
/// it whenever a pause-menu item use teaches an art; both play hosts park the
/// notice on `MenuRuntime` and paint window 8 while it is up.
pub fn notify_window_operands(window: (i16, i16), selector: i16, base: u8) -> NotifyWindow {
    let (wx, wy) = window;
    NotifyWindow {
        c1_operand: selector as u8,
        c5_operand: base.wrapping_add((selector.wrapping_mul(0x40)) as u8),
        text_pen: (wx, wy),
        cursor_pen: (wx + 0xE6, wy + 0xD),
    }
}

/// VA of the notify window's message template - a MES-markup string in
/// the menu overlay's data segment (PROT 0899), not a runtime buffer:
/// its only reference is `FUN_801DCD58`'s own `lui`/`addiu` pair
/// (`0x801DCD68` / `0x801DCD6C`).
pub const NOTIFY_TEMPLATE_VA: u32 = 0x801E_4700;

/// The notify-window template out of a PROT 0899 image: the bytes at
/// [`NOTIFY_TEMPLATE_VA`] up to the terminator, `0xC0..=0xCF` tokens kept
/// whole so a `0x00` operand does not end the string. `None` when the VA is
/// outside the image or the string is empty.
///
/// No text is committed: the VA is the coordinate and this reads the bytes
/// from the image the user supplied.
pub fn notify_template_from_menu_overlay(overlay: &[u8]) -> Option<Vec<u8>> {
    // A template a language pack moved is read where `FUN_801DCD58` now
    // finds it.
    let va = legaia_game_tables::menu_string_refs::resolve_va(overlay, NOTIFY_TEMPLATE_VA);
    let off = va.checked_sub(MENU_OVERLAY_BASE_VA)? as usize;
    let rest = overlay.get(off..)?;
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(&b) = rest.get(i) {
        if b & 0xF0 == 0xC0 {
            out.extend_from_slice(rest.get(i..i + 2)?);
            i += 2;
            continue;
        }
        if b < 0x1F {
            break;
        }
        out.push(b);
        i += 1;
    }
    (!out.is_empty()).then_some(out)
}

/// Refill the template's two operand slots the way `FUN_801DCD58` does:
/// the byte after the first `0xC1` and the byte after the first `0xC5`
/// take [`notify_window_operands`]' two results.
pub fn patch_notify_template(template: &mut [u8], selector: i16, base: u8) {
    let ops = notify_window_operands((0, 0), selector, base);
    for (token, operand) in [(0xC1u8, ops.c1_operand), (0xC5u8, ops.c5_operand)] {
        let mut i = 0usize;
        while i + 1 < template.len() {
            let b = template[i];
            if b == token {
                template[i + 1] = operand;
                break;
            }
            i += if b & 0xF0 == 0xC0 { 2 } else { 1 };
        }
    }
}

/// Expand a patched notify template into display lines: `0xC1 x` splices
/// the party name `name(x)`, `0xC5 x` the arts name `art(x >> 6, x & 0x3F)`,
/// the colour escape `0xCF` and every other two-byte token draw nothing, and
/// `0x7C` breaks the line (`docs/formats/dialog-font.md`). A token the
/// resolver cannot answer splices nothing.
pub fn expand_notify_lines(
    patched: &[u8],
    name: impl Fn(u8) -> Option<String>,
    art: impl Fn(u8, u8) -> Option<String>,
) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut i = 0usize;
    while let Some(&b) = patched.get(i) {
        if b & 0xF0 == 0xC0 {
            let arg = patched.get(i + 1).copied().unwrap_or(0);
            let spliced = match b {
                0xC1 => name(arg),
                0xC5 => art(arg >> 6, arg & 0x3F),
                _ => None,
            };
            if let Some(text) = spliced {
                lines.last_mut().expect("never empty").push_str(&text);
            }
            i += 2;
            continue;
        }
        if b == 0x7C {
            lines.push(String::new());
        } else if (0x20..0x7F).contains(&b) {
            lines.last_mut().expect("never empty").push(b as char);
        }
        i += 1;
    }
    lines
}

/// The window-8 notification beat: a pause-menu item use taught `art_id` to
/// roster slot `character` (retail's `FUN_80035C00(slot, art)` pair), with
/// the disc template already patched and expanded. Held by
/// `legaia_engine_menu_runtime::menu_runtime::MenuRuntime` until a confirm / cancel press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtLearnedNotice {
    /// Roster slot that learned the art (retail `_DAT_8007BB70`).
    pub character: u8,
    /// The art id inserted (retail `_DAT_8007BB78`).
    pub art_id: u8,
    /// The message, one entry per `0x7C`-separated line.
    pub lines: Vec<String>,
}
