//! Disc-gated wall-press oracles for the field-collision sub-cell indexing.
//!
//! Retail's static-wall probe (`FUN_801cfe4c`, field overlay 0897) derives
//! each walkability-grid sub-cell as `zc = (z>>6)+2`, `xc = ((x+0x3f)>>6)-1`.
//! `World::field_tile_is_wall` uses the same derivation — these tests pin it
//! against live blocked positions from two cheat-free wall-press captures:
//!
//!  - `rimelm_wall_press_left` (screen-left = world `X-`): the player rests
//!    at the maximal 2-unit-step position whose leading-edge probe
//!    (47 units ahead) reads the last wall sub-cell; one step shallower
//!    reads clear.
//!  - `rimelm_wall_press_down` (screen-down = world `Z-`, toward the
//!    camera): the Z-row discriminator that PROVED the `+2` bias is
//!    authored into the wall bits, not an optional look-ahead. The live
//!    player legally stands at a position whose plain floor-indexed cell is
//!    an all-quads wall byte — under floor indexing the position would be
//!    unreachable — while the biased read places that wall band one tile
//!    north, exactly where the press blocks with a step-exact 47-unit
//!    standoff. (The floor sampler `FUN_80019278` reads the same bytes with
//!    plain floor indexing: one byte's two nibbles are addressed under two
//!    different world-to-cell mappings by their two retail consumers.)
//!
//! Leading-edge probe layout per `docs/subsystems/field-locomotion.md`
//! (`DAT_801f2214`, applied as `x+dx`, `z-dz`; disc-pinned from the field
//! overlay file at `0x239FC`): dir 0 `Z-` probes `(x±16, z-48)`; dir 1 `X-`
//! probes `(x-47, z±16)` — the crossing distance is 48 in the positive
//! directions and 47 in the negative ones under the biased cell mapping.
//!
//! Ground truth is the RETAIL LIVE grid: the field buffer pointer is read
//! from scratchpad `_DAT_1f8003ec` (`SaveState::scratch_ram`), and the
//! `+0x4000` walkability region is lifted out of main RAM. The capture's
//! scene is read from scene-bundle pool slot 0. NOTE: both captures park in
//! the `town0c` Rim Elm variant, whose own `.MAP` (PROT 0019, the universal
//! `define-2` resolution) is byte-identical to town01's - the Rim Elm
//! variants share one map. The engine-side cross-check still tries both
//! scene candidates and takes the best match.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN`, `extracted/`, the scenario
//! manifest, or the library save is missing - CI runs without disc data.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::World;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_mednafen::{SaveState, ScenarioManifest};

const PLAYER_PTR_ADDR: u32 = 0x8007C364;
const FIELD_BUFFER_PTR_SCRATCH_OFF: usize = 0x3EC;
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

/// Retail's `FUN_801cfe4c` sub-cell derivation (reference copy; the engine
/// implementation is unit-tested equal in `world.rs::tests`).
fn retail_subcell(ix: i32, iz: i32) -> (i32, i32, u8) {
    let iz2 = if iz < 0 { iz + 0x3f } else { iz };
    let zc = (iz2 >> 6) + 2;
    let xc = ((ix + 0x3f) >> 6) - 1;
    let col = (xc / 2) & 0x7f;
    let row = (zc - (zc >> 31)) >> 1;
    let mask = 1u8 << (((zc & 1) << 1 | (xc & 1)) as u32);
    (col, row, mask)
}

