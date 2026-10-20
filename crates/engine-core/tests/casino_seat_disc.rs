//! Disc-gated: the **casino seat** - a `koin1` cabinet's interaction walks the
//! player onto the stool tile and turns them to the machine before the warp.
//!
//! Each of the six cabinet records (`koin1` P1[51..=53] Baka Fighter,
//! P1[54..=56] slot machine) opens its interaction section the same way:
//!
//! ```text
//! A2 F8 04          ExecMove on the player (the walk clip)
//! C7 F8 tx tz 33    op 0x47 walk-to-tile on the player   -> parks the record
//! AB F8 03 / 4A 14  clip hold, 20-frame wait
//! B8 F8 8d 17       op 0x38 turn to compass d over 23 frames -> parks
//! A2 F8 05          the sit clip
//! ... 4E coin check, 4C E5 debit, fade, 3E 67|68 warp
//! ```
//!
//! `FUN_8003C83C` resolves the `0xF8` target byte to the player, so both legs
//! move and turn the **player**, and only a player target parks the calling
//! record (`0x801DF034..0x801DF044`, `0x801DEEFC`). The talk runner used to
//! step both ops on the talker's own context: the player stayed wherever the
//! action button found them and played the sit clip there, off the stool.
//!
//! Retail anchor: the `baka_fighter_entry_pretransition` PCSX-Redux state
//! (Vahn taking the seat at P1[52]) holds the player at `(2752, 5952)`,
//! which is P1[52]'s `C7 F8 15 2E 33` tile `(0x15, 0x2E)` decoded by the
//! `0x47` arm (`tile * 0x80 + 0x40`).
//!
//! Skips + passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md
//! disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::world::World;
use legaia_engine_field::man_field_scripts::{PlacementKind, classify_placements};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::LinearWalker;

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    Some(Arc::new(
        ProtIndex::open_extracted(&extracted).expect("open ProtIndex"),
    ))
}

/// The seat a cabinet record names: the decoded `C7 F8` tile and the
/// `B8 F8` compass index, read off the record's own bytes.
struct Seat {
    slot: u8,
    tile: (i16, i16),
    compass: u8,
}

fn seat_of(body: &[u8], entry_pc: usize) -> Option<((i16, i16), u8)> {
    let mut tile = None;
    for insn in LinearWalker::new(body, entry_pc) {
        let Ok(insn) = insn else { break };
        let pc = insn.pc;
        match (body.get(pc), body.get(pc + 1)) {
            (Some(0xC7), Some(0xF8)) if tile.is_none() => {
                // The `0x47` arm's decode: a set high bit lands on the
                // tile's far edge (`+ 0x80` instead of `+ 0x40`).
                let decode =
                    |b: u8| i16::from(b & 0x7F) * 0x80 + if b & 0x80 != 0 { 0x80 } else { 0x40 };
                tile = Some((decode(body[pc + 2]), decode(body[pc + 3])));
            }
            (Some(0xB8), Some(0xF8)) if tile.is_some() => {
                return Some((tile?, body[pc + 2] & 0xF));
            }
            _ => {}
        }
        if body.get(pc) == Some(&0x3E) {
            break;
        }
    }
    None
}

#[test]
fn a_casino_cabinet_seats_the_player_on_its_stool() {
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "koin1").expect("koin1 loads");
    let man = scene
        .field_man_payload(&index)
        .expect("koin1 MAN reads")
        .expect("koin1 carries a MAN");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse MAN");

    let mut seats = Vec::new();
    for (p, kind) in classify_placements(&mf, &man) {
        // Sub-id 3 (slot machine) and 4 (Baka Fighter); the dome clerk's
        // walk (P1[9], sub-id 5) is a different choreography.
        let PlacementKind::Portal { target_map } = kind else {
            continue;
        };
        if !matches!(target_map, 3 | 4) {
            continue;
        }
        let Some(rec) =
            legaia_engine_field::man_field_scripts::placement_interaction_record(&mf, &man, &p)
        else {
            continue;
        };
        if let Some((tile, compass)) = seat_of(&rec.body, rec.entry_pc) {
            seats.push(Seat {
                slot: p.index as u8,
                tile,
                compass,
            });
        }
    }
    assert_eq!(
        seats.len(),
        6,
        "koin1 carries three Baka Fighter and three slot-machine seats"
    );
    // The retail anchor (see the module doc).
    assert!(
        seats.iter().any(|s| s.tile == (2752, 5952)),
        "P1[52]'s stool tile decodes to the retail pretransition position"
    );

    for seat in &seats {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_carriers_from_man(&mf, &man);
        world.install_field_player(0);
        world.toggles.use_vm_dialogue = true;
        world.minigames.casino_coins = 50;
        // Stand the player a tile and a half off the stool, facing away, the
        // way an action press from the cabinet's side finds them.
        let start = (seat.tile.0 + 0xC0, seat.tile.1 + 0xC0);
        if let Some(p) = world.player_actor_slot
            && let Some(a) = world.actors.get_mut(p as usize)
        {
            a.move_state.world_x = start.0;
            a.move_state.world_z = start.1;
            a.move_state.render_26 = 0;
        }
        world.trigger_field_interact(0, seat.slot);
        let mut armed = None;
        // No presses: with coins in the bank the seat runs to its warp with
        // no box, and a Cross would re-probe whichever placement stands
        // nearest the start (a slot-row patron, for P1[54]).
        for _ in 0..2400 {
            world.set_pad(0);
            let _ = world.tick();
            if let Some(w) = world.minigames.pending_warp {
                armed = Some(w);
                break;
            }
        }
        let (x, z, yaw) = world
            .player_actor_slot
            .and_then(|p| world.actors.get(p as usize))
            .map(|a| {
                (
                    a.move_state.world_x,
                    a.move_state.world_z,
                    a.move_state.render_26,
                )
            })
            .expect("a player actor");
        eprintln!(
            "[seat] koin1 P1[{}] tile={:?} compass={} -> player=({x},{z}) yaw=0x{:03X} warp={armed:?}",
            seat.slot,
            seat.tile,
            seat.compass,
            yaw & 0xFFF
        );
        assert!(
            armed.is_some(),
            "P1[{}]: the seat never reached its warp",
            seat.slot
        );
        assert_eq!(
            (x, z),
            seat.tile,
            "P1[{}]: the warp fired with the player off the stool tile",
            seat.slot
        );
        let want =
            legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(seat.compass)
                .expect("compass index");
        assert_eq!(
            yaw & 0xFFF,
            want & 0xFFF,
            "P1[{}]: the player does not face the cabinet",
            seat.slot
        );
    }
}
