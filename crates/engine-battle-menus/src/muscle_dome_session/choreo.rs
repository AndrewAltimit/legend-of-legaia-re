//! The leg's play-out choreography: one resolved turn acted out by the two
//! bodies on the battle's own animation and locomotion kernels.
//!
//! A dome leg is an ordinary battle, so its play-out is the battle's:
//!
//! - **Clips play at the battle rate.** Each body's clip runs on the battle
//!   anim player ([`MonsterAnimPlayer`]) under the retail rate law
//!   (`FUN_80047430`: the 12.4 cursor advances `dt * actor[+0x21D] *
//!   entry[+0x78] >> 1` a frame at the normal rate `8`), the same advance
//!   `World::tick_battle_animations` gives a battle actor.
//! - **The approach stops on the range law.** An attacker out of reach walks
//!   its walk clip (party action slot `1`; a monster's first tag-`0x20`
//!   entry, else tag `1`) along its facing at the clip's root speed until
//!   `FUN_8004E2F0` reads zero (`legaia_engine_battle_vm::battle_action::motion::range_metric`
//!   over the attacker's live pair and the target's body pair, the party
//!   reach offsets `DAT_80078878` and the monster size class `+0x1F`) - the
//!   state-`0x14` / `0x19` approach.
//! - **Root motion moves the bodies.** A playing clip's signed `+0x0C`
//!   speed steps its body (a positive speed only while the range poll still
//!   fails, a negative one unconditionally - the reactions' slide back), and
//!   a clip's natural end steps the body by its `+0x0E` end step.
//! - **The hit lands on the swing's event frame.** The defender reacts on
//!   the first entry of the swing's `+0x10` event list, with the melee
//!   kernel's reaction pick (`FUN_801EC3E4`): the `+0x1EF` / `+0x1F0` flinch
//!   by the queued byte, the `+0x1F1` knockdown on the killing hit or on a
//!   string's closing hit that drops a get-up-carrying survivor under a
//!   quarter of its maximum (or totals more than a quarter of it), the
//!   `+0x1F2` get-up after a survived knockdown.
//! - **No walk home.** Nothing returns a combatant to its seat
//!   (`engine-core::world::battle::locomotion`), so each attacker's string
//!   starts from wherever the last one left the pair.
//!
//! A session with no bodies installed ([`DomeChoreo::install`]) plays the
//! turn out on [`turn_timeline`]'s fixed schedule instead, with nothing
//! moving.
//!
//! REF: FUN_80047430 (anim tick: cursor advance, root motion, end step)
//! REF: FUN_8004E2F0 (range law), FUN_801E295C (states 0x14 / 0x19 / 0x1E)
//! REF: FUN_801EC3E4 (reaction pick), FUN_80054CB0 / FUN_80053CB8 (reaction map)

use legaia_actor_formats::battle_char_assembly as bca;
use legaia_actor_formats::monster_archive::{self, MonsterAnimation};
use legaia_engine_battle::battle_anim::{MonsterAnimPlayer, TweenTarget};
use legaia_engine_battle_vm::battle_action::motion;
use legaia_engine_battle_vm::battle_anim_rate::AnimRate;
use legaia_engine_minigames::muscle_dome::DomePlay;

/// Clip-less schedule: ticks the leg's first acting play walks in for.
pub const APPROACH_TICKS: u32 = 48;
/// Clip-less schedule: ticks one swing plays for.
pub const PLAY_CADENCE_TICKS: u32 = 34;
/// Ticks the done band holds after an attacker's string: the action SM's
/// `0x50` arm seeds the tail timer `ctx[+0x6D8] = 0x3C` and `0x51` counts it
/// down.
pub const DONE_TAIL_TICKS: u32 = 0x3C;

/// The approach drive's `(speed, scale)` when the walk clip carries no
/// positive root speed: the captured retail Move drive (Gaza: entry `+0xC =
/// 20`, `+0x21D = 8`), the same fallback `World::tick_battle_locomotion`
/// takes.
const FALLBACK_APPROACH: (i16, u8) = (20, 8);
/// The normal per-actor anim-rate / speed-scale byte `actor[+0x21D]`.
const SPEED_SCALE: u8 = 8;
/// The render scale `+0x72` the body pair is scaled by (`0x1000`, unity).
const RENDER_SCALE: i32 = 0x1000;
/// Ticks an approach may run before the choreography gives up on the range
/// poll and swings anyway - an engine guard; the retail poll has no
/// timeout (`battle-action-exit-gates.md`).
const APPROACH_GUARD_TICKS: u32 = 600;

/// A body's clips, by the role the battle stages them in.
#[derive(Debug, Clone)]
pub struct ChoreoClips {
    /// Entry `0`, the idle loop.
    pub idle: MonsterAnimation,
    /// The approach walk.
    pub walk: Option<MonsterAnimation>,
    /// The reaction map `+0x1EF..=+0x1F2`: the two light flinches, the
    /// knockdown and the get-up.
    pub reactions: [Option<MonsterAnimation>; 4],
    /// The swing per queued direction `0xC..=0xF`.
    pub swings: [Option<MonsterAnimation>; 4],
    /// A monster's archive entries by index: the monster AI queues entry
    /// indices into the stream (`FUN_801E9FD4`'s physical branch), and the
    /// attack band stages each one as its own clip. Empty for the party.
    pub by_entry: Vec<Option<MonsterAnimation>>,
}

