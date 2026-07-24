//! Small `SCUS_942.54`-resident leaf kernels: setters, seeders and one list
//! initialiser that the overlays call but that have no home of their own.
//!
//! Every one of these is a self-entry body ending in `jr ra`, read out of
//! `extracted/SCUS_942.54` at the file offsets noted per item. Four of the
//! five have Ghidra dumps that carry **decompiled C only** (`0 instructions`),
//! which is one of the catalogued decompiler artifacts - the rows below are
//! therefore read from the executable itself with
//! `scripts/ghidra-analysis/disasm-overlay-fn.py --base 0x80010000
//! --header 0x800`.
//!
//! PORT: FUN_8001FA00 - identity index-list init.
//! PORT: FUN_80035BAC - SFX-cue delay set on the parked slot.
//! PORT: FUN_80035C00 - staged-character selector setter.
//! PORT: FUN_800267A8 - timed sound-source arm.
//! PORT: FUN_800265E8 - boot-time offset-table seeder.
//! PORT: FUN_8003A024 - scene control-block allocate + reset.
//!
//! NOT WIRED: these are leaf setters whose callers are overlay-resident
//! dispatchers this lane does not own; the state they write lives in
//! host structs other lanes are still shaping.
//!
//! REF: FUN_800267FC - the tick half of the timed sound-source pair, ported
//! in [`crate::sound_state`].
//! REF: FUN_80062004 - the libsnd `SsSeqSetVol` shim the arm tail-calls.
//! REF: FUN_80017888 - the allocator the scene control-block reset calls.
//! REF: FUN_8003A110 - the per-scene populate pass that runs after the reset.

/// Identity index-list init (`FUN_8001FA00`, file offset `0x10200`,
/// 13 instructions).
///
/// Fills `list[i] = i` for `i in 0..n` and returns `n - 1`, which is the
/// value retail stores through the `count_out` pointer. Nothing is written to
/// the list when `n <= 0` (`blez a2`), but the `n - 1` store is
/// **unconditional** - it sits after the loop's exit label, so a zero-length
/// call still parks `-1`.
///
/// The `-1` is not an error code: it is the *top index* convention the
/// matching pop / push pair at `0x8001FA34` / `0x8001FA68` consumes, where an
/// empty list is `-1` rather than `0`.
pub fn init_identity_index_list(list: &mut [u16], n: i16) -> i16 {
    if n > 0 {
        for (i, slot) in list.iter_mut().take(n as usize).enumerate() {
            *slot = i as u16;
        }
    }
    n.wrapping_sub(1)
}

/// The per-slot SFX-cue delay table `DAT_8007C338`, indexed by the parked
/// slot number at `gp+0x15A`.
///
/// `FUN_80035BAC` (file offset `0x263AC`, 9 instructions) is the only writer
/// that stores a **non-zero** delay: it sign-extends its `i16` argument and
/// stores it as a full word at `DAT_8007C338 + slot * 4`. The enqueue path
/// and the overwrite at `0x80035BD0` both store zero there, so a non-zero
/// entry means "this cue is scheduled, not fired".
///
/// The slot index is read fresh from `gp+0x15A` on every call
/// (`lh v1,0x15a(gp)`), so the delay lands on whichever slot the enqueue
/// last parked - the caller does not name it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SfxCueDelays {
    slots: Vec<i32>,
}

impl SfxCueDelays {
    /// A table with `slots` entries, all zero (fired immediately).
    pub fn new(slots: usize) -> Self {
        Self {
            slots: vec![0; slots],
        }
    }

    /// `FUN_80035BAC(delay)` against the currently parked slot.
    ///
    /// Returns `false` when the parked slot is outside the table; retail does
    /// no bounds check at all, so a host that can produce an out-of-range
    /// `gp+0x15A` is describing a state retail would have corrupted.
    pub fn set_delay(&mut self, parked_slot: i16, delay: i16) -> bool {
        let Ok(idx) = usize::try_from(parked_slot) else {
            return false;
        };
        match self.slots.get_mut(idx) {
            Some(cell) => {
                // `sll a0,a0,0x10; sra a0,a0,0x10; sw a0,(v1)` - a
                // sign-extended halfword stored as a word.
                *cell = i32::from(delay);
                true
            }
            None => false,
        }
    }

