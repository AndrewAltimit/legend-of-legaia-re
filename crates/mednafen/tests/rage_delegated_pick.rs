//! Disc + library gated: the one catalogued mid-Rage state pins what the
//! delegated (AI-controlled) party-member pick actually is.
//!
//! When a party member wears the Evil Medallion, `FUN_80047430` promotes its
//! Rage accessory passive to the per-actor AI-delegation bits `+0x16E |= 0x380`,
//! the command ring passes the member over (`+0x16E & 0xF84`), and the round's
//! state-`0x00` pass fills its action stream: `FUN_801F0450`'s delegated arm
//! (gated on the character record's `+0xF8 & 0x2000`) commits category `3`,
//! rolls a target and draws learned arts (`id + 0x1B`) until a `rand() % 7`
//! stop. A PCSX-Redux write-watch (`autorun_rage_delegated_pick.lua`) pins the
//! writer at `0x801F068C` / `0x801F06B8` and shows the seed's builder call
//! (`FUN_801EED1C` from `0x801E2C7C`) leaving the stream as it is. The engine
//! port is `battle_action::round_state_zero` + `World::arm_party_delegated`.
//! This test pins the one mid-Rage library sample, which is that loop's output
//! shape.
//!
//! Pinned facts (battle-actor pool `0x800EC9E8`, stride `0x2D4`, indexed via the
//! 8-slot pointer table `0x801C9370`):
//!
//!   - Exactly the delegated actor (the Evil Medallion wearer, slot 0) carries
//!     the AI-delegation bits `+0x16E & 0x380 == 0x380`; the other party slots
//!     carry `+0x16E == 0`.
//!   - The battle-actor `+0xF8` bit `0x2000` is set on every party slot here, so
//!     within the battle-actor struct it is NOT the per-actor delegation
//!     discriminator - `+0x16E & 0x380` is. (Corrects the scenario prose's
//!     "+0xF8 bit 0x2000 marks the Rage actor" reading.)
//!   - The delegated actor's resolved pick is category `+0x1DE == 3` (Attack)
//!     with the `+0x1DF` stream `[0x22,0x26,0x25,0x22,0x21]` - these are *art
//!     constants* (`0x1B..0x32` range), i.e. a Tactical Arts combo, not a plain
//!     multi-strike of target indices. The non-delegated party slots carry the
//!     default `+0x1DE == 4` (Spirit) with an empty stream.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `scripts/scenarios.toml` /
//! `saves/library`.

use std::path::PathBuf;

use legaia_mednafen::SaveState;
use legaia_ram_image::{ScenarioManifest, extract::ram_slice};

/// 8-slot battle-actor pointer table (`DAT_801C9370`).
const ACTOR_TABLE: u32 = 0x801C_9370;
/// Main-RAM virtual-address range (actor-pointer sanity check).
const RAM_RANGE: std::ops::Range<u32> = 0x8000_0000..0x8020_0000;

/// The captured delegated pick: category 3 (Attack) + this `+0x1DF` stream.
const RAGE_CATEGORY: u8 = 3;
const RAGE_STREAM: [u8; 5] = [0x22, 0x26, 0x25, 0x22, 0x21];

fn manifest_path() -> Option<PathBuf> {
    [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}
fn library_dir() -> Option<PathBuf> {
    ["saves/library", "../saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.is_dir())
}
fn ru32(ram: &[u8], va: u32) -> u32 {
    u32::from_le_bytes(ram_slice(ram, va, va + 4).unwrap()[..4].try_into().unwrap())
}
fn ru16(ram: &[u8], va: u32) -> u16 {
    u16::from_le_bytes(ram_slice(ram, va, va + 2).unwrap()[..2].try_into().unwrap())
}

#[test]
fn rage_delegated_pick_is_pinned() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (Some(mp), Some(lib)) = (manifest_path(), library_dir()) else {
        eprintln!("[skip] scenarios.toml / saves/library missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&mp).expect("parse manifest");
    let Some(scn) = manifest
        .scenarios
        .iter()
        .find(|s| s.label == "evil_medallion_rage_battle")
    else {
        eprintln!("[skip] scenario evil_medallion_rage_battle absent");
        return;
    };
    let Some(path) = manifest.library_save_path(scn, &lib) else {
        eprintln!("[skip] no mednafen library backup");
        return;
    };
    let save = SaveState::from_path(&path).expect("parse save");
    let ram = save.main_ram().expect("main RAM");

    // Resolve the four party-slot battle actors (slots 0..3).
    let mut delegated = Vec::new();
    let mut f8_2000_party = 0usize;
    let mut sampled = 0usize;
    for slot in 0..4u32 {
        let actor = ru32(ram, ACTOR_TABLE + slot * 4);
        if !RAM_RANGE.contains(&actor) {
            continue;
        }
        sampled += 1;
        if ru32(ram, actor + 0xF8) & 0x2000 != 0 {
            f8_2000_party += 1;
        }
        if ru16(ram, actor + 0x16E) & 0x380 == 0x380 {
            delegated.push((slot, actor));
        }
    }
    assert!(
        sampled >= 3,
        "expected a 3-member party, got {sampled} actors"
    );

    // Exactly one delegated party member (the Evil Medallion wearer).
    assert_eq!(
        delegated.len(),
        1,
        "expected exactly one +0x16E&0x380 delegated party member, got {}",
        delegated.len(),
    );
    // +0xF8 0x2000 is party-wide here, so it is not the delegation discriminator.
    assert!(
        f8_2000_party >= 2,
        "+0xF8 bit 0x2000 should be party-wide (got {f8_2000_party}); \
         it does not discriminate the delegated actor",
    );

    // The delegated actor's resolved pick: category 3 + the multi-strike stream.
    let (slot, actor) = delegated[0];
    let cat = ram_slice(ram, actor + 0x1DE, actor + 0x1DF).unwrap()[0];
    assert_eq!(
        cat, RAGE_CATEGORY,
        "delegated slot {slot}: +0x1DE category {cat} != {RAGE_CATEGORY} (Attack)",
    );
    let stream = ram_slice(ram, actor + 0x1DF, actor + 0x1DF + RAGE_STREAM.len() as u32).unwrap();
    assert_eq!(
        stream, RAGE_STREAM,
        "delegated slot {slot}: +0x1DF stream {stream:02X?} != {RAGE_STREAM:02X?}",
    );

    eprintln!(
        "delegated party slot {slot} (actor 0x{actor:08X}): +0x16E&0x380 set, \
         +0x1DE={cat} (Attack), +0x1DF stream {stream:02X?}; \
         +0xF8 0x2000 set on {f8_2000_party}/{sampled} party slots",
    );
}
