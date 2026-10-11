//! The [`FieldPlayerHost`] supertrait of the field-VM host surface.

use crate::field::{FieldCtx, PlayerCoords};

/// The player object, which the VM reaches by channel `0xF8` rather than by context.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldPlayerHost {
    /// Op `4C 3A`: the player takes the **arrival facing**.
    ///
    /// The arm (`0x801E10DC..0x801E10F4`) writes
    /// `*(short *)(_DAT_8007C364 + 0x26) = _DAT_80073EFC`: the player's
    /// heading from the arrival-facing word a door's op `0x3F` sets through
    /// the compass table at `0x80073F04` (`0x801DEBAC`), and which the card
    /// load (`FUN_8003AEB0`, `0x8003B778`) and the new-game seed zero. Every
    /// scene's entry script issues it once. The player is host state the VM
    /// does not thread through, so the host owns the write. PC += 2.
    fn apply_arrival_facing(&mut self) {}

    /// Op `0x31` / `0x32` (`CFLAG_SET` / `CFLAG_CLR`) aimed at the player
    /// channel `0xF8`: retail's extended prologue resolves `0xF8` to the
    /// player object `_DAT_8007C364` (`FUN_8003C83C`), so `B1 F8 <bit>` /
    /// `B2 F8 <bit>` write the **player's** `+0x10` word, not the caller's.
    /// The player object is host state the VM does not thread through, so
    /// the host owns the write. Return `true` when the host applied it (the
    /// VM then advances without touching `ctx`); the default `false` keeps
    /// the op on the context the caller passed.
    fn player_cflag(&mut self, bit: u8, set: bool) -> bool {
        let _ = (bit, set);
        false
    }

    /// Op `4C 50` (the actor model set) aimed at the player channel `0xF8`
    /// (`CC F8 50 lo hi`): the extended prologue resolves `0xF8` to the
    /// player object (`FUN_8003C83C`), so the arm's `+0x10` party-bank write
    /// (`value >= 0xF0` raises `0x01000000`, otherwise clears it) and the
    /// re-stage `FUN_80024E08` land on the player, not on the caller. Return
    /// `true` when the host applied it (the VM then advances without
    /// touching `ctx`); the default `false` keeps the op on the context the
    /// caller passed.
    fn player_set_model(&mut self, value: i16) -> bool {
        let _ = value;
        false
    }

    /// Op 0x4C sub-3 sub-7 (player-coord copy onto a non-player ctx).
    ///
    /// 2-byte instruction `[4C, 0x37]`. When `ctx` is not the player ctx
    /// (`_DAT_8007C364`), the original copies the player's world coords +
    /// `+0x26` slot onto `ctx`:
    ///
    /// ```text
    /// ctx[+0x14] = player[+0x14]   ; world_x
    /// ctx[+0x16] = player[+0x16]   ; world_y
    /// ctx[+0x18] = player[+0x18]   ; world_z
    /// ctx[+0x26] = player[+0x26]   ; field_26
    /// if ctx[+0x10] & 0x20000000:
    ///     ctx[+0x8e] = -player[+0x16]    ; inverted Y
    ///     return caseD_4()                ; bail via STATE_RESUME path
    /// ```
    ///
    /// The host owns the player-context lookup (`_DAT_8007C364`). When the
    /// host returns `None`, the player ctx isn't initialised yet OR `ctx`
    /// IS the player ctx - both fall through to a regular advance.
    ///
    /// When the host returns `Some(coords)`, the VM:
    /// - copies `world_x / world_y / world_z / field_26` onto `ctx`,
    /// - if `ctx.flags & 0x20000000` is set, additionally calls
    ///   [`Self::set_inverted_y_mirror`] with `-player.world_y` and returns
    ///   `OpResume::StateResume` (the `caseD_4` exit - the state-resume
    ///   subsystem decides whether to advance or halt),
    /// - otherwise returns `OpResume::Advance` (normal PC += 2).
    fn fetch_player_coords(&self, ctx: &FieldCtx) -> Option<PlayerCoords> {
        let _ = ctx;
        None
    }

    /// Side-effect for Op 0x4C sub-3 sub-7's bit-`0x20000000` branch.
    ///
    /// Writes the inverted-Y value at `ctx[+0x8e]`. Hosts that don't track
    /// `+0x8e` separately can ignore this.
    fn set_inverted_y_mirror(&mut self, ctx: &mut FieldCtx, inverted_y: i16) {
        let _ = (ctx, inverted_y);
    }

    /// Op 0x4C outer-nibble-C sub-0xE - write `_DAT_8007B6AC`.
    ///
    /// 3-byte instruction `[4C, 0xCE, value]`. Writes `_DAT_8007B6AC = value`
    /// (zero-extended, `0x801E2A24..0x801E2A30`): the player **clip
    /// override**. Its readers are the player clip picks - the settle tail
    /// `FUN_801D1BA0` (`0x801D1DD8`, `0x801D1E58`), op `0x22`'s and op
    /// `4C 51`'s player arms (`0x801DEA04`, `0x801E19B4`) and the
    /// unreferenced `FUN_801E58A8`; a `lui`-pair sweep of every image finds
    /// no other. (An earlier note here named op `0x43` sub-1 as its reader;
    /// no instruction of that arm touches the word.) PC += 3.
    fn set_player_clip_override(&mut self, value: u8) {
        let _ = value;
    }
}
