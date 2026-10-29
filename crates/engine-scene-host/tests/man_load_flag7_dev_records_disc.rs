//! The MAN loader's flag 7 keeps the developer walk-on records shut.
//!
//! `FUN_8003AEB0` raises system flag `7` (`0x80085758 |= 1`,
//! `0x8003AEEC..0x8003AEF8`) on every scene load, and no shipped script
//! clears it. `town01` carries two partition-2 records named `サウンド外` /
//! `サウンド内` whose whole body is a "Greetings." box, gated `C1 = [7]`
//! (`FUN_8003BDE0` blocks a C1 record while any listed flag is set), with
//! walk-on tiles on the paths out of the Genesis Tree clearing. Retail never
//! spawns them; without the loader's write the port opened "Greetings." on
//! the first walk out of the opening area.
//!
//! Disc-gated: skips and passes without `LEGAIA_DISC_BIN`.

use legaia_engine_core::world::MAN_LOAD_FLAG;
use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn disc_path() -> Option<PathBuf> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    path.is_file().then_some(path)
}

/// `town01`'s opening has played (`P2[3]`'s one-shot C1 flag), so the entry
/// leaves the player free on the field.
const OPENING_PLAYED: u16 = 0x225;

/// Every gate-1 walk-on trigger of the loaded scene whose partition-2 record
/// is gated `C1 = [7]` and carries a text segment, as
/// `(tile_x, tile_z, record)`.
fn flag7_triggers(host: &SceneHost) -> Vec<(u8, u8, usize)> {
    let scene = host.scene.as_ref().expect("scene loaded");
    let man = scene
        .field_man_payload(&host.index)
        .expect("MAN read")
        .expect("town01 has a MAN");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("MAN parses");
    let (primary, fallback) = scene.field_tile_triggers(&host.index).expect("triggers");
    primary
        .iter()
        .chain(fallback.iter())
        .filter(|t| t.gate == 1)
        .filter_map(|t| {
            let (c1, _) = legaia_engine_field::man_field_scripts::partition2_record_gates(
                &mf,
                &man,
                usize::from(t.record),
            )?;
            let (start, _, len) = legaia_engine_field::man_field_scripts::partition_record_span(
                &mf,
                &man,
                2,
                usize::from(t.record),
            )?;
            let speaks = man.get(start..start + len)?.contains(&0x1F);
            (c1 == [MAN_LOAD_FLAG] && speaks).then_some((t.tile_x, t.tile_z, usize::from(t.record)))
        })
        .collect()
}

/// The text of whatever box a script context or a talk is showing.
fn box_page(host: &SceneHost) -> Option<Vec<u8>> {
    let w = &host.world;
    if let Some(p) = w.script_dialog_panel() {
        return Some(p.page_bytes());
    }
    w.dialog
        .inline
        .as_ref()
        .and_then(|id| id.panel.as_ref())
        .map(|p| p.page_bytes())
}

/// Seat the player on `tile` and run the field: does a box open?
fn walk_on_opens_a_box(host: &mut SceneHost, tile: (u8, u8)) -> bool {
    // Step off first so the seat is a crossing.
    host.world
        .seat_player_at_tile(tile.0, tile.1.wrapping_sub(3));
    for _ in 0..4 {
        let _ = host.tick();
    }
    host.world.seat_player_at_tile(tile.0, tile.1);
    for _ in 0..60 {
        let _ = host.tick();
        if box_page(host).is_some_and(|p| !p.is_empty()) {
            return true;
        }
    }
    false
}

#[test]
fn the_sound_records_never_greet_on_walk_on() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut host = SceneHost::open_disc(&path).expect("open disc");
    host.world.system_flag_set(OPENING_PLAYED);
    host.enter_field_scene("town01", 0).expect("enter town01");
    assert!(
        host.world.system_flag_test(MAN_LOAD_FLAG),
        "the scene load raises flag 7"
    );

    let triggers = flag7_triggers(&host);
    assert!(
        triggers.len() >= 2,
        "town01 walk-on tiles reach its C1 = [7] records: {triggers:?}"
    );
    eprintln!("town01 C1 = [7] walk-on tiles: {triggers:?}");
    let (tx, tz, record) = triggers[0];
    for _ in 0..30 {
        let _ = host.tick();
    }
    assert!(
        !walk_on_opens_a_box(&mut host, (tx, tz)),
        "P2[{record}] at ({tx},{tz}) opened a box under flag 7"
    );

    // The control: the same walk with flag 7 down spawns the record and
    // shows its box - the gate, not the seat, is what holds it shut.
    host.world.system_flag_clear(MAN_LOAD_FLAG);
    assert!(
        walk_on_opens_a_box(&mut host, (tx, tz)),
        "with flag 7 clear P2[{record}] at ({tx},{tz}) spawns and greets"
    );
}
