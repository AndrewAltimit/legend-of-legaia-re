//! The `World` side of an expansion pack's soundscape (`audio.toml`, each
//! map's `[audio]`): map and zone music, mode music, boss-phase and victory
//! music, pack sounds and positional ambient sources.
//!
//! Every music change is an ordinary `FieldEvent::Bgm` start (sub-op `1`,
//! or [`crate::world::BGM_SUB_OP_ENGINE_FADE`] to cross-fade) with
//! `AudioState::current_bgm` kept in step, so the battle swap stashes and
//! restores a zone's track exactly as it does a retail scene's. A pack track
//! rides an id of its own (`legaia_engine_expansion::audio::PACK_TRACK_BASE`
//! and up); the scene host resolves it against the pack.
//!
//! Identity with no pack: [`World::tick_expansion_audio`] runs from
//! `tick_expansion`, which returns before it without one, and every other
//! entry point here reads the pack first.
//!
//! The kernels (music ids, zone lookup, the positional law) are
//! `World`-free in `legaia_engine_expansion::audio`.

use legaia_engine_expansion::audio::{
    self as xa, AmbientLevel, Applied, KeyedCue, SoundPlay, ambient_level, map_music_at,
    source_position,
};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_vm::sfx_emitter::EmitterOp;
use legaia_expansion_format::audio::{AMBIENT_CUE_VOICES, MusicRef, SoundRef};

use super::World;

/// The frames a mode's music and a boss phase's music cross-fade over.
const MODE_FADE_FRAMES: u16 = 30;

/// A full-volume raw SPU voice level, the emitter law's clamp
/// (`docs/subsystems/audio.md`, "Positional sound emitters").
const CUE_FULL_VOLUME: f32 = 0x3F80 as f32;

/// The `[music.modes]` key of a mode kind.
fn mode_kind_key(kind: legaia_engine_modes::active::ModeKind) -> &'static str {
    use legaia_engine_modes::active::ModeKind as K;
    match kind {
        K::Poker => "poker",
        K::CardDuel => "card_duel",
        K::Golf => "golf",
        K::Race => "race",
        K::PropHunt => "prop_hunt",
    }
}

impl World {
    /// What `m` plays against the active pack: `Some(Some(id))` a music id,
    /// `Some(None)` silence, `None` without a pack or for a track the pack
    /// does not have.
    pub fn expansion_music_id(&self, m: &MusicRef) -> Option<Option<u16>> {
        let pack = self.expansion.pack.as_ref()?;
        xa::music_id(&pack.audio, &pack.manifest.id, m)
    }

    /// Play sound `s`: a retail cue through the field's cue ring (the path
    /// op `0x36` takes), a pack sound on the host's PCM bus. For cutscene
    /// steps, triggers and boss telegraphs. `false` with no pack.
    pub fn expansion_play_sound(&mut self, s: &SoundRef) -> bool {
        self.expansion_play_sound_panned(s, (1.0, 1.0))
    }

    /// [`Self::expansion_play_sound`] at a `(left, right)` level; a retail
    /// cue ignores it.
    pub fn expansion_play_sound_panned(&mut self, s: &SoundRef, pan: (f32, f32)) -> bool {
        if self.expansion.pack.is_none() {
            return false;
        }
        match s {
            SoundRef::Sfx { id } => {
                self.push_sfx_cue(i16::try_from(*id).unwrap_or(i16::MAX));
            }
            SoundRef::Sound { id } => {
                let key = self.expansion.pack.as_ref().and_then(|p| {
                    legaia_expansion_format::battle::local_key(&p.manifest.id, id)
                        .filter(|k| p.audio.sounds.contains_key(*k))
                        .map(str::to_string)
                });
                let Some(key) = key else {
                    log::warn!("expansion: no pack sound `{id}`");
                    return false;
                };
                self.expansion.audio.sounds.push(SoundPlay { key, pan });
            }
        }
        true
    }

