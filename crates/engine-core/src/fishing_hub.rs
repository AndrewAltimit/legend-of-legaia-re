//! The fishing venue's **hub menu** - the five-row screen the pond opens over
//! itself, and the screens its rows lead to - as one session-side kernel every
//! fishing host drives.
//!
//! Retail reaches it from the idle shore, not from the venue door. State `0x0C`
//! of the fishing session machine `FUN_801CF3BC` (`0x801CF990`) tests the
//! packed pad edge `_DAT_8007B874` twice: `& 0xC0` starts the cast, and
//! `& 0x110` (Triangle `0x10` / Select `0x100`) raises SFX `0x21`, zeroes the
//! menu cursor `0x801D912C` and jumps to state `0x64`. So a host that enters
//! the pond directly has not skipped the menu; it has left the player no key
//! that opens it. [`HUB_OPEN_MASK`] is that test.
//!
//! The states the menu leads to, read off the session machine's jump table
//! (`0x801CEBE0`, indexed by `DAT_801D926C`):
//!
//! | State | Body | Port |
//! |---|---|---|
//! | `0x64` | `FUN_801D0474(1)`, the interactive menu | [`HubScreen::Menu`] over [`FishingMenu`] |
//! | `0x65` | help page 0 at `(0x14, 0x10)`; any `& 0xF0` edge -> `0x66` (SFX `0x21`) | [`HubScreen::Help`]`(0)` |
//! | `0x66` | help page 1; any `& 0xF0` edge -> `0x64` (SFX `0x37`) | [`HubScreen::Help`]`(1)` |
//! | `0x6E` | `FUN_801D0F5C(1)`, then the menu non-interactive under it | [`HubScreen::Tackle`] over [`RodLureSelect`] |
//! | `0x78` | the prize list `FUN_801D0C3C(1)`, the menu under it | [`HubScreen::Exchange`], the world's exchange sub-screen |
//!
//! Both sub-screens hand back to `0x64`: the tackle screen's cancel stores
//! `100` at `0x801D1088`, the prize list's at `0x801D0DB0`. Row 0 and the
//! menu's own cancel go to `0x0A`, the run-loop init that returns to the idle
//! shore ([`HubExit::Resume`]); row 4 goes to `0xC8`, the venue exit
//! ([`HubExit::Leave`]).
//!
//! The menu's rows 2 and 3 copy the persistent lure index `_DAT_80084450` into
//! `0x801D90DC` on the way out (`0x801D0680..0x801D0690`). `0x801D90DC` is the
//! tackle screen's cursor, so the tackle screen opens on the equipped lure.
//! [`crate::fishing::FishingMenuTick::snapshot_points`] names that flag after
//! the points bank; the word it copies is the lure index, not `_DAT_8008444C`.
//!
//! The text is the user's disc: [`FishingHubText::from_overlay`] reads the menu
//! rows, the two help pages and their footers out of the as-loaded fishing
//! overlay (PROT 0972), at the addresses the draw routines form. Nothing here
//! carries a string.

use crate::fishing::{FishingMenu, RodLureSelect, help_panel_layout};

/// Load base of the fishing overlay (PROT 0972), slot A.
pub const FISHING_OVERLAY_BASE: u32 = 0x801C_E818;

/// The five menu-row strings, as `FUN_801D0474` forms them
/// (`lui a0,0x801D` + `addiu a0,a0,imm` at `0x801D0530..0x801D05B4`).
pub const MENU_ROW_VAS: [u32; 5] = [
    0x801C_EF04,
    0x801C_EF18,
    0x801C_EF24,
    0x801C_EF34,
    0x801C_EF44,
];

/// The two help pages' string-pointer tables (`0x801D72DC` / `0x801D7330`).
pub const HELP_TABLE_VAS: [u32; 2] = [0x801D_8130, 0x801D_8168];

/// Lines per help page (the `sltiu v0,s0,0xE` / `0xF` loop bounds).
pub const HELP_LINE_COUNTS: [usize; 2] = [14, 15];

/// The per-page footer strings (`0x801D7324` / `0x801D7374`).
pub const HELP_FOOTER_VAS: [u32; 2] = [0x801C_F048, 0x801C_F050];

/// Where states `0x65` / `0x66` put the help panel (`a0 = 0x14`, `a1 = 0x10`
/// at `0x801CFFA0` / `0x801CFFE4`).
pub const HELP_PANEL_ORIGIN: (i16, i16) = (0x14, 0x10);

