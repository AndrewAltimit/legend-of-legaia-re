//! The **Incense wear-off notice** - what the field shows on the walk tick
//! where the Incense window runs out.
//!
//! ## What retail does
//!
//! The walk-regen tick `FUN_801D0B90` (field overlay, PROT 0897) decrements
//! the Incense window `_DAT_8007B600` once per running tick. On the tick it
//! reaches zero (`0x801D0CE0..0x801D0D24`) it:
//!
//! 1. stores the static record `0x801F2278` as the entry context
//!    `_DAT_8007B450` - a 16-byte record whose kind byte is `0x0B`;
//! 2. raises bit `0x80000` in the `+0x10` word of the actor at
//!    `*(0x80083C48 + 0x1C)` - the movement lock the walk controller reads;
//! 3. spawns the submode driver actor (`FUN_80020DE0(0x8007065C, ...)`).
//!
//! The driver's enter half `FUN_801F1278` maps the record's kind byte through
//! the sub-op table at `0x801F33A4` (`lb v0,0(v0)` at `0x801F1468`): kind
//! `0x0B` -> handler slot `0x32`, which is `FUN_801F1E48`. That handler is a
//! three-state notice (`legaia_engine_vm::baka_hub_actors::submenu`):
//!
//! - state `0` installs descriptor `0x801F3294` (show window record `16`) and
//!   clears `_DAT_8007B888`;
//! - state `1` waits until `_DAT_8007B880 == 0` and the pad edge hits the
//!   configured confirm/cancel masks (`0x80084590 | 0x80084594`), plays cue
//!   `0x20` and installs `0x801F32A4` (hide window `16`);
//! - state `2` zeroes `_DAT_8007B450` and hands the actor back (slot `0x1A`,
//!   the close tick), whose retire releases the movement lock.
//!
//! Window record `16`'s painter `FUN_801F1B64` draws one string -
//! `0x801CF1A4` in the field overlay's data segment - at the panel origin
//! plus `0x0C`, and a marker sprite (`FUN_8002B994(1, 1, ...)`) at the
//! right edge. The string opens with the `0xC2 0x8A` item-name escape: item
//! `0x8A` is the Incense, so the line names the item and says its effect is
//! gone. The literal text is disc data and is read off the image at run time
//! ([`notice_line`]); nothing here carries it.
//!
//! The same handler is also reachable from a field script as op-`0x49`
//! sub-op `0xB`; no shipped script issues that sub-op.
//!
//! ## Engine shape
//!
//! [`World::raise_incense_notice`] is the zero-edge half, called by the walk
//! tick on both the field and the overworld (the overworld is a mode-3
//! field-run scene with the same field overlay resident, so the same tick
//! runs there). [`World::tick_incense_notice`] runs the ported
//! `FUN_801F1E48` body until it hands back, then drops the notice and the
//! movement lock. It does not go through the op-`0x49` park
//! ([`crate::field_submode_screen`]): no script is parked on it, and a stale
//! `Done` there would resume the next op-`0x49` a script arms.
//!
//! REF: FUN_801D0B90, FUN_801F1278, FUN_801F1E48, FUN_801F1B64

use legaia_engine_vm::baka_hub_actors::{self as hub, HubAction, HubActor, HubEnv, HubGrid};

use crate::world::World;

/// The kind byte of the static record `0x801F2278` the walk tick installs.
pub const NOTICE_KIND: u8 = 0x0B;
/// VA of that record in the field overlay (PROT 0897).
pub const NOTICE_RECORD_VA: u32 = 0x801F_2278;
/// VA of the notice string window record `16`'s painter draws.
pub const NOTICE_STRING_VA: u32 = hub::STR_SINGLE;
/// The field overlay's load base (PROT 0897, slot A).
const FIELD_OVERLAY_BASE: u32 = 0x801C_E818;
/// PROT entry (extraction index) of the field overlay.
pub const FIELD_OVERLAY_PROT_INDEX: u32 = 897;

