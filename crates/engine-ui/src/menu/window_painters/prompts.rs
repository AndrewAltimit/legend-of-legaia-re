//! Prompt windows - 7 and 31 - and the amount / count panels.

use super::*;

// ---------------------------------------------------------------------
// Prompt windows - 7 and 31
// ---------------------------------------------------------------------

/// Window 7: one prompt line with a single substituted character, plus the
/// confirm cursor in the bottom-right corner.
///
/// `FUN_801DCCB4` patches byte `+1` of a small scratch string at
/// `0x801E46E4` with a byte lifted out of the `0x80084140` record block
/// (record stride `0x414`, field `+0x705`, indexed by the two `i16`
/// selectors at `0x8007BB70` and `0x8007BB78`) and then draws that string.
/// So the prompt is one fixed sentence with one live character in it - the
/// port takes the whole assembled line.
///
/// The cursor lands at a fixed inset from the content origin, not from the
/// window extent: `(WX + 0xE6, WY + 0xD)`.
///
/// The record indexing is worth spelling out, because it is what a host
/// would have to reproduce: `0x8007BB70` is scaled by the `0x414` character
/// record stride (`(x<<6 + x)<<2 + x` then `<<2`) and `0x8007BB78` is added
/// as a plain byte offset, so the substituted glyph is
/// `record[0x13D + DAT_8007BB78]` off the `0x80084708` record base - not a
/// name byte, whose field is `+0x2A7`.
///
/// `+0x13D` is not an arbitrary field. It is the first entry of the
/// character's **learned-magic id list**, whose length byte sits one earlier
/// at `+0x13C` - the same byte the records page prints under "Magic"
/// (`crate::menu::screens::records_screen`, whose own rebase table pins the pair).
/// So `DAT_8007BB78` is a list index and the substituted byte is a spell id.
///
/// **Which flow opens it is pinned**, and it is narrower than "the magic
/// side": the two magic-cast sub-screens `FUN_801D9280` (all-targets) and
/// `FUN_801D9594` (single target), menu sub-screens `0x0F` / `0x10`. Both
/// run the same three beats - seed `_DAT_8007BB70` and `_DAT_8007BB78` to
/// `0xFF`, debit MP, call the effect-apply handler `FUN_800402F4`, and then
/// open this window (scripts `0x801E4D50` / `0x801E4D78`, each one command:
/// `01 07`) **only if the sentinel changed**, holding for a confirm /
/// cancel press before closing it again.
///
/// What changes the sentinel is a **spell level-up**. The apply handler's
/// two HP-heal arms compare a per-spell u16 threshold against the caster's
/// `+0x5D0` accumulator, bump the level byte at record `+0x161 + index` and
/// call the notification setter `FUN_80035C00(slot, index)`, which is the
/// pair `(gp+0x858, gp+0x860)` = `(_DAT_8007BB70, _DAT_8007BB78)`. So the
/// window says *this spell of this character went up a level*, and the
/// substituted glyph is that spell's id.
///
/// PORT: FUN_801DCCB4
/// REF: FUN_80035C00 - the `(slot, list index)` notification setter
///
/// Wired: the menu-cast leveling arm exists now -
/// `field_menu_dispatch::apply_spell_outcome` accrues the `FUN_800402F4`
/// heal grants into the caster record's per-spell XP accumulator (the same
/// `+0x8` array the battle summon path trains; `+0x5D0` off the
/// `0x80084140` save-context window is that array), runs the shared
/// threshold kernel (`magic_xp::accrue_and_level`) and returns the
/// `FUN_80035C00` pair as a `magic_xp::SpellLevelNotice`.
/// `MenuRuntime::arm_spell_level_notice` holds the beat; both hosts paint
/// this window off the disc-parsed id-7 rect while it is up and hold the
/// pad for a confirm / cancel press, exactly like the window-31 toast
/// below. The prompt line arrives as font bytes: retail's own sentence at
/// `0x801E46E4`, read off the user's disc, with its spell-name token
/// expanded to the raw name (element plate escape included), so the font
/// lays the plate out exactly where the string primitive draws it.
pub fn char_prompt_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    line: &[u8],
) -> (Vec<TextDraw>, PainterSprite) {
    let l = font.layout(line);
    (
        text_draws_for(&l, (rect.x, rect.y), MENU_TEXT_WHITE),
        PainterSprite {
            sprite: 1,
            variant: 1,
            x: rect.x + 0xE6,
            y: rect.y + 0x0D,
        },
    )
}

/// Line pitch of a `0x7C` break inside a menu string: the string primitive
/// steps down `0xE` (`docs/formats/dialog-font.md`).
pub const MENU_STRING_LINE_PITCH: i32 = 0xE;