    /// A script's music pick (a cutscene step): `Some(m)` plays `m`,
    /// cross-fading over `fade_frames`, and holds it over the map's zones
    /// until cleared with `None`, which hands the music back to them. `false`
    /// when `m` names a track the pack does not have.
    pub fn expansion_set_music(&mut self, m: Option<&MusicRef>, fade_frames: u16) -> bool {
        match m {
            Some(m) => {
                let Some(id) = self.expansion_music_id(m) else {
                    return false;
                };
                self.expansion.audio.script_music = Some(id);
                self.expansion_push_music(id, fade_frames);
            }
            None => {
                self.expansion.audio.script_music = None;
                // The zones re-apply on the next tick, cross-fading.
            }
        }
        true
    }

    /// Put music id `id` (`None`: silence) in the music slot, cross-fading
    /// over `fade` frames (`0` cuts), and record it as applied.
    fn expansion_push_music(&mut self, id: Option<u16>, fade: u16) {
        use legaia_engine_field::field_events::FieldEvent;
        self.audio.current_bgm = id;
        self.expansion.audio.applied = Applied::Music(id);
        self.expansion.audio.fade_frames = fade;
        self.pending_field_events.push(match id {
            Some(text_id) => FieldEvent::Bgm {
                text_id,
                sub_op: if fade == 0 {
                    1
                } else {
                    crate::world::BGM_SUB_OP_ENGINE_FADE
                },
            },
            None => FieldEvent::Bgm {
                text_id: 0,
                sub_op: crate::world::BGM_SUB_OP_ENGINE_STOP,
            },
        });
    }

    /// The boss track of a boss with `music`: its own, else `music.boss`.
    pub(in crate::world) fn expansion_boss_track(&self, music: Option<&MusicRef>) -> Option<u16> {
        let pack = self.expansion.pack.as_ref()?;
        let m = music.or(pack.audio.music.boss.as_ref())?;
        self.expansion_music_id(m).flatten()
    }

    /// `music.battle` for a fight on one of the pack's maps.
    pub(in crate::world) fn expansion_map_battle_track(&self) -> Option<u16> {
        self.expansion.map.as_ref()?;
        let m = self.expansion.pack.as_ref()?.audio.music.battle.clone()?;
        self.expansion_music_id(&m).flatten()
    }

    /// A boss phase (or enrage) begins: its music cross-fades in and its
    /// sound plays.
    pub(in crate::world) fn expansion_phase_audio(
        &mut self,
        music: Option<&MusicRef>,
        sound: Option<&SoundRef>,
    ) {
        if let Some(m) = music
            && let Some(id) = self.expansion_music_id(m)
            && id != self.audio.current_bgm
        {
            self.expansion_push_music(id, MODE_FADE_FRAMES);
        }
        if let Some(s) = sound {
            self.expansion_play_sound(s);
        }
    }