/// Window record `16`'s geometry words (`0x801F2B98 + 16 * 0x1C`, `+0x08`
/// .. `+0x0E`): `(x, y, w, h)` = `(0x22, 0x20, 0xFC, 0x0C)`, a one-line
/// panel centred on the 320-wide screen. Read as a content rect by analogy
/// with the menu overlay's window records; the painter's `+0x0A` / `+0x0C` /
/// `+0x0E` reads are x / y / width.
pub const NOTICE_CONTENT_RECT: (i32, i32, i32, i32) = (0x22, 0x20, 0xFC, 0x0C);
/// The painter's text inset from the panel origin (`addiu a3,a3,0xc` at
/// `0x801F1B98`).
pub const NOTICE_TEXT_INSET: i32 = 0x0C;
/// The caller-drawn frame extends past the content rect by this much on
/// every side (the window-system convention `legaia_asset::menu_windows`
/// pins for the menu overlay's records).
pub const NOTICE_FRAME_PAD: i32 = 8;

/// The frame rect `(x, y, w, h)` a host draws the notice chrome at.
pub fn notice_frame_rect() -> (i32, i32, i32, i32) {
    let (x, y, w, h) = NOTICE_CONTENT_RECT;
    (
        x - NOTICE_FRAME_PAD,
        y - NOTICE_FRAME_PAD,
        w + 2 * NOTICE_FRAME_PAD,
        h + 2 * NOTICE_FRAME_PAD,
    )
}

/// The text pen `(x, y)` in 320x240 stage pixels.
pub fn notice_text_pen() -> (i32, i32) {
    let (x, y, _, _) = NOTICE_CONTENT_RECT;
    (x + NOTICE_TEXT_INSET, y)
}

/// Read the notice string's raw bytes (to its `NUL`) out of the field
/// overlay image (PROT entry `0897` as the disc holds it).
pub fn notice_string(field_overlay: &[u8]) -> Option<Vec<u8>> {
    let off = NOTICE_STRING_VA.checked_sub(FIELD_OVERLAY_BASE)? as usize;
    let tail = field_overlay.get(off..)?;
    let end = tail.iter().position(|&b| b == 0)?;
    Some(tail[..end].to_vec())
}

/// Expand the item-name escapes (`0xC2 X` / `0xC4 X`, the item table's name
/// pointer `*(0x8007436C + X * 0xC)`) through `item_name`. Other bytes pass
/// through; an escape whose name does not resolve is dropped.
pub fn expand_item_names(raw: &[u8], item_name: impl Fn(u8) -> Option<String>) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 16);
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if (b == 0xC2 || b == 0xC4)
            && let Some(&id) = raw.get(i + 1)
        {
            if let Some(name) = item_name(id) {
                out.extend_from_slice(name.as_bytes());
            }
            i += 2;
            continue;
        }
        out.push(b);
        i += 1;
    }
    out
}

/// [`notice_string`] with its item-name escapes expanded.
pub fn notice_line(
    field_overlay: &[u8],
    item_name: impl Fn(u8) -> Option<String>,
) -> Option<Vec<u8>> {
    notice_string(field_overlay).map(|raw| expand_item_names(&raw, item_name))
}

/// The live notice: the `FUN_801F1E48` actor's view.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncenseNotice {
    /// The driver actor's `+0x0A..+0x54` fields the handler reads/writes.
    pub actor: HubActor,
    /// The submode cursor context the hand-back writes into.
    pub grid: HubGrid,
    /// Window record `16` is installed (shown) - state `0` shows it, state
    /// `1`'s confirm hides it.
    pub shown: bool,
}

impl IncenseNotice {
    fn new() -> Self {
        let (x, y, w, _) = NOTICE_CONTENT_RECT;
        Self {
            actor: HubActor {
                x: x as i16,
                y: y as i16,
                width: w as i16,
                state: hub::slot::SUBMENU,
                ..HubActor::default()
            },
            grid: HubGrid {
                done_gate: 1,
                ..HubGrid::default()
            },
            shown: false,
        }
    }

