//! The attack band's **hit-event driver**: the head of the damage kernel
//! `FUN_801EC3E4` that decides, once per anim-tick call, whether the frame the
//! committed clip is on is one of that clip's hit events - and the per-hit
//! resolution helpers the engine runs when it is.
//!
//! PORT: FUN_801EC3E4 (head guard chain `0x801EC41C..0x801EC484` and the
//! `+0x1F4` epilogue bump `0x801EECDC..0x801EECE8`)
//! REF: FUN_80047430 (the anim tick that calls the kernel every frame with
//! the cursor frame in `a2`: `0x800478A0`, and again post-commit at
//! `0x80047BF0`)
//! REF: FUN_8004AD80 (every clip commit zeroes `+0x1F4`: `0x8004B064`)
//!
//! ## What retail does
//!
//! The strike loop of the action SM (`FUN_801E295C` state `0x1E`) stages one
//! action byte into `+0x1DA` and **never calls a damage kernel**. Damage is
//! the anim tick's business: `FUN_80047430` calls
//! `FUN_801EC3E4(actor, committed_entry, cursor >> 4)` on every frame a
//! battle clip plays (`0x8004787C..0x800478A4`, gated only on the battle
//! overlay being resident, `DAT_8007BD71 == 0xFF`), and the kernel's own head
//! decides whether *this* frame is a hit:
//!
//! ```text
//! 801ec41c  lbu v1,0x7(v0)  ; li v0,0x5a ; beq -> exit     ctx[7] != 0x5A
//! 801ec440  lbu a0,0x0(a1)                                  a0 = entry[0]  (power run byte 0)
//! 801ec448  addiu a0,a0,-0xc ; sltiu a0,a0,0x14 ; beq -> exit   entry[0] in 0x0C..=0x1F
//! 801ec45c  lbu v1,0x1f4(v0)                                v1 = hit index (+0x1F4)
//! 801ec464  addu a1,a1,v1 ; lbu a0,0x10(a1)                 a0 = entry[0x10 + hit]  (event frame)
//! 801ec46c  addiu v0,v0,0x1 ; slt v0,v0,a0 ; bne -> exit    frame + 1 >= event frame
//! 801ec47c  beq a0,zero -> exit                             event frame != 0
//! 801ec480  _sltiu v0,v1,0x4 ; beq -> exit                  hit index < 4
//! 801ec494  lbu a0,0x0(a1)                                  power byte = entry[hit]
//! ...
//! 801eecdc  lbu v0,0x1f4(v1) ; addiu ; sb v0,0x1f4(v1)      +0x1F4 += 1 (epilogue, every resolved call)
//! ```
//!
//! So one committed clip fires **one hit per non-zero entry of its
//! `+0x10..+0x13` event list** (at most four), each on the first frame at
//! or past `event_frame - 1`, each with the power byte at the same index of
//! the entry's `+0x00..+0x03` run; and because every commit zeroes `+0x1F4`
//! (`FUN_8004AD80`, `0x8004B064`) the index restarts per clip. The staged
//! byte only picks the clip; the clip's own header paces and powers the
//! hits. A swing entry (`0x0C..0x0F`) carries one event frame and its own
//! tag as power byte 0, so a direction swing is exactly one hit with the
//! command it was staged with; an art record carries up to four; the `0x19`
//! starter records and every reaction / idle entry carry `entry[0]` outside
//! the band and never hit.
//!
//! The kernel's *body* - the roll pair, the equipment fold, the finisher -
//! is ported in `crate::battle_formulas` (`physical_predamage`,
//! `arms_weapon_atk_fold`); this module is only the head that decides
//! *when* the body runs and *which* power byte it takes.

use super::*;
use crate::battle_formulas::{arms_resolver_admits, arms_weapon_atk_fold};

/// Number of hit-event slots an entry carries (`sltiu v0,v1,0x4` at
/// `0x801EC480`): the `+0x00..+0x03` power run and the `+0x10..+0x13`
/// event-frame list are both four wide.
pub const HIT_EVENT_SLOTS: u8 = 4;

/// Lowest power-run byte 0 the kernel admits (`addiu a0,a0,-0xc` at
/// `0x801EC448`).
pub const HIT_POWER_BASE: u8 = 0x0C;

/// Width of the admitted power band (`sltiu a0,a0,0x14`): `0x0C..=0x1F`.
pub const HIT_POWER_SPAN: u8 = 0x14;

