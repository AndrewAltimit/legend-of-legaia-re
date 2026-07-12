//! Battle-action validator, ported clean-room from `FUN_8003FB10`
//! (`SCUS_942.54`-resident), plus its arm-`0x82` leaf callee `FUN_80046898`.
//!
//! PORT: FUN_8003FB10, FUN_80046898
//!
//! The 16-arm gate the menu / battle UI runs against a candidate slot
//! before committing the player's action - the layer between "the cursor
//! is on this target" and "the state machine in this module is allowed to
//! run the action." Retail dispatches on the outer arm byte (bounded
//! `< 0x84`, jump table at `0x80014D70`; unhandled slots return 0) and,
//! for arm `0x06`, a sub-case byte through a second 7-entry table at
//! `0x80014F80`.
//!
//! The retail setup loop caches, per slot, four pointers to the active
//! record's `(hp, hp_max, mp, mp_max)` quad:
//!
//! - in battle (`_DAT_8007B83C == 0x15`): battle-actor pointer table
//!   `DAT_801C9370`, offsets `+0x14C/+0x14E/+0x150/+0x152`, 7 slots;
//! - out of battle: character records `0x80084708 + slot*0x414`, offsets
//!   `+0x106/+0x104/+0x10A/+0x108` (cur/max order swapped vs battle),
//!   3 slots.
//!
//! The port abstracts that quad behind
//! [`ActionValidatorHost::slot_resources`] and keeps the arm logic here.
//! Results land in two places, exactly as retail: the `bool` return value
//! and a per-slot validity bitmask byte (retail `gp + 0x9A8`) passed as
//! `validity_bits` - some arms clear their slot's bit before testing, some
//! overwrite the whole byte, and some (`0x03` in battle, `0x08` in battle,
//! `0x80..=0x83`) never touch it. The bit semantics matter because the menu
//! greying reads the byte, not the return value.
//!
//! Contrary to an earlier reading in `docs/subsystems/battle.md`, the dump
//! contains **no call to the ability bit-test `FUN_800431D0`** - the only
//! real callees are the system-flag test `FUN_8003CE64` (arms `0x80`/`0x81`;
//! already ported as [`crate::field_helpers::party_flag_test`], surfaced
//! here through [`ActionValidatorHost::system_flag`]) and the inventory
//! item-count gate `FUN_80046898` (arm `0x82`, ported as
//! [`item_count_gate`] over [`ActionValidatorHost::inventory_count`]).
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` live here. The Ghidra decompilation at
//! `ghidra/scripts/funcs/8003fb10.txt` is the *spec*, not source. Tests use
//! synthetic hosts.

/// Per-slot resource quad the validator's setup loop caches pointers to.
///
/// In battle these are battle-actor `+0x14C` (hp), `+0x14E` (hp_max),
/// `+0x150` (mp), `+0x152` (mp_max); out of battle they are character-record
/// `+0x106` (hp), `+0x104` (hp_max), `+0x10A` (mp), `+0x108` (mp_max).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SlotResources {
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
}

/// Effective (passive-boosted, capped) character-record stats read by the
/// arm-`0x06` stat-cap walker. Offsets are into the record at
/// `0x80084708 + slot*0x414`; names match `legaia_save::character`'s live
/// stat block. Arm `0x06` reads the **character records** regardless of
/// game mode - only the hp liveness pre-check uses the mode-selected quad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStat {
    /// `+0x104` - effective max HP (cap 9999).
    HpMax,
    /// `+0x108` - effective max MP (cap 999).
    MpMax,
    /// `+0x110` - AGL class (cap `0x118` = 280).
    Agl,
    /// `+0x112` - Attack (cap 999).
    Atk,
    /// `+0x114` - Up-defence (cap 999).
    Udf,
    /// `+0x116` - Down-defence (cap 999).
    Ldf,
    /// `+0x118` - Speed (cap 999).
    Spd,
    /// `+0x11A` - Intelligence (cap 999).
    Int,
}

/// Engine-side reads the validator dispatches into. Mirrors the
/// [`super::BattleActionHost`] abstraction pattern: every method documents
/// which retail global / helper it stands in for, and everything except the
/// record accessors has a default so a minimal host compiles.
pub trait ActionValidatorHost {
    /// `_DAT_8007B83C == 0x15` - the in-battle game mode. Selects the
    /// battle-actor table vs the character-record array as the resource
    /// source, and gates the out-of-battle arms (`0x80..=0x82` return
    /// invalid in battle; arms `0x03`/`0x08` branch per mode).
    fn in_battle(&self) -> bool;

    /// The `(hp, hp_max, mp, mp_max)` quad for a slot (see
    /// [`SlotResources`] for the per-mode offsets). Return `None` for a
    /// slot outside the table - retail's setup loop fills 7 slots in
    /// battle / 3 out of battle and an out-of-range index would read
    /// uninitialised stack; the port treats `None` as an all-zero quad
    /// (dead slot), which is the safe deterministic subset.
    fn slot_resources(&self, slot: u8) -> Option<SlotResources>;

    /// Status word: battle-actor `+0x16E` in battle, character-record
    /// `+0x12E` out of battle (retail reads the latter signed - arm
    /// `0x08` keeps the sign-extension quirk). Default 0 (no status).
    fn status_word(&self, _slot: u8) -> u16 {
        0
    }

    /// Arm-`0x06` character-record effective stat read
    /// (`0x80084708 + slot*0x414 + offset`, see [`RecordStat`]).
    /// Default 0 (below every cap - "stat can still be raised").
    fn record_stat(&self, _slot: u8, _stat: RecordStat) -> u16 {
        0
    }

    /// `DAT_80084594` - present-party member count (arm `0x01` walk bound).
    /// Default 0 (empty party - arm `0x01` validates nothing).
    fn party_count(&self) -> u8 {
        0
    }

    /// `(&DAT_80084598)[index]` - the slot occupied by the `index`-th
    /// present party member. Default: identity.
    fn party_member_slot(&self, index: u8) -> u8 {
        index
    }

    /// `_DAT_1F800394` - the scratchpad engine/UI flag word. Arm `0x80`
    /// tests bit `0x100000`, arm `0x81` bit `0x200000` (set = blocked).
    /// Default 0 (no block).
    fn engine_flag_word(&self) -> u32 {
        0
    }

    /// `FUN_8003CE64(idx)` - system-flag bank test (`DAT_80085758`; arm
    /// `0x80` tests flag 5, arm `0x81` flag 6; set = blocked). The bit
    /// arithmetic is already ported as
    /// [`crate::field_helpers::party_flag_test`]; hosts bridge to their
    /// flag bank. Default `false` (clear).
    ///
    /// REF: FUN_8003CE64
    fn system_flag(&self, _idx: u16) -> bool {
        false
    }

    /// The `gp + 0x2E8` word - the running inventory item count the arm-`0x82`
    /// callee `FUN_80046898` compares against the `0xE0` (224) cap. See
    /// [`item_count_gate`]. Default 0 (empty inventory - gate open).
    fn inventory_count(&self) -> i32 {
        0
    }
}

/// The out-of-battle inventory gate arm `0x82` tails into: a 3-instruction
/// leaf returning `*(int *)(gp + 0x2E8) < 0xE0` - "the inventory has room"
/// (signed compare against the 224-slot cap). Retail returns this raw value
/// as the validator result; the validity byte is not touched.
///
/// PORT: FUN_80046898
pub fn item_count_gate(count: i32) -> bool {
    count < 0xE0
}

/// Byte-truncated per-slot mask, `(byte)(1 << (slot & 0x1F))` - slots
/// `8..=31` truncate to 0 (their "bit" writes are no-ops), matching the
/// retail `sllv` + byte store.
fn slot_mask(slot: u8) -> u8 {
    ((1u32 << (u32::from(slot) & 0x1F)) & 0xFF) as u8
}

fn resources<H: ActionValidatorHost + ?Sized>(host: &H, slot: u8) -> SlotResources {
    host.slot_resources(slot).unwrap_or_default()
}

/// Validate a queued battle/menu action against a candidate slot.
///
/// PORT: FUN_8003FB10
///
/// `arm` is the outer dispatch byte (`param_1`), `sub_case` the arm-`0x06`
/// stat selector (`param_2`), `slot` the candidate actor/record slot
/// (`param_3`). `validity_bits` models the per-slot validity bitmask byte
/// at retail `gp + 0x9A8`; the per-arm write discipline (clear-then-set /
/// whole-byte overwrite / untouched) matches the dump. Returns `true` when
/// the action may proceed (retail's non-zero return).
///
/// Arm map (see `docs/subsystems/battle-action.md` § Action validator):
/// `0x00` heal target, `0x01` party walk, `0x02` MP-restore target,
/// `0x03` status present, `0x04` dead target (revive), `0x05`/`0x07`
/// alive, `0x06` stat-below-cap walker, `0x08` status bits 0-1,
/// `0x09`/`0x0A` force-valid (byte = 7), `0x0B..=0x0D` exact-slot match,
/// `0x80`/`0x81` out-of-battle flag gates, `0x82` external gate, `0x83`
/// always valid. Every other arm byte (`0x0E..=0x7F`, `>= 0x84`) is an
/// unhandled jump-table slot returning invalid with `validity_bits`
/// untouched.
pub fn validate_action<H: ActionValidatorHost + ?Sized>(
    host: &mut H,
    arm: u8,
    sub_case: u8,
    slot: u8,
    validity_bits: &mut u8,
) -> bool {
    let mask = slot_mask(slot);
    match arm {
        // Arm 0x00 (dump 8003fb10.txt asm lines 95..116, decomp 565..573):
        // clear the slot bit; alive AND hp < hp_max (heal target) sets it.
        0x00 => {
            *validity_bits &= !mask;
            let r = resources(host, slot);
            if r.hp == 0 {
                return false;
            }
            if r.hp < r.hp_max {
                *validity_bits |= mask;
                return true;
            }
            false
        }
        // Arm 0x01 (asm 117..158, decomp 574..591): zero the whole byte,
        // then walk the present party (count DAT_80084594, slots
        // DAT_80084598[i]) setting a bit per alive-and-not-full member.
        0x01 => {
            *validity_bits = 0;
            let count = host.party_count();
            let mut any = false;
            for i in 0..count {
                let member = host.party_member_slot(i);
                let r = resources(host, member);
                if r.hp != 0 && r.hp < r.hp_max {
                    any = true;
                    *validity_bits |= slot_mask(member);
                }
            }
            any
        }
        // Arm 0x02 (asm 159..182, decomp 592..601): clear bit; alive AND
        // mp < mp_max (MP-restore target) sets it.
        0x02 => {
            *validity_bits &= !mask;
            let r = resources(host, slot);
            if r.hp != 0 && r.mp < r.mp_max {
                *validity_bits |= mask;
                return true;
            }
            false
        }
        // Arm 0x03 (asm 183..215, decomp 602..612): status-flag presence.
        // Battle branch returns on the raw status word WITHOUT touching
        // the bit byte; the field branch clears then conditionally sets.
        0x03 => {
            if host.in_battle() {
                return host.status_word(slot) != 0;
            }
            *validity_bits &= !mask;
            if host.status_word(slot) != 0 {
                *validity_bits |= mask;
                return true;
            }
            false
        }
        // Arm 0x04 (asm 216..229, decomp 613..620): clear bit; DEAD target
        // (revive validator) sets it.
        0x04 => {
            *validity_bits &= !mask;
            let r = resources(host, slot);
            if r.hp != 0 {
                return false;
            }
            *validity_bits |= mask;
            true
        }
        // Arms 0x05 (asm 230..240, decomp 621..626) and 0x07 (asm 690..695
        // decomp; a byte-identical twin arm in the jump table): clear bit;
        // alive sets it.
        0x05 | 0x07 => {
            *validity_bits &= !mask;
            let r = resources(host, slot);
            if r.hp != 0 {
                *validity_bits |= mask;
                return true;
            }
            false
        }
        // Arm 0x06 (asm 241..262 entry + 263..396 sub-arms, decomp
        // 627..689): stat-below-cap walker. Clear bit; dead slot is
        // invalid; sub-case picks which effective record stat(s) to test
        // against their caps (second jump table at 0x80014F80). Any stat
        // still below cap sets the bit.
        0x06 => {
            *validity_bits &= !mask;
            let r = resources(host, slot);
            if r.hp == 0 {
                return false;
            }
            let below = |stat: RecordStat, cap: u16| host.record_stat(slot, stat) < cap;
            // The UDF/LDF pair is the shared tail (LAB_800400C0) that
            // sub-cases 2 and 6 both run.
            let udf_ldf = || below(RecordStat::Udf, 999) || below(RecordStat::Ldf, 999);
            let any = match sub_case {
                // asm 263..274: HP max < 9999.
                0 => below(RecordStat::HpMax, 9999),
                // asm 275..286: ATK < 999.
                1 => below(RecordStat::Atk, 999),
                // decomp 640..642 -> LAB_800400C0 (asm 368..388):
                // UDF < 999 or LDF < 999.
                2 => udf_ldf(),
                // asm 287..298: SPD < 999.
                3 => below(RecordStat::Spd, 999),
                // asm 299..310: INT < 999.
                4 => below(RecordStat::Int, 999),
                // asm 311..322: MP max < 999.
                5 => below(RecordStat::MpMax, 999),
                // asm 323..388 (decomp 652..677): every stat, AGL included
                // (AGL cap 0x118 = 280), falling into the UDF/LDF tail.
                6 => {
                    below(RecordStat::Agl, 0x118)
                        || below(RecordStat::HpMax, 9999)
                        || below(RecordStat::Atk, 999)
                        || below(RecordStat::Spd, 999)
                        || below(RecordStat::Int, 999)
                        || below(RecordStat::MpMax, 999)
                        || udf_ldf()
                }
                // sub_case >= 7: no handled table slot - result stays 0
                // (asm 253..254 bound check jumps to the s0 test).
                _ => false,
            };
            if any {
                *validity_bits |= mask;
                return true;
            }
            false
        }
        // Arm 0x08 (asm 408..449, decomp 696..710): alive AND status bits
        // 0-1. Dead slot returns invalid with the bit byte UNTOUCHED; the
        // battle branch also never writes it. The field branch clears the
        // bit, reads the record status word SIGNED and tests it against
        // 0xFFFF0003 - a negative status word (bit 15 set) validates even
        // with bits 0-1 clear, a faithful sign-extension quirk.
        0x08 => {
            let r = resources(host, slot);
            if r.hp == 0 {
                return false;
            }
            if host.in_battle() {
                return host.status_word(slot) & 3 != 0;
            }
            *validity_bits &= !mask;
            let sign_extended = host.status_word(slot) as i16 as i32 as u32;
            if sign_extended & 0xFFFF_0003 != 0 {
                *validity_bits |= mask;
                return true;
            }
            false
        }
        // Arms 0x09 / 0x0A (asm 450..453, decomp 718..722): force-valid -
        // overwrite the whole byte with 7 (all three party slots).
        0x09 | 0x0A => {
            *validity_bits = 7;
            true
        }
        // Arms 0x0B / 0x0C / 0x0D (asm 454..463, decomp 723..731): valid
        // only when the slot equals `arm - 0x0B`; on match the whole byte
        // becomes that slot's mask.
        0x0B..=0x0D => {
            if slot != arm - 0x0B {
                return false;
            }
            *validity_bits = mask;
            true
        }
        // Arms 0x80 / 0x81 (asm 464..494, decomp 732..748): out-of-battle
        // gates. Invalid in battle, invalid while the scratchpad flag-word
        // bit (0x100000 / 0x200000) is up, invalid while system flag 5 / 6
        // is set. No bit-byte writes.
        0x80 | 0x81 => {
            if host.in_battle() {
                return false;
            }
            let (word_bit, flag_idx) = if arm == 0x80 {
                (0x0010_0000, 5)
            } else {
                (0x0020_0000, 6)
            };
            if host.engine_flag_word() & word_bit != 0 {
                return false;
            }
            !host.system_flag(flag_idx)
        }
        // Arm 0x82 (asm 495..503, decomp 756..761): invalid in battle;
        // otherwise the inventory-count gate FUN_80046898 decides.
        0x82 => {
            if host.in_battle() {
                return false;
            }
            item_count_gate(host.inventory_count())
        }
        // Arm 0x83 (asm 504, decomp 762..763): always valid.
        0x83 => true,
        // Unhandled jump-table slots (0x0E..=0x7F) and the `sltiu 0x84`
        // bound (asm 85..86): invalid, bits untouched.
        _ => false,
    }
}
