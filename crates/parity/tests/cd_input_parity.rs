//! Disc + library-gated: the SPU's **CD input** - the path every CD-XA line
//! (battle voices, announcer lines, movie audio) takes into the mix - against
//! every retail mednafen state in the library.
//!
//! Three registers carry it, none of which the voice oracles read:
//!
//! - `SPUCNT` bit 0, CD audio enable. Set in every state: the port mixes XA
//!   unconditionally, and this pins that retail never turns the input off
//!   (it mutes it through the volume instead).
//! - `SPUCNT` bit 2, CD audio reverb. Raised only by the Baka Fighter and
//!   Muscle Dome overlays (`SpuSetCommonAttr` under mask `0x300`). Each state
//!   is mapped to the port's world by the game mode and the slot-A overlay
//!   whose bytes are resident, and `World::cd_input_reverb` must agree.
//! - The CD input volume (`0x1F801DB0`). `0` while no line is playing (the
//!   clip starter `FUN_8003D53C` mutes it through `FUN_8003EE7C(0)`),
//!   `127 * 0x102 = 0x7FFE` - one LSB under unity, which is the gain the
//!   port mixes every XA stream at - and `117 * 0x102 = 0x75EA` in a set of
//!   battle states whose writer is not located (no `SsSetSerialVol` or
//!   `SpuSetCommonAttr` call site on the disc passes `117`). The test pins
//!   that vocabulary, so a fourth value fails loudly.
//!
//! Skip-pass: `LEGAIA_DISC_BIN` unset, `extracted/` or the save library
//! missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_mednafen::{PsxSpu, SaveState};
use legaia_ram_image::ScenarioManifest;

fn first_dir(cands: &[&str], probe: &str) -> Option<PathBuf> {
    cands
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join(probe).exists())
}

/// The slot-A base both minigame overlays link at.
const SLOT_A_BASE: u32 = 0x801C_E818;

/// The overlays that write `SPUCNT` bit 2, each with the init routine whose
/// bytes identify it in RAM: the Baka Fighter init `FUN_801CF00C` (PROT
/// 0976) and the Muscle Dome contest init / re-entry `FUN_801CEA6C` (PROT
/// 0977).
const CD_REVERB_OVERLAYS: [(&str, u32, SceneMode); 2] = [
    (
        "overlay_baka_fighter_0976.bin",
        0x801C_F00C,
        SceneMode::BakaFighter,
    ),
    (
        "overlay_arena_init_0977.bin",
        0x801C_EA6C,
        SceneMode::MuscleDome,
    ),
];

/// Bytes compared at each init routine.
const SIGNATURE_LEN: usize = 0x100;

/// The CD input volumes the corpus holds (see the module docs).
const CD_VOLUME_MUTED: i16 = 0;
const CD_VOLUME_FULL: i16 = 127 * 0x102;
const CD_VOLUME_BATTLE_117: i16 = 117 * 0x102;

#[test]
fn retail_cd_input_matches_the_port() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = first_dir(
        &["extracted", "../extracted", "../../extracted"],
        "overlays",
    ) else {
        eprintln!("[skip] extracted/overlays missing");
        return;
    };
    let Some(root) = first_dir(&[".", "..", "../.."], "scripts/scenarios.toml") else {
        eprintln!("[skip] scripts/scenarios.toml missing");
        return;
    };
    let lib = root.join("saves/library");
    if !lib.join("mednafen").is_dir() {
        eprintln!("[skip] saves/library/mednafen missing");
        return;
    }
    let mut signatures = Vec::new();
    for (file, init_va, mode) in CD_REVERB_OVERLAYS {
        let Ok(image) = std::fs::read(extracted.join("overlays").join(file)) else {
            eprintln!("[skip] {file} missing");
            return;
        };
        let off = (init_va - SLOT_A_BASE) as usize;
        signatures.push((init_va, image[off..off + SIGNATURE_LEN].to_vec(), mode));
    }
    let manifest = ScenarioManifest::from_path(root.join("scripts/scenarios.toml")).unwrap();

    let mut states = 0usize;
    let mut reverb_states = Vec::new();
    let mut volumes = BTreeMap::<i16, usize>::new();
    let mut misses = Vec::new();
    for scn in &manifest.scenarios {
        let Some(path) = manifest.library_save_path(scn, &lib).filter(|p| p.exists()) else {
            continue;
        };
        let Ok(state) = SaveState::from_path(&path) else {
            continue;
        };
        let Ok(ram) = state.main_ram() else {
            continue;
        };
        let spu = PsxSpu::new(&state);
        let (Some(cd_audio), Some(cd_reverb), Some((vol_l, vol_r))) = (
            spu.cd_audio_enabled(),
            spu.cd_reverb_enabled(),
            spu.cd_volume(),
        ) else {
            continue;
        };
        states += 1;
        let at = |va: u32| (va & 0x1F_FFFF) as usize;
        let game_mode = ram[at(0x8007_B83C)];
        let resident = signatures
            .iter()
            .find(|(va, sig, _)| &ram[at(*va)..at(*va) + SIGNATURE_LEN] == sig.as_slice())
            .map(|(_, _, mode)| *mode);
        // The mode-24 door warp hands the frame to the overlay as mode `0x19`;
        // a dome leg is fought in the battle mode with the arena overlay
        // paged out.
        let port_mode = match resident {
            Some(m) if matches!(game_mode, 0x18 | 0x19) => m,
            _ => SceneMode::Field,
        };
        let mut world = World::new();
        world.mode = port_mode;
        if !cd_audio {
            misses.push(format!("{}: SPUCNT clears CD audio enable", scn.label));
        }
        if world.cd_input_reverb() != cd_reverb {
            misses.push(format!(
                "{}: retail CD reverb {cd_reverb}, port {} (game mode {game_mode:#04x}, \
                 overlay {resident:?})",
                scn.label,
                world.cd_input_reverb()
            ));
        }
        if cd_reverb {
            reverb_states.push(scn.label.clone());
        }
        if vol_l != vol_r {
            misses.push(format!(
                "{}: CD volume is panned ({vol_l:#06x}, {vol_r:#06x})",
                scn.label
            ));
        }
        if ![CD_VOLUME_MUTED, CD_VOLUME_FULL, CD_VOLUME_BATTLE_117].contains(&vol_l) {
            misses.push(format!(
                "{}: CD volume {vol_l:#06x} is outside the known vocabulary",
                scn.label
            ));
        }
        *volumes.entry(vol_l).or_default() += 1;
    }
    let volumes: Vec<String> = volumes
        .iter()
        .map(|(v, n)| format!("{v:#06x} x{n}"))
        .collect();
    eprintln!(
        "[ran] {states} states; CD reverb in {reverb_states:?}; CD volumes {volumes:?}; \
         {} unmatched",
        misses.len()
    );
    assert!(misses.is_empty(), "{misses:#?}");
    if states > 0 {
        assert!(
            !reverb_states.is_empty(),
            "the library holds the duel and the dome hub; neither read as CD reverb"
        );
    }
}