/// Action-state (`ctx[7]`) value that makes the kernel return before reading
/// anything (`0x801EC41C..0x801EC424`).
pub const HIT_BLOCKED_ACTION_STATE: u8 = 0x5A;

/// The frame slack the tick's event-path commit waits past the gate frame
/// (`addiu v0,v0,0x2 ; slt v0,v0,s0` at `0x80047930..0x80047934`).
pub const EVENT_COMMIT_SLACK: i16 = 2;

/// One hit event the kernel admitted for the frame it was called on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitEvent {
    /// The actor's `+0x1F4` value the hit resolved under (0-based; the
    /// caller bumps it afterwards, the retail epilogue).
    pub hit_index: u8,
    /// `entry[hit_index]` - the power byte this hit resolves damage from.
    pub power_byte: u8,
    /// `entry[0x10 + hit_index]` - the clip frame the hit was authored on.
    pub event_frame: u8,
}

/// The head guard chain of `FUN_801EC3E4`: does the call made on `frame` fire
/// hit `hit_index` of the committed entry?
///
/// `power_run` is the entry's `+0x00..+0x04`, `event_frames` its
/// `+0x10..+0x14`, `hit_index` the actor's `+0x1F4`, `frame` the anim
/// cursor in whole keyframes (`node[+0x68] >> 4`, truncated to a byte the
/// way `andi a2,a2,0xff` does).
///
/// The five guards in retail order. The event-frame read happens before
/// the `< 4` bound in retail (index 4 reads `entry[0x14]`, the first effect
/// record's gate byte, and then bails on the bound) - the read is harmless
/// and the port simply bounds first.
///
/// PORT: FUN_801EC3E4 (`0x801EC41C..0x801EC484`)
pub fn hit_event_admits(
    action_state: u8,
    power_run: &[u8; 4],
    event_frames: &[u8; 4],
    hit_index: u8,
    frame: u8,
) -> Option<HitEvent> {
    if action_state == HIT_BLOCKED_ACTION_STATE {
        return None;
    }
    if power_run[0].wrapping_sub(HIT_POWER_BASE) >= HIT_POWER_SPAN {
        return None;
    }
    if hit_index >= HIT_EVENT_SLOTS {
        return None;
    }
    let event_frame = event_frames[usize::from(hit_index)];
    if event_frame == 0 {
        return None;
    }
    // `frame + 1 < event_frame` exits; the compare is on the byte-truncated
    // frame plus one, so widen before adding.
    if u16::from(frame) + 1 < u16::from(event_frame) {
        return None;
    }
    Some(HitEvent {
        hit_index,
        power_byte: power_run[usize::from(hit_index)],
        event_frame,
    })
}

/// The beat the tick's two `FUN_80050E00` consumers resolve from an entry's
/// event list: `entry[0x10]` whenever any of `+0x11..+0x13` is zero, and
/// `None` for the four-slot list, where retail's return register carries a
/// load-address-dependent pointer byte instead (see
/// `docs/formats/monster-animation.md` § Event-frame list). Only
/// attack-band entries carry a four-slot list; for those the port treats
/// the gate as unresolvable and leaves the clip to its natural end.
///
/// PORT: FUN_80050E00 (as consumed by `FUN_80047430` at `0x8004791C` /
/// `0x80047E30`)
pub fn event_commit_gate_frame(event_frames: &[u8; 4]) -> Option<u8> {
    if event_frames[1..].contains(&0) {
        Some(event_frames[0])
    } else {
        None
    }
}

/// The tick's mid-clip **event-path commit** test (`+0x1DC` bit 1 arm,
/// `0x80047900..0x80047938`): with the entry's `+0x76` lock clear, the queued
/// clip commits - cutting the playing one short - once the cursor frame is
/// past the resolved gate frame by more than [`EVENT_COMMIT_SLACK`]. This is
/// what chains one swing into the next a few frames after its hit instead
/// of at the clip's natural end.
///
/// `frame` is the whole-keyframe cursor (`sra s0,s0,0x14` of the 12.4 word
/// shifted up 16 - i.e. a signed value).
///
/// PORT: FUN_80047430 (`0x80047900..0x80047948`)
pub fn event_commit_due(event_frames: &[u8; 4], lock: u8, frame: i16) -> bool {
    if lock != 0 {
        return false;
    }
    match event_commit_gate_frame(event_frames) {
        Some(gate) => i16::from(gate) + EVENT_COMMIT_SLACK < frame,
        None => false,
    }
}

