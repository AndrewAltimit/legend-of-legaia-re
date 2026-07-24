//! The battle side-band **streaming transfer** state machine - the three-stage
//! sequencer that pulls one `0x10800`-byte slot of `summon.dat` / `readef.DAT`
//! into the battle scratch buffer while a cast plays.
//!
//! Retail is `FUN_801F17F8` (battle overlay `0898`, `see
//! ghidra/scripts/funcs/overlay_battle_action_801f17f8.txt`), dispatched every
//! frame from the battle scene loader's case `0xFF`. The slot layout of the
//! two files, and what each slot carries, is
//! [`docs/formats/summon-readef.md`](../../../docs/formats/summon-readef.md);
//! this module is only the transfer sequencer.
//!
//! Two context bytes drive it: `ctx[+0x26B]` is the **request** (`0` = idle,
//! otherwise `slot + 1`) and `ctx[+0x26C]` is the **stage**. The open handle
//! lives at `ctx[+0x314]`.
//!
//! Transcribed from the disassembly, not the C.

/// Bytes per streamed slot - 33 sectors of 2048 (`a2 = 0x10800` at the read
/// call, `0x801F1970`). The seek offset is the same figure times the slot
/// index, which retail builds as `((i << 5) + i) << 11`.
pub const SLOT_BYTES: u32 = 0x1_0800;

/// The two CDNAME entry numbers the loader opens, as the 4th argument to the
/// open call (`0x801F18F4` / `0x801F192C`).
///
/// These are `#define` numbers, so the extracted files are two lower - `895`
/// is extraction entry **893** (`summon.dat`) and `896` is **894**
/// (`readef.DAT`). See
/// [`docs/formats/cdname.md`](../../../docs/formats/cdname.md#numbering-space).
pub const SUMMON_CDNAME: u16 = 0x37F;
/// The `readef.DAT` half of [`SUMMON_CDNAME`].
pub const READEF_CDNAME: u16 = 0x380;

/// Which of the two side-band files a request selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFile {
    /// `summon.dat` - selected when bit 7 of `request - 1` is **set**.
    Summon,
    /// `readef.DAT` - selected when that bit is clear.
    Readef,
}

impl StreamFile {
    /// The CDNAME entry number retail passes to the open call.
    pub fn cdname(self) -> u16 {
        match self {
            StreamFile::Summon => SUMMON_CDNAME,
            StreamFile::Readef => READEF_CDNAME,
        }
    }
}

/// The file + slot index a request byte resolves to.
///
/// Retail decodes `request - 1` once and reads bit 7 of it (`andi v0,v0,0x80`
/// at `0x801F18D8`); the summon arm then masks the index back down to seven
/// bits (`andi v0,v0,0x7f` at `0x801F1914`) while the readef arm uses
/// `request - 1` whole. A request of `0` is idle and has no target.
///
/// PORT: FUN_801F17F8 (the request-byte decode)
/// NOT WIRED: the engine has no slot-at-a-time CD streamer. Retail reaches
/// this from the battle scene loader `FUN_800520F0` case `0xFF`, whose port
/// (`engine-core::overlay_loader::battle_stage_overlay_entry`) is itself
/// inert, and the side-band files are read whole off the extracted PROT
/// entries by `legaia_asset::summon_readef` instead of a slot at a time.
pub fn decode_request(request: u8) -> Option<(StreamFile, u8)> {
    if request == 0 {
        return None;
    }
    let index = request.wrapping_sub(1);
    if index & 0x80 != 0 {
        Some((StreamFile::Summon, index & 0x7F))
    } else {
        Some((StreamFile::Readef, index))
    }
}

/// Byte offset of a slot from the start of its file - retail builds it as
/// `((i << 5) + i) << 11` in the seek call's argument slots
/// (`0x801F1948..0x801F1954`).
///
/// Not a separate port anchor: it is one arithmetic expression inside
/// [`StreamSlotSm::step`], which carries the `// PORT:` tag and the wiring
/// disclosure for the whole sequencer.
///
/// REF: FUN_801F17F8
pub fn slot_offset(index: u8) -> u32 {
    u32::from(index) * SLOT_BYTES
}

/// What one frame of the sequencer asks the host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamStep {
    /// Nothing to do - no request armed, or the stage byte is past the end.
    Idle,
    /// The CD is still busy (`FUN_8003DE7C(1)` returned non-zero); hold.
    WaitCd,
    /// Stage `0` cleared: release any handle left over from the previous
    /// transfer. The stage byte has already advanced.
    ReleasePrevious,
    /// Stage `1`: open `file`, seek to `offset`, read [`SLOT_BYTES`] into the
    /// battle scratch buffer (`*0x8007BD74`).
    Transfer {
        /// Which side-band file.
        file: StreamFile,
        /// Byte offset of the slot inside it.
        offset: u32,
    },
    /// Stage `2`: close the handle and retire the request.
    Finish,
}

/// The two context bytes the sequencer owns, plus the handle slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamSlotSm {
    /// `ctx[+0x26B]` - `0` idle, else `slot + 1`.
    pub request: u8,
    /// `ctx[+0x26C]` - stage cursor.
    pub stage: u8,
    /// `ctx[+0x314]` - non-zero while a file handle is open.
    pub handle_open: bool,
}