/// Pad-edge mask that opens the menu from the idle shore (`andi v0,v0,0x110`
/// at `0x801CF9EC`): Triangle or Select, in the packed retail layout.
pub const HUB_OPEN_MASK: u32 = 0x110;

/// Pad-edge mask that turns a help page (`andi v0,v0,0xF0` at `0x801CFFBC` /
/// `0x801D0000`): any of the four face buttons.
pub const HELP_TURN_MASK: u32 = 0xF0;

/// The tackle screen's row column (`s1 - 0x27`, `s1 = 0xB0`).
pub const TACKLE_ROW_X: i16 = 0x89;
/// Its lure-count column (`s1 | 0x41`).
pub const TACKLE_COUNT_X: i16 = 0xF1;
/// First row's y (`s0 = 0x50`) and the row pitch.
pub const TACKLE_ROW_Y0: i16 = 0x50;
/// Row pitch of the tackle list.
pub const TACKLE_ROW_PITCH: i16 = 0x10;
/// The tackle cursor icon's x (`s1 - 0x38`).
pub const TACKLE_CURSOR_X: i16 = 0x78;

/// The menu's cursor icon x (`FUN_8002C488(0x5B, ..)` at `0x801D05E4`).
pub const MENU_CURSOR_X: i16 = 0x5B;

/// The hub text off the user's disc.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FishingHubText {
    /// The five menu rows, in row order.
    pub menu_rows: Vec<Vec<u8>>,
    /// Help page 0 and page 1, one entry per drawn line (empty lines kept).
    pub help: [Vec<Vec<u8>>; 2],
    /// The per-page footers.
    pub footers: [Vec<u8>; 2],
}

/// The NUL-terminated string at `va` in an image loaded at
/// [`FISHING_OVERLAY_BASE`], or `None` when `va` is outside the image or the
/// string runs off its end.
fn c_string_at(image: &[u8], va: u32) -> Option<Vec<u8>> {
    let off = va.checked_sub(FISHING_OVERLAY_BASE)? as usize;
    let rest = image.get(off..)?;
    let end = rest.iter().position(|&b| b == 0)?;
    Some(rest[..end].to_vec())
}

impl FishingHubText {
    /// Read the hub text out of the as-loaded fishing overlay.
    ///
    /// Every pointer the help tables hold must land inside the image; one
    /// that does not means this is not the fishing overlay, and the whole
    /// read is refused rather than drawn half-empty.
    pub fn from_overlay(image: &[u8]) -> Option<Self> {
        let menu_rows = MENU_ROW_VAS
            .iter()
            .map(|&va| c_string_at(image, va))
            .collect::<Option<Vec<_>>>()?;
        if menu_rows.iter().any(Vec::is_empty) {
            return None;
        }
        let mut help: [Vec<Vec<u8>>; 2] = Default::default();
        for (page, lines) in help.iter_mut().enumerate() {
            let table = (HELP_TABLE_VAS[page] - FISHING_OVERLAY_BASE) as usize;
            for i in 0..HELP_LINE_COUNTS[page] {
                let o = table + i * 4;
                let ptr = u32::from_le_bytes(image.get(o..o + 4)?.try_into().ok()?);
                lines.push(c_string_at(image, ptr)?);
            }
        }
        let footers = [
            c_string_at(image, HELP_FOOTER_VAS[0])?,
            c_string_at(image, HELP_FOOTER_VAS[1])?,
        ];
        Some(Self {
            menu_rows,
            help,
            footers,
        })
    }
}

/// Which hub screen is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubScreen {
    /// State `0x64`: the five-row menu.
    Menu,
    /// States `0x65` / `0x66`: help page 0 or 1.
    Help(u8),
    /// State `0x6E`: the rod / lure select screen.
    Tackle,
    /// State `0x78`: the point-exchange list is up (the host owns it); the
    /// menu draws under it without input.
    Exchange,
}

/// What a hub step asks the host to do beyond the session itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubExit {
    /// Back to the idle shore (row 0, or the menu's cancel): state `0x0A`.
    Resume,
    /// Open the point-exchange list (row 3, state `0x78`).
    OpenExchange,
    /// Leave the venue (row 4, state `0xC8`).
    Leave,
}

/// One hub step's outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HubStep {
    /// The SFX the step raised (`sh id, 0x8007B6D8`).
    pub sfx: Option<u16>,
    /// A request the host acts on.
    pub exit: Option<HubExit>,
}

