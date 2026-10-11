//! The [`FieldSceneHost`] supertrait of the field-VM host surface.

/// Scene changes, scripted encounters, mode handoffs and the scene control block.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldSceneHost {
    /// The **scripted-battle install** (op `0x3E` with `op0 < 100` or
    /// `op0 == 0xFF`). `row` is `op1`, an index into the scene MAN's
    /// formation table; `op0` is carried for visibility only - the arm never
    /// reads it past the `0xFF` / `< 100` test.
    ///
    /// The arm (`FUN_801DE840` `0x801E06D4..0x801E0788`, PROT 0897) first
    /// stamps `player[+0x8E] = player[+0x8F] = 0xFF` (both arms share this
    /// head), then calls `FUN_801D9E1C(player, 0)` - the region reader with
    /// its roll disabled, so it only re-seats the current region's battle
    /// setup - and, unless the dev word `_DAT_8007B868` is set or the system
    /// entity (`FUN_8003C83C(0xFB)`) is missing, installs the row:
    /// `sys[+0x8A] = 1`, `sys[+0x94] = formation_table + row * stride + 1`
    /// (table `*(ctrl+0x20)`, stride byte `ctrl+0x5D`, `ctrl = *0x801C6EA4`),
    /// rerolls the step counter (`FUN_801DDF48` into `_DAT_8007B5FC`) and
    /// requests the battle mode switch (`FUN_8003CE08(0xE)`). It opens no
    /// dialogue: talking to an NPC never goes through this op.
    // REF: FUN_801DE840 case 0x3e at 0x801E06D4
    fn scripted_battle(&mut self, op0: u8, row: u8) {
        let _ = (op0, row);
    }

    /// Trigger a scene transition by map id.
    ///
    /// **Not reachable from op `0x3E` any more.** The `op0 >= 100` arm is the
    /// mode-24 minigame door-warp, not a scene change - see
    /// [`Self::minigame_door_warp`]. This entry point survives for hosts that
    /// drive a map-id transition from a *non*-script producer (the field MAP's
    /// walk-on door triggers), which do carry real map ids.
    fn scene_transition(&mut self, map_id: u8) {
        let _ = map_id;
    }

    /// The **mode-24 minigame door-warp** (op `0x3E`, `op0 >= 100`).
    ///
    /// `sub_id` is `op0 - 100` and selects a *code overlay*, not a scene: the
    /// arm stores `_DAT_8007BA34 = sub_id` and `_DAT_8007B83C = 0x18` (game
    /// mode 24 `OTHER INIT`), zeroes the session-winnings accumulator
    /// `_DAT_80084440` and `_DAT_8007BAC0`, clears `player[+0x10] & 0x80000`,
    /// and calls `func_0x8003CE08(0xE)`. It calls **no** scene-change packet -
    /// the op carries no destination name, because the destination is the
    /// overlay the mode-24 init loads with `FUN_8003EBE4(sub_id + 0x4D)`. The
    /// VM clears `ctx.flags & 0x80000` itself, so a no-op host still mirrors
    /// the player-flag write.
    ///
    /// The whole `sub_id` space is `0..=6`; hosts decode it with
    /// `engine-core`'s `MinigameSubId`.
    // REF: FUN_801DE840 case 0x3e at 0x801E078C
    fn minigame_door_warp(&mut self, sub_id: u8) {
        let _ = sub_id;
    }

    /// Trigger a *named* scene transition (op `0x3F`, the named scene-change /
    /// "warp by name"). Unlike [`Self::scene_transition`] (the `0x3E` door-warp,
    /// which carries only a 7-id scene-*type* `map_id`), this op carries the
    /// **destination scene name inline** in the bytecode, so the host loads
    /// `scene` directly with no map-id resolver. `entry_x` / `entry_z` are the
    /// destination entry-tile bytes the op also carries (`& 0x7F` tile,
    /// `& 0x80` half-tile). Default is a no-op so a bare host still advances past
    /// the op. The VM only passes a name that cleared the clean-CDNAME-label gate
    /// ([`legaia_scene_formats::field_disasm::clean_scene_name`]); a desync-phantom `0x3F` inside
    /// message text is skipped (no transition) but still advances the PC.
    /// `dir & 7` is the arrival-facing selector: retail resolves it through
    /// the 8-entry compass table at SCUS `0x80073F04` (`facing = (dir & 7) *
    /// 0x200` in the 12-bit angle space) into `_DAT_80073EFC`.
    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8, dir: u8) {
        let _ = (scene, entry_x, entry_z, dir);
    }

    /// Scene-register write (op 0x4F). The original writes three `u16` values
    /// to `_DAT_801C6EA4 + 0x10/+0x12/+0x14`. Each value is one raw byte
    /// zero-extended.
    fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
        let _ = (slot_10, slot_12, slot_14);
    }

    /// Is the active field entity an encounter carrier?
    ///
    /// This is the engine-side stand-in for retail's scripted-encounter
    /// **discriminator**: there is no dedicated "encounter" opcode - the
    /// arm-encounter opcodes (`0x37`/`0x41`, `0x38`, `0x43`, `0x47`, `0x4C`)
    /// are the field VM's generic halt-acquire family, and what turns a halt
    /// into an encounter arm is the *consumer*. Retail only reads `+0x94` as a
    /// formation record on entities ticked by the 5-state `FUN_801DA51C` SM,
    /// once that SM reaches the encounter-confirm state. The field VM cannot
    /// see that per-entity SM, so it asks the host whether the active entity
    /// is armed; only then does the bare arm-encounter op forward the record.
    ///
    /// The default impl returns `false`, so generic script yields never get
    /// mistaken for encounter arms.
    ///
    /// REF: FUN_801DA51C
    fn is_scripted_encounter_armed(&self) -> bool {
        false
    }

    /// Install a scripted encounter from the bytecode window at the bare
    /// arm-encounter op (`0x37`/`0x41`).
    ///
    /// The retail writer (`0x801DEEDC..0x801DEEEC`) sets `actor[+0x94] = s0`,
    /// where `s0 = bytecode_buffer + pc_offset` is the **current opcode
    /// pointer** - i.e. the encounter record overlays the install opcode
    /// itself: `record[+0] = opcode byte`, `[+1..+2] = its operands`,
    /// `[+3] = monster_count`, `[+4..] = ids`. The consumer (`FUN_801DA51C`)
    /// reads that pointer to fill the formation cell exactly once per arm.
    ///
    /// The VM hands the host the bounded window starting at the current PC
    /// (`[opcode][op1][op2][count][<=4 ids]`, at most 8 bytes). Called only
    /// when [`Self::is_scripted_encounter_armed`] returned `true`; the host is
    /// expected to install the formation and disarm (fire-once semantics). The
    /// default impl ignores the window.
    fn install_scripted_encounter(&mut self, window: &[u8]) {
        let _ = window;
    }

    /// Op 0x4C outer-nibble-8 sub-8 - set the Door of Light (Riremito)
    /// destination: the map number and the arrival tile the travel art
    /// resolves (`docs/subsystems/field-locomotion.md`).
    ///
    /// 6-byte instruction `[4C, 0x88, lo, hi, b3, b4]`. Writes:
    /// - `_DAT_80084628 = signed_16(operand[1..3])`
    /// - `_DAT_80084624 = b3`
    /// - `_DAT_8008462C = b4`
    ///
    /// PC += 6.
    fn set_door_of_light_destination(&mut self, value: i16, b3: u8, b4: u8) {
        let _ = (value, b3, b4);
    }

    /// Op 0x4C outer-nibble-C sub-9 - wait for the BGM swap.
    ///
    /// 2-byte instruction `[4C, 0xC9]`. The original at line 6851 reads two
    /// 16-bit globals - `_DAT_8007BAB8`, the BGM index the resolver produced,
    /// and `_DAT_8007BA9C`, the index actually loaded - and halts at PC if
    /// they differ; otherwise advances by 2. Default returns `false` (globals
    /// match → advance). Real hosts return `true` while a swap is in flight.
    fn bgm_swap_pending(&self) -> bool {
        false
    }

    /// Op 0x4C outer-nibble-D sub-0xF - write `_DAT_801C6EA4 + 0x61` byte.
    ///
    /// 3-byte instruction `[4C, 0xDF, value]`. Writes one byte at the
    /// per-scene state struct's `+0x61` offset. PC += 3.
    fn op4c_nd_subf_scene_ctrl_61_write(&mut self, value: u8) {
        let _ = value;
    }

    /// Op 0x4C outer-nibble-E sub-2 - FMV trigger.
    ///
    /// 7-byte instruction `[4C, 0xE2, lo, hi, _, _, _]` (PC advances by
    /// 1+6). Writes `_DAT_8007BA78 = signed_16(operand[1..3])` (the
    /// FMV index passed to the STR/MDEC overlay) and
    /// `_DAT_8007B83C = 0x1A` (next-game-mode = 26 / StrInit). The
    /// FMV index selects a 32-byte entry from the static dispatch
    /// table at `0x801D0A6C` in the str_fmv overlay (master dispatch
    /// `FUN_801CEA3C`, selector `sll v0,v0,0x5`; decoder
    /// `legaia_game_tables::fmv_dispatch`). Retail ids are `0..=8` -
    /// `MV1`/`MV2`/four `MV3` frame-range segments/`MV4`/`MV5`/`MV6`,
    /// every movie on the disc. The trailing 3 bytes are reserved by
    /// the dispatcher's PC math but unused.
    // REF: FUN_801CEA3C
    fn trigger_fmv(&mut self, fmv_id: i16) {
        let _ = fmv_id;
    }

    /// Op 0x4C outer-nibble-E sub-0xC - write `_DAT_8007B5FC` from `FUN_801DDF48`.
    ///
    /// 2-byte instruction `[4C, 0xEC]`. Writes `_DAT_8007B5FC = FUN_801DDF48()`:
    /// the encounter step-counter reroll `r1 % 487 - r2 % 487 + 0x3CE`
    /// (triangular over `488..=1460`), ported as
    /// `legaia_engine_battle::region_encounter::encounter_counter_reroll` and
    /// seated by the engine host into the shared counter. No shipped script
    /// issues a clean `4C EC`. PC += 2.
    // REF: FUN_801DDF48
    fn reroll_encounter_step_counter(&mut self) {}

    /// Op 0x4C outer-nibble-E sub-9 - clear `_DAT_8007B9C4` then PC += 2.
    ///
    /// 1-byte instruction `[4C, 0xE9]`. The original at line 7362 clears
    /// `_DAT_8007B9C4` (the PROT 0874 party-pack load-state gate `FUN_8001E890`
    /// branches on, `docs/reference/memory-map.md`) and tail-calls
    /// `switchD_801e0f24::caseD_4()` - which is the `addiu s8, s8, 0x2;
    /// j epilogue` block at `0x801df098`, i.e. PC += 2.
    fn reset_party_pack_load_state(&mut self) {}

    /// Op 0x4C outer-nibble-E sub-A - call `FUN_8003C7EC` then halt at PC:
    /// the **scripted game-over trigger**.
    ///
    /// 1-byte instruction `[4C, 0xEA]`. `FUN_8003C7EC` (SCUS, not an
    /// actor-list mutator - an earlier decompile-derived label) is the
    /// helper twin of the party-wipe gate's inline handoff (see
    /// `docs/subsystems/battle.md` § party wipe): it stores master mode
    /// `_DAT_8007B83C = 0x16` (22 = CARD INIT, the CARD/CONTINUE title
    /// flow), raises the continue-flow flag `_DAT_8007BB00 = 1`, clears
    /// bit 1 of `DAT_8007B750`, and pauses the BGM
    /// (`FUN_800266E0(0x8007052C)`). The dispatcher then exits via
    /// `switchD_801e00f4::default()` → halt at PC for opcode 0x4C.
    fn trigger_game_over(&mut self) {}
}
