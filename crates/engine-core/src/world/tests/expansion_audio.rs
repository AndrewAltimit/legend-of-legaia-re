//! An expansion pack's soundscape on a disc-free world: a map's music and
//! its zones (cross-fading as the player walks between them), a pack track's
//! id, a boss phase's music swap and sounds, positional ambient sources (a
//! pack sound's levels, a retail cue's voice key / volume / release), the
//! victory music, and the identity with no pack.

use std::sync::Arc;

use super::*;
use legaia_engine_expansion::audio::{PACK_TRACK_BASE, is_pack_track};
use legaia_engine_expansion::map::ActiveMap;
use legaia_engine_field::field_events::FieldEvent;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::sound_state::SfxRingOp;
use legaia_engine_vm::sfx_emitter::EmitterOp;
use legaia_expansion_format::audio_clip::encode_wav;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const PACK_TOML: &str = r#"
id = "sound"
name = "Sound Pack"
version = "0.1.0"
"#;

const AUDIO_TOML: &str = r#"
[tracks.hum]
file = "audio/hum.wav"

[sounds.chime]
file = "audio/chime.wav"

[sounds.torch]
file = "audio/chime.wav"

[music]
victory = { kind = "bgm", id = 9 }
"#;

/// A 12 x 4 floor: the map plays bgm 24, the right half is a games-room
/// zone on bgm 15, the bottom-right corner a zone on the pack's own track;
/// a torch (pack sound) sits at (1, 1) and a retail cue at (10, 1).
const MAP_TOML: &str = r#"
name = "Sound Hall"
base = { scene = { kind = "scene", name = "town01" } }
bgm = { kind = "bgm", id = 24 }

[[spawns]]
id = "in"
tile = [1, 1]
[tiles]
origin = [0, 0]
rows = ["............", "............", "............", "............"]
[tiles.legend]
"." = {}

[audio]
fade_frames = 20

[[audio.zones]]
id = "corner"
rect = [10, 3, 11, 3]
music = { kind = "track", id = "hum" }

[[audio.zones]]
id = "games"
rect = [6, 0, 11, 3]
music = { kind = "bgm", id = 15 }

[[audio.ambient]]
id = "torch"
sound = { kind = "sound", id = "torch" }
tile = [1, 1]
radius = 512
inner = 64

[[audio.ambient]]
id = "water"
sound = { kind = "sfx", id = 0x200 }
tile = [10, 1]
radius = 384
"#;

const BATTLE_TOML: &str = r#"
[monsters.wolf]
base = { kind = "monster", id = 9 }
hp = 600

[monsters.wolf.script]
[[monsters.wolf.script.phases]]
moves = [{ attack = 0 }]

[[monsters.wolf.script.phases]]
when = { hp_below_pct = 50 }
music = { kind = "bgm", id = 61 }
sound = { kind = "sfx", id = 0x2E }

[[monsters.wolf.script.phases]]
when = { hp_below_pct = 20 }
moves = [{ attack = 0, charge = true, charge_sound = { kind = "sound", id = "chime" } }]

[formations.den]
monsters = ["wolf"]

[bosses.wolf_king]
formation = "den"
music = { kind = "track", id = "hum" }
"#;

fn pack() -> Arc<ExpansionPack> {
    let wav = encode_wav(22050, 1, &[1000; 2205], None);
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert("audio.toml".into(), AUDIO_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), BATTLE_TOML.as_bytes().to_vec());
    files.insert("maps/hall.toml".into(), MAP_TOML.as_bytes().to_vec());
    files.insert("assets/audio/hum.wav".into(), wav.clone());
    files.insert("assets/audio/chime.wav".into(), wav);
    let pack = ExpansionPack::from_files(&files).expect("the pack parses");
    let issues = legaia_expansion_format::validate::validate(&pack);
    assert!(
        !legaia_expansion_format::validate::has_errors(&issues),
        "{issues:?}"
    );
    Arc::new(pack)
}

/// A field world with the pack active, standing on the hall at `tile`.
fn on_map(tile: (u16, u16)) -> World {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    w.actors[0].active = true;
    w.activate_expansion(pack()).expect("the pack activates");
    let p = w.expansion.pack.clone().unwrap();
    let def = p.maps["hall"].clone();
    let grid = def.tile_grid().unwrap();
    w.expansion.map = Some(ActiveMap::new(
        "hall",
        Arc::new(def),
        Arc::new(grid),
        tile,
        |_| false,
    ));
    stand(&mut w, tile);
    w
}

