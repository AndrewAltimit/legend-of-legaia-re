//! How long the CD drive keeps a slot-B summon module waiting before its voice.
//!
//! A summon module parks the drive on its voice's `XA*.XA` file on one arm
//! (`FUN_8003EAE4(0, slot)`, `CdlSetloc` + `CdlSeekL`) and polls the seek on a
//! later one (`FUN_8003F2B8(1)`, `CdSync(1)`), returning busy until the drive
//! reports it done; only then does the arm call the head cue. The battle's own
//! seek-ahead selector (`FUN_8004DA00`, `engine-audio`'s `battle_voice`) has
//! parked the head on the caster's fanfare file a moment before, and the
//! module's seek waits that one out first. The port's streams are resident, so
//! nothing reads, but the wait is retail's: a module that passed it at once
//! started its voice up to a second early.
//!
//! [`seek_vsyncs`] is the drive's seek time over the sector distance, fitted to
//! mednafen's CD controller across the library's summon states
//! (`scripts/mednafen/drive-timeline.py`, `PSRCounter` at the seek's first
//! frame): a fixed `0.3` s for a seek of `2250` sectors or more, plus a full-disc
//! traverse of `72 * 60 * 75` sectors a second, plus two vsyncs of the logical
//! seek (`DriveStatus` `3`, then `-2`) before the command completes. Orb's
//! `XA27 -> XA13` (`53944` sectors) reads `15.60M..16.17M` cycles at its first
//! frame against `15.80M` here; the `XA8` seeks from the side-band region
//! (`25147`, `25444`, `25741` sectors) `12.44M..13.07M` against `12.79M..12.85M`;
//! `XA8 -> XA13` `11.17M` in `11.14M..11.71M`; `XA8 -> XA7` `10.53M` in
//! `10.12M..10.68M`. This is the emulator's drive, which the real-PCM oracle
//! scores against; no console has been timed.
//!
//! REF: FUN_8003EAE4, FUN_8003F2B8, FUN_8004DA00

/// The R3000's clock, cycles a second.
pub const CPU_HZ: u64 = 33_868_800;
/// The console's vsync in millihertz (`59.8261` Hz).
pub const VSYNC_MILLIHZ: u64 = 59_826;
/// Every seek of [`LONG_SEEK_SECTORS`] or more costs this much on top of its
/// distance: `0.3` s.
pub const LONG_SEEK_CYCLES: u64 = CPU_HZ * 3 / 10;
/// The distance from which a seek takes [`LONG_SEEK_CYCLES`].
pub const LONG_SEEK_SECTORS: u32 = 2250;
/// The sled crosses the whole disc in a second: `72 * 60 * 75` sectors.
pub const TRAVERSE_SECTORS_PER_SECOND: u64 = 72 * 60 * 75;
/// The shortest seek, cycles.
pub const MIN_SEEK_CYCLES: u64 = 20_000;
/// The logical seek's vsyncs after the physical one, before `CdSync` reports
/// the command complete.
pub const LOGICAL_SEEK_VSYNCS: u32 = 2;

/// Absolute start LBA of `XA1.XA` .. `XA34.XA` on the USA disc (the ISO9660
/// directory; `disc-extract list`). The seek table `0x801C6ED8 + n * 8` that
/// `FUN_8003EAE4` reads holds file `XA<n + 1>` at index `n`: the summon states'
/// seeks to clip `0x1A`, `0xC`, `0xD`, `7` and `6` land on `XA27`, `XA13`,
/// `XA14`, `XA8` and `XA7`'s first sectors.
pub const XA_FILE_LBA: [u32; 34] = [
    59449, 60513, 61025, 62001, 62369, 63265, 63697, 67185, 69273, 70785, 72337, 74297, 76833,
    80913, 86441, 93673, 95393, 98865, 103153, 104505, 108265, 108913, 117089, 121665, 123873,
    128649, 130777, 131449, 132161, 132833, 133153, 133697, 134065, 134497,
];

/// Where the head stands once a cast's side-band read is done: the start of
/// `summon.dat` (extraction PROT 0893, raw TOC index `0x37F`, relative LBA
/// `41006` past `PROT.DAT` at `242`). The summon states read `41347..42104`
/// there just before their fanfare seek; the slot's own offset moves a seek by
/// well under a vsync.
pub const SIDE_BAND_LBA: u32 = 242 + 41006;