/// One fighter as the choreography moves it.
#[derive(Debug, Clone)]
pub struct ChoreoBody {
    pub clips: ChoreoClips,
    /// A party seat (`< 3` in retail's actor table).
    pub party: bool,
    /// The party reach offset `DAT_80078878[char - 1]`; `0` for a monster.
    pub reach: i16,
    /// The monster record's size class `+0x1F`; `0` for the party.
    pub size_class: u8,
}

/// One battle-form clip by runtime action slot, expanded per assembled
/// object: slot `0` the record-0 idle loop, `0xC..=0xF` the per-command
/// swing records, any other slot the record-0 action table by tag (the walk
/// `1`, the reaction family `[2, 3, 4, 5, 0xB]`).
pub fn fighter_clip(
    raw: &[u8],
    asm: &bca::AssembledCharacter,
    slot: u32,
) -> Option<MonsterAnimation> {
    let anim = if (0xC..=0xF).contains(&slot) {
        let pack = legaia_actor_formats::battle_data_pack::parse(raw).ok()?;
        bca::swing_battle_animations(raw, &pack, &[0u8; 5])
            .ok()?
            .into_iter()
            .find(|s| u32::from(s.slot) == slot)?
            .anim
    } else if slot == 0 {
        bca::idle_battle_animation(raw).ok()??
    } else {
        bca::battle_animations(raw)
            .ok()?
            .into_iter()
            .find(|a| u32::from(a.action_id) == slot)?
    };
    Some(bca::expand_animation_for_objects(&anim, &asm.anm_bones))
}

/// The party installer's fixed reaction slots (`FUN_80053CB8` hardcodes
/// `[2, 3, 4, 5]` for `+0x1EF..=+0x1F2`: the player files store the family
/// identity-ordered).
const PARTY_REACTION_SLOTS: [u32; 4] = [2, 3, 4, 5];

impl ChoreoBody {
    /// The party fighter `char_slot` (`0` Vahn, `1` Noa, `2` Gala) from its
    /// player battle file, at the dome's no-equipment section defaults.
    pub fn fighter(player_file: &[u8], char_slot: u32) -> Option<Self> {
        let pack = legaia_actor_formats::battle_data_pack::parse(player_file).ok()?;
        let asm = bca::assemble_character(player_file, &pack, &[0u8; 5]).ok()?;
        let clip = |slot: u32| fighter_clip(player_file, &asm, slot);
        Some(Self {
            clips: ChoreoClips {
                idle: clip(0)?,
                walk: clip(1),
                reactions: PARTY_REACTION_SLOTS.map(clip),
                swings: [0xC, 0xD, 0xE, 0xF].map(clip),
                by_entry: Vec::new(),
            },
            party: true,
            reach: motion::PARTY_REACH_OFFSETS
                .get(char_slot as usize)
                .copied()
                .unwrap_or(0),
            size_class: 0,
        })
    }

    /// Monster `monster_id` from the archive (PROT 867).
    ///
    /// The reaction map is the installer's (`FUN_80054CB0`): one forward pass
    /// over the entry table, last match wins, index `0` reads as absent, and
    /// a missing knockdown falls back to the first flinch. The walk is the
    /// state-`0x14` search: the first tag-`0x20` entry, else tag `1`. The
    /// swings cycle the picker's strike candidates (tags `0x0C..=0x1F` with
    /// a real AGL cost) over the four queued directions - a host model: the
    /// dome opponent commits directions, not the picker's budgeted draw.
    pub fn monster(archive: &[u8], monster_id: u16) -> Option<Self> {
        let entries = monster_archive::animations_by_entry(archive, monster_id).ok()??;
        let record = monster_archive::record(archive, monster_id).ok()??;
        let get = |i: usize| entries.get(i).and_then(Clone::clone);
        let first_tag = |t: u8| {
            entries
                .iter()
                .position(|e| e.as_ref().is_some_and(|a| a.action_id == t))
        };
        let mut map = [0usize; 4];
        for (i, e) in entries.iter().enumerate() {
            let Some(a) = e else { continue };
            match a.action_id {
                2 => map[0] = i,
                3 => map[1] = i,
                4 => map[2] = i,
                5 => map[3] = i,
                _ => {}
            }
        }
        if map[2] == 0 {
            map[2] = map[0];
        }
        let reactions = map.map(|i| (i != 0).then(|| get(i)).flatten());
        let walk = first_tag(0x20).or_else(|| first_tag(1)).and_then(get);
        // The picker's physical branch (`FUN_801E9FD4`): a candidate swing
        // is an entry tagged `0x0C..=0x1F` whose `+0x74` AGL cost is not the
        // `0xFF` not-an-attack sentinel - the set
        // `engine-battle::monster_catalog` budgets a monster's strikes from.
        let attacks: Vec<usize> = record
            .spells
            .iter()
            .filter(|s| (0x0C..=0x1F).contains(&s.id) && s.agl_cost != 0xFF)
            .map(|s| usize::from(s.entry_index))
            .filter(|&i| get(i).is_some())
            .collect();
        let swings = std::array::from_fn(|k| {
            (!attacks.is_empty())
                .then(|| get(attacks[k % attacks.len()]))
                .flatten()
        });
        Some(Self {
            clips: ChoreoClips {
                idle: get(0)?,
                walk,
                reactions,
                swings,
                by_entry: entries.clone(),
            },
            party: false,
            reach: 0,
            size_class: record.size_class,
        })
    }
}