    /// `true` once the handler has handed the actor back.
    pub fn handed_back(&self) -> bool {
        self.actor.state == hub::HUB_RETURN_STATE
    }
}

impl World {
    /// The Incense window's zero edge (`FUN_801D0B90` at `0x801D0CEC`):
    /// install the kind-`0x0B` notice and lock the player's movement.
    pub fn raise_incense_notice(&mut self) {
        self.locomotion.incense_notice = Some(IncenseNotice::new());
        self.lock_player_for_incense_notice(true);
    }

    fn lock_player_for_incense_notice(&mut self, on: bool) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        if let Some(a) = self.actors.get_mut(slot as usize) {
            if on {
                a.move_state.flags |= 0x0008_0000;
            } else {
                a.move_state.flags &= !0x0008_0000;
            }
        }
    }

    /// Whether the notice panel is up this frame, for the hosts' draw.
    pub fn incense_notice_shown(&self) -> bool {
        self.locomotion
            .incense_notice
            .as_ref()
            .is_some_and(|n| n.shown)
    }

    /// One frame of the notice: run the ported `FUN_801F1E48` body, fold its
    /// panel installs and confirm cue, and close the notice (releasing the
    /// movement lock) once it hands back. No-op with no notice up.
    ///
    /// REF: FUN_801F1E48 (body: [`legaia_engine_vm::baka_hub_actors::submenu`])
    pub fn tick_incense_notice(&mut self) {
        let Some(mut notice) = self.locomotion.incense_notice.take() else {
            return;
        };
        // The handler tests the **packed** retail pad word against packed
        // masks; the world's input holds the raw word.
        let pad = u32::from(crate::dev_menu::retail_packed(self.input.pad()));
        let prev = u32::from(crate::dev_menu::retail_packed(self.input.pad_prev()));
        let env = HubEnv {
            pad_edge: pad & !prev,
            pad_held: pad,
            confirm_mask: crate::field_submode_screen::SUBMODE_ACCEPT_MASK
                | crate::field_submode_screen::SUBMODE_BACK_MASK,
            ..HubEnv::default()
        };
        let frame = hub::submenu(&mut notice.actor, &env, &mut notice.grid);
        for a in &frame.actions {
            match a {
                HubAction::InstallPanel(hub::PANEL_SUBMENU_IDLE) => notice.shown = true,
                HubAction::InstallPanel(hub::PANEL_SUBMENU_CONFIRM) => notice.shown = false,
                HubAction::ConfirmCue(id) => self.replace_last_sfx_cue(i16::from(*id)),
                _ => {}
            }
        }
        if notice.handed_back() {
            self.lock_player_for_incense_notice(false);
            return;
        }
        // Keep the lock asserted while the notice is up: other field kernels
        // own the same bit and may drop it under the panel.
        self.lock_player_for_incense_notice(true);
        self.locomotion.incense_notice = Some(notice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_escapes_expand_and_other_bytes_pass_through() {
        let raw = [0xC2, 0x8A, b' ', b'x', 0xC4, 0x01, b'!'];
        let out = expand_item_names(&raw, |id| (id == 0x8A).then(|| "Item".to_string()));
        assert_eq!(out, b"Item x!".to_vec());
    }

    #[test]
    fn the_string_is_read_to_its_nul_at_the_overlay_offset() {
        let off = (NOTICE_STRING_VA - FIELD_OVERLAY_BASE) as usize;
        let mut img = vec![0xEEu8; off + 8];
        img[off..off + 4].copy_from_slice(&[0xC2, 0x8A, b'a', 0]);
        assert_eq!(notice_string(&img), Some(vec![0xC2, 0x8A, b'a']));
        assert_eq!(notice_string(&img[..off]), None);
    }

    #[test]
    fn the_frame_wraps_the_content_rect() {
        assert_eq!(notice_frame_rect(), (0x1A, 0x18, 0x10C, 0x1C));
        assert_eq!(notice_text_pen(), (0x2E, 0x20));
    }
}