/// Which art constant - if any - the committed clip on `slot` belongs to,
/// read off the **latched** raw staged id (`+0x1DB`, the byte the anim
/// commit copies before rewriting an art id to its dynamic slot) with
/// `chosen_art` as the fallback for a host that stages plain slot ids.
///
/// Party-only, exactly as retail draws the line: a monster's ids are archive
/// entry indices across the whole byte range, so the same `0x1B+` value
/// names a plain clip there, and `FUN_801EED1C` is the party setup hook.
pub fn staged_art_constant(
    latched: u8,
    chosen_art: Option<legaia_art::ActionConstant>,
    party_slot: bool,
) -> Option<legaia_art::ActionConstant> {
    if !party_slot {
        return None;
    }
    // A direction swing is a committed arms command and never an art hit,
    // whatever the host's fallback says - otherwise an arts turn's leading
    // arrows would be charged as the art as well as as swings.
    if is_swing_command(latched) {
        return None;
    }
    legaia_art::ActionConstant::from_byte(latched)
        .filter(|a| a.is_art())
        .or(chosen_art)
}

/// Build the [`ArtStrikeInfo`] for one admitted hit of an art clip on `slot`:
/// the power byte and event frame are the **clip entry's** (what the kernel
/// reads), the status effect and hit cue come from the art record the host
/// resolves for `(character, art)` when it has one.
///
/// The record is consulted for its side data only. Retail's kernel never
/// reads the art record at all - the materialized clip's entry *is* the
/// record's embedded entry - so a host with no record still resolves the
/// hit with the clip's power byte and no status / cue.
pub fn art_strike_info_for_hit<H: BattleActionHost + ?Sized>(
    host: &H,
    slot: u8,
    art: legaia_art::ActionConstant,
    hit: &HitEvent,
) -> ArtStrikeInfo {
    let (target, character, latched) = host
        .actor(slot)
        .map(|a| (a.active_target, a.character, a.latched_anim))
        .unwrap_or((0, legaia_art::Character::default(), 0));
    let rec = host.art_record(character, art);
    let idx = usize::from(hit.hit_index);
    ArtStrikeInfo {
        strike_index: hit.hit_index,
        anim_byte: latched,
        actor_slot: slot,
        target_slot: target,
        character,
        art,
        power: Some(legaia_art::PowerByte::from_byte(hit.power_byte)),
        dmg_timing: Some(hit.event_frame),
        enemy_effect: rec.map(|r| r.enemy_effect).unwrap_or_default(),
        hit_cue: rec.and_then(|r| r.hit_cues.get(idx).copied()),
    }
}

