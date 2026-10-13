//! The [`FieldPartyInventoryHost`] supertrait of the field-VM host surface.

use crate::field::ActorSearchResult;

/// The party roster, the character records, the bag, gold and coins.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldPartyInventoryHost {
    /// Give the player one of inline item `item_id` (op 0x39 `GIVE_ITEM` - the
    /// treasure-chest / scripted-gift item-give). The original calls
    /// `func_0x8004313C()` (HUD/inventory window-bounds setup - writes the
    /// `gp+0x2D2/0x2D4/0x2D6` start/end/span triple; see
    /// `docs/reference/functions.md` `8004313C`) then the capacity-checked
    /// add-item-by-id primitive `func_0x800421D4(item_id, 1)`. (The earlier
    /// `play_sfx` name was wrong: SFX cues go through `FUN_80035B50`, not 0x39.)
    fn give_item(&mut self, item_id: u8) {
        let _ = item_id;
    }

    /// Add or subtract money (op 0x3A). `delta` is a 24-bit two's-complement
    /// integer already sign-extended to i32. The VM does NOT clamp; the host
    /// applies the new running total and clamps to `[0, 9999999]` per the
    /// original. Returning the new total is informational only.
    fn add_money(&mut self, delta: i32) {
        let _ = delta;
    }

    /// Write one equipment slot of a character record (op 0x3B,
    /// `SET_EQUIP_SLOT`). `selector` is the raw operand: the high nibble is
    /// the character record, the low nibble the slot byte. Retail stores
    /// `*(u8*)(0x80084140 + 0x75E + (selector & 0xF) + (selector >> 4) *
    /// 0x414) = item` - record `selector >> 4`'s equipment byte `+0x196 +
    /// (selector & 0xF)` - in the delay slot of its `FUN_80042558` call
    /// (`0x801E0810`), which rebuilds that record's ability bits and live
    /// stats. Nothing touches the bag: this is how a Ra-Seru is put on and
    /// grows a stage (`3B 03 xx` for the first character's slot 3).
    fn set_equip_slot(&mut self, selector: u8, item: u8) {
        let _ = (selector, item);
    }

    /// Add a character to the active party (op 0x3C). The original maintains
    /// a sorted insertion in `_DAT_80084598..` (cap 4), and updates
    /// `_DAT_8007B8F8` (party leader) when count was 0. Returns `true` on
    /// success, `false` if the party was full or the character was already
    /// in the party (matches the original's silent-no-op fallthrough).
    fn party_add(&mut self, char_id: u8) -> bool {
        let _ = char_id;
        true
    }

    /// Remove a character from the active party (op 0x3D).
    fn party_remove(&mut self, char_id: u8) {
        let _ = char_id;
    }

    /// Set the party leader (op 0x4C sub-0). `leader_id` is `op0 & 7`. The
    /// original mirrors this onto `DAT_80084597` and `_DAT_8007B8F8`.
    fn set_party_leader(&mut self, leader_id: u8) {
        let _ = leader_id;
    }

    /// Op 0x4E sub-ops 0 and 1 - one character's HP or MP pair.
    ///
    /// The original reads two `u16` values from the character record
    /// `page`, addressed as `0x80084340 + page * 0x414` (sub-op 0 is the
    /// record's HP current / max `+0x106` / `+0x104`, sub-op 1 its MP
    /// current / max `+0x10A` / `+0x108`):
    /// - sub-op 0: `state` from `+0x4CE`, `factor` from `+0x4CC`
    ///   (`DAT_8008480E + page*0x414` and `DAT_8008480C + page*0x414`).
    /// - sub-op 1: `state` from `+0x4D2`, `factor` from `+0x4D0`
    ///   (`DAT_80084812 + page*0x414` and `DAT_80084810 + page*0x414`).
    ///
    /// Returning `(0, 0)` (the default) makes the comparison resolve to
    /// false (`0 < 0` is false in both modes), matching the original's
    /// behaviour when the slots are zero-initialised.
    fn char_hp_mp_pair(&self, page: u8, sub_op: u8) -> (i32, i32) {
        let _ = (page, sub_op);
        (0, 0)
    }

    /// Op 0x4E party-bank read (sub-ops 3 / 9 via the 7-byte u16 compare,
    /// sub-ops 10 / 11 via the 9-byte u32 compare).
    ///
    /// The original reads a 32-bit value from `_DAT_8008459C` (party gold;
    /// sub-3 loader `0x801E0AC0`-family sibling at `0x801E0AEC`, sub-10 at
    /// `0x801E0B50`) or `_DAT_800845A4` (coin bank; sub-9 at `0x801E0B34`,
    /// sub-11 at `0x801E0B60`). The VM passes `sub_op` 10 for the gold bank
    /// and 11 for the coin bank regardless of which encoded form triggered
    /// the read.
    ///
    /// Default returns 0; comparisons resolve to `0 < scaled` or `scaled < 0`
    /// depending on the mode, matching the original's behaviour when the
    /// bank is empty.
    fn party_bank_value(&self, sub_op: u8) -> i32 {
        let _ = sub_op;
        0
    }

    /// Op 0x4E sub-op 2 - character-record level byte read.
    ///
    /// The loader at `0x801E0AC0` reads `u8` at
    /// `0x80084140 + page * 0x414 + 0x6F8` = character record `page`'s
    /// displayed-level byte `+0x130`, then joins the shared 7-byte
    /// compare-and-skip continuation at `0x801E0B40`. Default returns 0.
    fn char_level(&self, page: u8) -> i32 {
        let _ = page;
        0
    }

    /// Op 0x4C sub-3 sub-3 - the party-roster rebuild.
    ///
    /// The original calls `FUN_801de190()` then writes `DAT_8007B648 = 0`.
    /// `FUN_801de190` is the **party-roster rebuild**: it clears the four
    /// bytes at `0x8007BD10`, writes `member id + 1` for each of the
    /// `0x80084594` active members listed at `0x80084598` (at most three),
    /// and adds member 4 when system flag `0x13` is set (`FUN_8003CE64`).
    /// (An earlier note here guessed an inventory / menu re-raster.) PC += 2.
    fn rebuild_party_roster(&mut self) {}

    /// Op 0x4C sub-3 sub-2 (clear the 512-byte party/inventory state region).
    ///
    /// 2-byte instruction `[4C, 0x32]`. The original zeroes the 512 bytes at
    /// `[0x80085718 .. 0x80085918)` - a region holding party / inventory state
    /// adjacent to the fourth-flag-bank bitfield (`DAT_80085758`). Hosts model
    /// their own party-state representation and call this hook to reset it.
    /// PC += 2.
    fn clear_party_state_region(&mut self) {}

    /// Op 0x4C sub-2 - the camera-octant / pad-rotation setter.
    ///
    /// 2-byte instruction `[4C, op0]`. The new octant is `op0 & 7`. The
    /// original conditionally updates `_DAT_8007B5F0` (the pad-rotation octant)
    /// and adjusts `player_ctx[+0x26]` by `(new_index - old_index) * 0x200`
    /// - but only when `_DAT_8007B6B0 == -1000` (a "input idle" sentinel).
    ///
    /// Hosts model the policy. Returning the previous index is informational.
    /// The VM advances PC by 2 in both branches.
    fn set_camera_octant(&mut self, new_index: u8) {
        let _ = new_index;
    }

    /// Op 0x4C sub-3 sub-F (per-character TMD-pose copy).
    ///
    /// 2-byte instruction `[4C, 0x3F]`. Calls `FUN_8001ebec()`, which is a
    /// 3-iteration per-party-slot copier (slots 0..2, character-record stride
    /// `0x414`): for each slot, indexes the global TMD pool `DAT_8007C018`
    /// through the slot-byte at `_DAT_8007B824 + i` (the slot-4 freeze flag
    /// area), then writes 7 u32s of pose data (28 bytes) at TMD offset
    /// `+0xC + bytes[i]*0x1C` from either `+0x124..0x140` or `+0x140..0x158`,
    /// gated on a per-record flag byte at `record+0x75E`. (Earlier comments
    /// labelled `FUN_8001ebec` the "retail dialog-box renderer" - that's
    /// wrong; the disassembly shows the TMD-pose copier described above.
    /// The real dialog SM is `FUN_80039b7c`, pager `FUN_801D84D0`.) PC += 2.
    fn copy_party_tmd_poses(&mut self) {}

    /// Op 0x4C outer-nibble-8 sub-2 - full HP/MP restore of one party slot.
    ///
    /// 3-byte instruction `[4C, 0x82, slot]`. The original copies each
    /// max into its matching current, inside the 0x414-stride character
    /// record based at `0x80084708`:
    ///
    /// ```text
    ///   *(short *)(0x8008480E + slot * 0x414) = *(short *)(0x8008480C + slot * 0x414);
    ///   *(short *)(0x80084812 + slot * 0x414) = *(short *)(0x80084810 + slot * 0x414);
    /// ```
    ///
    /// `+0x104`/`+0x106` are HP max/current and `+0x108`/`+0x10A` are MP
    /// max/current, so the arm is exactly `hp_cur = hp_max; mp_cur =
    /// mp_max`. The max/current roles are corroborated by the level-up
    /// routine, which grows `+0x104`/`+0x108`, clamps them to 9999/999,
    /// then clamps `+0x106`/`+0x10A` against them.
    ///
    /// This is **retail's inn/rest heal**. There is no inn opcode and no
    /// native inn routine: a stay is composed inline in the scene script
    /// out of generic ops (dialogue, a 2-option picker, an op-0x4E gold
    /// gate, op-0x3A `ADD_MONEY` with the negative charge, screen fades)
    /// and then one of these per party slot. Charge and restore are fully
    /// decoupled - a free rest is the same tail minus the gate and debit.
    /// See `docs/subsystems/field-menu.md`.
    ///
    /// PC += 3.
    fn restore_party_slot(&mut self, slot: u8) {
        let _ = slot;
    }

    /// Op 0x4C outer-nibble-D sub-C - small-table lookup, then conditional
    /// per-party-record write.
    ///
    /// 5-byte instruction `[4C, 0xDC, b1, ?, ?]` (the trailing operand
    /// bytes are consumed by the LAB_801E360C tail's `func_0x8003CE9C` read).
    ///
    /// The original at line 7087:
    /// 1. Calls [`field_helpers::small_table_search`] (`FUN_80042EE0(b1)`)
    ///    to look up `b1` in the gp-relative short-table at `0x80085958`.
    /// 2. If the table lookup hits AND a party-record search across
    ///    `DAT_80084594` slots (party-record stride `0x414`, byte at `+0x196`)
    ///    matches `b1`, the slot is updated.
    /// 3. If neither hits, returns `param_2 + 5` (PC += 5).
    /// 4. On hit, falls through to `LAB_801E360C` which reads `ce9c(p+2)`
    ///    and returns it as the new PC.
    ///
    /// The host hook returns `Some(new_pc)` for the ce9c-jump path, or `None`
    /// to take the PC += 5 path. Default impl returns `None` (safe fallback).
    ///
    /// [`field_helpers::small_table_search`]: crate::field::dispatch_leaves::small_table_search
    fn op4c_nd_subc_party_search_set(&mut self, needle: u8) -> Option<usize> {
        let _ = needle;
        None
    }

    /// Op 0x4C outer-nibble-D sub-E - small-table lookup query (sister of
    /// sub-C without the per-party-record write side-effect).
    ///
    /// 5-byte instruction `[4C, 0xDE, b1, ?, ?]`. Same control flow as
    /// sub-C but the party-record loop only **tests** for a match - no slot
    /// update. PC is `param_2 + 5` on miss, ce9c-jump on hit.
    fn op4c_nd_sube_party_search_query(&mut self, needle: u8) -> Option<usize> {
        let _ = needle;
        None
    }

    /// Op 0x4C outer-nibble-E sub-5 - add to the casino coin bank.
    ///
    /// 5-byte instruction `[4C, 0xE5, b1, b2, b3]`. The original (dispatcher
    /// dump 7256-7267) reads a 24-bit signed value via
    /// [`field_helpers::load_u24_le`] + [`field_helpers::sign_extend_24`],
    /// adds it to `_DAT_800845A4` (the casino coin bank -
    /// `docs/reference/memory-map.md`; the earlier "party-XP global" label
    /// was wrong), caps it at `9999999` (`0x801E32C8`), and calls
    /// `FUN_8003CE08(8)` (system flag 8 set). There is **no lower clamp** -
    /// the gold delta beside it floors at zero and this arm does not
    /// (`0x801E328C..0x801E32E4`).
    ///
    /// `coin_delta` is the sign-extended 24-bit value already prepared by
    /// the dispatcher. Hosts apply the cap + flag set; default no-op (the
    /// world's host: `legaia_engine_core::casino_coin_bank`).
    ///
    /// [`field_helpers::load_u24_le`]: crate::field::dispatch_leaves::load_u24_le
    /// [`field_helpers::sign_extend_24`]: crate::field::dispatch_leaves::sign_extend_24
    fn add_coins(&mut self, coin_delta: i32) {
        let _ = coin_delta;
    }

    /// Count active actors of a given type (op 0x4C n8 sub-B, 5 bytes).
    ///
    /// `[4C, 0x8B, type_byte, target_lo, target_hi]`. The original at lines
    /// 6621-6644 walks the active actor pool (`DAT_80084594` = pool count;
    /// each slot is 0x414 bytes wide, indexed by the per-slot type byte at
    /// `+0x458`) and increments a count for every entry whose stored type
    /// matches `type_byte`.
    ///
    /// If any match: jump to absolute `LE_u16(operand+2..=operand+3)`.
    /// If no match: advance PC by 5.
    ///
    /// The host decides the pool layout - this hook just answers
    /// "is there at least one actor of `type_byte` active?". Default impl
    /// returns `false` (no actor pool).
    fn op4c_n8_subb_actor_type_present(&self, type_byte: u8) -> bool {
        let _ = type_byte;
        false
    }

    /// Search a per-character actor sub-table for a matching marker
    /// (op 0x4C n8 sub-D, 6 bytes).
    ///
    /// `[4C, 0x8D, char_idx, marker, target_lo, target_hi]`. The original at
    /// lines 6652-6667 indexes `DAT_8008488D[char_idx * 0x414]` for the
    /// per-character sub-table size and walks the table comparing each entry
    /// against `marker`.
    ///
    /// Three outcomes:
    /// - [`ActorSearchResult::EmptySlot`]: per-character sub-table is empty
    ///   (size byte is 0). Advance PC by 6.
    /// - [`ActorSearchResult::Found`]: found a matching entry. Jump to
    ///   absolute `LE_u16(operand+3..=operand+4)`.
    /// - [`ActorSearchResult::NoMatch`]: sub-table is non-empty but no entry
    ///   matched. Halt at PC.
    ///
    /// The default impl returns [`ActorSearchResult::EmptySlot`] (advance,
    /// no match) which is the safest fallback for engines without an actor
    /// pool.
    fn op4c_n8_subd_actor_search(&self, char_idx: u8, marker: u8) -> ActorSearchResult {
        let _ = (char_idx, marker);
        ActorSearchResult::EmptySlot
    }

    /// Op 0x4C n5 sub-2 - **TAKE_ITEM**: remove one `item_id` from the party.
    ///
    /// 3-byte instruction `[4C, 0x52, item_id]`, and the give-side mirror of
    /// op `0x39` `GIVE_ITEM` (which is `FUN_8004313C()` then
    /// `FUN_800421D4(item_id, 1)`). Read off the arm at `0x801E1ABC` in
    /// `FUN_801DE840`, not off the decompile:
    ///
    /// ```text
    /// 801e1abc  jal 0x8004313c        ; inventory active-window setup
    /// 801e1ac4  lbu a0,0x1(s6)        ; operand byte = ITEM id
    /// 801e1ac8  jal 0x80042310        ; bag consume (item_id, 1)
    /// 801e1acc  _li a1,0x1
    /// 801e1ad0  sll v0,v0,0x10        ; sign-extend the i16 result
    /// 801e1ad4  sra v0,v0,0x10
    /// 801e1ad8  li v1,0x100           ; the NOT-FOUND sentinel
    /// 801e1adc  bne v0,v1,0x801e00b8  ; found -> common exit
    /// 801e1ae4  lbu a0,0x1(s6)
    /// 801e1ae8  jal 0x800430ac        ; miss -> party unequip-by-id fallback
    /// 801e1aec  _addiu s8,s8,0x3      ; PC += 3, in the DELAY SLOT
    /// ```
    ///
    /// Two things an earlier reading of this arm got backwards, both of them
    /// the decompiler artifacts `docs/tooling/ghidra.md` catalogues:
    ///
    /// * **`0x100` is the bag *miss* sentinel, not a "finished" signal.**
    ///   `FUN_80042310` is the inventory consume primitive (the same one the
    ///   sell-quantity confirm and the Door-of-Light use run); `0x100` is what
    ///   it returns when no slot holds the id. So the `== 0x100` branch is the
    ///   fallback, and `FUN_800430AC` -
    ///   `legaia_engine_menus::equipment::party_unequip_accessory_by_id` - takes
    ///   the item off whoever is *wearing* it.
    /// * **There is no poll and no halt.** The PC delta sits in the branch
    ///   delay slot, so it retires on both paths; the `bne` target
    ///   `0x801E00B8` is the shared advancing exit. Reading
    ///   `switchD_801e00f4::default()` as "no advance" is the exact trap
    ///   `docs/subsystems/script-vm.md` warns about.
    ///
    /// The VM therefore advances by 3 unconditionally and this hook is a pure
    /// side effect. Default is a no-op: the script still steps, the party just
    /// keeps the item. A host that owns an inventory overrides it, consumes one
    /// `item_id`, and on a miss runs the unequip fallback.
    fn take_item(&mut self, item_id: u8) {
        let _ = item_id;
    }
}
