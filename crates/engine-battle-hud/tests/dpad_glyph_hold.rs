//! The D-pad glyph across the ring -> attack-mode prompt -> arts entry
//! walk (#626). Retail draws it from each state arm (`FUN_801DB8F4` first
//! thing in `0x28`, `0x78` and `0x50`), and the menu SM dispatches no arm
//! while a sub-draw step's records glide (`0x801D0B2C..0x801D0BA4`), so the
//! glyph goes for the length of each edge's glide and comes back at the
//! same seat. A PCSX-Redux film of `battle_menu_rugi_barra_retail`
//! (`CROSS`, `LEFT`, `RIGHT`) holds exactly that: gone for eighteen vsyncs
//! at each edge while the chips slide, then back.

use legaia_engine_battle_hud::{
    CommandSurface, battle_chip_dpad_visible, battle_chip_offsets, battle_command_surface,
};
use legaia_engine_battle_menus::battle_input::BattleCommandSession;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_core::world::*;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;

/// A one-member fight parked on a freshly opened round prompt.
fn prompt_world() -> World {
    let mut world = World::new();
    world.party.party_count = 1;
    world.battle.player_driven = true;
    world.toggles.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..3 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world.battle.flow = legaia_engine_battle_menus::battle_flow::BattleFlowState::TurnPrompt;
    world.battle.command = Some(BattleCommandSession::new_round_open(0, 0, false));
    world.battle.start_round_prompt_glide();
    world
}

fn tick(world: &mut World, mask: u16) {
    world.set_pad(mask);
    world.live_battle_tick();
}

/// Tick with no press until the glyph is up, returning how many ticks it
/// stayed down.
fn wait_for_glyph(world: &mut World) -> usize {
    let mut down = 0;
    while !battle_chip_dpad_visible(world) {
        assert!(down < 64, "the glyph never came back");
        tick(world, 0);
        down += 1;
    }
    down
}

/// Press `button` on a dispatched pass and return the glyph's visibility
/// over the edge pass and the passes after it, until it is back.
fn press_and_film(world: &mut World, button: PadButton) -> Vec<bool> {
    tick(world, button.mask());
    let mut film = vec![battle_chip_dpad_visible(world)];
    for _ in 0..24 {
        tick(world, 0);
        film.push(battle_chip_dpad_visible(world));
    }
    film
}

/// The glyph drops for the whole of each edge's glide and returns once its
/// records have landed, where before it stayed up through the prompt's
/// edge and the arts entry's slide-in.
#[test]
fn the_glyph_waits_out_each_edges_glide() {
    let mut world = prompt_world();
    wait_for_glyph(&mut world);
    // `Begin` opens the ring under its own glide.
    tick(&mut world, PadButton::Left.mask());
    assert_eq!(battle_command_surface(&world), Some(CommandSurface::Ring));
    let ring_hold = wait_for_glyph(&mut world);
    assert!(ring_hold >= 16, "ring hold {ring_hold}");

    // The ring's Attack arm opens the attack-mode prompt (option `Select`).
    let film = press_and_film(&mut world, PadButton::Left);
    assert_eq!(
        battle_command_surface(&world),
        Some(CommandSurface::AttackMode)
    );
    let down = film.iter().skip(1).take_while(|v| !**v).count();
    assert!(
        (16..=18).contains(&down),
        "prompt edge: glyph down {down} passes: {film:?}"
    );
    assert!(film[1 + down], "the glyph returns on the prompt");

    // `Command` opens the arts entry. Its chips slide in during the edge's
    // hold - the stepper walks them on the held passes - and the glyph
    // comes back as they land, not a hold later.
    tick(&mut world, PadButton::Right.mask());
    assert_eq!(
        battle_command_surface(&world),
        Some(CommandSurface::ArtsInput)
    );
    let mut film = Vec::new();
    for _ in 0..40 {
        tick(&mut world, 0);
        let slide = world.arts_input_view().expect("arts entry open").chip_slide;
        film.push((battle_chip_dpad_visible(&world), slide));
    }
    let down = film.iter().take_while(|(v, _)| !*v).count();
    assert!(
        (15..=17).contains(&down),
        "arts edge: glyph down {down} passes: {film:?}"
    );
    assert!(
        matches!(film[0].1, Some(e) if e > 0),
        "the chips move under the hold: {film:?}"
    );
    assert_eq!(film[down].1, None, "landed when the glyph returns");
}

/// The prompt's `Auto` / `Command` slide in from `168` right of their seats
/// over the hold (records `0x55` / `0x54`, seat A `328` / `416`).
#[test]
fn the_attack_mode_chips_slide_in() {
    let mut world = prompt_world();
    wait_for_glyph(&mut world);
    tick(&mut world, PadButton::Left.mask());
    wait_for_glyph(&mut world);
    tick(&mut world, PadButton::Left.mask());
    assert_eq!(
        battle_command_surface(&world),
        Some(CommandSurface::AttackMode)
    );
    tick(&mut world, 0);
    let first = battle_chip_offsets(&world);
    assert!(first[0].0 > 100 && first[0] == first[1], "{first:?}");
    let mut last = first[0].0;
    for _ in 0..20 {
        tick(&mut world, 0);
        let o = battle_chip_offsets(&world)[0].0;
        assert!(o <= last, "the chips move one way");
        last = o;
    }
    assert_eq!(battle_chip_offsets(&world), [(0, 0); 4]);
}
