//! The battle HUD's stage layout: the active-actor bar, plaques, tabs and the resting party panels.

/// Battle-HUD stage geometry, mirrored as local literals from the
/// packet-pinned `crate::battle_chrome`; `engine-shell`'s HUD
/// tests pin the two sets equal, which is the only thing that keeps a
/// copy honest. (The roster panels' name-pen anchors are NOT mirrored -
/// [`party_panel_stage_x`] reads the `engine-vm` kernels directly.)
///
/// Retail's party readout is **two mutually-exclusive surfaces**, not one:
/// per-member roster panels at rest, and a single full-width bar for the
/// actor entering a command or acting. The bar does not replace the panels
/// by hiding them - retail parks them at `y = 230`, under its 228-line
/// display window. The engine stage is 240 lines, so `y = 230` would still
/// be visible here and the port omits the parked draws instead.
///
/// Every seat below is packet-pinned from a save-state display-list walk.
/// What is **not** pinned, and is called out where it is used: the 102x48
/// marbled panel background and the 8x16 `/` separator have no rect in the
/// engine's system-UI atlas set yet, so the panel draws in the shared
/// chrome and the separator as a font glyph.
pub(crate) const BAR_X: i32 = 8;
/// Plate top of the active-actor bar.
pub(crate) const BAR_Y: i32 = 188;
/// Interior width of the active-actor bar; the plate spans `8 ..= 312`.
pub(crate) const BAR_INTERIOR_W: i32 = 288;
/// Plate height, every plate run on the battle screen.
pub(crate) const PLATE_H: i32 = 20;
/// Width a plate run occupies for a given interior (a cap at each end).
pub(crate) const PLATE_CAP_W: i32 = 8;

/// Name-glyph pen inside the active-actor bar.
pub(crate) const BAR_NAME: (i32, i32) = (16, 192);
/// HP / MP label-sprite seats inside the bar.
pub(crate) const BAR_HP_LABEL: (i32, i32) = (80, 194);
pub(crate) const BAR_MP_LABEL: (i32, i32) = (192, 194);
/// `/` separator seats - the separator sits four rows above its numerals.
pub(super) const BAR_HP_SEPARATOR: (i32, i32) = (136, 188);
pub(super) const BAR_MP_SEPARATOR: (i32, i32) = (240, 188);
/// Numeral pen row, every field in the bar.
pub(crate) const BAR_DIGIT_Y: i32 = 192;
/// Right edges the four numeral fields are laid out back from. **Both**
/// halves of a `cur / max` pair are right-aligned - the field grows leftward
/// one 8-px cell per digit - which is what keeps a four-digit HP inside its
/// own field. A forward-running maximum is what a capture whose values are
/// all three digits looks like, and it overruns as soon as they are not.
pub(crate) const BAR_HP_CUR_RIGHT: i32 = 134;
pub(super) const BAR_HP_MAX_RIGHT: i32 = 178;
pub(crate) const BAR_MP_CUR_RIGHT: i32 = 238;
pub(super) const BAR_MP_MAX_RIGHT: i32 = 274;

/// Width and horizontal pitch of one HUD numeral cell - retail's, and the
/// unit every numeral field above is measured in.
pub(super) const DIGIT_W: i32 = 8;

