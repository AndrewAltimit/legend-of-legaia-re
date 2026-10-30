//! `audio.toml` and the map `[audio]` table: the pack's soundscape.
//!
//! A pack plays two kinds of sound. **Retail** sound is named by
//! coordinate, exactly as the rest of a pack names disc content: a music
//! cue by its sound-test id (`{ kind = "bgm", id = 30 }`) and a sound
//! effect by its cue id (`{ kind = "sfx", id = 0x21 }`). **Modder** sound is
//! the pack's own WAV / Ogg Vorbis files under `assets/`, declared here as
//! `tracks` (music, with loop points) and `sounds` (stingers, effects,
//! ambient loops) and named by key (`{ kind = "track", id = "tower_hum" }`,
//! `{ kind = "sound", id = "chime" }`).
//!
//! ```toml
//! [tracks.tower_hum]
//! file = "audio/tower_hum.wav"
//! loop_start = 22050          # sample frames; the file's own loop, else 0
//! loop_end = 441000           # exclusive; the file's own loop, else its end
//! gain = 0.9
//!
//! [sounds.chime]
//! file = "audio/chime.wav"
//!
//! [music]
//! battle = { kind = "bgm", id = 26 }          # fights on the pack's maps
//! boss = { kind = "bgm", id = 28 }            # a boss with no `music`
//! victory = { kind = "track", id = "fanfare" } # the results screen
//! [music.modes]
//! poker = { kind = "bgm", id = 15 }           # by mode instance id or kind
//! ```
//!
//! What a map plays is its own `[audio]` table ([`MapAudio`]): a track for
//! the whole map, music zones that cross-fade as the player walks between
//! them, and positional ambient sources. The format page is
//! `docs/formats/expansion-pack-audio.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::map::TileRect;
use crate::refs::{BGM_COUNT, DiscRef};

/// What plays as music: a retail cue, a pack track, or nothing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MusicRef {
    /// A retail music cue by its sound-test id (`0..81`), the id space of
    /// [`DiscRef::Bgm`].
    Bgm { id: u16 },
    /// A pack track (`[tracks.<id>]`).
    Track { id: String },
    /// No music.
    Silence,
}

impl MusicRef {
    /// The retail coordinate a [`MusicRef::Bgm`] is.
    pub fn disc_ref(&self) -> Option<DiscRef> {
        match self {
            MusicRef::Bgm { id } => Some(DiscRef::Bgm { id: *id }),
            _ => None,
        }
    }
}

impl std::fmt::Display for MusicRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MusicRef::Bgm { id } => write!(f, "bgm {id}"),
            MusicRef::Track { id } => write!(f, "track {id}"),
            MusicRef::Silence => write!(f, "silence"),
        }
    }
}

/// A sound effect: a retail cue or a pack sound.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SoundRef {
    /// A retail sound-effect cue: a static-table id (`0..0x100`,
    /// `docs/formats/sfx-table.md`) or a runtime-bank id (`0x200..`) of the
    /// map's base scene.
    Sfx { id: u16 },
    /// A pack sound (`[sounds.<id>]`).
    Sound { id: String },
}

impl std::fmt::Display for SoundRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoundRef::Sfx { id } => write!(f, "sfx 0x{id:03X}"),
            SoundRef::Sound { id } => write!(f, "sound {id}"),
        }
    }
}

/// The highest static-table cue id a pack may name (the table holds
/// `0..0x100`); runtime-bank ids start at [`RUNTIME_CUE_BASE`].
pub const STATIC_CUE_END: u16 = 0x100;
/// The first runtime-bank cue id (`docs/formats/bse-dat.md`).
pub const RUNTIME_CUE_BASE: u16 = 0x200;
/// The last runtime-bank cue id accepted.
pub const RUNTIME_CUE_END: u16 = 0x2FF;

