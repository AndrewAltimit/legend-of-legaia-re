//! The strike clock: clip headers, per-fighter strike tables and the action-table keyframe lookup.

use super::*;
use legaia_minigame_formats::baka_opponents::BakaActionSet;

/// Where a fighter's strike clock stands: the per-fighter word at block
/// `+0x0C` (`&DAT_801dbfc8[slot * 0x2a]`) the combat tick and the damage
/// kernel hand between them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StrikeState {
    /// `0` - a fresh commit; the keyframe lookup runs every tick.
    #[default]
    Armed,
    /// `1` - the cursor crossed a new sub-keyframe; the resolution SM may
    /// book the exchange (`FUN_801D3468` tests `== 1` before every damage
    /// call, `0x801D36DC` / `0x801D3730` / `0x801D378C`).
    Landed,
    /// `2` - the damage kernel consumed the strike (`FUN_801D3B18` writes it
    /// to the winner at `0x801D3EB0`); the lookup re-arms, so a later
    /// sub-keyframe of the same clip can land again (the special's second
    /// strike).
    Consumed,
}

/// One fighter's strike clock - the frame cursor the retail combat tick
/// `FUN_801D3F44` runs over the chosen attack, and the lookup it feeds.
///
/// Per tick, while an attack is chosen: unless a strike is already
/// [`StrikeState::Landed`], look the cursor's last step up through
/// [`keyframe_in_range`] (`0x801D4334`: `from` = the cursor before the step,
/// block `+0x90`; `to` = after it, actor `+0x68`), and a hit on a sub-keyframe
/// other than the one cached at block `+0x98` lands it. Then the step itself:
/// `from = to; to += frame_step * speed * 8 >> 3` (`+0x6A` from record `+0x04`
/// times the frame-rate divisor `DAT_1F80037D`, which the round setup seeds to
/// `8` at `0x801D01A4`; the clip selector `FUN_800204F8` adds it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StrikeClock {
    /// Actor `+0x68` - the clip cursor, 1/16-frame fixed point.
    pub cursor: i32,
    /// Block `+0x90` - the cursor before the last step.
    pub prev: i32,
    /// Block `+0x98` - the sub-keyframe last landed (`-1` = `None` at commit).
    pub landed: Option<usize>,
    /// Block `+0x0C`.
    pub state: StrikeState,
    /// Set on the commit tick: retail's lookup that tick ran *before* the
    /// commit, against the previous clip, so the new clip's first lookup is
    /// the next tick's `[0, step]`.
    fresh: bool,
}

/// The frame-rate divisor the round setup stores at `DAT_1F80037D`
/// (`0x801D01A4`); the cursor step is `speed * divisor >> 3`, so at `8` the
/// step is the record's speed verbatim. A special's commit lowers it to
/// [`crate::baka_fighter_chrome::SPECIAL_RATE_DIVISOR`] for the rest of the
/// round (`0x801D4568`), and the tick recomputes both fighters' steps from it
/// every frame (`0x801D4780..0x801D47CC`) - the special plays in slow motion.
pub const STRIKE_RATE_DIVISOR: i32 = 8;

/// The cameo's clip step: its animator forces `+0x6A = 8` every frame
/// (`0x801D6320..0x801D632C`) - half a frame per tick.
pub const CAMEO_STEP: i32 = 8;

/// The three ANM record header fields the clip selector `FUN_800204F8` and
/// the afterimage read off a fighter's clip record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipHeader {
    /// Record `+0x02` - the clip's frame count.
    pub frames: u16,
    /// Record `+0x01` bit 0: the selector steps the cursor by
    /// `(step * 2 + n - 1) / n` instead of `step` (`0x800205B4..0x800205E4`).
    pub double_step: bool,
    /// Record `+0x06` - the `n` of that formula.
    pub n: u8,
}

impl ClipHeader {
    /// From a parsed record's header words (`legaia_actor_formats::player_anm`
    /// `PlayerAnmRecord { a, b, flag, .. }`: `a` = bytes `+0/+1`, `b` =
    /// `+2/+3`, `flag` = `+6/+7`).
    pub fn from_record_words(a: u16, b: u16, flag: u16) -> Self {
        ClipHeader {
            frames: b,
            double_step: (a >> 8) & 1 != 0,
            n: (flag & 0xFF) as u8,
        }
    }