/// The FALSIFIED plain floor derivation (the engine's pre-realignment
/// model), kept as the negative reference the down-press capture refutes.
fn floor_subcell(ix: i32, iz: i32) -> (i32, i32, u8) {
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

/// One loaded wall-press capture: player position, the retail live grid,
/// and a `World` carrying that grid so the REAL engine sampler is what gets
/// asserted.
struct WallPress {
    px: i32,
    pz: i32,
    live_grid: Vec<u8>,
    world: World,
    /// A fresh full `BootSession` entered on the grid-matched scene - the
    /// engine's own scene context (resolver-loaded `.MAP` + prescript
    /// paints), for the full-stack press legs.
    session: BootSession,
}

/// Resolve + load a wall-press scenario; `None` = skip (missing disc /
/// manifest / save), with the reason printed.
fn load_wall_press(label: &str) -> Option<WallPress> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing");
        None
    })?;
    let manifest_path = manifest_path().or_else(|| {
        eprintln!("[skip] scripts/scenarios.toml not found");
        None
    })?;
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
        eprintln!("[skip] scenario '{label}' not in manifest");
        return None;
    };
    let save_path = match manifest.mednafen_save_path(scn, library_dir().as_deref()) {
        Ok(p) if p.exists() => p,
        _ => {
            eprintln!("[skip] no save on disk for scenario '{label}'");
            return None;
        }
    };

    // --- retail side: scene + player position + live grid ---
    let state = SaveState::from_path(&save_path).expect("parse mednafen save state");
    let ram = state.main_ram().expect("save state has main RAM");
    let scratch = state.scratch_ram().expect("save state has scratchpad RAM");
    let scene =
        legaia_engine_core::capture_observations::field_pack_intra_transition::read_pool_slot_name(
            ram, 0,
        )
        .expect("scene name in pool slot 0");
    let player = ram_u32(ram, PLAYER_PTR_ADDR);
    assert_eq!(player & 0xFF00_0000, 0x8000_0000, "player struct pointer");
    let px = ram_i16(ram, player + 0x14) as i32;
    let pz = ram_i16(ram, player + 0x18) as i32;

    let field_buf = u32::from_le_bytes(
        scratch[FIELD_BUFFER_PTR_SCRATCH_OFF..FIELD_BUFFER_PTR_SCRATCH_OFF + 4]
            .try_into()
            .unwrap(),
    );
    assert_eq!(field_buf & 0xFF00_0000, 0x8000_0000, "field buffer pointer");
    let grid_off = ((field_buf & 0x1F_FFFF) as usize) + 0x4000;
    let live_grid = ram[grid_off..grid_off + GRID_BYTES].to_vec();
    eprintln!(
        "[{label}] scene {scene}, player at ({px}, {pz}), live grid at 0x{:08X}",
        field_buf + 0x4000
    );

    // --- engine side: cross-check the loaded grid against scene entry.
    // The town0c variant session keeps town01's field buffer, so try the
    // pool-slot scene first and fall back to town01. The session's
    // story-flag state differs from a cold boot, so a handful of
    // story-conditional 0x4C wall paints may legitimately diverge - require
    // a near-identical match, not byte equality.
    const PRESCRIPT_DELTA_TOLERANCE: usize = 32;
    let cfg = BootConfig {
        scene: scene.clone(),
        enable_audio: false,
    };
    let mut best: Option<(String, usize)> = None;
    for candidate in [scene.as_str(), "town01"] {
        let mut session = BootSession::open(&extracted, &cfg).expect("boot session");
        if session
            .enter_field_live(candidate, &FieldLiveOpts::default())
            .is_err()
        {
            continue;
        }
        for _ in 0..10 {
            session.host.world.set_pad(0);
            let _ = session.host.world.tick();
        }
        let engine_grid = &session.host.world.field_collision_grid;
        let diffs = live_grid
            .iter()
            .zip(engine_grid)
            .filter(|(a, b)| a != b)
            .count();
        eprintln!("[{label}] engine '{candidate}' grid: {diffs} bytes differ from live");
        if best.as_ref().is_none_or(|(_, d)| diffs < *d) {
            best = Some((candidate.to_string(), diffs));
        }
        if diffs == 0 {
            break;
        }
    }
    let (matched_scene, matched_diffs) = best.expect("at least one candidate scene enters");
    eprintln!("[{label}] grid source: '{matched_scene}' ({matched_diffs} story-delta bytes)");
    assert!(
        matched_diffs <= PRESCRIPT_DELTA_TOLERANCE,
        "a candidate scene's engine grid (base .MAP + prescript paints) matches the \
         retail live grid up to story-conditional deltas (got {matched_diffs} diffs)"
    );

    // A fresh full session on the matched scene for the full-stack legs.
    let mut session = BootSession::open(&extracted, &cfg).expect("boot session");
    session
        .enter_field_live(&matched_scene, &FieldLiveOpts::default())
        .expect("re-enter matched scene");
    for _ in 0..10 {
        session.host.world.set_pad(0);
        let _ = session.host.world.tick();
    }

    // A World carrying the LIVE grid - the engine sampler under test.
    let mut world = World::new();
    world.load_field_collision_grid(&live_grid);
    Some(WallPress {
        px,
        pz,
        live_grid,
        world,
        session,
    })
}

