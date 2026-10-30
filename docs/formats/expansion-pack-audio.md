# Expansion pack: audio

`audio.toml` and each map's `[audio]` table give an [expansion pack](expansion-pack.md) its soundscape: which music a map, a zone, a boss phase, a victory or a game mode plays, which sounds triggers, cutscene steps and boss telegraphs raise, and which loops sound from points on a map. Retail sound is named by coordinate like every other disc asset; the modder's own WAV and Ogg Vorbis files under `assets/` play beside it. The parser is `legaia_expansion_format::{audio, audio_clip}`, the runtime `legaia_engine_expansion::audio`, wired to the `World` by `engine-core`'s `world::expansion_audio`, mixed by `engine-audio`'s PCM bus. With no pack active every hook is the identity and the bus stays empty, which the mixer skips: the retail output is bit-identical.

## At a glance

| Property | Value |
|---|---|
| Files | `audio.toml` at the pack root (optional); `[audio]` in any `maps/<id>.toml` |
| Declares | `track` and `sound` ids |
| Assets | `.wav` (8/16/24/32-bit PCM, 32-bit float) and `.ogg` (Vorbis), mono or stereo, 8-96 kHz, at most 360 s |
| Referenced from | Map `[audio]`, map `sound` triggers, boss `music`, boss-script phase `music` / `sound` and move `charge_sound` |
| Hosts | The native window (cpal) and the browser page (WebAudio) through one director, `AudioDirector` |
| Sample | [`packs/rebirth/audio.toml`](../../packs/rebirth/audio.toml) |

## Music and sound references

A **music** field takes a `MusicRef`:

| Form | Plays |
|---|---|
| `{ kind = "bgm", id = 30 }` | A retail cue by sound-test id (`0..81`, [music-tracks.md](../reference/music-tracks.md)), the same shape a map's `bgm` and a `DiscRef::Bgm` take |
| `{ kind = "track", id = "tower_hum" }` | A pack track (`[tracks.<id>]`) |
| `{ kind = "silence" }` | Nothing |

A **sound** field takes a `SoundRef`:

| Form | Plays |
|---|---|
| `{ kind = "sfx", id = 0x21 }` | A retail cue: a static-table id `0x000..0x100` ([sfx-table.md](sfx-table.md)) or a runtime-bank id `0x200..=0x2FF` of the map's base scene ([bse-dat.md](bse-dat.md)), through the field's cue ring (the path field op `0x36` takes) |
| `{ kind = "sound", id = "chime" }` | A pack sound (`[sounds.<id>]`) on the PCM bus |

## `audio.toml`

```toml
[tracks.echo_drone]
file = "audio/tower_drone.wav"   # relative to assets/
gain = 0.8                       # 0..=4 on top of normalisation (default 1)
normalize = true                 # default
loop_start = 0                   # frames; the file's own loop, else 0
loop_end = 132300                # exclusive; the file's own loop, else its end

[sounds.chime]
file = "audio/chime.wav"
loop = false                     # a sound plays once unless true

[music]
battle = { kind = "bgm", id = 26 }          # fights on the pack's maps
boss = { kind = "bgm", id = 28 }            # a boss with no `music`
victory = { kind = "track", id = "fanfare" }
boss_victory = { kind = "bgm", id = 65 }    # victory when unset

[music.modes]
poker = { kind = "bgm", id = 18 }           # by mode kind
tower-high-table = { kind = "bgm", id = 45 } # by instance id (wins)
```

