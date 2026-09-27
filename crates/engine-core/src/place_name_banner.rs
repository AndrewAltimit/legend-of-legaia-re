//! The **place-name banner**: the scene's display name shown once on entry.
//!
//! What draws it is not a panel script and not a dedicated state machine. It
//! is the ordinary `4C E1` text balloon ([`crate::text_balloon`], handler
//! `FUN_801DA7F0`), spawned by the tail of the MAN loader `FUN_8003AEB0` with
//! the scene MAN's section-2 name as its line
//! ([`docs/formats/place-names.md`](../../../docs/formats/place-names.md)).
//! Read off the loader's disassembly (`see ghidra/scripts/funcs/8003aeb0.txt`,
//! `0x8003BB40..0x8003BBDC`):
//!
//! ```text
//! 8003BB44  lw   s1,0x6ea0(v0)        ; s1 = _DAT_801C6EA0 (section-2 name)
//! 8003BB4C  lbu  v0,0x0(s1)
//! 8003BB54  beq  v0,zero,0x8003BBC4   ; empty name -> no banner
//! 8003BB5C  lbu  v0,0x1618(s0)        ; 0x80085758 byte 0
//! 8003BB64  andi v0,v0,0x20           ; system flag 2 (mask 0x80 >> 2)
//! 8003BB74  beq  v1,zero,0x8003BBC4   ; flag clear -> no banner
//! 8003BB80  jal  0x80020de0           ; spawn from template 0x8007431C
//! 8003BB98  sw   a0,0x90(s0)          ; +0x90 = the name
//! 8003BB9C  sw   zero,0x94(s0)        ; +0x94 = 0 - no parent link
//! 8003BBA0  sh   zero,0x54(s0)        ; timer 0
//! 8003BBA8  sh   v0,0x9c(s0)          ; total 0x78
//! ...                                 ; x = (0x140 - width) >> 1, y = 0xB4
//! 8003BBCC  lbu  v1,0x1618(a0)
//! 8003BBD4  andi v1,v1,0xdf           ; clear system flag 2 - on EVERY path
//! 8003BBD8  sb   v1,0x1618(a0)
//! ```
//!
//! So the banner is the text-balloon spawner `FUN_8003C764` inlined (same
//! template `0x8007431C`, whose `+0x8` handler word is `0x801DA7F0`; same
//! list `_DAT_8007C34C`; same `0x78` total and centring) with one difference:
//! the parent-link word `+0x94` is stored **zero**, where the field-VM
//! spawner stores the calling script's context. With no link, the balloon's
//! handshake reads "a new engagement starts" as the cue to end early - so
//! talking to someone during the banner dismisses it.
//!
//! The gate is system flag `2` (`0x80085758` bit `0x20`), a one-shot: the
//! loader clears it whether or not a banner spawned. Nothing in the executable
//! or the field overlay sets it with a literal index - its only writers are
//! the generic flag helpers (`FUN_8003CE08` and the motion VM's set arm at
//! `0x80039100`), i.e. a script raises it before a scene change that should
//! announce the destination.
//!
//! The field overlay's fill-fade actor `FUN_801EE5D4` was filed as the banner
//! and is not: its panel script `0x801F32B4` is one `op 5` record (close every
//! panel) and a terminator, read out of the PROT 0897 image. What that actor
//! does with the name is latch it into `_DAT_8007B44C`, which only the save
//! screen reads.
//!
//! Both hosts draw the balloon through the shared `text_balloon_box` builders,
//! so seating it on the world is the whole wiring.

use crate::text_balloon::TextBalloon;

/// System-flag index that arms the banner (`0x80085758` bit `0x20`,
/// `andi v0,v0,0x20` at `0x8003BB64` under the `0x80 >> (idx & 7)` bank law).
pub const PLACE_NAME_BANNER_FLAG: u16 = 2;

/// The balloon the MAN loader seats for `name`, or `None` when it seats none.
///
/// `name` is the section-2 body up to its NUL; `armed` is system flag
/// [`PLACE_NAME_BANNER_FLAG`]. Retail tests the name's first byte before the
/// flag, and spawns only when both hold. The caller clears the flag
/// unconditionally afterwards (see [`crate::world::World::man_load_place_name_banner`]).
///
/// PORT: FUN_8003AEB0 (`0x8003BB40..0x8003BBC0`, the inlined `FUN_8003C764`
/// spawn with a zero parent link)
pub fn man_load_banner(name: &[u8], armed: bool) -> Option<TextBalloon> {
    if name.first().copied().unwrap_or(0) == 0 || !armed {
        return None;
    }
    let mut balloon = TextBalloon::spawn(name);
    // `sw zero,0x94(s0)` at 0x8003BB9C.
    balloon.parent_link = false;
    Some(balloon)
}

