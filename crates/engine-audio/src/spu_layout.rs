//! The play hosts' SPU RAM map, and the one track that does not fit it.
//!
//! Both play hosts (the native `AudioBgmDirector` and the browser page's
//! `WebBgmDirector`) split the 512 KiB of SPU RAM the same way:
//!
//! | range | holds |
//! |---|---|
//! | `0 .. SPU_RESERVED_BYTES` | reserved (voice-0 scratch) |
//! | `SPU_RESERVED_BYTES .. SFX_REGION_BASE` | the BGM region: the current track's bank, and any bank borrowing its free tail |
//! | `SFX_REGION_BASE .. SPU_RAM_BYTES` | the resident SFX banks: slot `0` (PROT 0868) at the bottom, the slot-2 / slot-6 shared region above it |
//!
//! Retail has no such split. `FUN_800265E8` seeds a fixed SPU base per VAB id
//! into the table at `0x800917B0`, and `FUN_8002630C` with `a3 = 0` opens a
//! bank at its id's base (`jal 0x80068D34` with `a2 = table[vabid]`,
//! `0x80026344..0x80026358`). The field initialiser's ending-theme arm
//! (`FUN_801D6704`, `0x801D71A0..0x801D72D0`) opens the credits bank as VAB
//! `10`, and `table[10]` is `0x1010` - the same base as the slot-0 system bank
//! (`sw v1,0x28(v0)` with `v1 = 0x1010` at `0x80026668`). So the credits bank
//! is laid over the resident banks from the bottom of SPU RAM up.
//!
//! Its VAG bodies total `0x631B0` bytes, more than the hosts' BGM region
//! ([`BGM_REGION_BYTES`]). [`upload_owned_bank`] therefore places a bank
//! that does not fit the BGM region the way retail places VAB `10`: from the
//! BGM base straight across the SFX region, reporting that the resident SFX
//! banks were overwritten ([`OwnedBankUpload::evicts_sfx`]). A host drops its
//! SFX banks on that report (a cue keyed against them would read the credits
//! samples, which is what retail's slot-0 cues would do) and re-stages them
//! with [`upload_resident_sfx`] once a track that fits the BGM region takes
//! the region back ([`sfx_region_free`]).
//!
//! Retail never needs the re-stage inside one session: the one-shot credits
//! latch `0x8007B9B8` makes the BGM resolver `FUN_800243F0` return at once
//! (`bne` at `0x8002440C`), and nothing outside the debug menu overlay clears
//! it, so no other track loads after the credits until a reset re-runs the
//! boot loader that stages PROT 0868 again. A port host can leave the credits
//! (a save load, a scene jump), and the re-stage is that exit's version of
//! the boot load.

use crate::spu::Spu;
use crate::spu::ram::{SPU_RAM_BYTES as SPU_RAM_USIZE, SpuAllocator};
use crate::vab_bind::VabBank;
use legaia_vab::VabReport;

/// Total SPU RAM in bytes.
pub const SPU_RAM_BYTES: u32 = SPU_RAM_USIZE as u32;
/// Bytes at the bottom of SPU RAM no bank is allocated into.
pub const SPU_RESERVED_BYTES: u32 = 0x1000;
/// The resident SFX region at the top of SPU RAM: the slot-0 system bank and
/// the slot-2 / slot-6 shared region above it. PROT 0868's bodies (59136 B)
/// plus the largest shared-region bank a host stages there (PROT 0869,
/// 188128 B) need 247264 B; `0x3D000` holds them, and one step up would
/// leave the BGM region under the two largest scene BGM VABs on the disc.
pub const SFX_REGION_BYTES: u32 = 0x3D000;
/// First byte of the SFX region.
pub const SFX_REGION_BASE: u32 = SPU_RAM_BYTES - SFX_REGION_BYTES;
/// Size of the BGM region between the reserved head and the SFX region.
pub const BGM_REGION_BYTES: u32 = SFX_REGION_BASE - SPU_RESERVED_BYTES;
/// Retail's SPU base for VAB ids `0` and `10` (`0x800917B0[0]` and
/// `[10]`, seeded by `FUN_800265E8`). Documentation of the retail placement;
/// the hosts' own floor is [`SPU_RESERVED_BYTES`].
pub const RETAIL_VAB10_SPU_BASE: u32 = 0x1010;

/// Bytes a bank's VAG bodies take once each is rounded to the allocator's
/// 16-byte ADPCM block.
pub fn vab_body_bytes(report: &VabReport) -> u32 {
    report
        .vag_samples
        .iter()
        .map(|v| (v.size as u32).div_ceil(16) * 16)
        .sum()
}

/// One past the highest byte a staged bank's samples use; `None` for a bank
/// with no uploaded sample.
pub fn bank_used_end(bank: &VabBank) -> Option<u32> {
    bank.samples.iter().flatten().map(|s| s.addr + s.size).max()
}