/// Probe the live grid via the retail reference derivation AND the real
/// engine sampler at a set of points; assert they agree; return whether any
/// non-"stand" point reads a wall.
fn run_probes(wp: &WallPress, probes: &[(&str, i32, i32)]) -> bool {
    let mut blocked = false;
    for &(name, x, z) in probes {
        let (rc, rr, rm) = retail_subcell(x, z);
        let rbits = grid_wall_bits(&wp.live_grid, rc, rr);
        let rhit = rbits & rm != 0;
        let ehit = wp.world.field_tile_is_wall(x as i16, z as i16);
        eprintln!(
            "[probe {name}] ({x},{z}) retail ({rc},{rr}) m{rm:04b} bits {rbits:04b} {} | \
             engine {}",
            if rhit { "WALL" } else { "clear" },
            if ehit { "WALL" } else { "clear" },
        );
        assert_eq!(
            rhit, ehit,
            "engine sampler agrees with the retail derivation at ({x},{z})"
        );
        if name != "stand" {
            blocked |= rhit;
        }
    }
    blocked
}

#[test]
fn wall_press_left_validates_probe_model() {
    let Some(wp) = load_wall_press("rimelm_wall_press_left") else {
        return;
    };
    let (px, pz) = (wp.px, wp.pz);
    // dir 1 (X-): probes (x-47, z+16) (x-47, z) (x-47, z-16).
    let probes = [
        ("stand", px, pz),
        ("edge z+16", px - 47, pz + 16),
        ("edge z+0", px - 47, pz),
        ("edge z-16", px - 47, pz - 16),
    ];
    let blocked = run_probes(&wp, &probes);

    // The player is captured pressed against the wall: the derivation MUST
    // read a wall at a leading-edge probe and MUST read the standing tile
    // clear - a live blocked position, not just decompiled arithmetic.
    assert!(blocked, "a leading-edge probe reads the blocking wall byte");
    assert!(
        !wp.world.field_tile_is_wall(px as i16, pz as i16),
        "the standing position reads clear"
    );
    // Standoff exactness at step granularity: locomotion advances in 2-unit
    // steps, so one step shallower all three probes must read clear.
    for (x, z) in [(px - 45, pz + 16), (px - 45, pz), (px - 45, pz - 16)] {
        assert!(
            !wp.world.field_tile_is_wall(x as i16, z as i16),
            "one 2-unit step shallower the probe at ({x},{z}) reads clear \
             (the standoff is step-exact)"
        );
    }
}