/// The hub's own state: the screen and the two cursors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingHub {
    pub screen: HubScreen,
    /// The menu cursor `0x801D912C`.
    pub menu: FishingMenu,
    /// The tackle screen's cursor `0x801D90DC`.
    pub tackle: RodLureSelect,
}

impl FishingHub {
    /// Open the menu the way state `0x0C` does: cursor zeroed.
    pub fn open() -> Self {
        Self {
            screen: HubScreen::Menu,
            menu: FishingMenu { cursor: 0 },
            tackle: RodLureSelect { cursor: 0 },
        }
    }

    // PORT: FUN_801cf3bc (hub states 0x64 / 0x65 / 0x66 / 0x6e / 0x78 of the session jump table 0x801cebe0)
    /// Run one frame of the hub over the packed pad edge word.
    ///
    /// `lure` is the persistent lure index (seeds the tackle cursor);
    /// `count_of` answers the bag for the tackle screen. An equip lands in
    /// `equip`, as `(lure, rod)` overrides for the session to apply.
    pub fn step(
        &mut self,
        pad_edge: u32,
        lure: u32,
        count_of: impl FnMut(u32) -> i32,
        equip: &mut (Option<u32>, Option<i32>),
    ) -> HubStep {
        let mut out = HubStep::default();
        match self.screen {
            HubScreen::Menu => {
                let t = self.menu.tick(pad_edge as u16, true);
                out.sfx = t.sfx;
                match t.next_state {
                    Some(0x0A) => out.exit = Some(HubExit::Resume),
                    Some(0x65) => self.screen = HubScreen::Help(0),
                    Some(0x6E) => {
                        self.tackle.cursor = lure as i32;
                        self.screen = HubScreen::Tackle;
                    }
                    Some(0x78) => {
                        self.tackle.cursor = lure as i32;
                        self.screen = HubScreen::Exchange;
                        out.exit = Some(HubExit::OpenExchange);
                    }
                    Some(0xC8) => out.exit = Some(HubExit::Leave),
                    _ => {}
                }
            }
            HubScreen::Help(page) => {
                if pad_edge & HELP_TURN_MASK != 0 {
                    if page == 0 {
                        out.sfx = Some(0x21);
                        self.screen = HubScreen::Help(1);
                    } else {
                        out.sfx = Some(0x37);
                        self.screen = HubScreen::Menu;
                    }
                }
            }
            HubScreen::Tackle => {
                let t = self.tackle.tick(pad_edge, pad_edge, true, count_of);
                out.sfx = t.sfx;
                if t.equip_lure.is_some() {
                    equip.0 = t.equip_lure;
                }
                if t.equip_rod.is_some() {
                    equip.1 = t.equip_rod;
                }
                if t.leave {
                    self.screen = HubScreen::Menu;
                }
            }
            // The host owns the list's input; it reports the close through
            // [`FishingHub::exchange_closed`].
            HubScreen::Exchange => {}
        }
        out
    }

    /// The exchange list closed: retail's cancel arm stores `0x64`.
    pub fn exchange_closed(&mut self) {
        if self.screen == HubScreen::Exchange {
            self.screen = HubScreen::Menu;
        }
    }