/// What one stretch of a turn's playback is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeatKind {
    /// The attacker walking into reach.
    Approach,
    /// One play's swing.
    Swing,
    /// The done band closing an attacker's string.
    Done,
}

/// One stretch of the play-out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    pub kind: BeatKind,
    /// The play it belongs to (the first play of the string for
    /// [`BeatKind::Approach`], the last for [`BeatKind::Done`]).
    pub play: usize,
    /// The acting fighter slot.
    pub attacker: usize,
    /// Tick the beat starts at, from the start of the playback.
    pub start: u32,
    /// The beat's length in ticks (an approach's is the predicted walk).
    pub len: u32,
}

/// The clip-less schedule: an approach ahead of the first play when
/// `closes_in`, one swing per play, and a done tail after each attacker's
/// last play.
pub fn turn_timeline(plays: &[DomePlay], closes_in: bool) -> Vec<Beat> {
    let mut out = Vec::new();
    let mut t = 0;
    let mut push = |out: &mut Vec<Beat>, kind, play, attacker, len| {
        out.push(Beat {
            kind,
            play,
            attacker,
            start: t,
            len,
        });
        t += len;
    };
    if closes_in && let Some(first) = plays.first() {
        push(
            &mut out,
            BeatKind::Approach,
            0,
            first.attacker.min(1),
            APPROACH_TICKS,
        );
    }
    for (i, p) in plays.iter().enumerate() {
        let a = p.attacker.min(1);
        push(&mut out, BeatKind::Swing, i, a, PLAY_CADENCE_TICKS);
        let string_ends = plays.get(i + 1).is_none_or(|n| n.attacker.min(1) != a);
        if string_ends {
            push(&mut out, BeatKind::Done, i, a, DONE_TAIL_TICKS);
        }
    }
    out
}

/// [`turn_timeline`]'s span.
pub fn turn_playback_ticks(plays: &[DomePlay], closes_in: bool) -> u32 {
    turn_timeline(plays, closes_in)
        .last()
        .map_or(0, |b| b.start + b.len)
}

/// What a body is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyAct {
    Idle,
    Walk,
    /// A swing for the queued byte.
    Swing(u8),
    /// A reaction by its `+0x1EF..=+0x1F2` index.
    Reaction(usize),
}

#[derive(Debug, Clone)]
struct Actor {
    player: Option<MonsterAnimPlayer>,
    act: BodyAct,
    /// The decoded pose, one `(translation, rotation)` per part.
    pose: Vec<([i16; 3], [i16; 3])>,
    carry: motion::RootMotionCarry,
    /// A knockdown on a body that did not survive: hold the last key.
    downed: bool,
}

impl Default for Actor {
    fn default() -> Self {
        Self {
            player: None,
            act: BodyAct::Idle,
            pose: Vec::new(),
            carry: motion::RootMotionCarry::default(),
            downed: false,
        }
    }
}

#[derive(Debug, Clone)]
struct Run {
    plays: Vec<DomePlay>,
    idx: usize,
    kind: BeatKind,
    /// Ticks into the current beat, and the beat's length.
    at: u32,
    len: u32,
    /// Ticks since the run started.
    elapsed: u32,
    start: u32,
    hit_done: bool,
    /// The clip-less schedule, when no bodies are installed.
    fixed: Option<Vec<Beat>>,
}

/// The leg's play-out state: both bodies' positions, facings and clips, and
/// the resolved turn being acted out.
#[derive(Debug, Clone, Default)]
pub struct DomeChoreo {
    bodies: Option<Box<[ChoreoBody; 2]>>,
    /// Live position pairs `(x, z)`, raw battle units.
    pos: [[i32; 2]; 2],
    /// Battle headings `+0x46`.
    facing: [u16; 2],
    actors: [Actor; 2],
    hp: [i32; 2],
    hp_max: [i32; 2],
    run: Option<Run>,
    tick: u64,
    seated: bool,
}

/// The lone party seat and the lone monster seat (`0x800775C8` /
/// `0x80077608` row 0), `(x, z)`.
pub fn seat_xz(i: usize) -> [i32; 2] {
    let s = if i == 0 {
        legaia_engine_battle::battle_seats::party_seat(1, 0)
    } else {
        legaia_engine_battle::battle_seats::monster_seat(1, 0, false)
    };
    [i32::from(s.x), i32::from(s.z)]
}