#[test]
fn wall_press_down_discriminates_z_row_bias() {
    let Some(wp) = load_wall_press("rimelm_wall_press_down") else {
        return;
    };
    let (px, pz) = (wp.px, wp.pz);
    // Screen-down walks TOWARD the camera = world Z- (dir 0): probes
    // (x-16, z-48) (x, z-48) (x+16, z-48).
    let probes = [
        ("stand", px, pz),
        ("edge x-16", px - 16, pz - 48),
        ("edge x+0", px, pz - 48),
        ("edge x+16", px + 16, pz - 48),
    ];
    let blocked = run_probes(&wp, &probes);
    assert!(blocked, "a leading-edge probe reads the blocking wall byte");
    assert!(
        !wp.world.field_tile_is_wall(px as i16, pz as i16),
        "the standing position reads clear under the biased derivation"
    );
    // Standoff: one 2-unit step shallower, all probes clear.
    for (x, z) in [(px - 16, pz - 46), (px, pz - 46), (px + 16, pz - 46)] {
        assert!(
            !wp.world.field_tile_is_wall(x as i16, z as i16),
            "one 2-unit step shallower the probe at ({x},{z}) reads clear \
             (the standoff is step-exact)"
        );
    }

    // THE DISCRIMINATOR: under the falsified plain floor derivation the
    // player's own standing cell is a wall byte - the live position would
    // be unreachable. This is what proves the +2 Z bias is authored into
    // the wall bits (and why field_tile_is_wall must use it).
    let (fc, fr, fm) = floor_subcell(px, pz);
    assert_ne!(
        grid_wall_bits(&wp.live_grid, fc, fr) & fm,
        0,
        "the floor-indexed standing cell ({fc},{fr}) is a wall byte - \
         plain floor indexing is refuted by this capture"
    );
}

/// Drive the REAL engine stepper (`World::advance_with_collision`) over the
/// capture's live grid from a shallow start and return where it rests after
/// pressing `dir_bits` for `frames` frames at the retail per-frame speed.
fn engine_press_rest(
    wp: &WallPress,
    edge_probes: bool,
    start: (i16, i16),
    dir_bits: u16,
) -> (i16, i16) {
    let mut world = World::new();
    world.install_field_player(0);
    world.load_field_collision_grid(&wp.live_grid);
    world.leading_edge_wall_probes = edge_probes;
    world.actors[0].move_state.world_x = start.0;
    world.actors[0].move_state.world_z = start.1;
    for _ in 0..100 {
        world.advance_with_collision(0, dir_bits, 8);
    }
    let ms = &world.actors[0].move_state;
    (ms.world_x, ms.world_z)
}

/// With `leading_edge_wall_probes` set, the ENGINE pad stepper reproduces
/// the captured retail rest position byte-exactly on the live grid - the
/// wired three-probe footprint, not just the sampler geometry. The
/// candidate-centre default demonstrably walks deeper (the standoff this
/// flag exists to close).
#[test]
fn wall_press_left_engine_rest_matches_retail() {
    let Some(wp) = load_wall_press("rimelm_wall_press_left") else {
        return;
    };
    let (px, pz) = (wp.px as i16, wp.pz as i16);
    // X- press (dir bits 0x8000) from 62 units shallower along the captured
    // approach line (even offset keeps the live 2-unit step parity).
    let rest = engine_press_rest(&wp, true, (px + 62, pz), 0x8000);
    assert_eq!(
        rest,
        (px, pz),
        "engine leading-edge press rests exactly at the captured retail position"
    );
    let centre = engine_press_rest(&wp, false, (px + 62, pz), 0x8000);
    assert!(
        centre.0 < px,
        "the candidate-centre default walks deeper than retail ({} >= {px})",
        centre.0
    );
}

#[test]
fn wall_press_down_engine_rest_matches_retail() {
    let Some(wp) = load_wall_press("rimelm_wall_press_down") else {
        return;
    };
    let (px, pz) = (wp.px as i16, wp.pz as i16);
    // Z- press (dir bits 0x4000).
    let rest = engine_press_rest(&wp, true, (px, pz + 62), 0x4000);
    assert_eq!(
        rest,
        (px, pz),
        "engine leading-edge press rests exactly at the captured retail position"
    );
    let centre = engine_press_rest(&wp, false, (px, pz + 62), 0x4000);
    assert!(
        centre.1 < pz,
        "the candidate-centre default walks deeper than retail ({} >= {pz})",
        centre.1
    );
}

