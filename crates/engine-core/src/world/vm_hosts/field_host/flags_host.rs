//! `FieldFlagsHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldFlagsHost for FieldHostImpl<'a> {
    fn global_flags(&self) -> u32 {
        self.world.flags.story_flags
    }

    fn set_global_flags(&mut self, value: u32) {
        self.world.flags.story_flags = value;
    }

    fn frame_delta(&self) -> u16 {
        // Retail's `DAT_1F800393` as a script context sees it: the game
        // frame's length on the vsyncs that are game frames, nothing between.
        self.world.clock.script_frame_delta()
    }

    /// Op `0x4E` loader 4 (`0x801E0AFC`): one BIOS `rand()` draw, masked to
    /// a byte by the VM - the random-chance branch. Draws on the world's one
    /// `rand()` stream, as retail's every caller shares the kernel seed.
    fn bios_rand(&mut self) -> i32 {
        self.world.next_rand() as i32
    }

    // `4C C5` / `4C C6` take the trait's default, retail's `FUN_8003CE64`
    // test over the world's flag bank: the shipped waits' raisers are the
    // move-VM parts the waiting record spawns (ext `0x1C` / `0x1D`, on the
    // world through `MoveHost::ext_set_flag_bank`), the scripted-scene actor
    // and the travel arts, all of which the engine runs.

    fn extra_flags(&self) -> u32 {
        self.world.flags.extra_flags
    }

    /// Op `0x42` mode 1's word is the **held pad** `_DAT_8007B850` in its
    /// packed form (`lw v0, -0x47B0(at)` at `0x801DFC08`): the d-pad in
    /// `0xF000`, Triangle / Circle / Cross / Square in `0x10` / `0x20` /
    /// `0x40` / `0x80` - the word the tile board and the world-map panels
    /// read. Rim Elm's "stand here and press Down" polls (`42 01 00 ..`,
    /// `town01` P2[12..14]) are mode-1 tests against it.
    fn held_pad(&self) -> u32 {
        u32::from(legaia_engine_field::world_map_panel_host::packed_pad(
            self.world.input.pad(),
        ))
    }

    /// The compass table at `0x801F28D0` (field overlay data): the
    /// `0xF000` d-pad value op `0x42` mode 1 compares the held pad against
    /// (`bne v0, v1` at `0x801DFC14`) - Down, Down+Left, Left, Left+Up,
    /// Up, Up+Right, Right, Right+Down.
    fn pad_compass_entry(&self, index: u8) -> Option<u32> {
        const PAD_COMPASS: [u32; 8] = [
            0x4000, 0xC000, 0x8000, 0x9000, 0x1000, 0x3000, 0x2000, 0x6000,
        ];
        PAD_COMPASS.get(usize::from(index)).copied()
    }

    // Shared system flag bank - same fourth-flag-bank at `_DAT_80085758`
    // that move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D query, plus the
    // 0x5x / 0x6x / 0x7x default-route opcodes.
    fn system_flag_set(&mut self, idx: u16) {
        self.world.system_flag_set(idx);
    }

    fn system_flag_clear(&mut self, idx: u16) {
        self.world.system_flag_clear(idx);
    }

    fn system_flag_test(&self, idx: u16) -> bool {
        self.world.system_flag_test(idx)
    }

    /// Op `4C CA <slot> <u16>` - store into the script-counter slot table
    /// `0x801C6460` (`sh v0, 0(v1)` at `0x801E2954`). See
    /// [`crate::world::FieldVmState::slot_table`].
    ///
    /// REF: FUN_801DE840 (the nibble-C sub-`0xA` arm)
    fn slot_table_write(&mut self, slot: u8, value: i16) {
        self.world.field_vm.slot_table[usize::from(slot)] = value;
    }

    /// Op `4C CB` / `4C CC <slot> <u16>` - add to / subtract from a slot
    /// (`lhu` / `addu` or `subu` / `sh` at `0x801E2988..0x801E29DC`): a
    /// 16-bit wrapping update. The VM has already substituted the frame tick
    /// for a `0xFFFF` literal.
    ///
    /// REF: FUN_801DE840 (the nibble-C sub-`0xB` / sub-`0xC` arms)
    fn slot_table_adjust(&mut self, slot: u8, delta: i16, subtract: bool) {
        let cell = &mut self.world.field_vm.slot_table[usize::from(slot)];
        *cell = if subtract {
            cell.wrapping_sub(delta)
        } else {
            cell.wrapping_add(delta)
        };
    }

    /// Op `0x4E` sub-ops `5..=8` - the signed slot read (`0x801E0B0C`).
    fn slot_table_read(&self, slot: u8) -> i16 {
        self.world.field_vm.slot_table[usize::from(slot)]
    }

    // Op 0x4C nibble-D sub-3 - arm the scripted countdown timer. The three
    // operands are the installer's four global writes; the world keeps them
    // as one `EscapeTimer` plus the packed flag word, and `World::tick`
    // drains it once per retail frame.
    // REF: FUN_801DE840 case 0xD sub 3, FUN_801D2EBC (the drain)
    fn schedule_timed_flags(&mut self, ab: u32, cd: u32, ef: u32) {
        self.world.schedule_timed_flags(ab, cd, ef);
    }
}