/// The seats' facings: the fighter faces `+Z`, the monster `-Z`.
pub const SEAT_FACINGS: [u16; 2] = [0, 0x800];

impl DomeChoreo {
    /// Install both bodies (`[fighter, opponent]`) and seat them.
    pub fn install(&mut self, bodies: [ChoreoBody; 2]) {
        self.bodies = Some(Box::new(bodies));
        self.seat();
    }

    /// Whether bodies are installed (otherwise the fixed schedule runs).
    pub fn installed(&self) -> bool {
        self.bodies.is_some()
    }

    fn seat(&mut self) {
        self.pos = [seat_xz(0), seat_xz(1)];
        self.facing = SEAT_FACINGS;
        self.actors = Default::default();
        self.run = None;
        self.seated = true;
        for i in 0..2 {
            self.play(i, BodyAct::Idle);
        }
    }

    /// Start acting out a resolved turn. `closes_in` feeds the clip-less
    /// schedule only; with bodies installed the range law decides each
    /// approach. `hp_before` / `hp_max` are both fighters' HP at the top of
    /// the turn and their maxima.
    pub fn start(
        &mut self,
        plays: &[DomePlay],
        closes_in: bool,
        hp_before: [i32; 2],
        hp_max: [i32; 2],
    ) {
        if !self.seated {
            self.seat();
        }
        self.hp = hp_before;
        self.hp_max = hp_max;
        if plays.is_empty() {
            self.run = None;
            return;
        }
        let fixed = (!self.installed()).then(|| turn_timeline(plays, closes_in));
        self.run = Some(Run {
            plays: plays.to_vec(),
            idx: 0,
            kind: BeatKind::Swing,
            at: 0,
            len: 0,
            elapsed: 0,
            start: 0,
            hit_done: false,
            fixed,
        });
        if self.installed() {
            self.begin_string();
        }
    }

    /// Whether a turn is still playing out.
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// The beat on screen now and the ticks into it.
    pub fn beat(&self) -> Option<(Beat, u32)> {
        let r = self.run.as_ref()?;
        if let Some(fixed) = &r.fixed {
            let b = fixed
                .iter()
                .find(|b| (b.start..b.start + b.len).contains(&r.elapsed))?;
            return Some((*b, r.elapsed - b.start));
        }
        let play = r.idx.min(r.plays.len().saturating_sub(1));
        let attacker = r.plays.get(play).map_or(0, |p| p.attacker.min(1));
        Some((
            Beat {
                kind: r.kind,
                play,
                attacker,
                start: r.start,
                len: r.len,
            },
            r.at,
        ))
    }

    /// Body `i`'s live floor position `(x, z)`.
    pub fn position(&self, i: usize) -> [f32; 2] {
        let p = self.seated_pos(i);
        [p[0] as f32, p[1] as f32]
    }

    fn seated_pos(&self, i: usize) -> [i32; 2] {
        if self.seated { self.pos[i] } else { seat_xz(i) }
    }

    /// Body `i`'s heading `+0x46`.
    pub fn facing(&self, i: usize) -> u16 {
        if self.seated {
            self.facing[i]
        } else {
            SEAT_FACINGS[i]
        }
    }

    /// Body `i`'s decoded pose (`(translation, rotation)` per part), or
    /// `None` with no bodies installed.
    pub fn pose(&self, i: usize) -> Option<&[([i16; 3], [i16; 3])]> {
        let a = self.actors.get(i)?;
        (self.installed() && !a.pose.is_empty()).then_some(a.pose.as_slice())
    }

    /// What body `i` is doing.
    pub fn act(&self, i: usize) -> BodyAct {
        self.actors[i].act
    }

    fn clip(&self, i: usize, act: BodyAct) -> Option<&MonsterAnimation> {
        let c = &self.bodies.as_ref()?[i].clips;
        match act {
            BodyAct::Idle => Some(&c.idle),
            BodyAct::Walk => c.walk.as_ref().or(Some(&c.idle)),
            BodyAct::Swing(cmd) if !c.by_entry.is_empty() => c
                .by_entry
                .get(usize::from(cmd))
                .and_then(Option::as_ref)
                .or(c.swings.iter().flatten().next())
                .or(Some(&c.idle)),
            BodyAct::Swing(cmd) => {
                let k = usize::from(cmd.wrapping_sub(0xC)).min(3);
                c.swings[k]
                    .as_ref()
                    .or(c.swings.iter().flatten().next())
                    .or(Some(&c.idle))
            }
            BodyAct::Reaction(r) => c.reactions.get(r)?.as_ref(),
        }
    }

    /// Install `act`'s clip on body `i` (a one-shot for everything but the
    /// idle and the walk). A missing clip leaves the body as it was.
    fn play(&mut self, i: usize, act: BodyAct) {
        let looping = matches!(act, BodyAct::Idle | BodyAct::Walk);
        let player = self.clip(i, act).and_then(|a| {
            if looping {
                MonsterAnimPlayer::new(a)
            } else {
                MonsterAnimPlayer::new_one_shot(a)
            }
        });
        if let Some(p) = player {
            self.actors[i].player = Some(p);
            self.actors[i].act = act;
            self.actors[i].downed = false;
        }
    }