/// One of the pack's own audio files, as a track or a sound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipDef {
    /// The file, relative to `assets/` (`audio/tower_hum.ogg`).
    pub file: String,
    /// Linear gain on top of normalisation, `0.0..=4.0` (default `1`).
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub gain: f32,
    /// Bring the clip to the engine's reference loudness
    /// ([`REFERENCE_RMS`]) before `gain`. Default on.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub normalize: bool,
    /// Loop: a track loops unless `false`; a sound plays once unless
    /// `true` (an ambient source loops its sound whatever this says).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<bool>,
    /// The loop's first frame. Defaults to the file's own loop start
    /// (a WAV `smpl` loop, an Ogg `LOOPSTART` comment), else `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_start: Option<u32>,
    /// One past the loop's last frame. Defaults to the file's own loop
    /// end, else the clip's end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_end: Option<u32>,
}

fn unity() -> f32 {
    1.0
}
fn is_unity(v: &f32) -> bool {
    *v == 1.0
}
fn yes() -> bool {
    true
}
fn is_yes(v: &bool) -> bool {
    *v
}

/// The loudness [`ClipDef::normalize`] brings a clip to: an RMS of this
/// fraction of full scale (about -23 dBFS), the level of the retail field
/// scores through the port's mixer
/// (`engine-session/tests/pack_audio_levels_disc.rs` measures it).
pub const REFERENCE_RMS: f32 = 0.07;
/// The most normalisation will raise or lower a clip, as a factor.
pub const MAX_NORMALIZE: f32 = 4.0;
/// The largest `gain` accepted.
pub const MAX_GAIN: f32 = 4.0;

impl ClipDef {
    /// The gain normalisation gives a clip whose RMS is `rms`: towards
    /// [`REFERENCE_RMS`], within `1 / MAX_NORMALIZE ..= MAX_NORMALIZE`; unity
    /// when normalisation is off or the clip is silent.
    pub fn normalize_gain(&self, rms: f32) -> f32 {
        if !self.normalize || rms <= 1e-6 {
            return 1.0;
        }
        (REFERENCE_RMS / rms).clamp(1.0 / MAX_NORMALIZE, MAX_NORMALIZE)
    }

    /// The loop `[start, end)` over a clip of `frames` frames whose file
    /// carries `own` - `None` when the clip plays once.
    pub fn loop_region(
        &self,
        frames: u32,
        own: Option<(u32, u32)>,
        default_loop: bool,
    ) -> Option<(u32, u32)> {
        if !self.r#loop.unwrap_or(default_loop) {
            return None;
        }
        let start = self.loop_start.or(own.map(|o| o.0)).unwrap_or(0);
        let end = self
            .loop_end
            .or(own.map(|o| o.1))
            .unwrap_or(frames)
            .min(frames);
        (end > start).then_some((start, end))
    }
}

/// Pack-wide music choices (`[music]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicSection {
    /// The battle theme of every fight on the pack's maps a boss does not
    /// score itself. Unset keeps the retail battle theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battle: Option<MusicRef>,
    /// The track of a pack boss with no `music` of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boss: Option<MusicRef>,
    /// What the results screen of a fight on the pack's maps plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub victory: Option<MusicRef>,
    /// What the results screen of a pack boss plays; `victory` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boss_victory: Option<MusicRef>,
    /// Music while a game mode runs, keyed by mode instance id
    /// (`modes.toml`) or by mode kind (`poker`, `card_duel`, `golf`,
    /// `race`, `prop_hunt`, ...). An instance key wins over its kind.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modes: BTreeMap<String, MusicRef>,
}

/// `audio.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSection {
    /// Music files, keyed by track id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<String, ClipDef>,
    /// Effect and ambient files, keyed by sound id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sounds: BTreeMap<String, ClipDef>,
    #[serde(default, skip_serializing_if = "is_default_music")]
    pub music: MusicSection,
}

fn is_default_music(m: &MusicSection) -> bool {
    *m == MusicSection::default()
}