| Key | Meaning |
|---|---|
| `tracks.<id>` | Music. A track loops unless `loop = false`. |
| `sounds.<id>` | Stingers, effects and ambient loops. A sound plays once unless `loop = true`; an ambient source loops it whatever this says. |
| `file` | The asset, `.wav` or `.ogg`. |
| `gain`, `normalize` | Level: see [Levels](#levels). |
| `loop_start`, `loop_end` | The loop `[start, end)` in sample frames. Unset, they take the file's own loop - a WAV `smpl` chunk's first loop (`dwEnd` is inclusive, so the region ends one past it), an Ogg `LOOPSTART` with `LOOPLENGTH` or `LOOPEND` comment - and else the whole clip. |
| `music.battle` | The battle theme of a fight on one of the pack's maps that no boss scores. Unset keeps retail's battle-sound-set pick. |
| `music.boss` | The track of a pack boss with no `music` of its own. |
| `music.victory`, `music.boss_victory` | What starts as a won fight's results open (the spoils step): `boss_victory` for a pack boss, falling back to `victory`; `victory` for any fight on a pack map. The field track the battle swap stashed comes back after, as ever. |
| `music.modes.<key>` | Music while a mode instance runs, keyed by instance id or mode kind (`poker`, `card_duel`, `golf`, `race`, `prop_hunt`, `party_night`); an instance key wins. What played before comes back when the mode closes. |

## A map's `[audio]`

```toml
[audio]
music = { kind = "track", id = "echo_drone" }  # wins over the map's `bgm`
fade_frames = 40                               # zone cross-fade, default 45

[[audio.zones]]
id = "games_room"
rect = [8, 7, 14, 10]           # map-local tiles, inclusive
music = { kind = "bgm", id = 18 }

[[audio.ambient]]
id = "hearth"
sound = { kind = "sound", id = "torch" }
tile = [13, 4]
offset = [0, 0]                 # world [x, z] from the tile centre
radius = 512                    # silent from here (a tile is 128)
inner = 96                      # full volume within
volume = 0.5                    # 0..=1
```

What plays on the map is the first zone holding the player's tile, else `[audio] music`, else the map's `bgm`, else nothing (a map that names no music never touches the slot). A change cross-fades over `fade_frames`; the first pick after the map is entered starts with a cut, as a retail scene's music does. A script's pick (`World::expansion_set_music`, below) holds over the zones until it is cleared. Walking off the pack's maps stops a pack track nobody replaced; a retail scene's own music replaces it the ordinary way.

A map places at most 16 ambient sources, at most 4 of them retail cues.

### Map sound triggers

A trigger zone may raise a sound:

```toml
[[triggers]]
rect = [3, 3, 4, 3]
event = { kind = "sound", sound = { kind = "sfx", id = 0x2E } }
```

## Boss music, phases and telegraphs

A boss's `music` ([expansion-pack-battle.md](expansion-pack-battle.md)) is a `MusicRef`, so it may be a pack track. A boss-script phase (and the enrage) takes `music`, which cross-fades in as the phase begins, and `sound`, played then; a telegraphed move takes `charge_sound`, played on its charge turn:

```toml
[[monsters.hollow_sovereign.script.phases]]
when = { hp_below_pct = 66 }
music = { kind = "bgm", id = 61 }
sound = { kind = "sound", id = "chime" }
moves = [{ spell = 0x63, charge = true, charge_sound = { kind = "sound", id = "chime" } }]
```

The phase's music becomes `AudioState::current_bgm` for the rest of the fight; the battle's end restores the stashed field track as for any fight.

## How music reaches the director

Every music change is an ordinary `FieldEvent::Bgm` start, with `AudioState::current_bgm` kept in step, so the battle swap stashes and restores a zone's track exactly as it does a retail scene's, and both hosts route it through `SceneHost::route_bgm_events`. Two things are new:

| Piece | Meaning |
|---|---|
| Pack track ids | A pack track rides music id `0xE000 + n` (`audio::PACK_TRACK_BASE`), `n` its index in `tracks` key order; the scene host resolves the id against the active pack and calls `BgmDirector::start_pack_track`. Retail ids stay `2000 + sound-test id`. |
| Sub-op `0xFC` | `BGM_SUB_OP_ENGINE_FADE`: a start that cross-fades over `World::expansion.audio.fade_frames`. Like the engine's other sub-ops (`0xFD..0xFF`) it is no retail op-`0x35` word; only a pack raises it. The scene host calls `BgmDirector::start_owned_vab_faded`. |

The director's cross-fades:

| From -> to | What happens |
|---|---|
| Pack track -> pack track | A true cross-fade on the PCM bus: the old track fades out as the new one fades in. |
| Retail score -> pack track | The SPU master fades the score out and the sequencer leaves at silence (`AudioSink::fade_out_sequencer`); the pack track fades in over the same span. |
| Pack track -> retail score | The pack track fades out on the bus; the score starts with the retail start's click guard. |
| Retail score -> retail score | The old score fades out first and the new one starts once it is silent (`pack_start_deferred`): the incoming bank is uploaded over the outgoing one's samples, so the two cannot overlap. |

A retail start, stop or pause (a battle swap, a cutscene's op `0x35`) takes the slot from a pack track. The BGM pause gate holds a pack track; the re-attach replays it from its start, as retail's does a score. The battle duck applies to it.

## Sounds and ambient sources

`World::expansion_play_sound` plays a `SoundRef`: a retail cue onto the cue ring, a pack sound onto the queue the session drains each audio frame (`World::take_expansion_sounds`) into a one-shot bus voice.

Each tick on a pack map the world recomputes every ambient source's level from the player and the camera (`audio::ambient_level`): full volume within `inner`, `(1 - (d - inner) / (radius - inner))^2` out to `radius`, silent beyond, times `volume`; panned equal-power by where the source sits across the view (screen-right is the camera azimuth's rotation of `+X`), narrowing to the centre as the player reaches it. It is presentation only: the simulation never reads a level back.

| Source | Driven by |
|---|---|
| A pack sound | `World::expansion_ambient_levels`, a looping bus voice per source keyed by map and index; the session keeps each at its level (eased over ~10 ms) and fades out (100 ms) the ones that left range |
| A retail cue | The positional-emitter path ([audio.md](../subsystems/audio.md#positional-sound-emitters)): an `EmitterOp::Key` on entering range, `Volume` (the raw `0..=0x3F80` voice level) each tick in range, `Release` on leaving it, a battle or the map. Each cue source holds one of the voices `23, 22, 21, 20`, by its rank among the map's cue sources; the cue loops if its sample does |

## Levels

A clip is decoded once per pack (`audio_clip::decode`, the one decoder the validator and both hosts run) and cached. With `normalize` on, its gain is `REFERENCE_RMS / rms`, clamped to `0.25..=4`, so every clip lands at an RMS of `0.07` of full scale (about -23 dBFS) before `gain` multiplies in. The reference sits at the level of the retail field scores through the port's mixer: `engine-session/tests/pack_audio_disc.rs` measures the 7F sanctum's drone against the floor's retail score at the mixer's output.

A pack track follows the BGM volume option and the battle duck; a sound and an ambient source follow the SFX volume option (`AudioSink::set_bus_volumes`). Mute, master volume, the full mono downmix and the options screen's Monaural row apply to the bus as to every other source.

## Decoders

WAV is read in `audio_clip`. Ogg Vorbis goes through `lewton` (pure Rust, with its `ogg` page reader): the smallest established pure-Rust Vorbis decoder, past the [publish cooldown](../tooling/dependency-policy.md), no C library and no unsafe code of its own. One Rust decoder on both hosts, instead of the browser's `decodeAudioData`, keeps the two hosts' bus input identical sample for sample and lets the validator check a file the way the engine will play it. The workbench previews a file with the browser's own decoder.

## Validation

| Check | Severity |
|---|---|
| `file` names no asset, is not `.wav` / `.ogg`, or does not probe (rate, channels, length; a WAV's `fmt ` / `data`, an Ogg's headers and last granule) | error |
| `gain` outside `0..=4` | error |
| `loop_end <= loop_start`, or either past the clip's end | error |
| A loop shorter than 50 ms | warning |
| A `track` / `sound` reference the pack does not declare; a `bgm` id outside the sound test | error |
| A retail cue outside `0x000..0x100` and `0x200..=0x2FF` | error |
| A `music.modes` key that is neither a mode instance nor a mode kind | warning |
| A zone's rect or a source's tile off the map; `radius` not past `inner`; `volume` outside `0..=1`; `fade_frames` past 600 | error |
| More than 16 ambient sources, or more than 4 retail-cue sources, on a map | error |

## The World API

For cutscene steps (`sound`, `music`, `music_release` in [`cutscenes.toml`](expansion-pack-cutscenes.md)), scripts and other runtimes:

| Call | Does |
|---|---|
| `World::expansion_play_sound(&SoundRef)` | Plays a sound (`expansion_play_sound_panned` at a level pair) |
| `World::expansion_set_music(Some(&MusicRef), fade_frames)` | Plays music and holds it over the map's zones; `None` hands the music back to them |
| `World::expansion_music_id(&MusicRef)` | The music id a reference plays against the active pack |

## Rebirth

[`packs/rebirth`](../../packs/rebirth/) keeps each floor's retail music and adds: a games-room zone on 5F (the Sol casino's "Wild Declaration" over the Rest's bar tune), the pack's own drone in 7F's inner sanctum, ember-crystal, brazier, hearth and throne-lamp crackle (1F, 2F, 5F, 8F), the Warden's Stone hum, running water on 3F, the casino music at the poker table and "Funky Ascent" over a Grid Duel, the Hollow Sovereign's phase music (Apostles of the Mist, then Apocalypse, then the Wild Arms battle theme for its unbound form) with a chime on its ultimate's telegraph, and the title theme over every tower boss's results.

Its four WAV files are synthesised by [`scripts/models/generate_rebirth_audio.py`](../../scripts/models/generate_rebirth_audio.py) - sine partials and seeded noise, no disc audio.
