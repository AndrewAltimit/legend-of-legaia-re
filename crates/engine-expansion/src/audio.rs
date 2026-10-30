//! The runtime of a pack's soundscape (`audio.toml`, each map's `[audio]`):
//! which music a map, a zone, a mode or a boss phase plays, and how loud and
//! where each ambient source sounds from the listener.
//!
//! **Music ids.** Every music change rides the field's ordinary BGM event
//! (`FieldEvent::Bgm`), so the battle swap, the restore after a fight and
//! both hosts' BGM routing carry a pack track exactly as they carry a retail
//! cue. A retail cue is its global id (`2000 + sound-test id`); a pack track
//! takes an id of its own, [`PACK_TRACK_BASE`] plus its index in
//! `audio.toml`'s `tracks` (key order). The scene host resolves an id in
//! that range against the active pack and hands the clip to the director.
//!
//! **Positional ambient** is presentation only: the levels are a pure
//! function of the listener's position and the camera's azimuth, recomputed
//! each tick, and nothing in the simulation reads them back.

use std::collections::BTreeMap;

use legaia_expansion_format::audio::{AudioSection, ClipDef, MapAudio, MusicRef};
use legaia_expansion_format::battle::local_key;
use legaia_expansion_format::map::{MapDef, rect_contains};
use legaia_expansion_format::refs::DiscRef;

use crate::battle::BGM_GLOBAL_BASE;

/// The first music id a pack track takes.
pub const PACK_TRACK_BASE: u16 = 0xE000;
/// The most tracks a pack's ids can name.
pub const PACK_TRACK_LIMIT: u16 = 0x1000;

/// Whether `id` is a pack track's music id.
pub fn is_pack_track(id: u16) -> bool {
    (PACK_TRACK_BASE..PACK_TRACK_BASE + PACK_TRACK_LIMIT).contains(&id)
}

/// The music id of track `key` (local or `pack:key`) in `audio`.
pub fn track_id(audio: &AudioSection, pack_id: &str, key: &str) -> Option<u16> {
    let key = local_key(pack_id, key)?;
    let i = audio.tracks.keys().position(|k| k == key)?;
    u16::try_from(i)
        .ok()
        .filter(|&i| i < PACK_TRACK_LIMIT)
        .map(|i| PACK_TRACK_BASE + i)
}

/// The track a pack music id names: its key and definition.
pub fn track_for_id(audio: &AudioSection, id: u16) -> Option<(&str, &ClipDef)> {
    if !is_pack_track(id) {
        return None;
    }
    audio
        .tracks
        .iter()
        .nth(usize::from(id - PACK_TRACK_BASE))
        .map(|(k, c)| (k.as_str(), c))
}

/// What `m` plays: `Some(Some(id))` a track, `Some(None)` silence, `None` a
/// pack track the pack does not have.
pub fn music_id(audio: &AudioSection, pack_id: &str, m: &MusicRef) -> Option<Option<u16>> {
    match m {
        MusicRef::Bgm { id } => Some(Some(BGM_GLOBAL_BASE.saturating_add(*id))),
        MusicRef::Track { id } => track_id(audio, pack_id, id).map(Some),
        MusicRef::Silence => Some(None),
    }
}

/// The music a map plays with the player on map-local `tile`: the first
/// zone holding the tile, else the map's `[audio] music`, else its `bgm`.
/// `None` when the map names no music for that spot.
pub fn map_music_at(def: &MapDef, tile: Option<(u16, u16)>) -> Option<MusicRef> {
    if let Some((c, r)) = tile
        && let Some(z) = def
            .audio
            .zones
            .iter()
            .find(|z| rect_contains(&z.rect, c, r))
    {
        return Some(z.music.clone());
    }
    map_default_music(def)
}

/// The map's own music, zones aside.
pub fn map_default_music(def: &MapDef) -> Option<MusicRef> {
    def.audio.music.clone().or(match def.bgm {
        Some(DiscRef::Bgm { id }) => Some(MusicRef::Bgm { id }),
        _ => None,
    })
}

/// A clip a host plays: where to find its bytes and how.
#[derive(Debug, Clone, Copy)]
pub struct PackClip<'a> {
    /// The pack the clip is from; with `def.file`, the key of a host's
    /// decode cache.
    pub pack_id: &'a str,
    /// The file's bytes.
    pub bytes: &'a [u8],
    pub def: &'a ClipDef,
}

