//! A Grid Duel played **at a table in the world**: the `duel_table` fixture
//! pack's duel runs through `World::tick` with its pack map standing under
//! it, and the table stage lays the cards out, seats the opponent, drives
//! the spectator and holds the results card for the reaction shot - without
//! changing what the session does.
//!
//! Synthetic world: the map's field image is the one the scene host would
//! seat, laid straight onto the world's terrain. No disc, no Sony bytes.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::world::World;
use legaia_engine_expansion::map::ActiveMap;
use legaia_engine_expansion::modes::ModeStage;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::ModeView;
use legaia_engine_modes::cards::duel::DuelPhase;
use legaia_engine_scene::expansion_map::{BaseInputs, build_field_map_image, floor_lut};
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::Ground;

const CROSS: u16 = PadButton::Cross as u16;

fn pack() -> Arc<ExpansionPack> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine-modes/tests/fixtures/duel_table");
    Arc::new(ExpansionPack::from_dir(&dir).expect("the duel_table pack parses"))
}

/// A field world with the pack active, standing on `duel_room` when
/// `on_map` (the duel then plays at its table) or off every map (its 2D
/// screen).
fn world(on_map: bool) -> World {
    world_with(pack(), on_map)
}

fn world_with(pack: Arc<ExpansionPack>, on_map: bool) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.spawn_actor(0).active = true;
    w.install_field_player(0);
    w.activate_expansion(pack.clone())
        .expect("the pack activates");
    if on_map {
        let def = &pack.maps["duel_room"];
        let grid = def.tile_grid().unwrap();
        let mut bare = def.clone();
        for style in bare.tiles.legend.values_mut() {
            if matches!(style.ground, Some(Ground::BaseCell { .. })) {
                style.ground = None;
            }
            style.wall = None;
        }
        let image = build_field_map_image(&bare, &grid, BaseInputs::default()).unwrap();
        w.terrain.collision_grid = image[0x4000..0x8000].to_vec();
        w.terrain.floor_height_lut = floor_lut(def, None).map(i16::wrapping_neg);
        let map = ActiveMap::new(
            "duel_room",
            Arc::new(def.clone()),
            Arc::new(grid),
            (2, 4),
            |_| false,
        );
        // The NPCs stand on their tiles, as the map's entry seats them.
        for (slot, d) in map.npcs() {
            let (x, z) = def.tile_centre(d.tile[0], d.tile[1]);
            w.npcs.positions.insert(slot, (x as i16, z as i16));
        }
        w.npcs.entry_positions = w.npcs.positions.clone();
        w.expansion.map = Some(map);
    }
    w
}

fn frame(w: &mut World, pad: u16) {
    w.input.set_pad(pad);
    w.tick();
}

fn in_results(w: &World) -> bool {
    matches!(
        w.expansion.mode_seat.as_deref().map(|s| &s.stage),
        Some(ModeStage::Results { .. })
    )
}

fn hash(w: &World) -> Option<u64> {
    w.expansion
        .mode_seat
        .as_deref()?
        .active()
        .map(|m| m.state_hash())
}

/// The player's pad: lift the cursor's card, then walk the cell cursor to
/// the first empty cell and place it. Every press is one frame with a
/// release after it.
fn duel_pad(w: &World, n: u32) -> u16 {
    if n % 2 == 1 {
        return 0;
    }
    let Some(ModeView::Duel(v)) = w.expansion.mode_seat.as_deref().and_then(|s| s.view()) else {
        return 0;
    };
    match v.phase {
        DuelPhase::PickCard { seat: 0 } => CROSS,
        DuelPhase::PickCell { seat: 0, .. } => {
            let want = (0..9).find(|&c| v.grid[c].is_none()).unwrap_or(0);
            let at = v.cell_cursor[0];
            let (wr, wc, ar, ac) = (want / 3, want % 3, at / 3, at % 3);
            if ar > wr {
                PadButton::Up as u16
            } else if ar < wr {
                PadButton::Down as u16
            } else if ac > wc {
                PadButton::Left as u16
            } else if ac < wc {
                PadButton::Right as u16
            } else {
                CROSS
            }
        }
        _ => 0,
    }
}