    /// A won fight's results: `music.boss_victory` (a pack boss) or
    /// `music.victory` (any fight on a pack map, a pack boss too) starts.
    /// The field track the battle swap stashed still comes back after.
    pub(in crate::world) fn expansion_victory_music(&mut self) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        let boss = self.expansion.battle.active_boss.is_some();
        let on_map = self.expansion.map.is_some();
        let m = &pack.audio.music;
        let pick = if boss {
            m.boss_victory.as_ref().or(m.victory.as_ref())
        } else if on_map {
            m.victory.as_ref()
        } else {
            None
        };
        if let Some(id) = pick.and_then(|p| self.expansion_music_id(p)) {
            self.expansion_push_music(id, 0);
        }
    }

    /// The music the open mode seat asks for: its instance's
    /// `[music.modes]` entry, else its kind's.
    fn expansion_mode_music(&self) -> Option<(String, Option<u16>)> {
        let seat = self.expansion.mode_seat.as_deref()?;
        let mode = seat.active()?;
        let modes = &self.expansion.pack.as_ref()?.audio.music.modes;
        let m = modes
            .get(&mode.id)
            .or_else(|| modes.get(mode_kind_key(mode.kind())))?;
        Some((mode.id.clone(), self.expansion_music_id(m)?))
    }

    /// One tick of the soundscape: mode music, else the map's zone or own
    /// music (a script pick winning), and the ambient sources' levels.
    pub(in crate::world) fn tick_expansion_audio(&mut self) {
        let map_id = self.expansion.map.as_ref().map(|m| m.map_id.clone());
        // A map entered (or left): its music starts afresh, with a cut, and
        // the last map's keyed cues let go.
        if self.expansion.audio.keyed_map != map_id {
            self.release_ambient_cues();
            self.expansion.audio.keyed_map = map_id.clone();
            if self.expansion.audio.mode.is_none() {
                if map_id.is_none() {
                    // Off the pack's maps: a pack track nobody replaced
                    // stops (a retail scene's own start has moved
                    // `current_bgm` on already).
                    if let Applied::Music(Some(id)) = self.expansion.audio.applied
                        && xa::is_pack_track(id)
                        && self.audio.current_bgm == Some(id)
                    {
                        self.expansion_push_music(None, 0);
                    }
                }
                self.expansion.audio.applied = Applied::Unset;
                self.expansion.audio.script_music = None;
            }
        }
        // The battle owns the slot (its swap, a phase, the victory); a
        // retail minigame its own.
        if self.mode == SceneMode::Battle
            || self.audio.battle_bgm_active
            || self.audio.minigame_bgm_active
        {
            self.expansion.audio.ambient.clear();
            self.release_ambient_cues();
            return;
        }
        match (
            self.expansion_mode_music(),
            self.expansion.audio.mode.clone(),
        ) {
            (Some((key, id)), held) => {
                if held.as_deref() != Some(key.as_str()) {
                    if held.is_none() {
                        self.expansion.audio.mode_resume = Some(self.audio.current_bgm);
                    }
                    self.expansion.audio.mode = Some(key);
                    self.expansion_push_music(id, MODE_FADE_FRAMES);
                }
            }
            (None, Some(_)) => {
                self.expansion.audio.mode = None;
                let resume = self.expansion.audio.mode_resume.take().flatten();
                if self.expansion.map.is_some() {
                    // The zones take over again, cross-fading from the mode's
                    // music.
                    self.expansion.audio.applied = Applied::Music(self.audio.current_bgm);
                } else if resume != self.audio.current_bgm {
                    self.expansion_push_music(resume, MODE_FADE_FRAMES);
                    self.expansion.audio.applied = Applied::Unset;
                }
            }
            (None, None) => {}
        }
        if self.expansion.audio.mode.is_none() && self.mode == SceneMode::Field {
            self.apply_map_music();
        }
        self.step_ambient_sources();
    }

    /// The map's zone (or own) music under the player, applied on a change.
    fn apply_map_music(&mut self) {
        let Some(map) = self.expansion.map.as_ref() else {
            return;
        };
        let Some(player) = self.actors.first() else {
            return;
        };
        let (x, z) = (
            i32::from(player.move_state.world_x),
            i32::from(player.move_state.world_z),
        );
        let def = map.def.clone();
        let tile = map.tile_at_world(x, z);
        let want = match self.expansion.audio.script_music {
            Some(id) => Some(id),
            None => map_music_at(&def, tile).and_then(|m| self.expansion_music_id(&m)),
        };
        let applied = self.expansion.audio.applied;
        let fade = match applied {
            Applied::Unset => 0,
            Applied::Music(_) => def.audio.fade(),
        };
        match want {
            Some(id) if applied != Applied::Music(id) => self.expansion_push_music(id, fade),
            // A spot the map names no music for: a zone's music stops there.
            None if matches!(applied, Applied::Music(Some(_))) => {
                self.expansion_push_music(None, fade)
            }
            _ => {}
        }
    }

    /// Recompute every ambient source's level from the player and the
    /// camera: a pack sound's level for the host's PCM bus, a retail cue's
    /// voice key, volume or release for the emitter path.
    fn step_ambient_sources(&mut self) {
        let mut levels = Vec::new();
        let mut want_cues: Vec<(usize, u16, (f32, f32))> = Vec::new();
        if self.mode == SceneMode::Field
            && let Some(map) = self.expansion.map.as_ref()
            && let Some(player) = self.actors.first()
        {
            let listener = (
                i32::from(player.move_state.world_x),
                i32::from(player.move_state.world_z),
            );
            let azimuth = self.locomotion.camera_azimuth;
            let map_index = self
                .expansion
                .pack
                .as_ref()
                .and_then(|p| p.maps.keys().position(|k| *k == map.map_id))
                .unwrap_or(0) as u32;
            for (i, src) in map.def.audio.ambient.iter().enumerate() {
                let pos = source_position(&map.def, src);
                let Some(pan) =
                    ambient_level(pos, listener, azimuth, src.inner, src.radius, src.volume)
                else {
                    continue;
                };
                match &src.sound {
                    SoundRef::Sound { id } => levels.push(AmbientLevel {
                        key: (map_index << 8) | i as u32,
                        sound: id.clone(),
                        pan,
                    }),
                    SoundRef::Sfx { id } => want_cues.push((i, *id, pan)),
                }
            }
        }
        self.expansion.audio.ambient = levels;
        // Retail cues: each cue source takes the voice of its rank among the
        // map's cue sources, so a source keeps its voice while it sounds.
        let cue_rank: Vec<usize> = self
            .expansion
            .map
            .as_ref()
            .map(|m| {
                m.def
                    .audio
                    .ambient
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| matches!(s.sound, SoundRef::Sfx { .. }))
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        let gone: Vec<usize> = self
            .expansion
            .audio
            .keyed
            .keys()
            .copied()
            .filter(|i| !want_cues.iter().any(|w| w.0 == *i))
            .collect();
        for i in gone {
            if let Some(k) = self.expansion.audio.keyed.remove(&i) {
                self.audio.sfx_emitter_ops.push(EmitterOp::Release {
                    cue: i32::from(k.cue),
                    voice: i16::from(k.voice),
                });
            }
        }
        for (i, cue, (l, r)) in want_cues {
            let Some(voice) = cue_rank
                .iter()
                .position(|&s| s == i)
                .and_then(|rank| AMBIENT_CUE_VOICES.get(rank).copied())
            else {
                continue;
            };
            let (left, right) = ((l * CUE_FULL_VOLUME) as u16, (r * CUE_FULL_VOLUME) as u16);
            let op = match self.expansion.audio.keyed.entry(i) {
                std::collections::btree_map::Entry::Occupied(_) => EmitterOp::Volume {
                    cue: i32::from(cue),
                    voice: i16::from(voice),
                    left,
                    right,
                },
                std::collections::btree_map::Entry::Vacant(v) => {
                    v.insert(KeyedCue { cue, voice });
                    EmitterOp::Key {
                        cue: i32::from(cue),
                        voice: i16::from(voice),
                        vol_l: (left >> 7) as i16,
                        vol_r: (right >> 7) as i16,
                    }
                }
            };
            self.audio.sfx_emitter_ops.push(op);
        }
    }

    /// Key off every retail-cue ambient source.
    fn release_ambient_cues(&mut self) {
        for (_, k) in std::mem::take(&mut self.expansion.audio.keyed) {
            self.audio.sfx_emitter_ops.push(EmitterOp::Release {
                cue: i32::from(k.cue),
                voice: i16::from(k.voice),
            });
        }
    }

    /// The pack sounds raised since the last call, for the host's PCM bus.
    pub fn take_expansion_sounds(&mut self) -> Vec<SoundPlay> {
        self.expansion.audio.take_sounds()
    }

    /// The pack-sound ambient sources sounding this tick, for the host's
    /// PCM bus.
    pub fn expansion_ambient_levels(&self) -> &[AmbientLevel] {
        &self.expansion.audio.ambient
    }
}