    /// The delay currently parked on `slot`.
    pub fn delay(&self, slot: usize) -> Option<i32> {
        self.slots.get(slot).copied()
    }
}

/// The staged-character selector pair (`FUN_80035C00`, file offset `0x26400`,
/// four instructions).
///
/// The whole body is `sh a0,0x858(gp)` / `sh a1,0x860(gp)` / `jr ra`, i.e. it
/// writes `_DAT_8007BB70` and `_DAT_8007BB78` and nothing else - no read, no
/// gate, no side effect. The pause-menu notify / message-box path reads the
/// pair back as a character-record selector.
///
/// The **8 bytes between** the two cells are deliberately untouched, which is
/// what separates this setter from the clear at `0x80035C10` (that one zeroes
/// `gp+0x854`, `gp+0x864` and `gp+0x86C` - a disjoint set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StagedCharacterSelector {
    /// `gp+0x858` (`_DAT_8007BB70`).
    pub primary: u16,
    /// `gp+0x860` (`_DAT_8007BB78`).
    pub secondary: u16,
}

impl StagedCharacterSelector {
    /// `FUN_80035C00(a, b)`.
    pub fn set(&mut self, primary: u16, secondary: u16) {
        self.primary = primary;
        self.secondary = secondary;
    }
}

/// The five `gp` cells `FUN_800267A8` (file offset `0x16FA8`, 21
/// instructions) writes when it arms the timed sound-source release.
///
/// This is the **arm** half of the pair whose tick half
/// ([`crate::sound_state::SoundReleaseTimer`]) is already ported. The tick
/// consumes three of these cells; the arm writes two more:
///
/// | Cell | Written |
/// |---|---|
/// | `gp+0x808` | `1` - the armed flag |
/// | `gp+0x80C` | `_DAT_8007B910`, latched |
/// | `gp+0x810` | the caller's tag |
/// | `gp+0x814` | the caller's deadline |
/// | `gp+0x81C` | `0` - elapsed |
///
/// It then tail-calls the libsnd volume shim `FUN_80062004` with three
/// arguments the disassembly makes explicit and the C rendering blurs:
/// `(*(i16*)0x80070536, (i16)(latched_level >> 1), deadline | 1)`. The middle
/// argument is the `sll a1,a1,0xf; sra a1,a1,0x10` pair at `0x800267E0` -
/// a halving with a sign-extending truncation, not a plain shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedSoundArm {
    /// `gp+0x810`.
    pub tag: u32,
    /// `gp+0x814`, in vsyncs.
    pub deadline: u32,
    /// `gp+0x80C` - the level latched off `_DAT_8007B910` at arm time.
    pub latched_level: i32,
}

impl TimedSoundArm {
    /// `FUN_800267A8(tag, deadline)` with the live `_DAT_8007B910`.
    pub fn arm(tag: u32, deadline: u32, brightness_level: i32) -> Self {
        Self {
            tag,
            deadline,
            latched_level: brightness_level,
        }
    }

    /// The second argument handed to `FUN_80062004`.
    ///
    /// `(level << 15) >> 16` - the low 16 bits of `level * 2` interpreted as
    /// a signed halfword and shifted back down, which for the retail range
    /// `0..=0xFF` is exactly `level / 2`.
    pub fn shim_level(&self) -> i16 {
        ((self.latched_level << 15) >> 16) as i16
    }

    /// The third argument handed to `FUN_80062004` (`ori a2,a2,1`).
    pub fn shim_deadline(&self) -> u32 {
        self.deadline | 1
    }
}