/// The mode kinds `[music.modes]` may key on, beside instance ids.
pub const MODE_KIND_KEYS: &[&str] = &[
    "poker",
    "card_duel",
    "golf",
    "race",
    "prop_hunt",
    "party_night",
];

/// The frames a music zone change takes to cross-fade by default.
pub const DEFAULT_FADE_FRAMES: u16 = 45;
/// The longest fade accepted (ten seconds).
pub const MAX_FADE_FRAMES: u16 = 600;
/// The most ambient sources a map may place.
pub const MAX_AMBIENT_SOURCES: usize = 16;
/// The most retail-cue ambient sources a map may place: each holds an SPU
/// voice of its own ([`AMBIENT_CUE_VOICES`]).
pub const MAX_AMBIENT_CUES: usize = 4;
/// The SPU voices retail-cue ambient sources key, one each from the top.
pub const AMBIENT_CUE_VOICES: [u8; MAX_AMBIENT_CUES] = [23, 22, 21, 20];

/// A map's `[audio]` table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MapAudio {
    /// The map's music; wins over the map's `bgm` when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub music: Option<MusicRef>,
    /// Frames a zone change cross-fades over ([`DEFAULT_FADE_FRAMES`] when
    /// unset).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade_frames: Option<u16>,
    /// Tile rectangles with music of their own, the first match winning.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<MusicZone>,
    /// Positional loops.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ambient: Vec<AmbientSource>,
}

impl MapAudio {
    /// Whether the table sets nothing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The zone fade, in frames.
    pub fn fade(&self) -> u16 {
        self.fade_frames
            .unwrap_or(DEFAULT_FADE_FRAMES)
            .min(MAX_FADE_FRAMES)
    }
}

/// A music zone: inside `rect` the map plays `music`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicZone {
    #[serde(default)]
    pub id: String,
    pub rect: TileRect,
    pub music: MusicRef,
}

/// A positional ambient loop: full volume within `inner` world units of its
/// point, falling to silence at `radius`, panned by where it sits across the
/// camera's view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientSource {
    #[serde(default)]
    pub id: String,
    pub sound: SoundRef,
    /// The map-local tile it sits on (its centre).
    pub tile: [u16; 2],
    /// World offset from the tile centre, `[x, z]`.
    #[serde(default, skip_serializing_if = "is_zero2")]
    pub offset: [i16; 2],
    /// Where it falls silent, in world units (a tile is `128`).
    #[serde(default = "default_radius")]
    pub radius: u16,
    /// Within this distance it plays at full volume.
    #[serde(default = "default_inner")]
    pub inner: u16,
    /// Its level at full volume, `0.0..=1.0`.
    #[serde(default = "unity")]
    pub volume: f32,
}

fn is_zero2(v: &[i16; 2]) -> bool {
    *v == [0, 0]
}
fn default_radius() -> u16 {
    768
}
fn default_inner() -> u16 {
    128
}

/// Check a [`MusicRef`] at `field`: a bgm id the sound test lists, a track
/// the pack declares.
pub fn check_music(cx: &mut crate::validate::Checker<'_>, field: &str, m: &MusicRef) {
    match m {
        MusicRef::Bgm { id } => cx.disc_ref(field, &DiscRef::Bgm { id: *id }),
        MusicRef::Track { id } => cx.reference("track", id, field),
        MusicRef::Silence => {}
    }
}

/// Check a [`SoundRef`] at `field`: a cue id in the static or runtime range,
/// a sound the pack declares.
pub fn check_sound(cx: &mut crate::validate::Checker<'_>, field: &str, s: &SoundRef) {
    match s {
        SoundRef::Sfx { id } => {
            if !(*id < STATIC_CUE_END || (RUNTIME_CUE_BASE..=RUNTIME_CUE_END).contains(id)) {
                cx.error(
                    field,
                    format!(
                        "cue 0x{id:03X} is outside the static table (0x000..0x{STATIC_CUE_END:03X}) \
                         and the runtime bank (0x{RUNTIME_CUE_BASE:03X}..=0x{RUNTIME_CUE_END:03X})"
                    ),
                );
            }
        }
        SoundRef::Sound { id } => cx.reference("sound", id, field),
    }
}