/// The clip of pack track `id` in `pack`.
pub fn track_clip(pack: &legaia_expansion_format::ExpansionPack, id: u16) -> Option<PackClip<'_>> {
    let (_, def) = track_for_id(&pack.audio, id)?;
    Some(PackClip {
        pack_id: &pack.manifest.id,
        bytes: pack.assets.get(&def.file)?,
        def,
    })
}

/// The clip of pack sound `key` (local) in `pack`.
pub fn sound_clip<'a>(
    pack: &'a legaia_expansion_format::ExpansionPack,
    key: &str,
) -> Option<PackClip<'a>> {
    let def = pack.audio.sounds.get(key)?;
    Some(PackClip {
        pack_id: &pack.manifest.id,
        bytes: pack.assets.get(&def.file)?,
        def,
    })
}

/// What the music slot is known to hold, as this runtime last set it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Applied {
    /// Nothing set yet (a map just entered, a mode just closed).
    #[default]
    Unset,
    /// This music (`None` = silence).
    Music(Option<u16>),
}

/// A pack sound to play this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundPlay {
    /// The `[sounds]` key.
    pub key: String,
    /// `(left, right)` level.
    pub pan: (f32, f32),
}

/// A pack-sound ambient source sounding this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbientLevel {
    /// Stable per source while its map stands.
    pub key: u32,
    /// The `[sounds]` key.
    pub sound: String,
    /// `(left, right)` level.
    pub pan: (f32, f32),
}

/// A retail-cue ambient source holding an SPU voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyedCue {
    pub cue: u16,
    pub voice: u8,
}

/// The `World`'s soundscape state.
#[derive(Debug, Clone, Default)]
pub struct AudioRuntime {
    /// What the runtime last put in the music slot.
    pub applied: Applied,
    /// The frames the next cross-fading start takes.
    pub fade_frames: u16,
    /// A cutscene's or script's music pick, winning over zones and the
    /// map until cleared (`World::expansion_set_music`).
    pub script_music: Option<Option<u16>>,
    /// The mode instance whose music is up, if any.
    pub mode: Option<String>,
    /// What played before the mode's music, to put back after.
    pub mode_resume: Option<Option<u16>>,
    /// Pack sounds raised since the host last took them.
    pub sounds: Vec<SoundPlay>,
    /// The pack-sound ambient sources sounding this tick.
    pub ambient: Vec<AmbientLevel>,
    /// The retail-cue ambient sources keyed, by source index.
    pub keyed: BTreeMap<usize, KeyedCue>,
    /// The map the keyed cues belong to.
    pub keyed_map: Option<String>,
}

impl AudioRuntime {
    /// Take the sounds raised since the last call.
    pub fn take_sounds(&mut self) -> Vec<SoundPlay> {
        std::mem::take(&mut self.sounds)
    }
}

