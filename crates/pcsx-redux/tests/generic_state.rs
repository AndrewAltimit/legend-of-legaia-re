//! Not disc-gated: the reader on a synthetic, non-Legaia save state built in
//! the test ([`legaia_pcsx_redux::fixture`]). No game bytes are involved, so
//! this runs in CI and is the proof that main RAM, the scratchpad and the GPU
//! snapshot are located from the emulator's layout alone.

use legaia_pcsx_redux::fixture::SyntheticState;
use legaia_pcsx_redux::gpu::PcsxGpu;
use legaia_pcsx_redux::{RamSource, SaveState, structural_ram_offset};
use legaia_ram_image::{GameProfile, GenericGame, PSX_RAM_SIZE};

/// A RAM image no Legaia anchor string occurs in: a made-up "game" whose
/// executable text starts with its own banner.
fn other_game_ram() -> Vec<u8> {
    let mut ram = vec![0u8; PSX_RAM_SIZE];
    ram[0x1_0000..0x1_0010].copy_from_slice(b"OTHER GAME v1.00");
    ram[0x0F_0000..0x0F_0004].copy_from_slice(&0xCAFE_F00Du32.to_le_bytes());
    // The last word of the console's 2 MiB: proves the slice is not short.
    ram[PSX_RAM_SIZE - 4..].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    ram
}

fn state(ram: &[u8]) -> SyntheticState<'_> {
    let mut gp1 = [0u32; 256];
    gp1[5] = 16 | (8 << 10); // display start (16, 8)
    gp1[8] = 0x01; // 320 x 240
    SyntheticState {
        ram,
        hardware: &[0x5A; 0x400],
        vram: &[0x11; 64],
        gp1,
    }
}

#[test]
fn a_non_legaia_state_opens_without_any_executable() {
    let ram = other_game_ram();
    for bytes in [state(&ram).to_payload(), state(&ram).to_gzip()] {
        // `ram_source` proves the pass did not come through the Legaia
        // anchor fallback.
        let st = SaveState::from_sstate_bytes(&bytes).expect("structural RAM location");
        assert_eq!(st.ram_source(), RamSource::Structural);
        assert_eq!(st.main_ram().len(), PSX_RAM_SIZE);
        assert_eq!(&st.main_ram()[0x1_0000..0x1_0010], b"OTHER GAME v1.00");
        assert_eq!(st.u32_at(0x800F_0000), 0xCAFE_F00D);
        assert_eq!(st.u32_at(0x801F_FFFC), 0x1234_5678);
        assert_eq!(st.scratchpad_u8_at(0x1F80_03FF), Some(0x5A));

        let st2 = SaveState::from_sstate_bytes_structural(&bytes).unwrap();
        assert_eq!(st2.main_ram(), st.main_ram());
        // A profile with no identity globals reports none, rather than
        // reading Legaia's addresses out of another game's RAM.
        assert_eq!(st.identity_with(&GenericGame), None);
    }
}

#[test]
fn the_gpu_snapshot_reads_from_the_same_synthetic_state() {
    let ram = other_game_ram();
    let gpu = PcsxGpu::from_sstate_bytes(&state(&ram).to_gzip()).unwrap();
    assert_eq!(gpu.display_crop_rect(), (16, 8, 320, 240));
    assert_eq!(gpu.vram[0], 0x11);
    assert_eq!(gpu.vram[64], 0);
}

#[test]
fn the_ram_member_is_the_first_large_one_not_the_parallel_port() {
    let ram = other_game_ram();
    let payload = state(&ram).to_payload();
    let at = structural_ram_offset(&payload).unwrap();
    assert_eq!(&payload[at + 0x1_0000..at + 0x1_0010], b"OTHER GAME v1.00");
}

#[test]
fn a_payload_with_no_memory_submessage_is_an_error_not_a_guess() {
    let junk = vec![0xFFu8; 4096];
    assert!(SaveState::from_sstate_bytes_structural(&junk).is_err());
    // The anchor fallback with a game's own executable and strings.
    struct Other;
    impl GameProfile for Other {
        fn name(&self) -> &str {
            "other"
        }
        fn exe_anchor_strings(&self) -> &[&'static [u8]] {
            &[b"OTHER GAME v1.00"]
        }
    }
    // A payload the walk cannot parse, holding RAM at an odd offset.
    let ram = other_game_ram();
    let mut payload = vec![0xFFu8; 0x33];
    payload.extend_from_slice(&ram);
    let mut exe = vec![0u8; 0x800 + 0x100];
    exe[..8].copy_from_slice(b"PS-X EXE");
    exe[0x18..0x1C].copy_from_slice(&0x8001_0000u32.to_le_bytes());
    exe[0x800..0x810].copy_from_slice(b"OTHER GAME v1.00");
    let st = SaveState::from_sstate_bytes_with_anchors(&payload, &exe, &Other).unwrap();
    assert_eq!(st.ram_source(), RamSource::Anchor);
    assert_eq!(st.main_ram(), &ram[..]);
    // With no anchors to try, the fallback refuses.
    assert!(SaveState::from_sstate_bytes_with_anchors(&payload, &exe, &GenericGame).is_err());
}