fn check_clip(cx: &mut crate::validate::Checker<'_>, at: &str, c: &ClipDef, default_loop: bool) {
    let path = format!("assets/{}", c.file);
    if !(c.gain.is_finite() && (0.0..=MAX_GAIN).contains(&c.gain)) {
        cx.error(
            &format!("{at}.gain"),
            format!("gain {} is outside 0..={MAX_GAIN}", c.gain),
        );
    }
    let lower = c.file.to_ascii_lowercase();
    if !(lower.ends_with(".wav") || lower.ends_with(".ogg")) {
        cx.error(&format!("{at}.file"), "an audio file is .wav or .ogg");
        return;
    }
    let Some(probed) = cx
        .pack()
        .assets
        .get(&c.file)
        .map(|b| crate::audio_clip::probe(b))
    else {
        cx.error(&format!("{at}.file"), format!("{path} is not in the pack"));
        return;
    };
    match probed {
        Err(e) => cx.error(&format!("{at}.file"), format!("{path}: {e:#}")),
        Ok(info) => {
            if let (Some(s), Some(e)) = (c.loop_start, c.loop_end)
                && e <= s
            {
                cx.error(&format!("{at}.loop_end"), "the loop ends before it starts");
            }
            for (name, v) in [("loop_start", c.loop_start), ("loop_end", c.loop_end)] {
                if let Some(v) = v
                    && v > info.frames
                {
                    cx.error(
                        &format!("{at}.{name}"),
                        format!("frame {v} is past the clip's end ({} frames)", info.frames),
                    );
                }
            }
            if c.loop_region(info.frames, info.loop_points, default_loop)
                .is_some_and(|(s, e)| e - s < info.sample_rate / 20)
            {
                cx.warning(
                    &format!("{at}.loop_start"),
                    "the loop is shorter than 50 ms; it will buzz",
                );
            }
        }
    }
}

impl crate::validate::SectionRules for AudioSection {
    fn declare(&self, cx: &mut crate::validate::Checker<'_>) {
        for key in self.tracks.keys() {
            cx.declare("track", key, &format!("tracks.{key}"));
        }
        for key in self.sounds.keys() {
            cx.declare("sound", key, &format!("sounds.{key}"));
        }
    }

    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        for (key, c) in &self.tracks {
            check_clip(cx, &format!("tracks.{key}"), c, true);
        }
        for (key, c) in &self.sounds {
            check_clip(cx, &format!("sounds.{key}"), c, false);
        }
        let m = &self.music;
        for (name, v) in [
            ("battle", &m.battle),
            ("boss", &m.boss),
            ("victory", &m.victory),
            ("boss_victory", &m.boss_victory),
        ] {
            if let Some(v) = v {
                check_music(cx, &format!("music.{name}"), v);
            }
        }
        for (key, v) in &m.modes {
            let known = cx.pack().modes.has_instance(key);
            if !known && !MODE_KIND_KEYS.contains(&key.as_str()) {
                cx.warning(
                    &format!("music.modes.{key}"),
                    format!("{key:?} is neither a mode instance in modes.toml nor a mode kind"),
                );
            }
            check_music(cx, &format!("music.modes.{key}"), v);
        }
    }
}