/// Where a track's own bank goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedBankPlacement {
    /// The bodies fit the BGM region; the SFX region is untouched.
    BgmRegion,
    /// The bodies do not fit the BGM region but do fit everything above the
    /// reserved head: laid from the BGM base across the SFX region, as
    /// retail lays VAB `10` over the resident banks.
    AcrossSfxRegion,
    /// Larger than all of SPU RAM above the reserved head: uploaded as far
    /// as it fits, like any bank, and the tail bodies stay silent.
    TooLarge,
}

/// Classify a bank of `body_bytes` (see [`vab_body_bytes`]).
pub fn owned_bank_placement(body_bytes: u32) -> OwnedBankPlacement {
    if body_bytes <= BGM_REGION_BYTES {
        OwnedBankPlacement::BgmRegion
    } else if body_bytes <= SPU_RAM_BYTES - SPU_RESERVED_BYTES {
        OwnedBankPlacement::AcrossSfxRegion
    } else {
        OwnedBankPlacement::TooLarge
    }
}

/// A track bank staged by [`upload_owned_bank`].
#[derive(Debug, Clone)]
pub struct OwnedBankUpload {
    pub bank: VabBank,
    pub placement: OwnedBankPlacement,
    /// The upload wrote into the SFX region: every resident SFX bank's
    /// samples are overwritten and the host must drop them.
    pub evicts_sfx: bool,
}

/// Upload a track's own bank (`report` parsed from `bank_buf`, which is the
/// buffer `legaia_vab::parse` was given or that buffer re-sliced at the
/// VAB). A bank that fits goes into the BGM region, capped below the SFX
/// region; one that does not is laid from the same base across the SFX
/// region, the retail VAB-`10` placement (module docs).
///
/// REF: FUN_8002630C, FUN_800265E8 - the open-at-fixed-base arm and the
/// per-VAB base table this placement follows; the SPU transfer itself is
/// libspu, outside the port boundary.
pub fn upload_owned_bank(spu: &mut Spu, report: &VabReport, bank_buf: &[u8]) -> OwnedBankUpload {
    let placement = owned_bank_placement(vab_body_bytes(report));
    let room = match placement {
        OwnedBankPlacement::BgmRegion => BGM_REGION_BYTES,
        OwnedBankPlacement::AcrossSfxRegion | OwnedBankPlacement::TooLarge => {
            SPU_RAM_BYTES - SPU_RESERVED_BYTES
        }
    };
    let mut alloc = SpuAllocator::new(SPU_RESERVED_BYTES, room);
    let bank = VabBank::upload(spu, &mut alloc, report, bank_buf);
    let evicts_sfx = bank_used_end(&bank).is_some_and(|end| end > SFX_REGION_BASE);
    OwnedBankUpload {
        bank,
        placement,
        evicts_sfx,
    }
}

/// Whether the SFX region is free of the BGM bank - `false` while a bank
/// staged by [`upload_owned_bank`] still reaches into it. A host holding its
/// SFX banks dropped re-stages them once this turns `true`.
pub fn sfx_region_free(bgm_bank: Option<&VabBank>) -> bool {
    bgm_bank
        .and_then(bank_used_end)
        .is_none_or(|end| end <= SFX_REGION_BASE)
}

/// The resident SFX banks as [`upload_resident_sfx`] staged them.
#[derive(Debug, Clone)]
pub struct ResidentSfxUpload {
    /// The slot-0 system bank (PROT 0868), at [`SFX_REGION_BASE`].
    pub slot0: VabBank,
    /// The shared slot-2 / slot-6 bank, above slot 0's samples; `None` when
    /// none was asked for or it does not fit the rest of the region.
    pub shared: Option<VabBank>,
}

/// Where the shared slot-2 / slot-6 region starts: one past slot 0's
/// samples, rounded to the ADPCM block.
pub fn shared_region_base(slot0: &VabBank) -> u32 {
    bank_used_end(slot0).unwrap_or(SFX_REGION_BASE).div_ceil(16) * 16
}

/// Upload the shared slot-2 / slot-6 bank above `slot0`. `None` when its
/// bodies do not fit the rest of SPU RAM (the dance's PROT 1231 is the one
/// shipped bank that does not), which leaves both slots closed.
pub fn upload_shared_region(
    spu: &mut Spu,
    slot0: &VabBank,
    report: &VabReport,
    bank_buf: &[u8],
) -> Option<VabBank> {
    let base = shared_region_base(slot0);
    let room = SPU_RAM_BYTES.saturating_sub(base);
    if vab_body_bytes(report) > room {
        return None;
    }
    let mut alloc = SpuAllocator::new(base, room);
    Some(VabBank::upload(spu, &mut alloc, report, bank_buf))
}