fn stand(w: &mut World, (c, r): (u16, u16)) {
    let p = &mut w.actors[0].move_state;
    p.world_x = (i32::from(c) * 128 + 64) as i16;
    p.world_z = (i32::from(r) * 128 + 64) as i16;
}

/// The BGM events the world raised since the last call.
fn bgm_events(w: &mut World) -> Vec<(u16, u8)> {
    w.drain_field_events()
        .into_iter()
        .filter_map(|e| match e {
            FieldEvent::Bgm { text_id, sub_op } => Some((text_id, sub_op)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_map_starts_its_music_and_zones_cross_fade() {
    let mut w = on_map((1, 1));
    w.tick_expansion_audio();
    assert_eq!(
        bgm_events(&mut w),
        [(2024, 1)],
        "entry cuts to the map's music"
    );
    assert_eq!(w.audio.current_bgm, Some(2024));
    w.tick_expansion_audio();
    assert!(bgm_events(&mut w).is_empty(), "no change, no event");
    // Into the games room: a cross-fade over the map's fade.
    stand(&mut w, (7, 1));
    w.tick_expansion_audio();
    assert_eq!(bgm_events(&mut w), [(2015, BGM_SUB_OP_ENGINE_FADE)]);
    assert_eq!(w.expansion.audio.fade_frames, 20);
    assert_eq!(w.audio.current_bgm, Some(2015));
    // The corner zone is listed first and wins: the pack's own track.
    stand(&mut w, (10, 3));
    w.tick_expansion_audio();
    let ev = bgm_events(&mut w);
    assert_eq!(ev, [(PACK_TRACK_BASE, BGM_SUB_OP_ENGINE_FADE)]);
    assert!(is_pack_track(ev[0].0));
    // Back out: the map's own music again.
    stand(&mut w, (2, 2));
    w.tick_expansion_audio();
    assert_eq!(bgm_events(&mut w), [(2024, BGM_SUB_OP_ENGINE_FADE)]);
}

#[test]
fn a_script_pick_holds_over_the_zones_until_cleared() {
    let mut w = on_map((1, 1));
    w.tick_expansion_audio();
    let _ = bgm_events(&mut w);
    assert!(w.expansion_set_music(Some(&MusicRef::Bgm { id: 40 }), 10));
    assert_eq!(bgm_events(&mut w), [(2040, BGM_SUB_OP_ENGINE_FADE)]);
    stand(&mut w, (7, 1));
    w.tick_expansion_audio();
    assert!(bgm_events(&mut w).is_empty(), "the script's pick holds");
    assert!(w.expansion_set_music(None, 0));
    w.tick_expansion_audio();
    assert_eq!(bgm_events(&mut w), [(2015, BGM_SUB_OP_ENGINE_FADE)]);
    assert!(!w.expansion_set_music(Some(&MusicRef::Track { id: "nope".into() }), 0));
}

#[test]
fn leaving_the_map_stops_a_pack_track_nobody_replaced() {
    let mut w = on_map((10, 3));
    w.tick_expansion_audio();
    assert_eq!(bgm_events(&mut w), [(PACK_TRACK_BASE, 1)]);
    w.expansion.map = None;
    w.tick_expansion_audio();
    assert_eq!(bgm_events(&mut w), [(0, BGM_SUB_OP_ENGINE_STOP)]);
    assert_eq!(w.audio.current_bgm, None);
}

#[test]
fn ambient_sources_follow_the_listener() {
    let mut w = on_map((1, 1));
    w.tick_expansion_audio();
    let near = w.expansion_ambient_levels().to_vec();
    assert_eq!(near.len(), 1, "the torch, a pack sound");
    assert_eq!(near[0].sound, "torch");
    let (l, r) = near[0].pan;
    assert!(l > 0.99 && r > 0.99, "at the source: full and centred");
    // Two tiles to the right of the torch (screen-right at azimuth 0):
    // quieter, and louder on the left.
    stand(&mut w, (3, 1));
    w.tick_expansion_audio();
    let (l, r) = w.expansion_ambient_levels()[0].pan;
    assert!(l > r && l < 0.99 && l > 0.0, "{l} {r}");
    // Out of range: gone.
    stand(&mut w, (6, 3));
    w.tick_expansion_audio();
    assert!(w.expansion_ambient_levels().is_empty());
}

#[test]
fn a_retail_cue_source_keys_holds_and_releases_its_voice() {
    let mut w = on_map((10, 1));
    w.tick_expansion_audio();
    let ops = w.take_sfx_emitter_ops();
    assert!(
        matches!(
            ops[..],
            [EmitterOp::Key {
                cue: 0x200,
                voice: 23,
                ..
            }]
        ),
        "{ops:?}"
    );
    w.tick_expansion_audio();
    let ops = w.take_sfx_emitter_ops();
    assert!(
        matches!(ops[..], [EmitterOp::Volume { cue: 0x200, voice: 23, left, right }] if left > 0x3000 && right > 0x3000),
        "{ops:?}"
    );
    stand(&mut w, (1, 3));
    w.tick_expansion_audio();
    let ops = w.take_sfx_emitter_ops();
    assert_eq!(
        ops,
        [EmitterOp::Release {
            cue: 0x200,
            voice: 23
        }]
    );
    // A battle lets every keyed voice go.
    stand(&mut w, (10, 1));
    w.tick_expansion_audio();
    let _ = w.take_sfx_emitter_ops();
    w.mode = SceneMode::Battle;
    w.tick_expansion_audio();
    assert_eq!(
        w.take_sfx_emitter_ops(),
        [EmitterOp::Release {
            cue: 0x200,
            voice: 23
        }]
    );
    assert!(w.expansion_ambient_levels().is_empty());
}

#[test]
fn sounds_ride_the_ring_or_the_pack_queue() {
    let mut w = on_map((1, 1));
    let _ = w.take_sfx_ring_ops();
    assert!(w.expansion_play_sound(&SoundRef::Sfx { id: 0x21 }));
    assert_eq!(w.take_sfx_ring_ops(), [SfxRingOp::Push(0x21)]);
    assert!(w.expansion_play_sound(&SoundRef::Sound { id: "chime".into() }));
    assert!(w.expansion_play_sound(&SoundRef::Sound {
        id: "sound:torch".into()
    }));
    let s = w.take_expansion_sounds();
    assert_eq!(s.len(), 2);
    assert_eq!((s[0].key.as_str(), s[1].key.as_str()), ("chime", "torch"));
    assert!(!w.expansion_play_sound(&SoundRef::Sound { id: "nope".into() }));
}

#[test]
fn a_boss_takes_its_track_and_a_phase_swaps_the_music() {
    let mut w = on_map((1, 1));
    // The boss's own track is the pack's: its music id is the pack range.
    let c = w.expansion.battle.compiled.clone().unwrap();
    let b = &c.bosses["wolf_king"];
    assert_eq!(
        w.expansion_boss_track(b.music.as_ref()),
        Some(PACK_TRACK_BASE)
    );
    // A phase's music and sound as it begins.
    let _ = w.take_sfx_ring_ops();
    w.expansion_phase_audio(
        Some(&MusicRef::Bgm { id: 61 }),
        Some(&SoundRef::Sfx { id: 0x2E }),
    );
    assert_eq!(bgm_events(&mut w), [(2061, BGM_SUB_OP_ENGINE_FADE)]);
    assert_eq!(w.audio.current_bgm, Some(2061));
    assert_eq!(w.take_sfx_ring_ops(), [SfxRingOp::Push(0x2E)]);
    // The same music again does nothing.
    w.expansion_phase_audio(Some(&MusicRef::Bgm { id: 61 }), None);
    assert!(bgm_events(&mut w).is_empty());
}

#[test]
fn a_fight_on_the_map_ends_on_the_victory_music() {
    let mut w = on_map((1, 1));
    w.expansion_victory_music();
    assert_eq!(bgm_events(&mut w), [(2009, 1)]);
    // Off the pack's maps, a retail fight keeps retail's results.
    w.expansion.map = None;
    w.expansion_victory_music();
    assert!(bgm_events(&mut w).is_empty());
}

#[test]
fn with_no_pack_nothing_sounds() {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    w.actors[0].active = true;
    for _ in 0..30 {
        w.tick();
    }
    assert!(w.expansion_ambient_levels().is_empty());
    assert!(w.take_expansion_sounds().is_empty());
    assert!(!w.expansion_play_sound(&SoundRef::Sfx { id: 0x21 }));
    assert_eq!(w.expansion_music_id(&MusicRef::Bgm { id: 1 }), None);
    w.expansion_victory_music();
    assert!(
        !w.drain_field_events()
            .iter()
            .any(|e| matches!(e, FieldEvent::Bgm { .. }))
    );
}

use legaia_expansion_format::audio::{MusicRef, SoundRef};