    // REF: FUN_801d0474 (menu rows + cursor), FUN_801d72a0 (help pages),
    // FUN_801d0f5c (the tackle list's row render, `0x801d1114..0x801d1398`)
    /// Lay the hub out as text lines, in retail's 320x240 screen space.
    ///
    /// `tackle_row` names one tackle item and its bag count, for the tackle
    /// screen's rows (lure ids `0x9D..=0x9F`, rod ids `0xA0..=0xA2`); retail
    /// draws them through the `0xC2 id` item-name escape.
    pub fn lines(
        &self,
        text: &FishingHubText,
        lure: u32,
        rod: i32,
        mut tackle_row: impl FnMut(u32) -> (Vec<u8>, i32),
    ) -> Vec<HubLine> {
        let mut out = Vec::new();
        let menu = |out: &mut Vec<HubLine>, interactive: bool, cursor: i32| {
            for (row, t) in text.menu_rows.iter().enumerate() {
                out.push(HubLine {
                    text: t.clone(),
                    x: crate::fishing::FISHING_MENU_ROW_X,
                    y: crate::fishing::FISHING_MENU_ROW_Y0
                        + crate::fishing::FISHING_MENU_ROW_PITCH * row as i16,
                    marked: false,
                });
            }
            if interactive {
                let (x, y) = FishingMenu { cursor }.cursor_pos();
                out.push(HubLine::cursor(x, y));
            }
        };
        match self.screen {
            HubScreen::Menu => menu(&mut out, true, self.menu.cursor),
            HubScreen::Exchange => menu(&mut out, false, self.menu.cursor),
            HubScreen::Help(page) => {
                let (x, y) = HELP_PANEL_ORIGIN;
                let layout = help_panel_layout(x, y, page != 0);
                let lines = &text.help[usize::from(page.min(1))];
                for l in &layout.lines {
                    if let Some(t) = lines.get(usize::from(l.string_index)) {
                        out.push(HubLine {
                            text: t.clone(),
                            x: l.x,
                            y: l.y,
                            marked: false,
                        });
                    }
                }
                out.push(HubLine {
                    text: text.footers[usize::from(page.min(1))].clone(),
                    x: layout.footer.0,
                    y: layout.footer.1,
                    marked: false,
                });
            }
            HubScreen::Tackle => {
                menu(&mut out, false, self.menu.cursor);
                let mut y = TACKLE_ROW_Y0;
                // Three lure rows, always drawn, each with its bag count; the
                // equipped one in the marked ink (`_DAT_8007B454 = 7`).
                for i in 0..3u32 {
                    let (name, count) = tackle_row(0x9D + i);
                    out.push(HubLine {
                        text: name,
                        x: TACKLE_ROW_X,
                        y,
                        marked: lure == i && count != 0,
                    });
                    out.push(HubLine {
                        text: format!("{count:>4}").into_bytes(),
                        x: TACKLE_COUNT_X,
                        y,
                        marked: false,
                    });
                    y += TACKLE_ROW_PITCH;
                }
                // Owned rods only, stacking up without gaps.
                for i in 0..3u32 {
                    let (name, count) = tackle_row(0xA0 + i);
                    if count == 0 {
                        continue;
                    }
                    out.push(HubLine {
                        text: name,
                        x: TACKLE_ROW_X,
                        y,
                        marked: rod == i as i32,
                    });
                    y += TACKLE_ROW_PITCH;
                }
                out.push(HubLine::cursor(
                    TACKLE_CURSOR_X,
                    TACKLE_ROW_Y0 + TACKLE_ROW_PITCH * self.tackle.cursor as i16,
                ));
            }
        }
        out
    }
}

/// One laid-out hub line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubLine {
    /// Disc text bytes (dialog-font encoding), or an engine-built string.
    pub text: Vec<u8>,
    pub x: i16,
    pub y: i16,
    /// Drawn in the highlight ink (the equipped tackle).
    pub marked: bool,
}

impl HubLine {
    /// The cursor icon (`FUN_8002C488(x, y, 0x4E)`), as the `>` stand-in every
    /// host draws for that icon.
    fn cursor(x: i16, y: i16) -> Self {
        Self {
            text: b">".to_vec(),
            x,
            y,
            marked: false,
        }
    }
}

/// Text bytes as a display string, for hosts without the dialog font: printable
/// ASCII kept, the two-byte `0xCE` / `0xCF` escapes dropped with their operand,
/// anything else dropped.
pub fn hub_text_lossy(bytes: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        match b {
            0xCE | 0xCF => i += 1,
            0x20..=0x7E => s.push(b as char),
            _ => {}
        }
    }
    s.trim_end().to_string()
}

impl crate::fishing::PondSession {
    /// The hub, when it is up.
    pub fn hub(&self) -> Option<&FishingHub> {
        self.hub.as_ref()
    }