/// Plate top-left of the actor-name plaque on its action seat (placement
/// record 68: content `(16, 14)`, plate `(8, 8)`).
pub(super) const PLAQUE_X: i32 = 8;
pub(super) const PLAQUE_Y: i32 = 8;
/// Plate x of the plaque when it sits behind the `Begin` tab (placement
/// record 26: content `(68, 14)`, plate `(60, 8)`).
pub(super) const PLAQUE_BEHIND_TAB_X: i32 = 60;
/// Plate top-left of the `Begin` breadcrumb tab (placement record 1's
/// target seat: content `(16, 14)`, interior 36, the gold plate class).
pub(super) const BEGIN_TAB_X: i32 = 8;
pub(super) const BEGIN_TAB_Y: i32 = 8;
/// Interior width of the `Begin` tab - record 1's `w`, which is the same
/// 36 the round prompt's `Begin` / `Run` chips are built at.
pub(super) const BEGIN_TAB_INTERIOR_W: i32 = 36;
/// The tab's word - the round prompt's own `Begin` chip label (record 1
/// keeps the chip's string pointer).
pub(super) const BEGIN_TAB_LABEL: &str = "Begin";
/// Interior width of the third breadcrumb tab - records `0x0D` / `0x0E`
/// carry `w = 0x30`. A wider engine-font label widens its own tab.
pub(super) const THIRD_TAB_INTERIOR_W: i32 = 0x30;
/// Pen row of the move-name label (placement records 76 / 77 at `y = 150`,
/// glyph pen two rows up).
pub(super) const MOVE_NAME_Y: i32 = 148;
/// Right edge the target plaque's right cap ends at (placement record 81:
/// `x = 304 - w`, so the `w + 16` plate closes at 312).
pub(super) const TARGET_PLAQUE_RIGHT: i32 = 312;
/// Vertical inset of the plaque's contents from its plate top.
pub(super) const PLAQUE_CONTENT_DY: i32 = 4;
/// Width of the element badge the plaque wears, and the gap between it and
/// the first name glyph (`battle_chrome::BADGE_W` / `PLAQUE_BADGE_GAP`).
pub(super) const PLAQUE_BADGE_W: i32 = 20;
pub(super) const PLAQUE_BADGE_GAP: i32 = 5;

/// How far a host-drawn window's skin extends past its centre rect, every
/// side - the dialog reading box's 9-slice inflation, which the tutorial
/// prompt reuses ([`crate::battle_tutorial_chrome_draws_for`]).
pub const HOST_BOX_SKIN: i32 = 8;

/// Roster-panel background size and row.
pub(super) const PANEL_W: i32 = 102;
pub(super) const PANEL_H: i32 = 48;
pub(super) const PANEL_Y: i32 = 164;

/// Per-member panel content seats, relative to the panel's top-left corner
/// (`battle_chrome::panel`).
pub(super) const PANEL_NAME: (i32, i32) = (5, 4);
pub(super) const PANEL_LV_LABEL: (i32, i32) = (64, 6);
/// Right edge the level numerals are laid out back from - right-aligned like
/// every other number on the screen, not a pen.
pub(super) const PANEL_LV_DIGITS_RIGHT: i32 = 96;
pub(super) const PANEL_LV_DIGIT_Y: i32 = 4;
pub(super) const PANEL_HP_LABEL: (i32, i32) = (4, 21);
pub(super) const PANEL_MP_LABEL: (i32, i32) = (4, 36);
pub(super) const PANEL_HP_SEPARATOR: (i32, i32) = (57, 15);
pub(super) const PANEL_MP_SEPARATOR: (i32, i32) = (57, 30);
pub(super) const PANEL_HP_DIGIT_Y: i32 = 19;
pub(super) const PANEL_MP_DIGIT_Y: i32 = 34;
pub(super) const PANEL_CUR_RIGHT: i32 = 57;
/// Right edge a maximum is laid out back from. `102 - 5`: the panel's
/// content box is inset five pixels on both sides, so a four-digit maximum
/// runs `65..97` and still clears the plate.
pub(super) const PANEL_MAX_RIGHT: i32 = 97;

/// Panel-background x seats for a party of `count`, left to right
/// (`battle_chrome::panel_seats`). These are the **backgrounds**;
/// [`party_panel_stage_x`] carries the same layout as *text* anchors, which
/// is the `+5` `PANEL_NAME` inset and only the slots `FUN_801D84C0` writes.
pub(super) fn panel_seats(count: usize) -> &'static [i32] {
    match count {
        1 => &[109],
        2 => &[58, 160],
        3 => &[7, 109, 211],
        _ => &[],
    }
}