/// Window 8, the notify window: the patched message at the content origin,
/// one row per `0x7C` line, and the corner cursor.
///
/// `FUN_801DCD58` refills the template's two operand bytes (ported as
/// `engine-menus::pause_screens::notify_window_operands`, which the caller's
/// composition runs), draws the string in ink `7` at `(WX, WY)` and puts the
/// hand sprite (kind 1, mode 1) at `(WX + 0xE6, WY + 0xD)` - the same pens as
/// window 7's [`char_prompt_draws_for`]. The lines arrive expanded: the string
/// primitive splices the `0xC1` / `0xC5` names at draw time, and the host
/// resolves them from the same tables before calling this.
///
/// REF: FUN_801DCD58
pub fn notify_prompt_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    lines: &[String],
) -> (Vec<TextDraw>, PainterSprite) {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let l = font.layout_ascii(line);
        out.extend(text_draws_for(
            &l,
            (rect.x, rect.y + i as i32 * MENU_STRING_LINE_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    (
        out,
        PainterSprite {
            sprite: 1,
            variant: 1,
            x: rect.x + 0xE6,
            y: rect.y + 0x0D,
        },
    )
}

/// Window 31: a heading, then a wide number field with a trailing label on
/// the row below, then the same corner cursor as window 7.
///
/// `FUN_801DCE20` draws the heading at the content origin, drops one row
/// pitch, writes the `0x800845B4` counter - the **Point Card** total (see
/// `docs/subsystems/shop.md`) - as an 8-digit field at the left margin in
/// the accent pen, and puts the unit label `0x40` to its right back in the
/// default pen.
///
/// **Which flow opens it** is on the disc, not a guess: the retail buy
/// commits hand the widget VM a script whose whole body is `01 1F` +
/// terminator - "open window `0x1F`" - immediately after crediting the
/// counter. `0x801E4EDC` is the quantity commit's (`FUN_801DB7F4` case 2),
/// `0x801E4EA8` the recipient picker's (`FUN_801DB380`), and both stall in
/// a following phase that returns to the buy list only on a confirm /
/// cancel press.
///
/// PORT: FUN_801DCE20
///
/// Wired: `engine-core::World::point_card` is the counter and
/// `MenuRuntime::point_card_toast` the beat; both hosts paint this window
/// while the toast is up, off the same disc-parsed rect as the shop's other
/// descriptor windows, and both label it with retail's own heading and
/// unit strings (`0x801CEA40` - whose `0xC2 0xFE` token is item `0xFE`'s
/// name - and `0x801CEA50`), read off the menu overlay by
/// `engine-menus::shop::ShopLabels`.
pub fn amount_prompt_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    heading: &str,
    value: u64,
    unit_label: &str,
) -> (Vec<TextDraw>, PainterSprite) {
    let row1 = rect.y + PAINTER_ROW_PITCH;
    let mut out = text_draws_for(
        &font.layout_ascii(heading),
        (rect.x, rect.y),
        MENU_TEXT_WHITE,
    );
    out.extend(digits_draws(
        font,
        value,
        rect.x,
        row1,
        COUNTER_DIGITS,
        PAINTER_INK_ACCENT,
    ));
    out.extend(text_draws_for(
        &font.layout_ascii(unit_label),
        (rect.x + 0x40, row1),
        MENU_TEXT_WHITE,
    ));
    (
        out,
        PainterSprite {
            sprite: 1,
            variant: 1,
            x: rect.x + 0xE6,
            y: rect.y + 0x0D,
        },
    )
}

/// Window 24: a two-digit count field, drawn only when the selection index
/// is live, over a reserved sub-rect.
///
/// `FUN_801DCC20` guards its whole body on `DAT_801E46B0 > 0`; the trailing
/// `FUN_8002C69C` box at `(WX, WY + 0x38)` sized `0x90 x 0x28` is emitted
/// either way, which is why an empty selection still reserves the space.
/// The count itself comes back from `FUN_80042F4C`, a lookup on the
/// selection index, and lands at `WX + 0x80` in the accent pen.
///
/// **This painter is only the delta.** The gated body opens with
/// `jal 0x801D0F1C` - the *shared item-info panel*, the same routine window
/// 17's `FUN_801DCB60` calls - so window 24 is the item-info window plus a
/// count. A host adopting it draws that panel into the same rect first;
/// its port is folded into [`crate::items_screen_draws_for`], which carries
/// the `FUN_801D0F1C` tag, rather than standing alone as a rect painter.
///
/// **The screen is pinned.** Window 24's rect is byte-identical to window
/// 17's, so which screen opens it could not be read off the geometry - but
/// it can be read off the open scripts. Exactly one command in the whole
/// menu overlay opens window `0x18`, and the same script opens windows `2`
/// and `0x19` with it: `0x801E4DC8`, run by menu sub-screen `0x13`
/// (`FUN_801D9C14`). Window 2 is the **Equip** title tab and window 25 the
/// active-character stat compare, so window 24 is the Equip screen's
/// item panel, not a second Items screen.
///
// Wired: the Equip screen's candidate step draws it, on both hosts, through
// the shared `pause_menu::pause_screen_draws` - window 24's frame, the shared
// item-info panel ([`crate::item_info_panel_draws_for`]) into the same rect,
// then this count over it.
/// PORT: FUN_801DCC20
/// REF: FUN_801d0f1c - the shared item-info panel this painter is the delta
/// over. Ported in `crate::menu::screens::pause_lists`, not here.
pub fn count_panel_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    selection: Option<u64>,
) -> (Vec<TextDraw>, (i32, i32, i32, i32)) {
    let reserved = (rect.x, rect.y + 0x38, 0x90, 0x28);
    let draws = match selection {
        Some(count) => digits_draws(font, count, rect.x + 0x80, rect.y, 2, PAINTER_INK_ACCENT),
        None => Vec::new(),
    };
    (draws, reserved)
}
