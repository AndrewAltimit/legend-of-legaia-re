//! Disc-gated: the **class gate** on retail's talk-time face-the-player snap.
//!
//! `FUN_80039B7C` stores the player bearing into the addressed actor's `+0x26`
//! only when its flag word satisfies `flags & 0x420000 == 0x20000` - moving
//! class set, `0x400000` clear. `FUN_8003A1E4` ORs `0x20000` into every
//! partition-1 placement and the allocator template
//! (`FUN_80020DE0`, `0x80073E58 +0x0C`) leaves `0x400000` clear, so on the
//! disc the discriminator is entirely the **opt-out**: a spawn prologue
//! `31 16` (`CFLAG_SET` bit 22) marks a placement whose authored pose must
//! survive being talked to.
//!
//! `koin6` is the clean fixture. Of its placements exactly four set bit 22 -
//! the two babies in cribs and the two treasure chests - and everything else
//! is an ordinary villager. A port that runs the snap ungated rotates the
//! babies out of their cribs to look at the player, which is the reported
//! symptom.
//!
//! Both halves are asserted, because either alone can pass vacuously: the
//! flags really do differ between the two classes (a decode that returned `0`
//! for everything would satisfy the behaviour half), and the behaviour really
//! does differ (a gate that returned early for everything would satisfy the
//! flag half).
//!
//! Assertions are structural - slots, flag bits, headings. No Sony bytes.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md).

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

const SCENE: &str = "koin6";
/// The two cribbed babies - `koin6`'s bit-22 NPC placements.
const STATIC_POSE_SLOTS: [u8; 2] = [10, 11];
/// Retail's "do not pose me at the player" bit (`flags & 0x400000`).
const STATIC_POSE_BIT: u32 = 0x0040_0000;

fn open_host() -> Option<SceneHost> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    None
}

/// Every placement the scene surfaced a position for, split by the bit.
fn split_by_class(host: &SceneHost) -> (Vec<u8>, Vec<u8>) {
    let mut statics = Vec::new();
    let mut movers = Vec::new();
    let mut slots: Vec<u8> = host.world.field_npc_positions.keys().copied().collect();
    slots.sort_unstable();
    for slot in slots {
        if host.world.field_channel_flags(slot) & STATIC_POSE_BIT != 0 {
            statics.push(slot);
        } else {
            movers.push(slot);
        }
    }
    (statics, movers)
}

#[test]
fn talking_to_a_static_pose_placement_does_not_turn_it() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ missing");
        return;
    };
    if host.enter_field_scene(SCENE, 0).is_err() {
        eprintln!("[skip] {SCENE} did not load");
        return;
    }

    let (statics, movers) = split_by_class(&host);

    // --- The disc half: the two classes exist and are the ones expected. ---
    for slot in STATIC_POSE_SLOTS {
        assert!(
            statics.contains(&slot),
            "{SCENE} placement {slot} should carry the static-pose bit; \
             statics={statics:?} movers={movers:?}"
        );
    }
    assert!(
        !movers.is_empty(),
        "{SCENE} must also surface ordinary turn-toward-player NPCs, else the \
         behaviour half below is vacuous (every slot read as static)"
    );

    // --- The behaviour half: the gate is actually consulted. ---
    for slot in STATIC_POSE_SLOTS {
        let before = host.world.field_npc_headings.get(&slot).copied();
        host.world.field_npc_facing_save = None;
        host.world.face_field_npc_at_player(slot);
        assert_eq!(
            host.world.field_npc_headings.get(&slot).copied(),
            before,
            "static-pose placement {slot} must keep its authored heading"
        );
        assert!(
            host.world.field_npc_facing_save.is_none(),
            "a skipped snap must not arm the restore for slot {slot}"
        );
    }

    // A mover with a surfaced position and a heading must still turn -
    // otherwise the gate is rejecting everything and the loop above proves
    // nothing.
    let turned = movers.iter().any(|&slot| {
        let before = host.world.field_npc_headings.get(&slot).copied();
        host.world.field_npc_facing_save = None;
        host.world.face_field_npc_at_player(slot);
        let after = host.world.field_npc_headings.get(&slot).copied();
        before.is_some() && after != before
    });
    assert!(
        turned,
        "no ordinary {SCENE} NPC turned toward the player - the class gate is \
         rejecting every placement, so the static-pose assertions are vacuous"
    );
}
