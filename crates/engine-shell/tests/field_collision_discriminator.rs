//! Disc-gated discriminator for the field-collision sub-cell indexing.
//!
//! Retail's static-wall probe (`FUN_801cfe4c`, field overlay 0897) derives
//! each walkability-grid sub-cell as `zc = (z>>6)+2`, `xc = ((x+0x3f)>>6)-1`;
//! the engine's `World::field_tile_is_wall` floor-indexes (`sx = x>>6`,
//! `sz = z>>6`). The two read DIFFERENT cells for the same world point
//! (retail one tile further in Z), but every parked library position read
//! walkable under BOTH derivations, so the offset was never proven to be a
//! player-visible misalignment.
//!
//! The discriminating capture is a save with the player WALKING INTO A WALL
//! (scenario `rimelm_wall_press_left`: Rim Elm, holding X- against a wall).
//! At that position the blocking wall byte MUST sit in the cell the retail
//! derivation reads for at least one of the three leading-edge probes
//! (`(-47, -16) (-47, 0) (-47, +16)`, applied as `x+dx`, `z-dz` — see
//! `docs/subsystems/field-locomotion.md`).
//!
//! What this capture settles (player at `(1838, 4528)` against the wall
//! column at grid col 13):
//!  - The decoded retail derivation + the 47-unit leading edge are validated
//!    against a live blocked position at step granularity: the probe at
//!    `x-47 = 1791` reads the LAST wall sub-cell (xc 27, col 13's east
//!    half) and one 2-unit step shallower (probe 1793) reads clear, so the
//!    resting x is the maximal steppable blocked position.
//!  - The X derivations agree except at exact 64-multiples (retail's
//!    `ceil-1` vs the engine's floor): at probe 1792 retail still reads the
//!    wall sub-cell while the floor reads the next clear column — but the
//!    odd resting parity that would expose the difference is unreachable
//!    under 2-unit stepping, so both models stop the player at the same x.
//!  - The Z `+1 row` bias is NOT discriminated by an X- press against a
//!    full-height wall column: both derivations read the same all-quads wall
//!    byte one row apart, and the floor tiers match too. The remaining
//!    discriminator is a `Z+` (down) wall press: there retail probes the
//!    wall's NEAR edge row where the engine floor-read lands one row short
//!    on a clear cell, so even a thick wall separates the two.
//!
//! Ground truth is the RETAIL LIVE grid: the field buffer's `+0x4000` region
//! lifted out of the save state's main RAM (located by searching RAM for a
//! high-entropy window of the disc-loaded base grid). The engine's grid
//! (`enter_field_live` + a few prescript ticks) is asserted byte-identical
//! to it (base `.MAP` copy + prescript paints both match retail).
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN`, `extracted/`, the scenario
//! manifest, or the library save is missing - CI runs without disc data.

use std::path::PathBuf;

use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_mednafen::{SaveState, ScenarioManifest};

const SCENARIO: &str = "rimelm_wall_press_left";
const PLAYER_PTR_ADDR: u32 = 0x8007C364;
const GRID_BYTES: usize = 0x4000;
const GRID_STRIDE: i32 = 0x80;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn manifest_path() -> Option<PathBuf> {
    for c in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn ram_u32(ram: &[u8], va: u32) -> u32 {
    let o = (va & 0x1F_FFFF) as usize;
    u32::from_le_bytes(ram[o..o + 4].try_into().unwrap())
}

fn ram_i16(ram: &[u8], va: u32) -> i16 {
    let o = (va & 0x1F_FFFF) as usize;
    i16::from_le_bytes(ram[o..o + 2].try_into().unwrap())
}

/// Retail's `FUN_801cfe4c` sub-cell derivation: world point ->
/// `(col, row, quad-mask)`.
fn retail_subcell(ix: i32, iz: i32) -> (i32, i32, u8) {
    let iz2 = if iz < 0 { iz + 0x3f } else { iz };
    let zc = (iz2 >> 6) + 2;
    let xc = ((ix + 0x3f) >> 6) - 1;
    let col = (xc / 2) & 0x7f;
    let row = (zc - (zc >> 31)) >> 1;
    let mask = 1u8 << (((zc & 1) << 1 | (xc & 1)) as u32);
    (col, row, mask)
}

/// The engine's `World::field_tile_is_wall` floor derivation.
fn engine_subcell(ix: i32, iz: i32) -> (i32, i32, u8) {
    let sx = ix >> 6;
    let sz = iz >> 6;
    let col = (sx >> 1) & 0x7f;
    let row = sz >> 1;
    let mask = 1u8 << (((sz & 1) << 1 | (sx & 1)) as u32);
    (col, row, mask)
}

fn grid_wall_bits(grid: &[u8], col: i32, row: i32) -> u8 {
    let idx = (col + row * GRID_STRIDE) as usize;
    grid.get(idx).map(|b| b >> 4).unwrap_or(0)
}

/// Locate the live field buffer's collision grid inside the save state's
/// main RAM by searching for a high-entropy window of the disc-loaded base
/// grid. Returns the grid's RAM byte offset.
fn find_live_grid(ram: &[u8], base_grid: &[u8]) -> Option<usize> {
    // Pick the densest 64-byte window of the base grid (most non-zero
    // bytes) as the search needle - zero runs match everywhere, and town
    // grids are sparse (town01: ~2100 non-zero of 0x4000), so the bar has
    // to be modest.
    let (mut best_off, mut best_nz) = (0usize, 0usize);
    for off in (0..base_grid.len().saturating_sub(64)).step_by(16) {
        let nz = base_grid[off..off + 64].iter().filter(|b| **b != 0).count();
        if nz > best_nz {
            best_nz = nz;
            best_off = off;
        }
    }
    if best_nz < 24 {
        return None;
    }
    let needle = &base_grid[best_off..best_off + 64];
    let mut hits = Vec::new();
    let mut at = 0usize;
    while let Some(p) = find_sub(&ram[at..], needle) {
        hits.push(at + p);
        at += p + 1;
        if hits.len() > 8 {
            break;
        }
    }
    // The needle may legitimately appear twice (live buffer + a staging
    // copy); take the hit whose surrounding 0x4000 window best matches the
    // base grid as the live one.
    hits.iter()
        .filter(|&&h| h >= best_off && h - best_off + GRID_BYTES <= ram.len())
        .max_by_key(|&&h| {
            let start = h - best_off;
            ram[start..start + GRID_BYTES]
                .iter()
                .zip(base_grid)
                .filter(|(a, b)| a == b)
                .count()
        })
        .map(|&h| h - best_off)
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn wall_press_capture_discriminates_subcell_indexing() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/scenarios.toml not found");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let Some(scn) = manifest.scenarios.iter().find(|s| s.label == SCENARIO) else {
        eprintln!("[skip] scenario '{SCENARIO}' not in manifest");
        return;
    };
    let save_path = match manifest.mednafen_save_path(scn, library_dir().as_deref()) {
        Ok(p) if p.exists() => p,
        _ => {
            eprintln!("[skip] no save on disk for scenario '{SCENARIO}'");
            return;
        }
    };

    // --- retail side: player position + live grid out of the capture ---
    let state = SaveState::from_path(&save_path).expect("parse mednafen save state");
    let ram = state.main_ram().expect("save state has main RAM");
    let player = ram_u32(ram, PLAYER_PTR_ADDR);
    assert_eq!(player & 0xFF00_0000, 0x8000_0000, "player struct pointer");
    let px = ram_i16(ram, player + 0x14) as i32;
    let pz = ram_i16(ram, player + 0x18) as i32;
    eprintln!("[capture] player at ({px}, {pz}), pressing X-");

    // --- engine side: boot the same scene, let the prescript paint ---
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("boot session");
    session
        .enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01 live");
    let base_grid = session.host.world.field_collision_grid.clone();
    assert_eq!(base_grid.len(), GRID_BYTES, "base grid loaded");
    for _ in 0..10 {
        session.host.world.set_pad(0);
        let _ = session.host.world.tick();
    }
    let engine_grid = session.host.world.field_collision_grid.clone();

    let live_off = find_live_grid(ram, &base_grid).expect("live field grid located in RAM");
    let live_grid = &ram[live_off..live_off + GRID_BYTES];
    let engine_diffs = live_grid
        .iter()
        .zip(&engine_grid)
        .filter(|(a, b)| a != b)
        .count();
    eprintln!(
        "[capture] live grid at RAM 0x{:08X}; {} bytes differ from disc base, {} from engine",
        0x8000_0000u32 + live_off as u32,
        live_grid
            .iter()
            .zip(&base_grid)
            .filter(|(a, b)| a != b)
            .count(),
        engine_diffs,
    );
    assert_eq!(
        engine_diffs, 0,
        "engine grid (base .MAP copy + prescript paints) matches the retail live grid"
    );

    // --- the discriminator: standing point + three X- leading-edge probes ---
    let probes = [
        ("stand", px, pz),
        ("edge z+16", px - 47, pz + 16),
        ("edge z+0", px - 47, pz),
        ("edge z-16", px - 47, pz - 16),
    ];
    let mut retail_blocked = false;
    let mut engine_blocked = false;
    for (name, x, z) in probes {
        let (rc, rr, rm) = retail_subcell(x, z);
        let (ec, er, em) = engine_subcell(x, z);
        let rbits = grid_wall_bits(live_grid, rc, rr);
        let ebits = grid_wall_bits(live_grid, ec, er);
        let rhit = rbits & rm != 0;
        let ehit = ebits & em != 0;
        eprintln!(
            "[probe {name}] ({x},{z}) retail ({rc},{rr}) m{rm:04b} bits {rbits:04b} {} | \
             engine ({ec},{er}) m{em:04b} bits {ebits:04b} {}",
            if rhit { "WALL" } else { "clear" },
            if ehit { "WALL" } else { "clear" },
        );
        if name != "stand" {
            retail_blocked |= rhit;
            engine_blocked |= ehit;
        }
    }

    // The player is captured pressed against the wall, so the retail
    // derivation MUST read a wall at one of its leading-edge probes - this
    // is the assertion that validates the decoded derivation against a live
    // blocked position (not just decompiled arithmetic).
    assert!(
        retail_blocked,
        "retail derivation must read the blocking wall byte at a leading-edge probe"
    );
    // Standoff exactness at step granularity: locomotion advances in 2-unit
    // steps, so the resting position must be the LAST steppable x whose
    // probe reads wall - one step shallower the retail probe must be clear.
    let (sc, sr, sm) = retail_subcell(px - 47 + 2, pz);
    assert_eq!(
        grid_wall_bits(live_grid, sc, sr) & sm,
        0,
        "one 2-unit step shallower the retail probe reads clear (the standoff is step-exact)"
    );
    // The exact-64-multiple probe (here x-46 = 1792) is the one X position
    // where retail's ceil-1 and the engine's floor genuinely diverge: retail
    // still reads the wall's east sub-cell, the engine floor reads the next
    // (clear) column. Step parity makes the odd resting position that would
    // expose it unreachable, so both models stop the player at the same x.
    let (bc, br, bm) = retail_subcell(px - 46, pz);
    let (fc, fr, fm) = engine_subcell(px - 46, pz);
    eprintln!(
        "[boundary] probe x-46={} retail ({bc},{br}) {} | engine ({fc},{fr}) {}",
        px - 46,
        if grid_wall_bits(live_grid, bc, br) & bm != 0 {
            "WALL"
        } else {
            "clear"
        },
        if grid_wall_bits(live_grid, fc, fr) & fm != 0 {
            "WALL"
        } else {
            "clear"
        },
    );
    // The X- press against a full-height wall column blocks BOTH
    // derivations (they only diverge by a Z row, and the column is wall at
    // both rows); the still-open Z-bias discriminator is a Z+ wall press.
    eprintln!(
        "[verdict] retail blocked={retail_blocked} engine blocked={engine_blocked} \
         (X derivation + 47-unit standoff validated; Z bias needs a Z+ press)"
    );
    assert!(
        engine_blocked,
        "the engine derivation also reads this wall column (X press is non-discriminating in Z)"
    );
}