    /// Run the hub for one frame over the packed pad edge word
    /// (`_DAT_8007B874` layout).
    ///
    /// Closed, it opens on [`HUB_OPEN_MASK`] - but only from the idle shore,
    /// as retail tests the mask in state `0x0C` alone. Open, it runs the
    /// screen and applies a tackle equip to the session's persistent lure /
    /// rod. [`HubExit::Resume`] and [`HubExit::Leave`] close it; the host acts
    /// on `Leave` and `OpenExchange`. While it is open,
    /// [`crate::fishing::PondSession::tick`] leaves the pond alone.
    // PORT: FUN_801cf3bc (state 0x0c's hub edge, `andi v0,v0,0x110` at 0x801cf9ec)
    pub fn hub_step(&mut self, pad_edge: u32, count_of: impl FnMut(u32) -> i32) -> HubStep {
        let Some(hub) = self.hub.as_mut() else {
            if self.phase() == crate::fishing::PondPhase::Idle && pad_edge & HUB_OPEN_MASK != 0 {
                self.hub = Some(FishingHub::open());
                return HubStep {
                    sfx: Some(0x21),
                    exit: None,
                };
            }
            return HubStep::default();
        };
        let mut equip = (None, None);
        let step = hub.step(pad_edge, self.lure, count_of, &mut equip);
        if let Some(l) = equip.0 {
            self.lure = l;
        }
        if let Some(r) = equip.1 {
            self.rod = r;
        }
        if matches!(step.exit, Some(HubExit::Resume | HubExit::Leave)) {
            self.hub = None;
        }
        step
    }

    /// The host's exchange list closed.
    pub fn hub_exchange_closed(&mut self) {
        if let Some(h) = self.hub.as_mut() {
            h.exchange_closed();
        }
    }

    /// The hub's draw lines (empty while it is closed).
    pub fn hub_lines(
        &self,
        text: &FishingHubText,
        tackle_row: impl FnMut(u32) -> (Vec<u8>, i32),
    ) -> Vec<HubLine> {
        self.hub
            .as_ref()
            .map(|h| h.lines(text, self.lure, self.rod, tackle_row))
            .unwrap_or_default()
    }
}

impl crate::world::World {
    /// The fishing hub's frame on the world: the step every world host (the
    /// native window, the browser play page) runs before the pond.
    ///
    /// Returns `true` when the hub owns the frame, so the pond must not tick.
    /// Row 3 opens the world's exchange sub-screen through
    /// [`crate::world::World::fishing_exchange_input`] (the same kernel both
    /// hosts' exchange keys drive), and the hub waits under it; row 4 leaves
    /// the venue exactly as each host's own quit key does
    /// ([`crate::world::World::exit_fishing`] then the minigame round-trip
    /// close).
    pub fn tick_fishing_hub(&mut self) -> bool {
        use crate::fishing_exchange_input::ExchangeInput;
        let exchange_open = self.minigames.fishing_exchange.is_some();
        let edge = (self.input.pad() & !self.input.pad_prev()).rotate_right(8) as u32;
        let bag = &self.party.inventory;
        let Some(session) = self.minigames.fishing.as_mut() else {
            return false;
        };
        if !exchange_open {
            session.hub_exchange_closed();
        }
        if exchange_open {
            let hub_up = session.hub().is_some();
            self.fishing_exchange_pad(edge);
            return hub_up;
        }
        let step = session.hub_step(edge, |id| {
            i32::from(bag.get(&(id as u8)).copied().unwrap_or(0))
        });
        let open = session.hub().is_some();
        if let Some(sfx) = step.sfx {
            self.minigames.pending_sfx.push(sfx);
        }
        match step.exit {
            Some(HubExit::Leave) => {
                self.exit_fishing();
                self.close_minigame_round_trip();
                return true;
            }
            Some(HubExit::OpenExchange) => {
                match self.minigames.fishing_prize_venues.clone() {
                    Some(venues) => {
                        self.fishing_exchange_input(&venues, ExchangeInput::Toggle);
                    }
                    // No pages decoded: nothing to open, so the menu stays.
                    None => {
                        if let Some(s) = self.minigames.fishing.as_mut() {
                            s.hub_exchange_closed();
                        }
                    }
                }
            }
            _ => {}
        }
        open || step.exit.is_some()
    }