/// Stage X of party section `ordinal` (0-based) for `count` live party
/// members - the roster panels' **name pens**, `+5` inside the panel
/// background ([`panel_seats`]).
///
/// The anchors come from the ported kernel
/// `legaia_engine_battle_vm::battle_party_panel::panel_anchors` (retail's
/// `FUN_801D84C0` per-party-size table) - read directly, not mirrored.
/// The seats that table leaves unwritten (a solo's trailing ordinals, a
/// full party's third panel) fall back to the panel seat plus the pinned
/// name inset, the identity `engine-vm`'s own `battle_chrome` test pins
/// against the same kernel.
pub fn party_panel_stage_x(count: usize, ordinal: usize) -> i32 {
    let size = count.clamp(1, 3) as u8;
    if let Some((primary, secondary)) =
        legaia_engine_battle_vm::battle_party_panel::panel_anchors(size)
    {
        if ordinal == 0 {
            return i32::from(primary);
        }
        if ordinal == 1
            && let Some(sec) = secondary
        {
            return i32::from(sec);
        }
    }
    // Seats FUN_801D84C0 writes no anchor for: seat + the +5 name inset.
    // `panel_seats` is never empty for a clamped size, so the index holds.
    let seats = crate::battle_chrome::panel_seats(size);
    let i = ordinal.min(seats.len() - 1);
    i32::from(seats[i]) + i32::from(crate::battle_chrome::PANEL_TEXT_INSET)
}

/// Pen y of the battle-intro enemy-name labels - the fifth argument `0x30`
/// `FUN_801D9D3C` hands `FUN_8003541C` (`0x801DA1FC`).
pub const BATTLE_INTRO_NAME_Y: i32 = 0x30;

/// Stage Y of the enemy target strip. An engine seat: the row layout the
/// strip reuses is `FUN_801D9D3C`'s, and that routine is the flow-`0x0A`
/// battle-**intro** enemy-name banner composer (its only reference is the
/// `jal` at `0x801D0DFC`), which places its own labels at y = 48 for the
/// intro timer and never draws the target picker. Retail's target-select
/// strip builder is unpinned; the band sits above the party panels.
pub const ENEMY_MENU_STAGE_Y: i32 = 166;

/// Row pitch a displaced enemy strip steps by ([`enemy_target_menu_rows_y`]).
/// One text row of the in-battle 14-px pitch.
const ENEMY_MENU_STEP: i32 = 14;

/// Stage Y the enemy target strip should draw at given the box a host is
/// drawing over the battle screen this frame (`BattleHudFrame::host_box`,
/// the same centre rect, `None` for "no box").
///
/// [`ENEMY_MENU_STAGE_Y`] is `166`, and retail's target-select tutorial hint
/// is style `5` - centred, bottom-anchored at `0xB0` - so a three-line hint
/// puts its own last row on **exactly** that Y. The two then draw on the same
/// pixels, which is what "Tetsu" reading through `...only one target.` is.
/// Retail's target-strip builder is unpinned (`FUN_801D9D3C` is the intro
/// banner's composer, not the picker's), so rather than guess a new fixed seat the strip steps up in whole text rows until it clears the box's
/// drawn footprint (skin included). It never moves when no box is up, so a
/// frame without a prompt is unchanged.
pub fn enemy_target_menu_rows_y(host_box: Option<(i32, i32, i32, i32)>) -> i32 {
    let Some((_, by, _, bh)) = host_box else {
        return ENEMY_MENU_STAGE_Y;
    };
    let (box_top, box_bot) = (by - HOST_BOX_SKIN, by + bh + HOST_BOX_SKIN);
    let mut y = ENEMY_MENU_STAGE_Y;
    // A glyph row is at most the pitch tall, so "clear" means the row band
    // `[y, y + pitch)` misses `[box_top, box_bot)`.
    while y + ENEMY_MENU_STEP > box_top && y < box_bot && y > ENEMY_MENU_STEP {
        y -= ENEMY_MENU_STEP;
    }
    y
}