/// The map `[audio]` rules, called from the map's own.
pub(crate) fn check_map_audio(
    a: &MapAudio,
    cx: &mut crate::validate::Checker<'_>,
    grid: Option<&crate::map::TileGrid>,
) {
    let on =
        |c: u16, r: u16| grid.is_none_or(|g| usize::from(c) < g.width && usize::from(r) < g.height);
    if let Some(m) = &a.music {
        check_music(cx, "audio.music", m);
    }
    if a.fade_frames.is_some_and(|f| f > MAX_FADE_FRAMES) {
        cx.error(
            "audio.fade_frames",
            format!("a fade runs at most {MAX_FADE_FRAMES} frames"),
        );
    }
    for (i, z) in a.zones.iter().enumerate() {
        let at = format!("audio.zones[{i}]");
        check_music(cx, &format!("{at}.music"), &z.music);
        if !on(z.rect[0], z.rect[1]) || !on(z.rect[2], z.rect[3]) {
            cx.error(&format!("{at}.rect"), "the rectangle runs off the map");
        }
    }
    if a.ambient.len() > MAX_AMBIENT_SOURCES {
        cx.error(
            "audio.ambient",
            format!(
                "{} sources; a map places at most {MAX_AMBIENT_SOURCES}",
                a.ambient.len()
            ),
        );
    }
    let cues = a
        .ambient
        .iter()
        .filter(|s| matches!(s.sound, SoundRef::Sfx { .. }))
        .count();
    if cues > MAX_AMBIENT_CUES {
        cx.error(
            "audio.ambient",
            format!(
                "{cues} retail-cue sources; each keys an SPU voice of its own and a map has \
                 {MAX_AMBIENT_CUES}"
            ),
        );
    }
    for (i, s) in a.ambient.iter().enumerate() {
        let at = format!("audio.ambient[{i}]");
        check_sound(cx, &format!("{at}.sound"), &s.sound);
        if !on(s.tile[0], s.tile[1]) {
            cx.error(
                &format!("{at}.tile"),
                format!("tile {:?} is off the map", s.tile),
            );
        }
        if s.radius == 0 || s.inner >= s.radius {
            cx.error(
                &format!("{at}.radius"),
                "the radius must be greater than zero and than `inner`",
            );
        }
        if !(s.volume.is_finite() && (0.0..=1.0).contains(&s.volume)) {
            cx.error(&format!("{at}.volume"), "volume is 0.0..=1.0");
        }
    }
}

