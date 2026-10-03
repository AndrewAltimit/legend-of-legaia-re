//! Disc-gated: field-VM op `0x4B` arms a placement's VDF morph lanes.
//!
//! `rikuroa`'s Genesis tree is three `.MAP` placed objects bound to MAN
//! records `P0[2..4]`. While story flag `0x142` is clear their bind-time
//! prologues run op `0x4B` (`4B 07 00 ..`, `4B 01 08 ..`, `4B 01 07 ..`)
//! and raise the HOLD bit
//! (`2B 0A`, `+0x62 & 0x400`) behind it, so the ramp envelope primes every
//! lane to `0x1000` and holds it: the withered tree. The
//! `rikuroa_pre_caruban` capture holds exactly that - lanes `0..6` / `7` /
//! `8` at weight `0x1000`, `+0x62 = 0x415` - and `rikuroa_post_genesis_tree`
//! holds them at `0`. With `0x142` set the records skip the op and nothing
//! arms.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::MorphOwner;

fn host() -> Option<SceneHost> {
    let disc = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists());
    let Some(disc) = disc else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    };
    Some(SceneHost::open_disc(&disc).expect("open disc"))
}

fn lanes(host: &SceneHost, record: u16) -> Option<Vec<(u8, u16)>> {
    host.world.field_morph_lanes(MorphOwner::Object(record))
}

#[test]
fn the_unrevived_genesis_tree_holds_its_morph_lanes_at_peak() {
    let Some(mut host) = host() else { return };
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    let trunk = lanes(&host, 2).expect("P0[2] armed");
    assert_eq!(
        trunk,
        (0u8..7).map(|i| (i, 0x1000)).collect::<Vec<_>>(),
        "P0[2]: sub-entries 0..6 held at peak"
    );
    assert_eq!(lanes(&host, 3), Some(vec![(8, 0x1000)]), "P0[3]");
    assert_eq!(lanes(&host, 4), Some(vec![(7, 0x1000)]), "P0[4]");
    // The bound draws' pack meshes come back displaced, not at rest.
    let slots = host
        .world
        .npcs
        .object_pack_slots
        .get(&2)
        .cloned()
        .unwrap_or_default();
    assert!(!slots.is_empty(), "P0[2] binds a placed draw");
    let moved = slots.iter().any(|&s| {
        (0..8u32).any(|g| {
            host.world
                .current_morph_deltas(s, g, 4096)
                .is_some_and(|d| d.iter().any(|v| *v != [0, 0, 0]))
        })
    });
    assert!(moved, "P0[2]'s pack slots {slots:?} carry non-zero deltas");
    println!("[ran] rikuroa P0[2..4] morph lanes at peak on pack slots {slots:?}");
}

#[test]
fn the_revived_genesis_tree_arms_no_morph() {
    let Some(mut host) = host() else { return };
    host.world.system_flag_set(0x142);
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    for record in 2..=4u16 {
        assert!(
            lanes(&host, record).is_none(),
            "P0[{record}] armed with 0x142 set"
        );
    }
    println!("[ran] rikuroa with 0x142 set arms nothing");
}
