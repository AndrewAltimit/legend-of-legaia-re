//! The battle overlay's two cast-effect dispatchers, plus the reward-banner
//! text composer they share the context text buffer with.
//!
//! PORT: FUN_801F1ED4
//! PORT: FUN_801F2160
//! PORT: FUN_801DBA90
//! REF: FUN_801E295C (the action SM that calls the two dispatchers),
//! REF: FUN_801F2410 (the shared tail both dispatchers gate on),
//! REF: FUN_801D8DE8 (the HUD case the banner is drawn by),
//! REF: FUN_801DCEAC (a control for the reference scan below),
//! REF: FUN_801F1CC8, FUN_801F20DC (the field image's twins at the same VAs)
//!
//! All three ports here are now wired, but to different depths, and the
//! per-item notes carry what each still owes.
//!
//! **The two dispatchers** resolve a cast to a **paged slot-B module** - one
//! PROT entry per spell, streamed into the overlay buffer at `0x801F69D8` for
//! the duration of the cast. Extraction `903 + row` for `row = 0..0x3F`
//! (PROT 0903..0966); every one of those images is mapped in
//! `crates/asset/data/static-overlays.toml` and dumped as
//! `overlay_<label>_<entry>_<addr>.txt`, and the per-module table is on
//! `docs/reference/functions/battle.md`. The module anatomy - phase machine,
//! staging ABI, damage shape - is `docs/subsystems/cast-module.md`.
//!
//! Each dispatcher therefore answers with **two** things, and the port carries
//! both: the retail emitter VA (what the `jr` table's arm jumps to, the fact
//! the dump pins) and the **band entry** that VA lives in
//! ([`CastDispatch::module`]) - which is a pool handle, because
//! `legaia_asset::cast_effect_pool` indexes the band's spawn records by PROT
//! entry. `legaia_engine_core::world::World::spawn_cast_module_fx` stages the
//! resolved module's records through the same move-VM effect stand-in the
//! summon path uses, at the paging seam and at the stager's first tick.
//!
//! What stays unported is the module's **code** half - the lift (the staged
//! `+0x1DA` clip chain), the camera arm, the `ctx+0x279` phase machine and the
//! damage shape - i.e. every row `docs/subsystems/cast-module.md`'s worklist
//! marks **PORT** rather than **DATA**. Those are module instructions and no
//! record can express them; [`CastDispatch::emitter`] names where they live.
//!
//! Two claims an earlier revision of this note made are **wrong** and are
//! corrected here. It said the `+0x01` effect-class byte "nothing decodes":
//! `legaia_asset::spell_names::SpellEntry` has read it as `sub_class` for as
//! long as it has read `+0`, and `World::spell_table_sub_class` already serves
//! it to the battle host. And it said `retail_magic` "carries name / MP /
//! target only": it now carries the byte as
//! `legaia_engine_core::retail_magic::RetailSpell::effect_class`. What was
//! true, and what the pool fixes, is that no consumer turned the byte into a
//! module. Beware also that `+0` is called "the cast class" in the parser
//! while `+1` is "the effect class" here; two different bytes, one word.
//!
//! **`FUN_801DBA90` is different: retail reaches it from nowhere.** It is a
//! genuine function entry - `27bdffe8 afb00010` at `0x801DBA90` in the
//! battle-action image, 39 instructions - and *no* image references it in any
//! form. A scan of `SCUS_942.54` plus all 31 base-mapped overlay images for the
//! literal little-endian word at every alignment, the `lui`+`addiu` / `lui`+`ori`
//! materialisation pair, and `jal` / `j` returns zero hits; the same scan
//! recovers all three `FUN_801F1ED4` call sites and all six `FUN_801DCEAC` ones
//! as controls. So it is not a dispatch-table entry either - a static table
//! holding it would show as the literal word.
//!
//! That is a fact about the *entry point*, and an earlier revision of this note
//! turned it into "no engine-side channel can therefore fix this row" - which
//! is a fact about the *rule*, and false. `FUN_801D8DE8`'s HUD case `0x59`
//! (`0x801D9154..0x801D91D0`, gated on its `param_2 == 0`) is the same
//! assembly **instruction for instruction**: the same `ctx[+0x13]` ->
//! `DAT_8007BD10` character id, the same `-1` index into the same
//! `0x801F4DFC` table, the same `ctx[+0x269] + 0x80` spell id at the same
//! `0x800754C8 + id*0x0C + 8`, the same `0x801F4C28` suffix, and the same
//! `FUN_8003CA78` / `FUN_8003CAC4` / `FUN_8003CAC4` calls against the same
//! `ctx + 0x1F9`. A whole-image scan finds exactly two materialisations of the
//! table VA and two of the suffix VA - one pair in each routine - so the two
//! are twins and the case-`0x59` one is the live site.
//!
//! So the rule runs on every dome win, and the port is wired to it:
//! `MuscleDomeSession::reward_banner` composes through this decode and both
//! hosts show the result (`minigames_muscle::muscle_reward_banner_json`
//! resolves the two overlay strings and the spell name). What stays true is
//! that **this entry point** is dead - a host is running the twin, not
//! reaching `0x801DBA90`. One residual on the scan: an address assembled in
//! more than two instructions, or reached as `table_base + index` where the
//! base is computed, would not be caught.
//!
//! ## The two dispatchers
//!
//! Both are reached from the battle action state machine `FUN_801E295C`
//! (`jal 0x801f1ed4` at `0x801E4B1C` / `0x801E4C7C` / `0x801E4CA8`,
//! `jal 0x801f2160` at `0x801E50C8`), both read the active actor through
//! `ctx[+0x13]` -> `DAT_801C9370`, and both dispatch through a 32-slot MIPS
//! `jr` table. They differ only in the key:
//!
//! | routine | key | span |
//! |---|---|---|
//! | `FUN_801F1ED4` | the queued action id `actor[+0x1DF]` itself | `0x81..=0xA0` |
//! | `FUN_801F2160` | the **effect class** byte `spell_table[id].+0x01` | `0x00..=0x1F` |
//!
//! `docs/formats/spell-table.md` calls that `+0x01` byte the **capture-class
//! sub-id** and uses it the same way: `935 + sub_id` is the PROT entry. Two
//! names, one byte.
//!
//! `0x81..=0xA0` is the player Seru-magic block of the spell-id space (see
//! `docs/formats/spell-table.md`); `FUN_801F1ED4` therefore hard-wires one
//! emitter per player spell, while `FUN_801F2160` routes *any* cast - a
//! monster's included - by the class byte its spell record carries. Because
//! several ids share one emitter, the two tables together are the map from
//! "which spell" to "which cast animation".
//!
//! Both end the same way: when `ctx[+0x27A]` is non-zero they additionally
//! call `FUN_801F2410`, and both return whatever the selected emitter
//! returned (`0` when the key falls outside the table).
//!
//! Handlers are named here by their retail VA because none of them is ported
//! yet, and the VA is what makes the table checkable against the dump. Each is
//! a **module entry**, not a loose address: for `FUN_801F1ED4` the arm for
//! action id `n` calls the tick of PROT `903 + (n - 0x81)`, and for
//! `FUN_801F2160` the arm for sub-id `k` calls PROT `935 + k` - where, in that
//! band, the arm usually lands on a small trampoline that re-reads
//! `actor[+0x1DF]` and picks one of the module's bodies by spell id (a
//! multi-spell cell is several whole choreographies in one image). Checking a
//! row therefore means opening that module's dump, not searching the battle
//! image.
//!
//! ## Dump provenance and a decompiler artifact
//!
//! Read from the disassembly of `overlay_battle_action_0898_801f1ed4.txt` /
//! `overlay_battle_action_0898_801f2160.txt` - the battle-action image at its
//! own recovered base, so the dump's name carries the identity the shared
//! slot-A base cannot. Two other dumps print at these VAs and neither should be
//! read for them: `overlay_muscle_dome_*` is the same bytes under a
//! capture-derived name that says nothing about which image they are, and the
//! `overlay_0897` image has *different* functions there (`FUN_801F1CC8` /
//! `FUN_801F20DC`) - a VA collision. The battle-action image's own
//! `FUN_801E295C` dump confirms both addresses as live `jal` targets, which
//! is what pins the identity.
//!
//! In `FUN_801F2160` the decompiled C renders class `0x0E` as
//! `thunk_EXT_FUN_8c000000`; the disassembly at `0x801F22BC` is a plain
//! `jal 0x801F6A10`. The table below follows the disassembly.
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_muscle_dome_801f1ed4.txt`,
//! `overlay_muscle_dome_801f2160.txt`,
//! `overlay_battle_action_801dba90.txt`.

/// Lowest player Seru-magic spell id `FUN_801F1ED4` dispatches.
pub const SERU_SPELL_ID_MIN: u8 = 0x81;

/// Number of slots in either dispatcher's jump table.
pub const CAST_TABLE_SLOTS: usize = 0x20;

/// Byte stride of a spell record in the static `SCUS_942.54` spell table
/// (`DAT_800754C8`), whose `+0x01` byte keys [`spell_class_emitter`].
pub const SPELL_RECORD_STRIDE: usize = 0x0C;

/// Per-spell-id emitters of `FUN_801F1ED4`, indexed by `id - 0x81`.
///
/// `None` is the one hole in the table: id `0x98` falls straight through to
/// the shared epilogue, so its slot points at the post-switch tail rather than
/// at an emitter. Every other slot is a real `jal`.
pub const SERU_SPELL_EMITTERS: [Option<u32>; CAST_TABLE_SLOTS] = [
    Some(0x801F_69D8), // 0x81
    Some(0x801F_69D8), // 0x82
    Some(0x801F_69D8), // 0x83
    Some(0x801F_69F4), // 0x84
    Some(0x801F_69E8), // 0x85
    Some(0x801F_69D8), // 0x86
    Some(0x801F_69F4), // 0x87
    Some(0x801F_69EC), // 0x88
    Some(0x801F_69D8), // 0x89
    Some(0x801F_69D8), // 0x8A
    Some(0x801F_69F0), // 0x8B
    Some(0x801F_69F0), // 0x8C
    Some(0x801F_69D8), // 0x8D
    Some(0x801F_69F8), // 0x8E
    Some(0x801F_6A30), // 0x8F
    Some(0x801F_6C70), // 0x90
    Some(0x801F_69D8), // 0x91
    Some(0x801F_69D8), // 0x92
    Some(0x801F_6A08), // 0x93
    Some(0x801F_6A3C), // 0x94
    Some(0x801F_69D8), // 0x95
    Some(0x801F_6A18), // 0x96
    Some(0x801F_6A00), // 0x97
    None,              // 0x98 - no emitter arm
    Some(0x801F_6A84), // 0x99
    Some(0x801F_69F4), // 0x9A
    Some(0x801F_69FC), // 0x9B
    Some(0x801F_6A74), // 0x9C
    Some(0x801F_6A58), // 0x9D
    Some(0x801F_6A34), // 0x9E
    Some(0x801F_6A30), // 0x9F
    Some(0x801F_6A40), // 0xA0
];

/// Per-effect-class emitters of `FUN_801F2160`, indexed by the spell record's
/// `+0x01` byte. Unlike [`SERU_SPELL_EMITTERS`] this table has no hole - all
/// 32 slots carry a `jal`.
pub const SPELL_CLASS_EMITTERS: [u32; CAST_TABLE_SLOTS] = [
    0x801F_69D8, // 0x00
    0x801F_69D8, // 0x01
    0x801F_69D8, // 0x02
    0x801F_7A40, // 0x03
    0x801F_69D8, // 0x04
    0x801F_8228, // 0x05
    0x801F_7D38, // 0x06
    0x801F_80A0, // 0x07
    0x801F_7624, // 0x08
    0x801F_7EBC, // 0x09
    0x801F_76F4, // 0x0A
    0x801F_69FC, // 0x0B
    0x801F_69F0, // 0x0C
    0x801F_69F0, // 0x0D
    0x801F_6A10, // 0x0E - the C's `thunk_EXT_FUN_8c000000` is an artifact
    0x801F_8190, // 0x0F
    0x801F_816C, // 0x10
    0x801F_7B28, // 0x11
    0x801F_69FC, // 0x12
    0x801F_6A58, // 0x13
    0x801F_92A4, // 0x14
    0x801F_7E4C, // 0x15
    0x801F_9BA8, // 0x16
    0x801F_8E60, // 0x17
    0x801F_87F4, // 0x18
    0x801F_8638, // 0x19
    0x801F_7A54, // 0x1A
    0x801F_8080, // 0x1B
    0x801F_8438, // 0x1C
    0x801F_8E3C, // 0x1D
    0x801F_7B1C, // 0x1E
    0x801F_6A74, // 0x1F
];

/// What a dispatcher resolved to for one cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastDispatch {
    /// Retail VA of the emitter to run, or `None` when the key fell outside
    /// the table (or hit the `0x98` hole). Retail then leaves `s0` at its
    /// seed and returns `0`.
    ///
    /// This is where the module's **code** half lives (lift, camera, the
    /// `ctx+0x279` phase machine); it is kept because it is what makes a row
    /// checkable against the module's own dump, not because anything jumps to
    /// it. The engine-runnable half is [`Self::module`].
    pub emitter: Option<u32>,
    /// The band entry the emitter lives in - the **pool handle**
    /// [`legaia_asset::cast_effect_pool`] keys the module's spawn records by.
    /// `None` when the key fell outside the dispatcher's `sltiu ..., 0x20`
    /// bound.
    ///
    /// Independent of [`Self::emitter`] being `Some`: action id `0x98` has no
    /// tick arm but its band row (PROT 0926) is a real entry - the null stub.
    pub module: Option<u32>,
    /// Whether the shared tail `FUN_801F2410` runs. Gated on `ctx[+0x27A]`.
    pub run_tail: bool,
}

/// Retail VA of the tail routine both dispatchers call when `ctx[+0x27A]` is
/// set.
pub const CAST_DISPATCH_TAIL: u32 = 0x801F_2410;

/// Resolve the player Seru-magic cast emitter for a queued action id.
/// `FUN_801F1ED4`.
///
/// `action_id` is the active actor's `+0x1DF`. Retail forms `id - 0x81` and
/// gates it with `sltiu ..., 0x20`, so ids below `0x81` wrap to a large
/// unsigned value and fail the bound exactly as ids above `0xA0` do.
///
/// Wired: the `module` it resolves is the pool handle
/// `legaia_engine_core::world::World::spawn_cast_module_fx` stages, at the
/// stager's first tick (retail's `0x801E4B1C`). The `emitter` VA it also
/// carries names the module's unported code half - see the module note.
///
/// PORT: FUN_801F1ED4
pub fn seru_spell_emitter(action_id: u8, ctx_27a: u8) -> CastDispatch {
    let index = action_id.wrapping_sub(SERU_SPELL_ID_MIN) as usize;
    CastDispatch {
        emitter: SERU_SPELL_EMITTERS.get(index).copied().flatten(),
        module: legaia_asset::cast_effect_pool::seru_module_prot(action_id),
        run_tail: ctx_27a != 0,
    }
}

/// Resolve the cast emitter for a spell's **effect class**. `FUN_801F2160`.
///
/// `effect_class` is byte `+0x01` of the spell record the queued action id
/// selects (`0x800754C8 + id * 0x0C + 1`). Retail bounds it with
/// `sltiu ..., 0x20`.
///
/// Wired: the key's live source is the disc spell table
/// (`World::spell_table_sub_class` - the same `+0x01` byte the pager
/// `FUN_8003EC70(record[+1] + 0x28)` resolves), and the `module` it names is
/// staged at the capture band's paging seam
/// (`BattleActionHost::load_capture_archive`, retail's `0x6E` arm; the tick
/// call itself is `jal 0x801f2160` at `0x801E50C8`).
///
/// PORT: FUN_801F2160
pub fn spell_class_emitter(effect_class: u8, ctx_27a: u8) -> CastDispatch {
    CastDispatch {
        emitter: SPELL_CLASS_EMITTERS.get(effect_class as usize).copied(),
        module: legaia_asset::cast_effect_pool::capture_module_prot(effect_class),
        run_tail: ctx_27a != 0,
    }
}

/// Read a spell record's effect-class byte out of a slice of the static spell
/// table. The slice is expected to start at the table base (`DAT_800754C8`).
///
/// Returns `None` when the record is not fully inside the slice - callers
/// then have no class byte and retail would have read out of bounds.
///
/// REF: FUN_801F2160 (the `lbu v1,0x1(v0)` this isolates)
pub fn spell_effect_class(spell_table: &[u8], spell_id: u8) -> Option<u8> {
    let off = (spell_id as usize) * SPELL_RECORD_STRIDE + 1;
    spell_table.get(off).copied()
}

// ---------------------------------------------------------------------------
// FUN_801DBA90 - reward-banner composer
// ---------------------------------------------------------------------------

/// Offset of the battle context's banner text buffer (`ctx + 0x1F9`).
pub const BANNER_BUFFER_OFFSET: u32 = 0x1F9;

/// The spell-id base the reward index is lifted into: `ctx[+0x269] + 0x80`.
/// `0x80` is one below the player Seru-magic block, so a stored reward index
/// of `1` names the first player spell.
pub const REWARD_SPELL_ID_BASE: u16 = 0x80;

/// The three string sources `FUN_801DBA90` concatenates into the banner, in
/// the order it writes them. All three are *pointers* in retail; this port
/// resolves them to indices/ids and leaves the string lookup to the host so
/// no disc text crosses the crate boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardBanner {
    /// Index into the per-character lead-in message table at `0x801F4DFC`
    /// (`legaia_asset::muscle_dome::VICTORY_MSG_TABLE_VA`). Retail forms it as
    /// `DAT_8007BD10[ctx[+0x13]] - 1`, i.e. the **1-based** character id of
    /// the actor in the active slot, minus one.
    pub lead_in_index: u8,
    /// Spell id whose name is appended second: `ctx[+0x269] + 0x80`. The name
    /// pointer lives at `DAT_800754D0 + id * 0x0C`.
    pub spell_id: u16,
    /// Whether the fixed suffix string at `0x801F4C28` is appended last.
    /// Always true - retail appends it unconditionally; the field exists so
    /// the shape of the banner is explicit at the call site.
    pub suffix: bool,
}

/// Retail VA of the per-character lead-in message table.
pub const BANNER_LEAD_IN_TABLE_VA: u32 = 0x801F_4DFC;

/// Retail VA of the fixed banner suffix string.
pub const BANNER_SUFFIX_VA: u32 = 0x801F_4C28;

/// Compose the three-part reward banner. `FUN_801DBA90`.
///
/// The routine makes three calls against the context text buffer at
/// `ctx + 0x1F9`: `FUN_8003CA78` (set) with the active character's lead-in
/// message, then `FUN_8003CAC4` (append) with the reward spell's name, then
/// `FUN_8003CAC4` again with the fixed suffix. The same three-part assembly
/// is what `FUN_801D8DE8`'s HUD case `0x59` shows (see
/// `docs/subsystems/minigame-muscle-dome.md`); this is the standalone routine
/// behind it.
///
/// `char_id` is the 1-based per-slot character id (`DAT_8007BD10[slot]`);
/// `reward_index` is `ctx[+0x269]`. Retail applies no bound check to either.
///
/// PORT: FUN_801DBA90
/// REF: FUN_8003CA78, FUN_8003CAC4 (the set/append text primitives)
pub fn reward_banner(char_id: u8, reward_index: u8) -> RewardBanner {
    RewardBanner {
        lead_in_index: char_id.wrapping_sub(1),
        spell_id: reward_index as u16 + REWARD_SPELL_ID_BASE,
        suffix: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seru_table_span_and_hole() {
        assert!(seru_spell_emitter(0x80, 0).emitter.is_none());
        assert_eq!(seru_spell_emitter(0x81, 0).emitter, Some(0x801F_69D8));
        assert_eq!(seru_spell_emitter(0xA0, 0).emitter, Some(0x801F_6A40));
        assert!(seru_spell_emitter(0xA1, 0).emitter.is_none());
        assert!(
            seru_spell_emitter(0x98, 0).emitter.is_none(),
            "0x98 has no emitter arm"
        );
    }

    #[test]
    fn seru_table_shares_emitters_across_ids() {
        // The default emitter 0x801F69D8 covers the plain elemental block.
        let shared: Vec<u8> = (0x81u8..=0xA0)
            .filter(|&id| seru_spell_emitter(id, 0).emitter == Some(0x801F_69D8))
            .collect();
        assert_eq!(
            shared,
            vec![0x81, 0x82, 0x83, 0x86, 0x89, 0x8A, 0x8D, 0x91, 0x92, 0x95]
        );
    }

    #[test]
    fn class_table_is_dense() {
        for class in 0u8..0x20 {
            assert!(spell_class_emitter(class, 0).emitter.is_some());
        }
        assert!(spell_class_emitter(0x20, 0).emitter.is_none());
        assert_eq!(
            spell_class_emitter(0x0E, 0).emitter,
            Some(0x801F_6A10),
            "disassembly, not the C's thunk rendering"
        );
    }

    #[test]
    fn each_dispatch_names_its_band_entry() {
        // FUN_801F1ED4 arm `i` is PROT 903 + i over the whole action span.
        assert_eq!(seru_spell_emitter(0x81, 0).module, Some(903));
        assert_eq!(seru_spell_emitter(0xA0, 0).module, Some(934));
        assert_eq!(seru_spell_emitter(0x80, 0).module, None);
        assert_eq!(seru_spell_emitter(0xA1, 0).module, None);
        // The one id with no tick arm still has a band row - the null stub.
        let hole = seru_spell_emitter(0x98, 0);
        assert!(hole.emitter.is_none());
        assert_eq!(hole.module, Some(926));
        // FUN_801F2160 arm `k` is PROT 935 + k.
        assert_eq!(spell_class_emitter(0, 0).module, Some(935));
        assert_eq!(spell_class_emitter(0x1F, 0).module, Some(966));
        assert_eq!(spell_class_emitter(0x20, 0).module, None);
        // The two bands are contiguous and disjoint.
        for class in 0u8..0x20 {
            let m = spell_class_emitter(class, 0).module.unwrap();
            assert!((935..=966).contains(&m));
        }
    }

    #[test]
    fn tail_gate_follows_ctx_27a() {
        assert!(!seru_spell_emitter(0x81, 0).run_tail);
        assert!(seru_spell_emitter(0x81, 1).run_tail);
        assert!(spell_class_emitter(0, 0xFF).run_tail);
    }

    #[test]
    fn effect_class_reads_record_byte_one() {
        // Two synthetic 12-byte records; byte +1 of record 2 is the class.
        let mut table = vec![0u8; SPELL_RECORD_STRIDE * 3];
        table[SPELL_RECORD_STRIDE * 2 + 1] = 0x11;
        assert_eq!(spell_effect_class(&table, 2), Some(0x11));
        assert_eq!(spell_effect_class(&table, 3), None);
    }

    #[test]
    fn banner_indices() {
        // Vahn (char id 1) winning a spell stored as reward index 3.
        let banner = reward_banner(1, 3);
        assert_eq!(banner.lead_in_index, 0);
        assert_eq!(banner.spell_id, 0x83);
        assert!(banner.suffix);
        // Retail does not bound-check the char id.
        assert_eq!(reward_banner(0, 0).lead_in_index, 0xFF);
    }
}