    /// The prize list's own pad step (state `0x78`, `FUN_801D0C3C(1)`), fed
    /// the packed pad edge. Without it the list opened from the hub's row 3
    /// answered only host keys (the native window's `P` family, the browser
    /// page's panel buttons), so a pad-only player - a gamepad, the play page
    /// - was left on a screen nothing could close.
    ///
    /// Read off the disassembly (`0x801D0CB8..0x801D0DB0`): Up (`0x1000`) and
    /// Down (`0x4000`) move the cursor with SFX `0x21`, clamped to the list
    /// floor; `& 0x44` (Cross / L1) buys at the cursor through the
    /// availability test `FUN_801D6F90` - SFX `0x20` when it passes, `0x22`
    /// when refused; `& 0x21` (Circle / L2) cancels with SFX `0x37` back to
    /// the hub menu (state `0x64`). Retail reads the cursor keys off the
    /// auto-repeat word; this reads the edge. Left / Right switch the venue
    /// page - a port affordance retail has no key for (each pond shows its
    /// own page).
    ///
    /// The buy is one unit: retail's quantity picker (`0x7A`) and confirm
    /// (`0x79`) screens are not modelled; the grant, the spend and the
    /// one-time latch are [`crate::world::World::fishing_exchange_buy`]'s.
    pub(crate) fn fishing_exchange_pad(&mut self, edge: u32) {
        use crate::fishing_exchange_input::{ExchangeInput, ExchangeOutcome};
        const UP: u32 = 0x1000;
        const RIGHT: u32 = 0x2000;
        const DOWN: u32 = 0x4000;
        const LEFT: u32 = 0x8000;
        const CONFIRM: u32 = 0x44;
        const CANCEL: u32 = 0x21;
        let Some(venues) = self.minigames.fishing_prize_venues.clone() else {
            // No pages decoded: nothing the list could show, so any cancel
            // or confirm closes it rather than holding the pad.
            if edge & (CONFIRM | CANCEL) != 0 {
                self.close_fishing_exchange();
            }
            return;
        };
        let mut sfx = Vec::new();
        for (mask, input) in [
            (UP, ExchangeInput::Up),
            (DOWN, ExchangeInput::Down),
            (LEFT | RIGHT, ExchangeInput::SwitchVenue),
        ] {
            if edge & mask != 0
                && self.fishing_exchange_input(&venues, input) != ExchangeOutcome::Refused
            {
                sfx.push(0x21);
            }
        }
        if edge & CONFIRM != 0 {
            match self.fishing_exchange_input(&venues, ExchangeInput::Buy) {
                ExchangeOutcome::Bought(_) => sfx.push(0x20),
                _ => sfx.push(0x22),
            }
        }
        if edge & CANCEL != 0 && self.minigames.fishing_exchange.is_some() {
            self.fishing_exchange_input(&venues, ExchangeInput::Toggle);
            sfx.push(0x37);
        }
        self.minigames.pending_sfx.extend(sfx);
    }