    /// The cursor step the clip selector applies for an actor step `step`.
    pub fn selector_step(&self, step: i32) -> i32 {
        if self.double_step {
            let n = i32::from(self.n.max(1));
            (step * 2 + n - 1) / n
        } else {
            step
        }
    }
}

/// One fighter's nine clip headers, by action.
pub type ClipHeaders =
    [Option<ClipHeader>; legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER];

/// A clip bank's headers for the nine actions starting at `first_record`.
pub fn clip_headers_from_bundle(
    bundle: &legaia_actor_formats::player_anm::PlayerAnmBundle,
    first_record: usize,
) -> ClipHeaders {
    std::array::from_fn(|action| {
        let r = bundle.record(first_record + action).ok()?;
        Some(ClipHeader::from_record_words(r.a, r.b, r.flag))
    })
}

/// Every roster fighter's clip headers, indexed by roster id, read off the
/// disc through `read_prot` (extraction PROT index -> entry bytes).
///
/// The party fighters (roster `0..3`) pose off the PROT 1203 battle-form bank,
/// nine records each (`id * 9 + action`); the ladder fighters (`3..`) off
/// their own pack's bank (PROT `1206 + id - 3`, record = action) - the same
/// two banks the duel's clip ids resolve into (see
/// `legaia_minigame_formats::baka_opponents::action_slot_label`). A fighter whose bank
/// does not decode gets all-`None` headers.
pub fn roster_clip_headers(read_prot: impl Fn(usize) -> Option<Vec<u8>>) -> Vec<ClipHeaders> {
    use legaia_minigame_formats::baka_opponents as bo;
    let party = read_prot(bo::BAKA_HUD_ART_PROT_INDEX).and_then(|e| {
        legaia_actor_formats::player_anm::find_in_entry(&e, 4)
            .into_iter()
            .next()
    });
    (0..bo::OPPONENT_COUNT)
        .map(|roster| {
            if roster < bo::FIGHTER_PACK_FIRST_ROSTER_ID {
                party
                    .as_ref()
                    .map(|b| clip_headers_from_bundle(b, roster * bo::ACTIONS_PER_FIGHTER))
                    .unwrap_or([None; bo::ACTIONS_PER_FIGHTER])
            } else {
                bo::fighter_pack_prot_index(roster)
                    .and_then(&read_prot)
                    .and_then(|e| bo::parse_fighter_pack(&e))
                    .and_then(|p| legaia_actor_formats::player_anm::parse(&p.anim_bytes).ok())
                    .map(|b| clip_headers_from_bundle(&b, 0))
                    .unwrap_or([None; bo::ACTIONS_PER_FIGHTER])
            }
        })
        .collect()
}

/// One fighter's strike data - the action records' `+0x04` speed and `+0x26`
/// strike-frame column, indexed by action (the attack types are actions
/// `1..=4`), plus the clip headers when a host staged the fighter's clip bank.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StrikeTable {
    pub speed: [i32; legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER],
    pub frames: [Vec<i16>; legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER],
    /// Per-action clip header; `None` until a host supplies the fighter's
    /// clip bank ([`BakaFight::set_clip_headers`]). Without it the strike
    /// clock steps by the record speed alone, which is exact for every clip
    /// whose record leaves the double-step bit clear.
    pub clips: [Option<ClipHeader>; legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER],
    /// Per-action strike offsets - each live sub-keyframe's `+0x20 / +0x22 /
    /// +0x24` TRS, in slot order (the column the impact pair reads).
    pub offsets: [Vec<[i16; 3]>; legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER],
}

impl StrikeTable {
    /// Lift a fighter's parsed action set.
    pub fn from_actions(actions: &BakaActionSet) -> Self {
        let mut frames: [Vec<i16>; legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER] =
            Default::default();
        for (a, f) in frames.iter_mut().enumerate() {
            *f = actions.strike_frames(a);
        }
        let offsets = std::array::from_fn(|a| {
            actions
                .sub_keyframes
                .get(a)
                .map(|k| k.iter().map(|s| s.offset).collect())
                .unwrap_or_default()
        });
        StrikeTable {
            speed: actions.speed,
            frames,
            clips: [None; legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER],
            offsets,
        }
    }