#[test]
fn a_duel_at_the_table_plays_to_completion_in_the_world() {
    let mut w = world(true);
    w.enter_pack_mode("table-duel").expect("enters");
    assert!(w.pack_mode_wants_map().is_none(), "already on the map");
    let opp_slot = w
        .expansion
        .map
        .as_ref()
        .unwrap()
        .npcs()
        .find(|(_, d)| d.id == "rival_noa")
        .map(|(s, _)| s)
        .unwrap();
    let (mut seated, mut flipped, mut held, mut cued) = (false, false, false, false);
    let mut n = 0;
    while !in_results(&w) {
        assert!(n < 60 * 60 * 5, "the duel ends");
        let pad = duel_pad(&w, n);
        frame(&mut w, pad);
        n += 1;
        let stage = w.pack_mode_stage().expect("the table draws").clone();
        let duel = stage.duel.as_ref().expect("a duel table frame");
        // The opponent sits across the board, facing it.
        let seat = duel.table.seat_pos(1);
        if let Some(&(x, z)) = w.npcs.positions.get(&opp_slot) {
            seated |=
                (f32::from(x) - seat[0]).abs() <= 1.0 && (f32::from(z) - seat[1]).abs() <= 1.0;
        }
        // The leader stands on the player's side of the table, on the
        // floor.
        let a = &w.actors[0].move_state;
        let c = duel.table.centre();
        let (across, _) = duel.table.axes(0);
        let along =
            (f32::from(a.world_x) - c[0]) * across[0] + (f32::from(a.world_z) - c[1]) * across[1];
        assert!(along < 0.0, "the leader is on the near side: {along}");
        assert_eq!(
            i32::from(a.world_y),
            w.sample_field_floor_height(a.world_x.into(), a.world_z.into())
        );
        flipped |= !duel.bursts.is_empty();
        cued |= !duel.cues.is_empty();
        assert_eq!(duel.cards.len(), 10, "every card is on the table");
    }
    // The results card waits for the reaction shot.
    let mut waited = 0;
    while w
        .expansion
        .mode_seat
        .as_deref()
        .is_some_and(|s| s.world_stage.holds_results())
    {
        held = true;
        frame(&mut w, if waited % 2 == 0 { CROSS } else { 0 });
        waited += 1;
        assert!(in_results(&w), "Cross does not leave while the shot plays");
        assert!(waited < 1000);
    }
    assert!(seated && flipped && held && cued);
    // Then Cross leaves, the opponent goes back to its tile and the player
    // to where the seat found it.
    frame(&mut w, 0);
    frame(&mut w, CROSS);
    assert_eq!(w.mode, SceneMode::Field);
    let entry = w.npcs.entry_positions.get(&opp_slot).copied();
    assert!(entry.is_some());
    assert_eq!(w.npcs.positions.get(&opp_slot).copied(), entry);
}

#[test]
fn the_table_does_not_change_the_session() {
    // The same entry and pads at the table and on the 2D screen: the
    // session state hashes match frame for frame.
    let mut a = world(true);
    let mut b = world(false);
    a.enter_pack_mode("table-duel").unwrap();
    b.enter_pack_mode("table-duel").unwrap();
    let mut staged = 0;
    for n in 0..2400u32 {
        let pad = duel_pad(&a, n);
        frame(&mut a, pad);
        frame(&mut b, pad);
        staged += usize::from(a.pack_mode_stage().is_some());
        assert!(b.pack_mode_stage().is_none());
        assert_eq!(hash(&a), hash(&b), "frame {n}");
        if in_results(&a) {
            break;
        }
    }
    assert!(staged > 0, "the table drew");
}

#[test]
fn the_card_page_is_written_and_put_back() {
    let mut w = world(true);
    let mut vram = legaia_tim::Vram::new();
    let (px, py, pw, ph) = legaia_engine_modes::cards::table_art::PAGE_RECT;
    // Some scene words under the page.
    let before: Vec<u8> = (0..u32::from(pw) * u32::from(ph) * 2)
        .map(|i| (i % 251) as u8)
        .collect();
    vram.write_block(px, py, pw, ph, &before);
    assert!(!w.step_duel_card_page(&mut vram), "no duel, no write");
    w.enter_pack_mode("table-duel").unwrap();
    frame(&mut w, 0);
    assert!(w.step_duel_card_page(&mut vram), "the page is written");
    assert!(!w.step_duel_card_page(&mut vram), "once");
    let back_uv = legaia_engine_modes::cards::table_art::slot_uv(
        legaia_engine_modes::cards::table_art::BACK_SLOT,
    );
    let mid = vram.pixel(
        usize::from(px) + usize::from(back_uv.0) + 26,
        usize::from(py) + usize::from(back_uv.1) + 36,
    );
    assert_ne!(mid, 0, "the back is drawn");
    w.exit_pack_mode();
    assert!(w.step_duel_card_page(&mut vram), "put back");
    let after: Vec<u8> = (0..usize::from(ph))
        .flat_map(|y| (0..usize::from(pw)).map(move |x| (usize::from(px) + x, usize::from(py) + y)))
        .flat_map(|(x, y)| vram.pixel(x, y).to_le_bytes())
        .collect();
    assert_eq!(after, before);
}

/// A table's `hide` takes the named props' draws (and lights) away while the
/// duel is seated on the map, and gives them back when the seat closes.
#[test]
fn the_table_hides_its_props_while_seated() {
    let mut pack = (*pack()).clone();
    let room = pack.maps.get_mut("duel_room").unwrap();
    room.props.push(legaia_expansion_format::map::PropDef {
        id: "plank".into(),
        tile: [5, 4],
        solid: true,
        ..Default::default()
    });
    room.props.push(legaia_expansion_format::map::PropDef {
        id: "stool".into(),
        tile: [5, 6],
        ..Default::default()
    });
    pack.modes.card_duels[0].table.as_mut().unwrap().hide = vec!["plank".into()];
    let mut w = world_with(Arc::new(pack), true);
    let key = legaia_engine_scene::expansion_map::PROP_RECORD_BASE;
    assert!(!w.hidden_object_records().contains(&key), "drawn before");
    let grid = w.terrain.collision_grid.clone();
    w.enter_pack_mode("table-duel").expect("enters");
    for _ in 0..4 {
        frame(&mut w, 0);
    }
    let hidden = w.hidden_object_records();
    assert!(hidden.contains(&key), "the plank hides while seated");
    assert!(!hidden.contains(&(key + 1)), "the stool stays");
    assert_eq!(w.terrain.collision_grid, grid, "collision is untouched");
    w.exit_pack_mode();
    for _ in 0..2 {
        frame(&mut w, 0);
    }
    assert!(
        !w.hidden_object_records().contains(&key),
        "drawn again once the seat closes"
    );
}