/// The kernel's execution-time equipment fold, run **at the hit** it belongs
/// to: for a party attacker whose committed command byte (`+0x1D9`) is one
/// of the six `PTR_801CF4B4` arms, add half the read equipment slots' ATK
/// bonus into ATK working (`+0x158`). The head guards are the same ones
/// [`hit_event_admits`] evaluated for this hit (`ctx[7]`, the admission band
/// on the power byte, the cursor bound, the party branch); the caller passes
/// the hit it admitted so the two cannot disagree.
///
/// PORT: FUN_801EC3E4 (the ATK-working fold at `+0x158`, reached from the
/// admitted head; arms in `crate::battle_formulas::arms_fold`)
pub fn fold_weapon_atk_on_hit<H: BattleActionHost + ?Sized>(
    host: &mut H,
    slot: u8,
    action_state: u8,
    hit: &HitEvent,
) -> Option<u16> {
    let (command, cursor) = host
        .actor(slot)
        .map(|a| (a.current_anim, a.input_cursor))
        .unwrap_or((0, 0));
    // `step_index + 1 >= step_count` is the `frame + 1 >= event_frame` test
    // the admitted hit already passed; hand the resolver the pair that
    // reproduces "passed".
    if !arms_resolver_admits(
        action_state,
        hit.power_byte,
        hit.event_frame,
        hit.event_frame,
        cursor,
        slot,
    ) {
        return None;
    }
    let bonuses = host.equip_attack_bonuses(slot);
    let delta = arms_weapon_atk_fold(command, &bonuses)?;
    let actor = host.actor_mut(slot)?;
    actor.atk_working = actor.atk_working.wrapping_add(delta);
    Some(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWING: [u8; 4] = [0x0C, 0, 0, 0];
    const ART: [u8; 4] = [0x1D, 0x19, 0x1F, 0x1A];

    #[test]
    fn a_swing_entry_fires_its_single_hit_at_the_event_frame_minus_one() {
        let events = [6, 0, 0, 0];
        for frame in 0..5u8 {
            assert!(
                hit_event_admits(0x1E, &SWING, &events, 0, frame).is_none(),
                "frame {frame} is before the beat"
            );
        }
        let hit = hit_event_admits(0x1E, &SWING, &events, 0, 5).expect("frame 5 = beat - 1");
        assert_eq!(
            hit,
            HitEvent {
                hit_index: 0,
                power_byte: 0x0C,
                event_frame: 6
            }
        );
        // After the bump the list's next slot is zero: no second hit, ever.
        assert!(hit_event_admits(0x1E, &SWING, &events, 1, 40).is_none());
    }

    #[test]
    fn an_art_entry_walks_its_four_hits_by_index() {
        let events = [4, 9, 14, 20];
        let mut fired = Vec::new();
        let mut idx = 0u8;
        for frame in 0..30u8 {
            if let Some(hit) = hit_event_admits(0x1E, &ART, &events, idx, frame) {
                fired.push((frame, hit.hit_index, hit.power_byte));
                idx += 1;
            }
        }
        assert_eq!(
            fired,
            vec![(3, 0, 0x1D), (8, 1, 0x19), (13, 2, 0x1F), (19, 3, 0x1A)]
        );
        assert!(
            hit_event_admits(0x1E, &ART, &events, 4, 40).is_none(),
            "the index bound stops a fifth call"
        );
    }

    #[test]
    fn out_of_band_entries_and_the_blocked_state_never_hit() {
        let events = [3, 0, 0, 0];
        // Idle / reaction / starter entries: byte 0 below 0x0C.
        assert!(hit_event_admits(0x1E, &[0, 0, 0, 0], &events, 0, 9).is_none());
        assert!(hit_event_admits(0x1E, &[2, 0, 0, 0], &events, 0, 9).is_none());
        // Above the band.
        assert!(hit_event_admits(0x1E, &[0x20, 0, 0, 0], &events, 0, 9).is_none());
        // ctx[7] == 0x5A.
        assert!(hit_event_admits(0x5A, &SWING, &events, 0, 9).is_none());
        // No event frame at all.
        assert!(hit_event_admits(0x1E, &SWING, &[0, 0, 0, 0], 0, 9).is_none());
    }

    #[test]
    fn event_commit_gate_resolves_the_first_beat_except_for_four_slot_lists() {
        assert_eq!(event_commit_gate_frame(&[6, 0, 0, 0]), Some(6));
        assert_eq!(event_commit_gate_frame(&[4, 9, 0, 0]), Some(4));
        assert_eq!(event_commit_gate_frame(&[0, 0, 0, 0]), Some(0));
        assert_eq!(event_commit_gate_frame(&[4, 9, 14, 20]), None);
        // The cut waits two frames past the gate, and a set lock blocks it.
        assert!(!event_commit_due(&[6, 0, 0, 0], 0, 8));
        assert!(event_commit_due(&[6, 0, 0, 0], 0, 9));
        assert!(!event_commit_due(&[6, 0, 0, 0], 1, 30));
        assert!(!event_commit_due(&[4, 9, 14, 20], 0, 30));
    }

    #[test]
    fn staged_art_constant_reads_the_latched_id_on_party_slots_only() {
        use legaia_art::ActionConstant;
        assert_eq!(
            staged_art_constant(ActionConstant::Art1C.as_byte(), None, true),
            Some(ActionConstant::Art1C)
        );
        assert_eq!(
            staged_art_constant(ActionConstant::Art1C.as_byte(), None, false),
            None,
            "a monster's 0x1C is an archive entry index"
        );
        assert_eq!(staged_art_constant(SWING_LEFT, None, true), None);
        assert_eq!(
            staged_art_constant(SWING_LEFT, Some(ActionConstant::Art1C), true),
            None,
            "a swing byte is never an art hit, fallback or not"
        );
        assert_eq!(
            staged_art_constant(0x10, Some(ActionConstant::Art1B), true),
            Some(ActionConstant::Art1B),
            "a plain slot id falls back to the host's chosen art"
        );
    }
}
