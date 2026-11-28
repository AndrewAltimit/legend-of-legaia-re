//! Disc-gated regression: **the Bio Castle's overworld door opens from the
//! west, and the castle's body is solid, exactly where retail puts both.**
//!
//! Once story flag `0x3BA` is up (`town0d` `P2[31]`, just before `MV4.STR`),
//! `map01` changes around Rim Elm's old site:
//!
//! - the system script (`P1[0]`, `0x00A2..0x00C1`) paints the castle's body
//!   into the collision grid with four op-`4C 7x` rectangles: tiles
//!   `(94..98, 25)`, `(95..97, 24..27)` and `(97, 23)` walled, `(96, 27)`
//!   cleared;
//! - Rim Elm's landmark (`P0[8]`, keyed on tile `(96, 21)`) moves to tile
//!   `(96, 25)` and wears pack model `30`, the castle;
//! - the walk-on trigger at tile `(95, 23)` runs `P2[37]`, whose `0x3F`
//!   enters `jou`, while the Rim Elm gate at `(96, 24)` (`P2[0]`) parks shut.
//!
//! The door tile sits on the castle's north-west corner, against the cliffs
//! of row 22, so it is reached only from the west: a walk from the south or
//! the east stops on the painted body. Every way back onto `map01` seats the
//! party on `(94, 23)`, the tile west of the door - the `jou` exit, the Door
//! of Light's region triple in every `jou*` room, and the Door of Wind's
//! Rim Elm record once `jou` has swapped its discovery flag (`0x20` off,
//! `0x33` on).
//!
//! A player report read the closed south and east sides as an offset
//! collider or warp tile. Retail is the same: a PCSX-Redux run of
//! `scripts/pcsx-redux/autorun_bio_castle_entrance.lua` from
//! `jouind_field_card_boot` (tile pokes out through `jouinc` / `jouinb` /
//! `jouina` / `jou` onto `map01`, then pad walks from poked seats) dumps a
//! collision grid identical to the port's over tiles `x 86..104, z 15..31`
//! and stops each pad walk below where the port stops it. Those rest
//! positions are pinned here.
//!
//! Both tests resume the playthrough card's `PRO-10` save onto `map01` with
//! the hosts' collision settings (leading-edge wall probes, solid NPCs).
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` / the save library.

use legaia_engine_save_state::WorldSaveState;
use std::path::{Path, PathBuf};

use legaia_engine_field::world_map_entity_config::WorldMapEntityConfig;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("extracted"));
    if d.join("PROT.DAT").is_file() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The SC block of the save named `save` on the library card `card`.
fn card_sc(lib: &Path, card: &str, save: &str) -> Option<Vec<u8>> {
    let mounted = legaia_save::emu::MountedCard::open(&lib.join("cards").join(card)).ok()?;
    (1..=15u8).find_map(|block| {
        let frame = mounted.dir_frame(block)?;
        let name: String = frame[0x0A..0x0A + 20]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if !name.ends_with(save) {
            return None;
        }
        mounted.sc_block(block).map(<[u8]>::to_vec)
    })
}

/// The castle door: `map01`'s walk-on trigger for `P2[37]`.
const DOOR_TILE: (i32, i32) = (95, 23);

fn player_pos(session: &NativeSession) -> (i32, i32) {
    let w = &session.host.world;
    let a = &w.actors[w.player_actor_slot.expect("player slot") as usize];
    (
        i32::from(a.move_state.world_x),
        i32::from(a.move_state.world_z),
    )
}

/// Resume `PRO-10` onto `map01` with the player seated on tile `seat`, under
/// the play hosts' collision settings.
fn seated(seat: (i32, i32)) -> Option<NativeSession> {
    let extracted = extracted_dir()?;
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("saves/library"));
    let Some(sc) = card_sc(&lib, "playthrough-endgame-7saves.mcr", "PRO-10") else {
        eprintln!("[skip] save library card PRO-10 missing");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let sf = legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
        .expect("lift SC block");
    session.host.world.load_full(sf.clone());
    session
        .host
        .set_entry_seat((seat.0 * 128 + 64) as i16, (seat.1 * 128 + 64) as i16);
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        battle_bgm: None,
    };
    let landing = session.resume_save(sf, "map01", &opts);
    assert!(landing.entered_scene(), "resume onto map01: {landing:?}");
    // Both play hosts run retail's three-probe wall footprint and solid NPCs.
    session.host.world.locomotion.leading_edge_wall_probes = true;
    session.host.world.npcs.solid = true;
    assert!(
        session.host.world.system_flag_test(0x3BA),
        "PRO-10 is past the fusion"
    );
    Some(session)
}

#[test]
fn the_castle_body_and_door_sit_where_the_disc_puts_them() {
    let Some(mut session) = seated((94, 23)) else {
        return;
    };
    for _ in 0..30 {
        session.host.world.set_pad(0);
        let _ = session.tick();
    }
    let w = &session.host.world;
    // The jou portal is the (95, 23) trigger and nothing else.
    let jou: Vec<(i32, i32)> = w
        .world_map
        .entity_configs
        .iter()
        .zip(w.world_map.entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == "jou" => {
                Some((i32::from(x) >> 7, i32::from(z) >> 7))
            }
            _ => None,
        })
        .collect();
    assert_eq!(jou, vec![DOOR_TILE], "the castle door is one trigger tile");
    // P0[8] stands two tiles south of its seat, wearing the castle.
    assert_eq!(
        w.object_draw_displacements().get(&8).map(|d| d[2]),
        Some(256),
        "Rim Elm's landmark moves to (96, 25)"
    );
    assert_eq!(w.object_live_models().get(&8), Some(&30), "castle model");
    // The system script's paint: the grid's wall nibble per tile, read at the
    // row the biased wall read uses for it (tile row + 1).
    let nibble = |tx: usize, tz: usize| w.terrain.collision_grid[(tz + 1) * 0x80 + tx] >> 4;
    let painted = (94..=98)
        .map(|x| (x, 25))
        .chain((95..=97).flat_map(|x| (24..=27).map(move |z| (x, z))))
        .chain([(97, 23)])
        .filter(|&t| t != (96, 27));
    for (x, z) in painted {
        assert_eq!(nibble(x, z), 0xF, "tile ({x}, {z}) is castle wall");
    }
    assert_eq!(nibble(96, 27), 0, "the system script clears (96, 27)");
    assert_eq!(nibble(95, 23), 0, "the door tile is open");
    assert_eq!(nibble(94, 23), 0, "the seat west of the door is open");
}

/// One retail pad walk: a poked seat, a held pad, and where retail ended up.
struct Walk {
    seat: (i32, i32),
    pad: u16,
    /// Vsyncs the pad is held (the probe's `LEGAIA_WALK_FOR`).
    hold: u32,
    end: End,
}

enum End {
    /// Crossed the door into `jou`.
    Enters,
    /// Came to rest here, in world units.
    Rests((i32, i32)),
    /// Still walking when the hold ran out, on this tile.
    OnTile((i32, i32)),
}

const DOWN: u16 = PadButton::Down as u16;
const LEFT: u16 = PadButton::Left as u16;
const RIGHT: u16 = PadButton::Right as u16;

/// The retail run's walks (`autorun_bio_castle_entrance.lua`, both passes).
/// Down is world `-Z` under the overworld's fixed camera; every rest is a
/// position retail held for over 200 vsyncs.
const RETAIL_WALKS: &[Walk] = &[
    // From the west along the door row: in.
    Walk {
        seat: (90, 23),
        pad: RIGHT,
        hold: 240,
        end: End::Enters,
    },
    // From the south-west, diagonally: in.
    Walk {
        seat: (91, 27),
        pad: DOWN | RIGHT,
        hold: 300,
        end: End::Enters,
    },
    // From the east along the door row: stopped by the (97, 23) wall.
    Walk {
        seat: (99, 23),
        pad: LEFT,
        hold: 240,
        end: End::Rests((12590, 3008)),
    },
    Walk {
        seat: (99, 24),
        pad: LEFT,
        hold: 300,
        end: End::Rests((12590, 3136)),
    },
    Walk {
        seat: (98, 27),
        pad: DOWN | LEFT,
        hold: 300,
        end: End::Rests((12590, 3374)),
    },
    // From the south-west, aimed at the body: stopped on its west face.
    Walk {
        seat: (93, 28),
        pad: DOWN | RIGHT,
        hold: 300,
        end: End::Rests((12114, 3374)),
    },
    // From behind the castle, straight at it: the wall slide carries the
    // party round the west side to the door row, one tile short of the door.
    Walk {
        seat: (95, 28),
        pad: DOWN,
        hold: 240,
        end: End::OnTile((93, 23)),
    },
];

#[test]
fn the_door_opens_from_the_west_and_the_body_stops_the_rest_as_in_retail() {
    for walk in RETAIL_WALKS {
        let Some(mut session) = seated(walk.seat) else {
            return;
        };
        let mut entered = false;
        for f in 0..30 + walk.hold {
            // Keep a random encounter out of the walk.
            if let Some(t) = session.host.world.world_map.region_tracker.as_mut() {
                t.set_counter(100);
            }
            let pad = if f >= 30 { walk.pad } else { 0 };
            session.host.world.set_pad(pad);
            let _ = session.tick();
            if session.host.world.scene_change_in_flight()
                || session.host.world.active_scene_label == "jou"
            {
                entered = true;
                break;
            }
            assert_eq!(
                session.host.world.mode,
                SceneMode::WorldMap,
                "walk from {:?} left the overworld",
                walk.seat
            );
        }
        let at = player_pos(&session);
        match walk.end {
            End::Enters => assert!(
                entered,
                "walk from {:?} (pad {:#x}) should cross the door; stopped at {at:?}",
                walk.seat, walk.pad
            ),
            End::Rests(rest) => {
                assert!(!entered, "walk from {:?} entered jou", walk.seat);
                assert_eq!(
                    at, rest,
                    "walk from {:?} (pad {:#x}) rests where retail does",
                    walk.seat, walk.pad
                );
            }
            End::OnTile(tile) => {
                assert!(!entered, "walk from {:?} entered jou", walk.seat);
                assert_eq!(
                    (at.0 >> 7, at.1 >> 7),
                    tile,
                    "walk from {:?} (pad {:#x}) ends on retail's tile; at {at:?}",
                    walk.seat,
                    walk.pad
                );
            }
        }
    }
}