/// Press a d-pad direction inside the FULL scene session (resolver-loaded
/// `.MAP`, prescript paints, real pad -> camera-remapped locomotion via
/// `World::tick`) and return where the player rests.
fn full_scene_press_rest(
    session: &mut BootSession,
    edge_probes: bool,
    start: (i16, i16),
    pad_mask: u16,
    frames: usize,
) -> (i16, i16) {
    let world = &mut session.host.world;
    world.leading_edge_wall_probes = edge_probes;
    let slot = world.player_actor_slot.expect("player actor installed") as usize;
    world.actors[slot].move_state.world_x = start.0;
    world.actors[slot].move_state.world_z = start.1;
    world.set_pad(pad_mask);
    for _ in 0..frames {
        let _ = world.tick();
    }
    world.set_pad(0);
    let ms = &session.host.world.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

/// Full-stack standoff: with the flag on, a held d-pad press inside a REAL
/// scene entry (`BootSession::enter_field_live` - the resolver-loaded `.MAP`
/// grid plus the engine-executed prescript paints, walked through the pad ->
/// camera-remap -> `step_field_locomotion` path) rests at the captured retail
/// wall standoff byte-exactly. The unit-level legs above isolate the stepper
/// on the raw captured grid; this leg proves the whole scene context
/// reproduces it.
#[test]
fn wall_press_left_full_scene_rest_matches_retail() {
    let Some(mut wp) = load_wall_press("rimelm_wall_press_left") else {
        return;
    };
    let (px, pz) = (wp.px as i16, wp.pz as i16);
    // A cold field camera has azimuth 0 (quadrant 0: screen-left = world X-).
    let rest = full_scene_press_rest(
        &mut wp.session,
        true,
        (px + 62, pz),
        PadButton::Left.mask(),
        100,
    );
    assert!(
        rest.0 < px + 62,
        "the held pad must move the player (start {} -> rest {})",
        px + 62,
        rest.0
    );
    assert_eq!(
        rest,
        (px, pz),
        "full-scene leading-edge press rests exactly at the captured retail position"
    );
}

#[test]
fn wall_press_down_full_scene_rest_matches_retail() {
    let Some(mut wp) = load_wall_press("rimelm_wall_press_down") else {
        return;
    };
    let (px, pz) = (wp.px as i16, wp.pz as i16);
    // Azimuth 0, quadrant 0: screen-down = world Z-.
    let rest = full_scene_press_rest(
        &mut wp.session,
        true,
        (px, pz + 62),
        PadButton::Down.mask(),
        100,
    );
    assert!(
        rest.1 < pz + 62,
        "the held pad must move the player (start {} -> rest {})",
        pz + 62,
        rest.1
    );
    assert_eq!(
        rest,
        (px, pz),
        "full-scene leading-edge press rests exactly at the captured retail position"
    );
}

/// NPC-press capture: the player holds the d-pad pressed into the sparring
/// partner Tetsu. Pins the ACTOR arm of the collision check from live RAM:
///
/// - the mutual `+0x98` collision link is live in-frame BOTH ways
///   (player -> actor, actor -> player) — `FUN_801cfc40`'s hit path;
/// - the village NPC's flags carry the `0x20000` class bit, putting him on
///   the MOVING-actor arm (result bit `1`, ±40-unit box around the live
///   position with the locomotion's zero caller extents) — the ground truth
///   for the engine's `FIELD_NPC_BOX_HALF`;
/// - the engine's ported probe (`World::field_actor_dir_blocked`) must
///   refuse the press direction at the captured configuration (the player
///   rests against Tetsu) while leaving the opposite direction clear.
#[test]
fn npc_press_pins_moving_actor_arm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/scenarios.toml not found");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let Some(scn) = manifest
        .scenarios
        .iter()
        .find(|s| s.label == "rimelm_npc_press_tetsu")
    else {
        eprintln!("[skip] scenario 'rimelm_npc_press_tetsu' not in manifest");
        return;
    };
    let save_path = match manifest.mednafen_save_path(scn, library_dir().as_deref()) {
        Ok(p) if p.exists() => p,
        _ => {
            eprintln!("[skip] no save on disk for 'rimelm_npc_press_tetsu'");
            return;
        }
    };
    let state = SaveState::from_path(&save_path).expect("parse mednafen save state");
    let ram = state.main_ram().expect("save state has main RAM");

    // Player.
    let player = ram_u32(ram, PLAYER_PTR_ADDR);
    assert_eq!(player & 0xFF00_0000, 0x8000_0000, "player struct pointer");
    let (px, pz) = (ram_i16(ram, player + 0x14), ram_i16(ram, player + 0x18));
    let player_link = ram_u32(ram, player + 0x98);

    // Active-actor table: `DAT_801c93c8`, count `_DAT_8007b6b8`.
    let count = ram_u32(ram, 0x8007_B6B8);
    assert!(
        count >= 1,
        "at least one actor in the collision table (got {count})"
    );
    let actor = ram_u32(ram, 0x801C_93C8);
    assert_eq!(actor & 0xFF00_0000, 0x8000_0000, "actor struct pointer");
    let (ax, az) = (ram_i16(ram, actor + 0x14), ram_i16(ram, actor + 0x18));
    let flags = ram_u32(ram, actor + 0x10);
    let actor_link = ram_u32(ram, actor + 0x98);

    // The mutual +0x98 collision link is live in-frame, both directions.
    assert_eq!(player_link, actor, "player +0x98 links the pressed actor");
    assert_eq!(actor_link, player, "actor +0x98 links the player back");

    // The village NPC takes the MOVING-actor arm: classifier bit set, and
    // the `0x40020000` test routes the hit to result bit 1 (not the static
    // prop bit 4).
    assert_ne!(
        flags & 0x0102_0000,
        0,
        "NPC flags 0x{flags:08X} classify to the moving-actor branch"
    );
    assert_ne!(
        flags & 0x4002_0000,
        0,
        "NPC flags 0x{flags:08X} route the hit to result bit 1"
    );

    // The press direction from the captured geometry (the actor sits on one
    // axis-aligned side of the player; Tetsu is at +Z here).
    let (dx, dz) = ((ax - px) as i32, (az - pz) as i32);
    let dir = if dz.abs() >= dx.abs() {
        if dz > 0 { 2 } else { 0 } // Z+ / Z-
    } else if dx > 0 {
        3 // X+
    } else {
        1 // X-
    };
    eprintln!("[npc-press] player ({px},{pz}) actor ({ax},{az}) flags 0x{flags:08X} dir {dir}");

    // Engine: the ported moving-actor probe refuses the press direction at
    // the captured rest configuration and leaves the opposite one clear.
    let mut world = World::new();
    world.install_field_player(0);
    world.solid_field_npcs = true;
    world.field_npc_positions.insert(1, (ax, az));
    assert!(
        world.field_actor_dir_blocked(px, pz, dir),
        "the engine probe blocks the captured press direction"
    );
    assert!(
        !world.field_actor_dir_blocked(px, pz, dir ^ 2),
        "walking away from the NPC stays clear"
    );
    // And the stepper holds the player at the captured rest on that axis.
    world.actors[0].move_state.world_x = px;
    world.actors[0].move_state.world_z = pz;
    let dir_bits = [0x4000u16, 0x8000, 0x1000, 0x2000][dir];
    for _ in 0..50 {
        world.advance_with_collision(0, dir_bits, 8);
    }
    let ms = &world.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        (px, pz),
        "the engine stepper rests at the captured press position"
    );
}