    /// The hub's draw lines on the world: the session's layout with the
    /// tackle rows named off the SCUS item table and counted off the bag.
    /// Empty while no hub is up (or no hub text decoded). The one layout
    /// both world hosts hand to `legaia_engine_ui::ui_fishing_hub`.
    pub fn fishing_hub_lines(&self) -> Vec<HubLine> {
        let (Some(s), Some(text)) = (
            self.minigames.fishing.as_ref(),
            self.minigames.fishing_hub_text.as_ref(),
        ) else {
            return Vec::new();
        };
        let names = self.menu.text.as_ref();
        let bag = &self.party.inventory;
        s.hub_lines(text, |id| {
            let name = names
                .and_then(|t| t.item_name(id as u8))
                .map(|n| n.as_bytes().to_vec())
                .unwrap_or_else(|| format!("item {id:#04x}").into_bytes());
            (name, i32::from(bag.get(&(id as u8)).copied().unwrap_or(0)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Vec<u8> {
        // A synthetic image with the hub's pointers and strings in place.
        let mut img = vec![0u8; 0xB000];
        let put = |img: &mut Vec<u8>, va: u32, s: &[u8]| {
            let o = (va - FISHING_OVERLAY_BASE) as usize;
            img[o..o + s.len()].copy_from_slice(s);
            img[o + s.len()] = 0;
        };
        for (i, &va) in MENU_ROW_VAS.iter().enumerate() {
            put(&mut img, va, format!("row{i}").as_bytes());
        }
        let mut va = 0x801C_E820u32;
        for page in 0..2 {
            for i in 0..HELP_LINE_COUNTS[page] {
                put(&mut img, va, format!("p{page}l{i}").as_bytes());
                let o = (HELP_TABLE_VAS[page] - FISHING_OVERLAY_BASE) as usize + i * 4;
                img[o..o + 4].copy_from_slice(&va.to_le_bytes());
                va += 8;
            }
        }
        put(&mut img, HELP_FOOTER_VAS[0], b"f0");
        put(&mut img, HELP_FOOTER_VAS[1], b"f1");
        img
    }

    #[test]
    fn the_text_reads_every_row_page_and_footer() {
        let t = FishingHubText::from_overlay(&image()).expect("reads");
        assert_eq!(t.menu_rows.len(), 5);
        assert_eq!(t.help[0].len(), 14);
        assert_eq!(t.help[1].len(), 15);
        assert_eq!(t.help[1][14], b"p1l14");
        assert_eq!(t.footers[1], b"f1");
    }

    #[test]
    fn a_help_pointer_outside_the_image_refuses_the_read() {
        let mut img = image();
        let o = (HELP_TABLE_VAS[0] - FISHING_OVERLAY_BASE) as usize;
        img[o..o + 4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        assert!(FishingHubText::from_overlay(&img).is_none());
    }

    #[test]
    fn row_one_walks_both_help_pages_and_back() {
        let mut h = FishingHub::open();
        let mut eq = (None, None);
        // Down once, confirm (Cross 0x40).
        h.step(0x4000, 0, |_| 0, &mut eq);
        h.step(0x40, 0, |_| 0, &mut eq);
        assert_eq!(h.screen, HubScreen::Help(0));
        let s = h.step(0x20, 0, |_| 0, &mut eq);
        assert_eq!((h.screen, s.sfx), (HubScreen::Help(1), Some(0x21)));
        let s = h.step(0x10, 0, |_| 0, &mut eq);
        assert_eq!((h.screen, s.sfx), (HubScreen::Menu, Some(0x37)));
    }

    #[test]
    fn the_help_page_draws_its_lines_on_the_13_px_pitch_and_the_footer() {
        let t = FishingHubText::from_overlay(&image()).unwrap();
        let h = FishingHub {
            screen: HubScreen::Help(1),
            ..FishingHub::open()
        };
        let l = h.lines(&t, 0, 0, |_| (Vec::new(), 0));
        assert_eq!(l.len(), 16);
        assert_eq!((l[0].x, l[0].y), (0x14, 0x10));
        assert_eq!(l[14].y, 0x10 + 13 * 14);
        assert_eq!(
            (l[15].x, l[15].y, &l[15].text[..]),
            (0xE0, 0xCA, &b"f1"[..])
        );
    }

    #[test]
    fn the_menu_cancel_and_row_zero_resume_and_row_four_leaves() {
        let mut eq = (None, None);
        let mut h = FishingHub::open();
        assert_eq!(h.step(0x20, 0, |_| 0, &mut eq).exit, Some(HubExit::Resume));
        let mut h = FishingHub::open();
        assert_eq!(h.step(0x40, 0, |_| 0, &mut eq).exit, Some(HubExit::Resume));
        let mut h = FishingHub::open();
        h.step(0x1000, 0, |_| 0, &mut eq); // up from 0 snaps to 4
        assert_eq!(h.step(0x40, 0, |_| 0, &mut eq).exit, Some(HubExit::Leave));
    }

    #[test]
    fn the_tackle_screen_opens_on_the_equipped_lure_and_equips_a_rod() {
        let mut eq = (None, None);
        let mut h = FishingHub::open();
        h.menu.cursor = 2;
        h.step(0x40, 1, |_| 1, &mut eq);
        assert_eq!((h.screen, h.tackle.cursor), (HubScreen::Tackle, 1));
        // Down twice to the first rod row, confirm.
        h.step(0x4000, 1, |_| 1, &mut eq);
        h.step(0x4000, 1, |_| 1, &mut eq);
        h.step(0x40, 1, |_| 1, &mut eq);
        assert_eq!(eq.1, Some(0));
        // Cancel hands back to the menu (state 100).
        h.step(0x20, 1, |_| 1, &mut eq);
        assert_eq!(h.screen, HubScreen::Menu);
    }

    #[test]
    fn the_exchange_row_waits_under_the_list_and_returns_to_the_menu() {
        let mut eq = (None, None);
        let mut h = FishingHub::open();
        h.menu.cursor = 3;
        assert_eq!(
            h.step(0x40, 0, |_| 0, &mut eq).exit,
            Some(HubExit::OpenExchange)
        );
        assert_eq!(h.screen, HubScreen::Exchange);
        h.exchange_closed();
        assert_eq!(h.screen, HubScreen::Menu);
    }

    #[test]
    fn the_lossy_text_drops_escapes_with_their_operand() {
        assert_eq!(hub_text_lossy(b"Next \xCE\x01"), "Next");
        assert_eq!(hub_text_lossy(b"a\xCF\x05b"), "ab");
    }
}