    /// The action record an attack type plays (types `1..=4` are actions
    /// `1..=4`: `record[+0x10] = anim - base - 1`).
    pub(super) fn action_of(attack: BakaAttack) -> usize {
        attack.type_id() as usize
    }
}

impl StrikeClock {
    /// A commit: cursor to the clip start, nothing landed (`+0x98 = -1`,
    /// `+0x68 = 0`, both blocks' `+0x0C = 0` at `0x801D44B8..0x801D44D8`).
    pub(super) fn commit(&mut self) {
        *self = StrikeClock {
            fresh: true,
            ..StrikeClock::default()
        };
    }

    /// One combat-tick step of the clock over `frames` at `speed`.
    ///
    /// REF: FUN_801d3f44 (`0x801D4304..0x801D435C` lookup, `0x801D47E0..
    /// 0x801D47EC` the `+0x90` store before the clip selector's advance)
    pub(super) fn step(
        &mut self,
        frames: &[i16],
        speed: i32,
        divisor: i32,
        clip: Option<ClipHeader>,
        frame_step: i32,
    ) {
        if !self.fresh
            && self.state != StrikeState::Landed
            && let Some(i) = keyframe_in_range(frames, self.prev, self.cursor)
            && self.landed != Some(i)
        {
            self.landed = Some(i);
            self.state = StrikeState::Landed;
        }
        self.fresh = false;
        let step = sra3_round_to_zero(speed * divisor);
        let step = clip.map_or(step, |c| c.selector_step(step));
        self.prev = self.cursor;
        self.cursor = self.cursor.wrapping_add(frame_step * step);
    }
}

/// `x >> 3` rounding toward zero - the `bgez; addiu 7; sra 3` idiom at
/// `0x801D47AC..0x801D47C4`.
fn sra3_round_to_zero(v: i32) -> i32 {
    (if v < 0 { v + 7 } else { v }) >> 3
}

// --- Action-table keyframe lookup ------------------------------------------

/// Arithmetic shift-right-by-4 rounding toward zero - the retail
/// `bgez v, skip; addiu v, v, 0xf; skip: sra v, v, 4` idiom (a plain `>> 4`
/// on a negative would round toward minus infinity).
fn sra4_round_to_zero(v: i32) -> i32 {
    (if v < 0 { v + 0xF } else { v }) >> 4
}

/// PORT: FUN_801d6e5c - action-table keyframe lookup by frame range.
///
/// Wired: the combat tick's call at `0x801D4334` is [`StrikeClock`]'s
/// per-tick lookup (`a0` = the fighter's table `s2[+0x94]`, `a1` = the action
/// `s4[+0x5C] - (s2[+0x14] + 1)`, `a2` = the pre-step cursor `s2[+0x90]`,
/// `a3` = the cursor `s4[+0x68]`), which every duel built from the disc
/// tables runs on all three hosts through [`BakaFight::tick_with_input`].
///
/// Returns the index of the first sub-keyframe whose whole-frame index (the
/// action record's `+0x26` field, one per `0x08`-byte sub-keyframe) falls
/// within the query range, or `None` when the range is inverted (`to < from`),
/// the action has no sub-keyframes, or none match.
///
/// The fixed point sits on the **query**, not on the record: `from` and `to`
/// are shifted right by 4 (rounding toward zero) and compared against the raw
/// frame indices, so callers pass a `<< 4` fixed-point frame range against the
/// whole-frame keyframe values. `frame_indices` is the action record's
/// per-sub-keyframe `+0x26` column (its length is the record's `+0x1c` count);
/// the retail function reaches it through `PTR_DAT_801db8b8[char][action]`.
pub fn keyframe_in_range(frame_indices: &[i16], from: i32, to: i32) -> Option<usize> {
    if to < from {
        return None;
    }
    let lo = sra4_round_to_zero(from);
    let hi = sra4_round_to_zero(to);
    frame_indices
        .iter()
        .position(|&f| lo <= f as i32 && (f as i32) <= hi)
}