impl StreamSlotSm {
    /// Arm a transfer for `slot` - retail's `FUN_80055B4C`, which writes
    /// `ctx[+0x26B] = slot + 1`.
    ///
    /// REF: FUN_80055B4C
    pub fn arm(&mut self, slot: u8) {
        self.request = slot.wrapping_add(1);
        self.stage = 0;
    }

    /// One frame of the sequencer. `cd_busy` is `FUN_8003DE7C(1) != 0`.
    ///
    /// Stage `0` and stage `2` both gate on the CD being idle and do nothing
    /// at all while it is busy - retail returns without touching the stage
    /// byte (`bne v0,zero,<epilogue>` at `0x801F1880` / `0x801F199C`), so a
    /// busy drive costs a frame rather than a stage.
    ///
    /// PORT: FUN_801F17F8 (the `ctx[+0x26C]` stage machine)
    /// NOT WIRED: the engine has no slot-at-a-time CD streamer. Retail reaches
    /// this from the battle scene loader `FUN_800520F0` case `0xFF`, whose port
    /// (`engine-core::overlay_loader::battle_stage_overlay_entry`) is itself
    /// inert, and the side-band files are read whole off the extracted PROT
    /// entries by `legaia_asset::summon_readef` instead of a slot at a time.
    pub fn step(&mut self, cd_busy: bool) -> StreamStep {
        if self.request == 0 {
            return StreamStep::Idle;
        }
        match self.stage {
            0 => {
                if cd_busy {
                    return StreamStep::WaitCd;
                }
                self.stage = self.stage.wrapping_add(1);
                if self.handle_open {
                    self.handle_open = false;
                    StreamStep::ReleasePrevious
                } else {
                    // Retail's `beq a0,zero,<epilogue>` at `0x801F18B0`: with
                    // no handle to release the frame ends here, the stage
                    // bump having already landed.
                    StreamStep::Idle
                }
            }
            1 => {
                let Some((file, index)) = decode_request(self.request) else {
                    return StreamStep::Idle;
                };
                self.handle_open = true;
                self.stage = self.stage.wrapping_add(1);
                StreamStep::Transfer {
                    file,
                    offset: slot_offset(index),
                }
            }
            2 => {
                if cd_busy {
                    return StreamStep::WaitCd;
                }
                self.stage = self.stage.wrapping_add(1);
                self.handle_open = false;
                self.request = 0;
                StreamStep::Finish
            }
            _ => StreamStep::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_bit7_selects_the_file_and_masks_the_index() {
        assert_eq!(decode_request(0), None);
        // request 1 -> index 0, bit 7 clear -> readef slot 0.
        assert_eq!(decode_request(1), Some((StreamFile::Readef, 0)));
        assert_eq!(decode_request(0x80), Some((StreamFile::Readef, 0x7F)));
        // request 0x81 -> index 0x80, bit 7 set -> summon slot 0.
        assert_eq!(decode_request(0x81), Some((StreamFile::Summon, 0)));
        assert_eq!(decode_request(0xFF), Some((StreamFile::Summon, 0x7E)));
    }

    #[test]
    fn cdname_numbers_are_the_two_side_band_entries() {
        assert_eq!(StreamFile::Summon.cdname(), 895);
        assert_eq!(StreamFile::Readef.cdname(), 896);
    }

    #[test]
    fn slot_offsets_step_by_33_sectors() {
        assert_eq!(SLOT_BYTES, 33 * 2048);
        assert_eq!(slot_offset(0), 0);
        assert_eq!(slot_offset(1), 0x1_0800);
        assert_eq!(slot_offset(5), 5 * 0x1_0800);
    }

    #[test]
    fn a_transfer_runs_release_then_read_then_close() {
        let mut sm = StreamSlotSm {
            handle_open: true,
            ..Default::default()
        };
        sm.arm(2);
        assert_eq!(sm.request, 3);
        // A busy drive costs a frame and leaves the stage alone.
        assert_eq!(sm.step(true), StreamStep::WaitCd);
        assert_eq!(sm.stage, 0);
        assert_eq!(sm.step(false), StreamStep::ReleasePrevious);
        assert_eq!(
            sm.step(false),
            StreamStep::Transfer {
                file: StreamFile::Readef,
                offset: 2 * SLOT_BYTES,
            }
        );
        assert_eq!(sm.step(true), StreamStep::WaitCd);
        assert_eq!(sm.step(false), StreamStep::Finish);
        assert_eq!(sm.request, 0, "the request retires with the transfer");
        assert_eq!(sm.step(false), StreamStep::Idle);
    }

    #[test]
    fn a_first_transfer_has_no_handle_to_release() {
        let mut sm = StreamSlotSm::default();
        sm.arm(0x80);
        assert_eq!(sm.step(false), StreamStep::Idle, "stage 0 with no handle");
        assert_eq!(sm.stage, 1, "the stage still advanced");
        assert_eq!(
            sm.step(false),
            StreamStep::Transfer {
                file: StreamFile::Summon,
                offset: 0,
            }
        );
    }
}