    /// What body `i` plays once its current clip ends: a survived
    /// knockdown gets up (`FUN_8004AD80` stages `+0x1F2` behind it), a swing
    /// or a reaction returns to idle, a loop re-commits itself.
    fn next_act(&self, i: usize) -> BodyAct {
        match self.actors[i].act {
            BodyAct::Reaction(2)
                if self.hp[i] > 0 && self.clip(i, BodyAct::Reaction(3)).is_some() =>
            {
                BodyAct::Reaction(3)
            }
            BodyAct::Swing(_) | BodyAct::Reaction(_) => BodyAct::Idle,
            act => act,
        }
    }

    /// Whether body `a` stands within reach of the other one
    /// ([`Self::in_range`]'s public face, for oracles).
    pub fn in_reach(&self, a: usize) -> bool {
        self.in_range(a)
    }

    /// Whether body `a` stands within reach of the other one: the range law
    /// over `a`'s live pair and the target's body pair (live pair plus the
    /// facing-rotated pose centroid, `FUN_8004998C`).
    fn in_range(&self, a: usize) -> bool {
        let Some(bodies) = self.bodies.as_ref() else {
            return true;
        };
        let d = a ^ 1;
        let (cx, cz) = motion::pose_centroid_xz(&self.actors[d].pose);
        let (ox, oz) = motion::body_offset(self.facing[d], cx, cz, RENDER_SCALE);
        let target_ref = (
            (self.pos[d][0] as i16).wrapping_add(ox),
            (self.pos[d][1] as i16).wrapping_add(oz),
        );
        let attacker_pos = (self.pos[a][0] as i16, self.pos[a][1] as i16);
        let (ba, bd) = (&bodies[a], &bodies[d]);
        let inputs = motion::RangeInputs {
            attacker_party: ba.party,
            target_party: bd.party,
            attacker_reach: if ba.party { ba.reach } else { 0 },
            attacker_size: if ba.party { 0 } else { ba.size_class },
            target_size: if bd.party { 0 } else { bd.size_class },
            attacker_pos,
            target_ref,
        };
        let bearing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
            target_ref.1,
            target_ref.0,
            attacker_pos.1,
            attacker_pos.0,
        );
        let angle = legaia_engine_battle_vm::battle_approach::approach_angle(bearing) as u16;
        let (sin, cos) = legaia_engine_system::trig::trig12(angle);
        motion::range_metric(&inputs, sin, cos) == 0
    }

    /// Step body `i` along its facing by one tick of `speed` at `scale`.
    fn step_along(&mut self, i: usize, speed: i16, scale: u8) -> (i32, i32) {
        let (sin, cos) = legaia_engine_system::trig::trig12(self.facing[i]);
        let raw = motion::root_motion_raw(sin, cos, speed, 1, scale);
        let (dx, dz) = self.actors[i].carry.step(self.tick, raw);
        self.pos[i][0] += dx;
        self.pos[i][1] += dz;
        (dx, dz)
    }

    /// Turn the attacker onto its target: `bearing(target -> attacker) +
    /// 0x800` (the state-`0x14` head, `0x801E32EC..0x801E3318`).
    fn face(&mut self, a: usize) {
        let d = a ^ 1;
        let b = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
            self.pos[d][1] as i16,
            self.pos[d][0] as i16,
            self.pos[a][1] as i16,
            self.pos[a][0] as i16,
        );
        self.facing[a] = b.wrapping_add(0x800) & 0xFFF;
    }

    /// The approach's predicted length: the projected gap over the walk's
    /// per-tick step. Only the beat's `len` reads it; the walk itself stops
    /// on the range law.
    fn predict_approach(&self, a: usize) -> u32 {
        let d = a ^ 1;
        let dx = (self.pos[d][0] - self.pos[a][0]) as f32;
        let dz = (self.pos[d][1] - self.pos[a][1]) as f32;
        let (speed, scale) = self.approach_drive(a);
        let per = (f32::from(speed) * f32::from(scale) / 8.0).max(1.0);
        ((dx * dx + dz * dz).sqrt() / per)
            .ceil()
            .min(APPROACH_GUARD_TICKS as f32) as u32
    }

    fn approach_drive(&self, a: usize) -> (i16, u8) {
        match self.actors[a].player.as_ref().map(|p| p.root_speed()) {
            Some(s) if s > 0 && self.actors[a].act == BodyAct::Walk => (s, SPEED_SCALE),
            _ => FALLBACK_APPROACH,
        }
    }

    fn begin_string(&mut self) {
        let Some(r) = self.run.as_ref() else { return };
        let Some(p) = r.plays.get(r.idx).copied() else {
            self.run = None;
            return;
        };
        let a = p.attacker.min(1);
        self.face(a);
        if self.in_range(a) {
            self.begin_swing();
        } else {
            self.play(a, BodyAct::Walk);
            let len = self.predict_approach(a);
            self.set_beat(BeatKind::Approach, len);
        }
    }

    fn begin_swing(&mut self) {
        let Some(p) = self.run.as_ref().and_then(|r| r.plays.get(r.idx).copied()) else {
            return;
        };
        let a = p.attacker.min(1);
        self.play(a, BodyAct::Swing(p.cmd));
        let len = self.actors[a]
            .player
            .as_ref()
            .map_or(PLAY_CADENCE_TICKS, |pl| {
                // The rate law's per-tick advance at the normal rate: `rate / 4`
                // keyframes.
                let rate = u32::from(
                    self.clip(a, BodyAct::Swing(p.cmd))
                        .map_or(2, |c| c.rate.max(1)),
                );
                (pl.frame_count() * 4).div_ceil(rate)
            });
        self.set_beat(BeatKind::Swing, len);
        if let Some(r) = self.run.as_mut() {
            r.hit_done = false;
        }
    }

    fn set_beat(&mut self, kind: BeatKind, len: u32) {
        if let Some(r) = self.run.as_mut() {
            r.kind = kind;
            r.at = 0;
            r.len = len;
            r.start = r.elapsed;
        }
    }

    /// The defender's reaction to play `idx` (`FUN_801EC3E4`'s pick).
    fn react(&mut self, idx: usize) {
        let Some(r) = self.run.as_ref() else { return };
        let p = r.plays[idx];
        let (a, d) = (p.attacker.min(1), p.attacker.min(1) ^ 1);
        self.hp[d] = p.hp_after[d];
        if p.damage <= 0 {
            return;
        }
        let string_ends = r.plays.get(idx + 1).is_none_or(|n| n.attacker.min(1) != a);
        let total: i32 = r.plays[..=idx]
            .iter()
            .rev()
            .take_while(|q| q.attacker.min(1) == a)
            .map(|q| q.damage.max(0))
            .sum();
        let has = |k: usize| self.clip(d, BodyAct::Reaction(k)).is_some();
        let quarter = self.hp_max[d] / 4;
        let killed = p.hp_after[d] <= 0;
        let knockdown =
            killed || (string_ends && has(3) && (total > quarter || p.hp_after[d] < quarter));
        let pick = if knockdown && has(2) {
            2
        } else {
            let first = usize::from(p.cmd.wrapping_sub(0x0C) % 10 >= 5);
            if has(first) { first } else { first ^ 1 }
        };
        self.play(d, BodyAct::Reaction(pick));
        if killed {
            self.actors[d].downed = true;
        }
    }

    /// One tick: every body's clip and root motion, then the play-out.
    pub fn tick(&mut self) {
        self.tick += 1;
        if !self.installed() {
            if let Some(r) = self.run.as_mut() {
                r.elapsed += 1;
                let span = r
                    .fixed
                    .as_ref()
                    .and_then(|f| f.last())
                    .map_or(0, |b| b.start + b.len);
                if r.elapsed >= span {
                    self.run = None;
                }
            }
            return;
        }
        if !self.seated {
            self.seat();
        }
        let approaching = self
            .run
            .as_ref()
            .filter(|r| r.kind == BeatKind::Approach)
            .and_then(|r| r.plays.get(r.idx))
            .map(|p| p.attacker.min(1));
        for i in 0..2 {
            self.tick_body(i, approaching == Some(i));
        }
        self.tick_run();
    }

    fn tick_body(&mut self, i: usize, approaching: bool) {
        let Some(mut player) = self.actors[i].player.take() else {
            return;
        };
        // The decoder blends the last frame toward the clip queued behind
        // this one, its Z delta biased by the entry's `+0x0E` end step
        // (`FUN_8004998C`), so the natural end's step is carried in model
        // space rather than snapped (`World::battle_tween_target`).
        let next = self.next_act(i);
        let frame0 = if player.is_looping() {
            Some(player.first_frame().to_vec())
        } else {
            self.clip(i, next).and_then(|c| c.frames.first().cloned())
        }
        .filter(|f| f.len() == player.part_count());
        let down = self.hp[i] == 0;
        player.set_tween_target(Some(TweenTarget {
            frame0: if down { None } else { frame0 },
            z_bias: if down { 0 } else { player.end_root_step() },
        }));
        let pose = player.tick_rated(AnimRate::default(), false);
        self.actors[i].pose = pose.bone_outputs;
        let speed = player.root_speed();
        let natural_end = player.take_natural_end();
        let end_step = player.end_root_step();
        let finished = player.finished();
        self.actors[i].player = Some(player);
        // The playing clip's root motion: a positive speed only while the
        // range poll still fails, a negative one unconditionally. The
        // approach walk is driven below instead.
        if !approaching && speed != 0 && !(speed > 0 && self.in_range(i)) {
            self.step_along(i, speed, SPEED_SCALE);
        }
        // The natural end's step along the facing, unless the body is down.
        if natural_end && end_step != 0 && self.hp[i] != 0 {
            let (sin, cos) = legaia_engine_system::trig::trig12(self.facing[i]);
            self.pos[i][0] += (i32::from(sin) * i32::from(end_step)) >> 12;
            self.pos[i][1] += (i32::from(cos) * i32::from(end_step)) >> 12;
        }
        if finished && !self.actors[i].downed {
            let next = self.next_act(i);
            if next != self.actors[i].act {
                self.play(i, next);
            }
        }
        if approaching {
            let d = i ^ 1;
            if !self.in_range(i) {
                let (speed, scale) = self.approach_drive(i);
                let before = self.pos[i];
                let (dx, dz) = self.step_along(i, speed, scale);
                // A step never crosses the body it walks at (engine guard,
                // as `World::drive_attack_approach`).
                for (k, step) in [dx, dz].into_iter().enumerate() {
                    let (lo, hi) = if before[k] <= self.pos[d][k] {
                        (before[k], self.pos[d][k])
                    } else {
                        (self.pos[d][k], before[k])
                    };
                    self.pos[i][k] = (before[k] + step).clamp(lo, hi);
                }
            }
        }
    }

    fn tick_run(&mut self) {
        let Some(r) = self.run.as_mut() else { return };
        r.elapsed += 1;
        r.at += 1;
        let (kind, idx, at) = (r.kind, r.idx, r.at);
        let Some(p) = r.plays.get(idx).copied() else {
            self.run = None;
            return;
        };
        let a = p.attacker.min(1);
        match kind {
            BeatKind::Approach => {
                if self.in_range(a) || at >= APPROACH_GUARD_TICKS {
                    self.begin_swing();
                }
            }
            BeatKind::Swing => {
                let (frame, first_event, frames) =
                    self.actors[a].player.as_ref().map_or((0, 0, 1), |pl| {
                        let ev = pl
                            .hit_source()
                            .and_then(|h| h.event_frames.into_iter().find(|&e| e != 0))
                            .map_or(pl.frame_count() / 2, u32::from);
                        (pl.current_frame().max(0) as u32, ev, pl.frame_count())
                    });
                let swinging = matches!(self.actors[a].act, BodyAct::Swing(_));
                let hit_done = self.run.as_ref().is_some_and(|r| r.hit_done);
                if !hit_done && (frame >= first_event.min(frames.saturating_sub(1)) || !swinging) {
                    if let Some(r) = self.run.as_mut() {
                        r.hit_done = true;
                    }
                    self.react(idx);
                }
                if !swinging {
                    // The swing ran out: the next play of the string swings
                    // from here; a string's last play closes on the done band.
                    let Some(r) = self.run.as_mut() else { return };
                    let next_same = r.plays.get(idx + 1).is_some_and(|n| n.attacker.min(1) == a);
                    if next_same {
                        r.idx += 1;
                        self.begin_swing();
                    } else {
                        self.set_beat(BeatKind::Done, DONE_TAIL_TICKS);
                    }
                }
            }
            BeatKind::Done => {
                if at >= DONE_TAIL_TICKS {
                    let Some(r) = self.run.as_mut() else { return };
                    r.idx += 1;
                    if r.idx >= r.plays.len() {
                        self.run = None;
                    } else {
                        self.begin_string();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_actor_formats::monster_archive::PartPose;

    fn anim(
        tag: u8,
        frames: usize,
        rate: u8,
        speed: i16,
        end_step: i16,
        event: u8,
    ) -> MonsterAnimation {
        let mut head = vec![0u8; 0x54];
        head[0] = tag;
        head[0x0C..0x0E].copy_from_slice(&speed.to_le_bytes());
        head[0x0E..0x10].copy_from_slice(&end_step.to_le_bytes());
        head[0x10] = event;
        MonsterAnimation {
            variant_track: Default::default(),
            action_id: tag,
            rate,
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            part_count: 1,
            frame_count: frames,
            frames: vec![vec![PartPose::default()]; frames],
            effect_script: head,
        }
    }

    fn body(party: bool) -> ChoreoBody {
        ChoreoBody {
            clips: ChoreoClips {
                idle: anim(0, 8, 2, 0, 0, 0),
                walk: Some(anim(1, 8, 2, 20, 0, 0)),
                reactions: [
                    Some(anim(2, 6, 2, -10, 0, 0)),
                    Some(anim(3, 6, 2, -10, 0, 0)),
                    Some(anim(4, 10, 2, 0, 0, 0)),
                    Some(anim(5, 6, 2, 0, 0, 0)),
                ],
                swings: [0xC, 0xD, 0xE, 0xF].map(|t| Some(anim(t, 12, 2, 8, 0, 5))),
                by_entry: Vec::new(),
            },
            party,
            reach: if party {
                motion::PARTY_REACH_OFFSETS[0]
            } else {
                0
            },
            size_class: if party { 0 } else { 0x10 },
        }
    }

    fn play(attacker: usize, damage: i32, hp_after: [i32; 2]) -> DomePlay {
        DomePlay {
            attacker,
            cmd: 0xC,
            power: 1,
            damage,
            hp_after,
        }
    }

    fn run(c: &mut DomeChoreo, guard: u32) -> Vec<(Beat, u32)> {
        let mut seen = Vec::new();
        for _ in 0..guard {
            if !c.running() {
                break;
            }
            if let Some(b) = c.beat() {
                seen.push(b);
            }
            c.tick();
        }
        assert!(!c.running(), "the play-out ends");
        seen
    }

    #[test]
    fn the_approach_stops_on_the_range_law_not_on_a_margin() {
        let mut c = DomeChoreo::default();
        c.install([body(true), body(false)]);
        let gap0 = c.pos[1][1] - c.pos[0][1];
        c.start(&[play(0, 10, [100, 90])], true, [100, 100], [100, 100]);
        assert_eq!(c.beat().unwrap().0.kind, BeatKind::Approach);
        let mut swung = false;
        for _ in 0..400 {
            c.tick();
            if c.beat().is_some_and(|(b, _)| b.kind == BeatKind::Swing) {
                swung = true;
                break;
            }
        }
        assert!(swung, "the walk reaches the swing");
        // Vahn's `+43` offset against a size-`0x10` monster: in range once
        // `43 + gap < 0x10 << 4`, so the walk closes to just under 213.
        let gap = c.pos[1][1] - c.pos[0][1];
        assert!(gap < gap0 && (180..=213).contains(&gap), "gap {gap}");
    }

    #[test]
    fn a_hit_lands_on_the_event_frame_and_the_string_closes_on_the_done_band() {
        let mut c = DomeChoreo::default();
        c.install([body(true), body(false)]);
        let plays = [
            play(0, 10, [100, 90]),
            play(0, 10, [100, 80]),
            play(1, 5, [95, 80]),
        ];
        c.start(&plays, true, [100, 100], [100, 100]);
        let mut reacted_at = None;
        let mut swing_start = None;
        for t in 0..2000 {
            if !c.running() {
                break;
            }
            if swing_start.is_none() && c.beat().is_some_and(|(b, _)| b.kind == BeatKind::Swing) {
                swing_start = Some(t);
            }
            c.tick();
            if reacted_at.is_none() && matches!(c.act(1), BodyAct::Reaction(_)) {
                reacted_at = Some(t);
            }
        }
        let (s, r) = (swing_start.unwrap(), reacted_at.unwrap());
        // Event frame 5 at `rate / 4 = 0.5` keyframes a tick: ten ticks in.
        assert!(
            (9..=11).contains(&(r - s)),
            "hit {} ticks into the swing",
            r - s
        );
        let mut c = DomeChoreo::default();
        c.install([body(true), body(false)]);
        c.start(&plays, true, [100, 100], [100, 100]);
        let kinds: Vec<_> = run(&mut c, 4000)
            .into_iter()
            .map(|(b, _)| (b.kind, b.attacker))
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0] != w[1])
            .map(|w| w[1])
            .collect();
        use BeatKind::*;
        assert_eq!(kinds, [(Swing, 0), (Done, 0), (Swing, 1), (Done, 1)]);
    }

    #[test]
    fn a_killing_hit_knocks_the_defender_down_and_holds() {
        let mut c = DomeChoreo::default();
        c.install([body(true), body(false)]);
        c.start(&[play(0, 100, [100, 0])], false, [100, 100], [100, 100]);
        run(&mut c, 2000);
        assert_eq!(c.act(1), BodyAct::Reaction(2));
        for _ in 0..100 {
            c.tick();
        }
        assert_eq!(c.act(1), BodyAct::Reaction(2), "no get-up off a kill");
    }

    #[test]
    fn clips_play_at_the_battle_rate() {
        // `FUN_80047430` at the normal rate: a 12-keyframe rate-2 swing runs
        // 24 ticks.
        let mut c = DomeChoreo::default();
        c.install([body(true), body(false)]);
        c.pos = [[0, -100], [0, 100]];
        c.start(&[play(0, 10, [100, 90])], false, [100, 100], [100, 100]);
        let (b, _) = c.beat().unwrap();
        assert_eq!((b.kind, b.len), (BeatKind::Swing, 24));
    }

    #[test]
    fn without_bodies_the_fixed_schedule_runs() {
        let mut c = DomeChoreo::default();
        let plays = [play(0, 1, [10, 9]), play(0, 1, [10, 8]), play(1, 1, [9, 8])];
        c.start(&plays, true, [10, 10], [10, 10]);
        let total = APPROACH_TICKS + 3 * PLAY_CADENCE_TICKS + 2 * DONE_TAIL_TICKS;
        assert_eq!(turn_playback_ticks(&plays, true), total);
        let mut n = 0;
        while c.running() {
            c.tick();
            n += 1;
            assert!(n <= total);
        }
        assert_eq!(n, total);
        let beats = turn_timeline(&plays, true);
        for w in beats.windows(2) {
            assert_eq!(w[0].start + w[0].len, w[1].start);
        }
    }
}