/// The stereo level of a source at world `src` heard from `listener` with
/// the camera at 12-bit `azimuth` (`0` looks along `+Z`, screen-right `+X`):
/// full volume within `inner`, a smooth fall to silence at `radius`, and an
/// equal-power pan by where the source sits across the view. `None` past
/// `radius`.
pub fn ambient_level(
    src: (i32, i32),
    listener: (i32, i32),
    azimuth: u16,
    inner: u16,
    radius: u16,
    volume: f32,
) -> Option<(f32, f32)> {
    let dx = (src.0 - listener.0) as f32;
    let dz = (src.1 - listener.1) as f32;
    let d = (dx * dx + dz * dz).sqrt();
    let (inner, radius) = (f32::from(inner), f32::from(radius));
    if d >= radius {
        return None;
    }
    let gain = if d <= inner {
        1.0
    } else {
        let t = 1.0 - (d - inner) / (radius - inner).max(1.0);
        t * t
    } * volume.clamp(0.0, 1.0);
    if gain <= 0.0 {
        return None;
    }
    // Screen-right in world space (the camera's azimuth rotation of +X).
    let a = f32::from(azimuth & 0x0FFF) * std::f32::consts::TAU / 4096.0;
    let (right_x, right_z) = (a.cos(), -a.sin());
    // The pan narrows to the centre as the source reaches the listener.
    let across = if d > 0.0 {
        (dx * right_x + dz * right_z) / d * (d / inner.max(1.0)).min(1.0)
    } else {
        0.0
    };
    let theta = (across.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    let sq2 = std::f32::consts::SQRT_2;
    Some((
        gain * (sq2 * theta.cos()).min(1.0),
        gain * (sq2 * theta.sin()).min(1.0),
    ))
}

/// A source's world position on its map.
pub fn source_position(
    def: &MapDef,
    src: &legaia_expansion_format::audio::AmbientSource,
) -> (i32, i32) {
    let (x, z) = def.tile_centre(src.tile[0], src.tile[1]);
    (x + i32::from(src.offset[0]), z + i32::from(src.offset[1]))
}

/// The map's zone fade.
pub fn fade_frames(audio: &MapAudio) -> u16 {
    audio.fade()
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::audio::{MusicZone, SoundRef};

    fn section() -> AudioSection {
        toml::from_str(
            r#"
[tracks.b_second]
file = "b.wav"
[tracks.a_first]
file = "a.wav"
"#,
        )
        .unwrap()
    }

    #[test]
    fn track_ids_follow_key_order_and_round_trip() {
        let s = section();
        assert_eq!(track_id(&s, "p", "a_first"), Some(PACK_TRACK_BASE));
        assert_eq!(track_id(&s, "p", "p:b_second"), Some(PACK_TRACK_BASE + 1));
        assert_eq!(track_id(&s, "p", "other:a_first"), None);
        assert_eq!(track_for_id(&s, PACK_TRACK_BASE + 1).unwrap().0, "b_second");
        assert!(track_for_id(&s, PACK_TRACK_BASE + 2).is_none());
        assert!(track_for_id(&s, 2030).is_none());
        assert_eq!(
            music_id(&s, "p", &MusicRef::Bgm { id: 30 }),
            Some(Some(2030))
        );
        assert_eq!(music_id(&s, "p", &MusicRef::Silence), Some(None));
        assert_eq!(
            music_id(&s, "p", &MusicRef::Track { id: "nope".into() }),
            None
        );
    }

    #[test]
    fn a_zone_wins_over_the_map_and_the_map_over_bgm() {
        let mut def = MapDef {
            bgm: Some(DiscRef::Bgm { id: 24 }),
            ..Default::default()
        };
        assert_eq!(
            map_music_at(&def, Some((0, 0))),
            Some(MusicRef::Bgm { id: 24 })
        );
        def.audio.music = Some(MusicRef::Track { id: "t".into() });
        def.audio.zones.push(MusicZone {
            id: "games".into(),
            rect: [5, 5, 8, 8],
            music: MusicRef::Bgm { id: 15 },
        });
        assert_eq!(
            map_music_at(&def, Some((6, 7))),
            Some(MusicRef::Bgm { id: 15 })
        );
        assert_eq!(
            map_music_at(&def, Some((1, 1))),
            Some(MusicRef::Track { id: "t".into() })
        );
        assert_eq!(
            map_music_at(&def, None),
            Some(MusicRef::Track { id: "t".into() })
        );
        let _ = SoundRef::Sfx { id: 1 };
    }

    #[test]
    fn ambient_falls_off_with_distance_and_pans_across_the_view() {
        let full = ambient_level((0, 0), (0, 0), 0, 128, 1024, 1.0).unwrap();
        assert!((full.0 - full.1).abs() < 1e-6, "centred at the source");
        assert!(full.0 > 0.99);
        // Off to screen-right (+X at azimuth 0), inside `inner`: louder right.
        let right = ambient_level((100, 0), (0, 0), 0, 128, 1024, 1.0).unwrap();
        assert!(right.1 > right.0);
        // Half way out: quieter than inside, louder than near the edge.
        let mid = ambient_level((0, 576), (0, 0), 0, 128, 1024, 1.0).unwrap();
        let edge = ambient_level((0, 1000), (0, 0), 0, 128, 1024, 1.0).unwrap();
        assert!(mid.0 < full.0 && edge.0 < mid.0 && edge.0 > 0.0);
        assert!((mid.0 - mid.1).abs() < 1e-4, "straight ahead is centred");
        assert!(ambient_level((0, 1024), (0, 0), 0, 128, 1024, 1.0).is_none());
        // The camera turned half way round: screen-right is now -X.
        let turned = ambient_level((300, 0), (0, 0), 2048, 128, 1024, 1.0).unwrap();
        assert!(turned.0 > turned.1, "{turned:?}");
        // Volume scales both sides.
        let quiet = ambient_level((0, 0), (0, 0), 0, 128, 1024, 0.5).unwrap();
        assert!((quiet.0 - full.0 * 0.5).abs() < 1e-6);
    }
}