/// The section-2 name bytes of a decompressed MAN, up to its first NUL -
/// retail's `_DAT_801C6EA0` pointer as the draw leaf sees it. Empty when the
/// MAN does not walk or carries no name.
///
/// Raw bytes, not [`legaia_asset::place_names::scene_name`]'s ASCII view:
/// retail tests only the first byte, so a Shift-JIS name is not "no name" to
/// the loader.
pub fn man_scene_name_bytes(man: &[u8]) -> Vec<u8> {
    let Ok(parsed) = legaia_asset::man_section::parse(man) else {
        return Vec::new();
    };
    let Some(section) = parsed
        .sections
        .get(legaia_asset::place_names::SCENE_NAME_SECTION)
    else {
        return Vec::new();
    };
    let start = section.body_offset();
    let end = section.end_offset().min(man.len());
    let Some(body) = man.get(start..end) else {
        return Vec::new();
    };
    let len = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    body[..len].to_vec()
}

impl crate::world::World {
    /// The MAN loader's banner tail: seat the place-name balloon when system
    /// flag [`PLACE_NAME_BANNER_FLAG`] is armed and the scene has a name,
    /// then clear the flag. Returns `true` when a balloon was seated.
    ///
    /// A seated banner replaces any live balloon, as retail's handler kills
    /// its predecessor on its first tick.
    ///
    /// PORT: FUN_8003AEB0 (`0x8003BB40..0x8003BBDC`)
    ///
    /// Wired: `SceneHost::load_scene` calls this right after
    /// [`Self::man_load_actor_reset`], the same order the loader runs the
    /// resume programs (`0x8003BAF0`) and then this tail. Both play hosts
    /// enter scenes through `SceneHost`, and both draw
    /// `World::cutscene.text_balloon` through `legaia_engine_ui`'s
    /// `text_balloon_*_draws_for`.
    pub fn man_load_place_name_banner(&mut self, name: &[u8]) -> bool {
        let armed = self.system_flag_test(PLACE_NAME_BANNER_FLAG);
        let banner = man_load_banner(name, armed);
        let seated = banner.is_some();
        if let Some(b) = banner {
            self.cutscene.text_balloon = Some(b);
        }
        // `andi v1,v1,0xdf` at 0x8003BBD4 - reached from every path.
        self.system_flag_clear(PLACE_NAME_BANNER_FLAG);
        seated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_balloon::{BALLOON_TOTAL, BALLOON_Y, BalloonTick};
    use crate::world::World;

    #[test]
    fn an_armed_flag_and_a_name_seat_an_unlinked_balloon() {
        let b = man_load_banner(b"Rim Elm", true).expect("banner");
        assert_eq!(b.text, b"Rim Elm");
        assert!(!b.parent_link, "the loader stores +0x94 = 0");
        assert_eq!(b.timer, 0);
        assert_eq!(b.total, BALLOON_TOTAL);
        assert_eq!(b.y, BALLOON_Y);
    }

    #[test]
    fn no_flag_or_no_name_seats_nothing() {
        assert!(man_load_banner(b"Rim Elm", false).is_none());
        assert!(man_load_banner(b"", true).is_none());
        assert!(man_load_banner(b"\0junk", true).is_none());
    }

    #[test]
    fn the_flag_is_cleared_on_every_path() {
        let mut w = World::new();
        w.system_flag_set(PLACE_NAME_BANNER_FLAG);
        assert!(w.man_load_place_name_banner(b"Sol"));
        assert!(!w.system_flag_test(PLACE_NAME_BANNER_FLAG));
        assert_eq!(w.cutscene.text_balloon.as_ref().unwrap().text, b"Sol");

        // Armed, but the scene has no name: still cleared, nothing seated.
        let mut w = World::new();
        w.system_flag_set(PLACE_NAME_BANNER_FLAG);
        assert!(!w.man_load_place_name_banner(b""));
        assert!(!w.system_flag_test(PLACE_NAME_BANNER_FLAG));
        assert!(w.cutscene.text_balloon.is_none());

        // Not armed: a live balloon is left alone.
        let mut w = World::new();
        w.cutscene.text_balloon = Some(TextBalloon::spawn(b"caption"));
        assert!(!w.man_load_place_name_banner(b"Sol"));
        assert_eq!(w.cutscene.text_balloon.as_ref().unwrap().text, b"caption");
    }

    #[test]
    fn an_unlinked_banner_ends_early_on_a_new_engagement() {
        let mut b = man_load_banner(b"Sol", true).unwrap();
        assert_eq!(b.tick(false, 1), BalloonTick::Startup);
        assert_eq!(b.tick(false, 1), BalloonTick::Draw);
        // The player engages something: the timer jumps to the total.
        assert_eq!(b.tick(true, 1), BalloonTick::Killed);
    }

    #[test]
    fn scene_name_bytes_stop_at_the_nul_and_tolerate_garbage() {
        assert!(man_scene_name_bytes(&[]).is_empty());
        assert!(man_scene_name_bytes(&[0u8; 8]).is_empty());
    }
}