/// The twelve-word table `FUN_800265E8` (file offset `0x16DE8`, 33
/// instructions) seeds at `0x800917B0`, plus the three enable halfwords it
/// sets alongside.
///
/// The stores are issued out of order (the classic MIPS scheduling the
/// decompiler re-sorts); in address order the image is the table below.
/// Word `+0x24` is **never written** - the routine leaves it at whatever the
/// loader left there, which is why the array is `Option`-free but the gap is
/// called out here.
///
/// The consumer is not identified from this dump. Every literal ends in the
/// low byte `0x10` and the values ascend to `0x6F010`, so they read as a set
/// of offsets into one region rather than as pointers. Six of the seven
/// distinct values share the low *three* nibbles `0x010`; `0x6C810` does not,
/// so that is a coincidence of the set, not a rule.
pub const BOOT_OFFSET_TABLE: [u32; 12] = [
    0x0000_1010, // +0x00
    0x0001_0010, // +0x04
    0x0003_3010, // +0x08
    0x0006_0010, // +0x0C
    0x0006_5010, // +0x10
    0x0001_0010, // +0x14
    0x0003_3010, // +0x18
    0x0006_5010, // +0x1C
    0x0006_C810, // +0x20
    0x0000_0000, // +0x24 - NOT written by the routine
    0x0000_1010, // +0x28
    0x0006_F010, // +0x2C
];

/// The index into [`BOOT_OFFSET_TABLE`] the seeder skips.
pub const BOOT_OFFSET_TABLE_UNWRITTEN: usize = 9;

/// The three enable halfwords `FUN_800265E8` sets to `1`, as absolute
/// addresses: `sh v0,0x4(v1)` / `0x64(v1)` / `0x94(v1)` off the base
/// `0x8007051C`.
pub const BOOT_ENABLE_FLAG_ADDRS: [u32; 3] = [0x8007_0520, 0x8007_0580, 0x8007_05B0];

/// The reset image `FUN_8003A024` (file offset `0x2A824`, 59 instructions)
/// writes over the freshly allocated **scene control block** - the `0x64`-byte
/// struct every field / cutscene subsystem reaches through `_DAT_801C6EA4`.
///
/// The routine is `FUN_80017888(0, 0x64)` followed by a flat run of stores.
/// The fields it sets non-zero are the ones below; every other byte of the
/// `0x64` is explicitly zeroed, so the block is fully defined after the call -
/// nothing is left at allocator residue. The per-scene contents (`+0x00`
/// motion-VM script table, `+0x04` zone / camera-region records, `+0x20`
/// encounter table bases) are installed afterwards by `FUN_8003A110` from the
/// scene MAN; what this routine parks in `+0x00` / `+0x04` is the same static
/// fallback table for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneControlBlockReset {
    /// `+0x00` and `+0x04` - both seeded with the same static table pointer.
    pub fallback_table: u32,
    /// `+0x12`.
    pub field_12: u16,
    /// `+0x14`.
    pub field_14: u16,
    /// `+0x4C`.
    pub field_4c: u16,
    /// `+0x4E`.
    pub field_4e: u16,
    /// `+0x50`.
    pub field_50: u16,
}

/// The literal image `FUN_8003A024` installs.
pub const SCENE_CONTROL_BLOCK_RESET: SceneControlBlockReset = SceneControlBlockReset {
    fallback_table: 0x8007_3EE8,
    field_12: 0x26,
    field_14: 0x10,
    field_4c: 0x40,
    field_4e: 0x08,
    field_50: 0x04,
};

/// Size of the scene control block, in bytes (`FUN_80017888(0, 0x64)`).
pub const SCENE_CONTROL_BLOCK_SIZE: usize = 0x64;

/// The error code `FUN_8003A024` parks in `_DAT_8007B828` when the allocation
/// **fails**.
///
/// The store is guarded by `bne a0,zero` on the allocator's return value, and
/// the null pointer is written to `_DAT_801C6EA4` in that branch's delay slot
/// regardless - so retail carries on and dereferences it. This is a genuine
/// out-of-memory path, not a normal one.
pub const SCENE_CONTROL_BLOCK_ALLOC_FAIL_CODE: u32 = 0x1BC;