/// The sound-test id range a [`MusicRef::Bgm`] may use, for UI listings.
pub const BGM_IDS: std::ops::Range<u16> = 0..BGM_COUNT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_clip::encode_wav;
    use crate::validate::{Severity, validate};
    use crate::{ExpansionPack, PackFiles};

    const PACK: &str = "id = \"a\"\nname = \"A\"\nversion = \"0.1.0\"\n";

    fn pack(audio: &str, map: Option<&str>) -> ExpansionPack {
        let mut files = PackFiles::new();
        files.insert("pack.toml".into(), PACK.as_bytes().to_vec());
        files.insert("audio.toml".into(), audio.as_bytes().to_vec());
        files.insert(
            "assets/audio/one.wav".into(),
            encode_wav(22050, 2, &vec![500; 22050 * 2], Some((100, 20000))),
        );
        if let Some(m) = map {
            files.insert("maps/m.toml".into(), m.as_bytes().to_vec());
        }
        ExpansionPack::from_files(&files).unwrap()
    }

    fn errors(p: &ExpansionPack) -> Vec<String> {
        validate(p)
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| format!("{} {}", i.field, i.message))
            .collect()
    }

    #[test]
    fn a_clean_section_validates_and_round_trips() {
        let p = pack(
            r#"
[tracks.t]
file = "audio/one.wav"
[sounds.s]
file = "audio/one.wav"
gain = 0.5
[music]
battle = { kind = "track", id = "t" }
victory = { kind = "silence" }
[music.modes]
poker = { kind = "bgm", id = 18 }
"#,
            None,
        );
        assert!(errors(&p).is_empty(), "{:?}", errors(&p));
        let back = ExpansionPack::from_files(&p.to_files().unwrap()).unwrap();
        assert_eq!(back.audio, p.audio);
    }

    #[test]
    fn bad_files_loops_and_refs_are_errors() {
        let p = pack(
            r#"
[tracks.gone]
file = "audio/missing.wav"
[tracks.mp3]
file = "audio/one.mp3"
[tracks.loopy]
file = "audio/one.wav"
loop_start = 900
loop_end = 800
[tracks.long]
file = "audio/one.wav"
loop_end = 99999
gain = 9.0
[music]
boss = { kind = "track", id = "nope" }
battle = { kind = "bgm", id = 200 }
"#,
            None,
        );
        let e = errors(&p);
        let has = |s: &str| e.iter().any(|x| x.contains(s));
        assert!(
            has("tracks.gone.file assets/audio/missing.wav is not in the pack"),
            "{e:?}"
        );
        assert!(
            has("tracks.mp3.file an audio file is .wav or .ogg"),
            "{e:?}"
        );
        assert!(
            has("tracks.loopy.loop_end the loop ends before it starts"),
            "{e:?}"
        );
        assert!(
            has("tracks.long.loop_end frame 99999 is past the clip's end"),
            "{e:?}"
        );
        assert!(has("tracks.long.gain"), "{e:?}");
        assert!(has("music.boss track \"nope\" is not defined"), "{e:?}");
        assert!(has("music.battle bgm id 200"), "{e:?}");
    }

    #[test]
    fn map_audio_rules() {
        let map = r#"
base = { scene = { kind = "scene", name = "town01" } }
[[spawns]]
id = "in"
tile = [0, 0]
[tiles]
rows = ["....", "...."]
[tiles.legend]
"." = {}
[audio]
music = { kind = "track", id = "t" }
fade_frames = 900
[[audio.zones]]
rect = [0, 0, 9, 1]
music = { kind = "bgm", id = 3 }
[[audio.ambient]]
sound = { kind = "sfx", id = 0x150 }
tile = [1, 1]
[[audio.ambient]]
sound = { kind = "sound", id = "s" }
tile = [1, 1]
radius = 100
inner = 200
volume = 2.0
[[triggers]]
rect = [0, 0, 0, 0]
event = { kind = "sound", sound = { kind = "sound", id = "nope" } }
"#;
        let p = pack(
            "[tracks.t]\nfile = \"audio/one.wav\"\n[sounds.s]\nfile = \"audio/one.wav\"\n",
            Some(map),
        );
        let e = errors(&p);
        let has = |s: &str| e.iter().any(|x| x.contains(s));
        assert!(has("audio.fade_frames"), "{e:?}");
        assert!(has("audio.zones[0].rect"), "{e:?}");
        assert!(has("audio.ambient[0].sound cue 0x150"), "{e:?}");
        assert!(has("audio.ambient[1].radius"), "{e:?}");
        assert!(has("audio.ambient[1].volume"), "{e:?}");
        assert!(has("triggers[0].event.sound sound \"nope\""), "{e:?}");
        assert!(!has("audio.music"), "the track is declared: {e:?}");
    }

    #[test]
    fn loop_region_takes_the_files_own_then_the_defs() {
        let mut d: ClipDef = toml::from_str("file = \"x.wav\"").unwrap();
        assert_eq!(d.loop_region(1000, None, true), Some((0, 1000)));
        assert_eq!(d.loop_region(1000, Some((10, 900)), true), Some((10, 900)));
        assert_eq!(d.loop_region(1000, None, false), None, "a sound plays once");
        d.loop_end = Some(500);
        assert_eq!(d.loop_region(1000, Some((10, 900)), true), Some((10, 500)));
        d.r#loop = Some(false);
        assert_eq!(d.loop_region(1000, None, true), None);
        let n = ClipDef {
            normalize: true,
            ..d.clone()
        };
        assert!((n.normalize_gain(REFERENCE_RMS / 2.0) - 2.0).abs() < 1e-6);
        assert_eq!(n.normalize_gain(0.0001), MAX_NORMALIZE);
        assert_eq!(n.normalize_gain(0.0), 1.0, "silence stays silent");
    }
}