/// The battle seek-ahead selector's clip for a party seat's Magic action
/// (`FUN_8004DA00`, class `2`): the seat's fanfare file `0x19 + slot` when the
/// spell's class byte (`DAT_800754C8`) is below `0x14`, the shared `7`
/// otherwise. `slot` is the seat's 1-based party slot (`DAT_8007BD10`). Orb's
/// Vahn seeks `0x1A` (`XA27`); Swordie, Theeder, Nighto and Gilium's casts `7`
/// (`XA8`).
pub fn party_magic_seek_clip(slot: u8, spell_class: u8) -> u8 {
    if spell_class < 0x14 {
        slot.wrapping_add(0x19)
    } else {
        7
    }
}

/// The drive's start LBA for seek-table clip `index` (`XA<index + 1>`).
pub fn clip_lba(index: u32) -> Option<u32> {
    XA_FILE_LBA.get(index as usize).copied()
}

/// Vsyncs from issuing a seek from `from` to `to` to the command's completion.
pub fn seek_vsyncs(from: u32, to: u32) -> u32 {
    let d = from.abs_diff(to);
    let mut cycles = (u64::from(d) * CPU_HZ / TRAVERSE_SECTORS_PER_SECOND).max(MIN_SEEK_CYCLES);
    if d >= LONG_SEEK_SECTORS {
        cycles += LONG_SEEK_CYCLES;
    }
    let per_vsync_milli = CPU_HZ * 1000;
    // cycles * vsync_hz / cpu_hz, rounded up.
    let v = (cycles * VSYNC_MILLIHZ).div_ceil(per_vsync_milli);
    v as u32 + LOGICAL_SEEK_VSYNCS
}

/// The drive as a cast sees it: where the head is bound and how many vsyncs
/// the commands already issued still take. A seek issued while one runs
/// starts when it ends (the module's `CdlSetloc` waits on the drive).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CdDrive {
    /// The LBA the last seek issued targets.
    pub lba: u32,
    /// Vsyncs until every issued command has completed.
    pub busy: u32,
}

impl CdDrive {
    /// A drive standing at `lba` with nothing in flight.
    pub fn at(lba: u32) -> Self {
        Self { lba, busy: 0 }
    }

    /// Issue a seek to `to`, after whatever is in flight.
    pub fn seek(&mut self, to: u32) {
        self.busy += seek_vsyncs(self.lba, to);
        self.lba = to;
    }

    /// One vsync passes.
    pub fn tick(&mut self) {
        self.busy = self.busy.saturating_sub(1);
    }

    /// `CdSync(1)` would report the last command complete.
    pub fn idle(&self) -> bool {
        self.busy == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One vsync of CPU cycles, `CPU_HZ / 59.8261`.
    const VSYNC_CYCLES: f64 = 33_868_800.0 / 59.8261;

    #[test]
    fn the_seeks_the_summon_states_measure() {
        // (from, to, cycles left at the seek's first logged frame), from the
        // library's mednafen summon states: the seek was issued inside that
        // frame, so the model's length lies within a vsync above it.
        for (from, to, first_frame) in [
            (130_777, 76_833, 15_600_590.0),
            (42_038, 67_185, 12_437_513.0),
            (41_444, 67_185, 12_502_914.0),
            (41_741, 67_185, 12_458_544.0),
            (67_185, 76_833, 11_140_330.0),
            (67_185, 63_697, 10_115_092.0),
        ] {
            let physical = f64::from(seek_vsyncs(from, to) - LOGICAL_SEEK_VSYNCS);
            let lo = (first_frame / VSYNC_CYCLES).floor();
            assert!(
                physical >= lo && physical <= lo + 2.0,
                "{from} -> {to}: {physical} vsyncs against {first_frame} cycles"
            );
        }
    }

    #[test]
    fn a_seek_issued_while_one_runs_waits_for_it() {
        let mut d = CdDrive::at(SIDE_BAND_LBA);
        d.seek(clip_lba(0x1A).unwrap());
        let first = d.busy;
        d.seek(clip_lba(0xC).unwrap());
        assert_eq!(
            d.busy,
            first + seek_vsyncs(XA_FILE_LBA[26], XA_FILE_LBA[12])
        );
        for _ in 0..d.busy {
            assert!(!d.idle());
            d.tick();
        }
        assert!(d.idle());
    }

    #[test]
    fn the_seek_table_index_is_the_file_number_less_one() {
        assert_eq!(clip_lba(0x1A), Some(130_777));
        assert_eq!(clip_lba(0xC), Some(76_833));
        assert_eq!(clip_lba(7), Some(67_185));
        assert_eq!(clip_lba(34), None);
    }
}