/// The two globals the reset clears alongside the block: the scene-scoped
/// scratch word `_DAT_8007B630` and the tile-descriptor pointer
/// `_DAT_8007B450` (the one the field VM's `0x49` opcode installs a tile board
/// into - see `docs/subsystems/tile-board.md`).
pub const SCENE_RESET_CLEARED_GLOBALS: [u32; 2] = [0x8007_B630, 0x8007_B450];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_list_uses_a_top_index_not_a_length() {
        let mut list = [0xFFFF_u16; 4];
        assert_eq!(init_identity_index_list(&mut list, 4), 3);
        assert_eq!(list, [0, 1, 2, 3]);
    }

    #[test]
    fn a_non_positive_count_still_parks_the_top_index() {
        let mut list = [0xAAAA_u16; 4];
        assert_eq!(init_identity_index_list(&mut list, 0), -1);
        assert_eq!(list, [0xAAAA; 4], "blez a2 skips the whole loop");
        assert_eq!(init_identity_index_list(&mut list, -3), -4);
    }

    #[test]
    fn the_sfx_delay_lands_on_the_parked_slot() {
        let mut t = SfxCueDelays::new(8);
        assert!(t.set_delay(3, 0x40));
        assert_eq!(t.delay(3), Some(0x40));
        assert_eq!(t.delay(2), Some(0));
    }

    #[test]
    fn the_sfx_delay_is_sign_extended_to_a_word() {
        let mut t = SfxCueDelays::new(2);
        assert!(t.set_delay(0, -2));
        assert_eq!(
            t.delay(0),
            Some(-2),
            "sll/sra before the sw - a negative halfword stays negative"
        );
    }

    #[test]
    fn an_out_of_range_parked_slot_is_reported_rather_than_written() {
        let mut t = SfxCueDelays::new(2);
        assert!(!t.set_delay(9, 1));
        assert!(!t.set_delay(-1, 1));
    }

    #[test]
    fn the_selector_setter_writes_exactly_two_cells() {
        let mut s = StagedCharacterSelector::default();
        s.set(0x11, 0x22);
        assert_eq!(
            s,
            StagedCharacterSelector {
                primary: 0x11,
                secondary: 0x22
            }
        );
    }

    #[test]
    fn the_sound_arm_halves_the_latched_level_for_the_shim() {
        let a = TimedSoundArm::arm(7, 60, 0xD7);
        assert_eq!(a.shim_level(), 0x6B, "0xD7 >> 1");
        assert_eq!(a.shim_deadline(), 61, "deadline | 1");
        assert_eq!(a.latched_level, 0xD7);
    }

    #[test]
    fn the_shim_deadline_or_never_clears_a_bit() {
        assert_eq!(TimedSoundArm::arm(0, 61, 0).shim_deadline(), 61);
        assert_eq!(TimedSoundArm::arm(0, 0, 0).shim_deadline(), 1);
    }

    #[test]
    fn the_scene_reset_seeds_both_table_slots_from_one_pointer() {
        // `+0x00` and `+0x04` take the same word, and it is also mirrored to
        // the sibling global 0x801C6EA0.
        assert_eq!(SCENE_CONTROL_BLOCK_RESET.fallback_table, 0x8007_3EE8);
        assert_eq!(SCENE_CONTROL_BLOCK_SIZE, 0x64);
        assert_eq!(SCENE_CONTROL_BLOCK_ALLOC_FAIL_CODE, 0x1BC);
        assert_eq!(
            SCENE_RESET_CLEARED_GLOBALS,
            [0x8007_B630, 0x8007_B450],
            "-0x49d0 and -0x4bb0 off 0x80080000"
        );
    }

    #[test]
    fn the_boot_table_repeats_three_of_its_offsets() {
        // +0x04 / +0x14, +0x08 / +0x18, +0x10 / +0x1C are the same words.
        assert_eq!(BOOT_OFFSET_TABLE[1], BOOT_OFFSET_TABLE[5]);
        assert_eq!(BOOT_OFFSET_TABLE[2], BOOT_OFFSET_TABLE[6]);
        assert_eq!(BOOT_OFFSET_TABLE[4], BOOT_OFFSET_TABLE[7]);
        assert_eq!(BOOT_OFFSET_TABLE[0], BOOT_OFFSET_TABLE[10]);
        assert_eq!(BOOT_OFFSET_TABLE[BOOT_OFFSET_TABLE_UNWRITTEN], 0);
        assert!(
            BOOT_OFFSET_TABLE
                .iter()
                .enumerate()
                .all(|(i, w)| i == BOOT_OFFSET_TABLE_UNWRITTEN || w & 0xFF == 0x10)
        );
    }
}
