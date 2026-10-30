//! Disc-gated: a Grid Duel played at a table in the world. The `duel_table`
//! fixture pack's duel and the Rebirth pack's Warden's Duel are entered as
//! modes; the scene host enters each one's pack map under the running seat,
//! builds the cards' portraits, and the world writes the card page into the
//! scene's VRAM while the table stage seats the opponent across the board.
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::cards::table_art::{self, BACK_SLOT, PAGE_X, PAGE_Y};
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_expansion_format::ExpansionPack;

fn pack(rel: &str) -> ExpansionPack {
    let dir = format!("{}/{rel}", env!("CARGO_MANIFEST_DIR"));
    let files = legaia_expansion_format::read_files(std::path::Path::new(&dir)).expect("read");
    ExpansionPack::from_files(&files).expect("the pack parses")
}

fn host(rel: &str) -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(pack(rel)))
        .expect("the pack activates");
    host.enter_field_scene("town01", 0).expect("enter town01");
    Some(host)
}

/// Enter duel `id` and tick until its map is up and the table draws.
fn enter(host: &mut SceneHost, id: &str, map: &str) {
    host.world.enter_pack_mode(id).expect("the duel builds");
    assert_eq!(
        host.world.pack_mode_wants_map().map(|m| m.0),
        Some(map.to_string())
    );
    let mut entered = false;
    for _ in 0..6 {
        host.world.set_pad(0);
        if let SceneTickEvent::SceneEntered { .. } = host.tick().expect("tick") {
            entered = true;
        }
        host.step_headless_frame_tail();
    }
    assert!(entered, "the map entered under the seat");
    assert_eq!(host.active_expansion_map(), Some(map));
    assert_eq!(host.world.mode, SceneMode::Expansion);
}

fn check_table(host: &mut SceneHost, opponent: &str) {
    let stage = host
        .world
        .pack_mode_stage()
        .expect("the table draws")
        .clone();
    let duel = stage.duel.expect("a duel table");
    assert_eq!(duel.cards.len(), 10);
    // The opponent NPC stands on its seat.
    let slot = host
        .world
        .expansion
        .map
        .as_ref()
        .unwrap()
        .npcs()
        .find(|(_, d)| d.id == opponent)
        .map(|(s, _)| s)
        .expect("the opponent is a map NPC");
    let (x, z) = host.world.npcs.positions[&slot];
    let seat = duel.table.seat_pos(1);
    assert!((f32::from(x) - seat[0]).abs() <= 1.0 && (f32::from(z) - seat[1]).abs() <= 1.0);
    // The portraits are built and the card page sits in the scene's VRAM.
    assert!(
        !host.world.expansion.cards.portraits.by_monster.is_empty(),
        "portraits rendered"
    );
    let vram = &host.resources.as_ref().expect("scene resources").vram;
    let (u, v) = table_art::slot_uv(BACK_SLOT);
    let at = |dx: u16, dy: u16| {
        vram.pixel(
            usize::from(PAGE_X + u16::from(u) + dx),
            usize::from(PAGE_Y + u16::from(v) + dy),
        )
    };
    assert_ne!(at(26, 36), 0, "the back is in VRAM");
    // A face's portrait window is drawn.
    let (fu, fv) = table_art::slot_uv(table_art::card_slot(0, 0));
    assert_ne!(
        vram.pixel(
            usize::from(PAGE_X + u16::from(fu) + 20),
            usize::from(PAGE_Y + u16::from(fv) + 20)
        ),
        0
    );
    // Leaving puts the field back.
    host.world.exit_pack_mode();
    assert_eq!(host.world.mode, SceneMode::Field);
}

#[test]
fn the_test_packs_duel_plays_at_its_table() {
    let Some(mut host) = host("../engine-modes/tests/fixtures/duel_table") else {
        return;
    };
    enter(&mut host, "table-duel", "duel_room");
    check_table(&mut host, "rival_noa");
}

#[test]
fn the_wardens_duel_plays_at_the_card_room_table() {
    let Some(mut host) = host("../../packs/rebirth") else {
        return;
    };
    enter(&mut host, "wardens-duel", "rebirth_tower_5f");
    check_table(&mut host, "duel_second");
}