/// Stage the resident SFX region: the slot-0 system bank at the region's
/// bottom, then (when given) the shared-region bank above it. Every host's
/// boot staging and every re-stage after an [`OwnedBankUpload::evicts_sfx`]
/// upload go through this, so the two layouts are the same by construction.
pub fn upload_resident_sfx(
    spu: &mut Spu,
    slot0: (&VabReport, &[u8]),
    shared: Option<(&VabReport, &[u8])>,
) -> ResidentSfxUpload {
    let mut alloc = SpuAllocator::new(SFX_REGION_BASE, SFX_REGION_BYTES);
    let slot0 = VabBank::upload(spu, &mut alloc, slot0.0, slot0.1);
    let shared = shared.and_then(|(r, b)| upload_shared_region(spu, &slot0, r, b));
    ResidentSfxUpload { slot0, shared }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vab_bind::UploadedVag;

    fn bank_ending_at(end: u32) -> VabBank {
        VabBank {
            master_vol: 127,
            samples: vec![Some(UploadedVag {
                addr: end - 0x10,
                size: 0x10,
            })],
            programs: Vec::new(),
        }
    }

    #[test]
    fn the_regions_partition_spu_ram() {
        assert_eq!(SPU_RESERVED_BYTES + BGM_REGION_BYTES, SFX_REGION_BASE);
        assert_eq!(SFX_REGION_BASE + SFX_REGION_BYTES, SPU_RAM_BYTES);
        assert_eq!(BGM_REGION_BYTES, 0x42000);
    }

    #[test]
    fn the_credits_bank_is_placed_across_the_sfx_region() {
        // The ending theme's bank (extraction 1056) carries 0x631B0 bytes of
        // bodies: over the BGM region, under all of SPU RAM.
        assert_eq!(
            owned_bank_placement(0x631B0),
            OwnedBankPlacement::AcrossSfxRegion
        );
        assert_eq!(
            owned_bank_placement(BGM_REGION_BYTES),
            OwnedBankPlacement::BgmRegion
        );
        assert_eq!(
            owned_bank_placement(SPU_RAM_BYTES),
            OwnedBankPlacement::TooLarge
        );
    }

    #[test]
    fn the_region_is_free_once_the_bank_ends_at_its_base() {
        assert!(sfx_region_free(None));
        assert!(sfx_region_free(Some(&bank_ending_at(SFX_REGION_BASE))));
        assert!(!sfx_region_free(Some(&bank_ending_at(
            SFX_REGION_BASE + 0x10
        ))));
    }

    /// [`SFX_REGION_BYTES`] is squeezed between two hard measurements, and
    /// this is both of them. Widening it silences BGM; narrowing it drops a
    /// resident SFX bank. Either failure is silent in play - a track that
    /// stops loading its instruments and a cue that keys a sibling sample both
    /// sound like "the audio is a bit off", which is why the numbers are
    /// asserted rather than left in a comment.
    ///
    /// The four constants are disc measurements from `vab list`: the two
    /// pinned banks' VAG-body totals (PROT 0868 / 0869) and the two largest
    /// VAB sample bodies in `PROT.DAT` that a BGM path can stage (269632 in
    /// `1071_music_01`, 268496 in `1113_vab_01`). Every VAG in all four is
    /// already a multiple of the allocator's 16-byte ADPCM block, so the
    /// packed footprint equals the raw total exactly.
    #[test]
    fn sfx_region_fits_both_pinned_banks_and_leaves_the_largest_bgm_room() {
        const SLOT0_BODY_BYTES: u32 = 59_136; // PROT 0868
        const SLOT2_BODY_BYTES: u32 = 188_128; // PROT 0869
        const LARGEST_STAGED_BGM_BODY_BYTES: u32 = 269_632; // 1071_music_01
        const SECOND_LARGEST_BGM_BODY_BYTES: u32 = 268_496; // 1113_vab_01
        let both = SLOT0_BODY_BYTES + SLOT2_BODY_BYTES;
        assert!(
            both <= SFX_REGION_BYTES,
            "both pinned banks must fit one region: {both} > {SFX_REGION_BYTES}"
        );
        for body in [LARGEST_STAGED_BGM_BODY_BYTES, SECOND_LARGEST_BGM_BODY_BYTES] {
            assert!(
                body <= BGM_REGION_BYTES,
                "a BGM VAB that fits today ({body}) must still fit: budget {BGM_REGION_BYTES}"
            );
        }
        // The two regions tile SPU RAM above the reserved floor.
        assert_eq!(SPU_RESERVED_BYTES + BGM_REGION_BYTES, SFX_REGION_BASE);
        assert_eq!(SFX_REGION_BASE + SFX_REGION_BYTES, SPU_RAM_BYTES);
    }
}
