//! Disc-gated end-to-end oracle for the **`.MAP` kind-0 map-door randomizer
//! at runtime** - the sibling of the chest / door / house-door oracles.
//!
//! The randomizer's own disc-gated test proves the patch is *written*
//! faithfully (`crates/patcher/tests/map_door_patch_real`: per-scene
//! destination multisets preserved, reachability oracle re-verified off the
//! patched image, sectors EDC/ECC-valid). What it doesn't prove is that a
//! runtime actually *reads the patched destination bytes and seats the player
//! there*.
//!
//! The engine's kind-0 dispatch (`SceneHost::dispatch_intra_scene_teleport`,
//! the port of `FUN_801D1EC4`'s kind-0 arm at `0x801d21c0`) is exactly:
//! parse the `.MAP` `+0x10000` trigger block's kind-0 sub-table
//! ([`legaia_engine_core::field_regions::parse_intra_scene_teleports`]), look
//! the crossed tile up
//! ([`legaia_engine_core::field_regions::lookup_intra_scene_teleport`]), and
//! seat the player at [`IntraSceneTeleport::dest_world`]. This test drives
//! those same kernels over the raw entry bytes:
//!
//!   1. baseline: the unpatched `town01` `.MAP` dispatches the runtime-pinned
//!      Vahn's-house exit - trigger tile `(97, 9)`, seat world
//!      `(4672, 3008)` = doorstep tile `(36, 23)` (the values the pad-driven
//!      `vahn_house_roundtrip_disc` oracle walks against);
//!   2. patch the map doors on a scratch copy of the disc
//!      (`apply::randomize_map_doors`) and re-read the patched `.MAP` off the
//!      patched image;
//!   3. dispatch a rewired trigger tile again and assert the runtime now
//!      seats the player at the **patched** destination's world coords - and
//!      no longer at the original ones.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_engine_core::field_regions::{lookup_intra_scene_teleport, parse_intra_scene_teleports};
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::drops::DropMode;

/// `.MAP` trigger-block offset (`legaia_engine_core::field_regions::MAP_REGION_BLOCK_OFFSET`).
const TRIGGER_BLOCK: usize = legaia_engine_core::field_regions::MAP_REGION_BLOCK_OFFSET;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Run the engine's kind-0 dispatch kernels over a raw `.MAP` entry: the
/// teleport crossed at `tile`, if any, and the world seat it produces.
fn dispatch(entry: &[u8], tile: (u8, u8)) -> Option<(i16, i16)> {
    let primary = parse_intra_scene_teleports(&entry[TRIGGER_BLOCK..]);
    // The randomizer only touches the primary block; the fallback table is
    // empty within the entry's own footprint.
    let tp = lookup_intra_scene_teleport(&primary, &[], tile.0, tile.1)?;
    Some(tp.dest_world())
}

#[test]
fn runtime_seats_the_player_at_the_patched_map_door_destination() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // --- baseline: the unpatched Vahn's-house exit seats at the doorstep ---
    let base = DiscPatcher::open(original.clone()).expect("open");
    let (town01_idx, _, vahn) = apply::current_map_doors(&base)
        .expect("enumerate map doors")
        .into_iter()
        .find(|(_, scene, s)| scene == "town01" && s.tile == (97, 9))
        .expect("town01 carries the Vahn's-house kind-0 exit at (97,9)");
    let base_entry = base.read_entry(town01_idx).expect("read town01 .MAP");
    let baseline = dispatch(&base_entry, (97, 9)).expect("baseline dispatch fires");
    assert_eq!(
        baseline,
        (4672, 3008),
        "unpatched kind-0 exit must seat the player on Vahn's doorstep \
         (dest_x*64+64, (dest_z+1)*64)"
    );
    assert_eq!(vahn.dest, (72, 46), "the randomizer sees the same record");

    // --- patch the map doors on a scratch copy of the disc ---
    let seed = 0x4B49_4E44_305F_5645; // arbitrary fixed seed
    let mut patcher = DiscPatcher::open(original).expect("open scratch");
    let report =
        apply::randomize_map_doors(&mut patcher, seed, DropMode::Shuffle).expect("shuffle");
    assert!(
        !report.rewires.is_empty(),
        "the shuffle must rewire something"
    );

    // --- drive the engine's dispatch over the patched bytes ---
    let r = &report.rewires[0];
    let patched_entry = patcher.read_entry(r.entry_idx).expect("read patched .MAP");
    let seated = dispatch(&patched_entry, r.tile).expect("patched dispatch fires");
    let expect_new = (i16::from(r.to.0) * 64 + 64, (i16::from(r.to.1) + 1) * 64);
    let expect_old = (
        i16::from(r.from.0) * 64 + 64,
        (i16::from(r.from.1) + 1) * 64,
    );
    assert_eq!(
        seated, expect_new,
        "runtime must seat the player at the rewired destination \
         (scene {}, tile {:?})",
        r.scene, r.tile
    );
    assert_ne!(
        seated, expect_old,
        "runtime must no longer seat the player at the original destination"
    );

    // Non-vacuous cross-check: the same tile on the *unpatched* image still
    // dispatches to the original destination.
    let base_seated = dispatch(
        &base.read_entry(r.entry_idx).expect("read baseline .MAP"),
        r.tile,
    )
    .expect("baseline dispatch fires");
    assert_eq!(
        base_seated, expect_old,
        "baseline still seats at the original"
    );
}
