//! PROT 0957's **Death Game** body (`0x801F798C`, action `0x76`): four cards
//! dealt face up, shuffled in thirteen swaps, one picked on the pad or by the
//! timer, and the card's result on the caster and its victim.
//!
//! See `ghidra/scripts/funcs/overlay_summon_effect_table_0957_801f798c.txt`.

use super::capture::{CaptureCamArm, CaptureDrift, CapturePoseSet, CaptureSeatWrite};
use super::v_windhash::{HIDDEN, seat};
use super::{ModuleCamSeats, ModuleCamState, ModuleShot, focus_on, yaw_from};

/// PROT 0957's Death Game tick body.
pub const DEATH_GAME_BODY: u32 = crate::cast_module_ticks::SUMMON_EFFECT_TICK_B;
/// Arm 1's four records (`0x801F7D7C`).
const BACKDROP_RECORD: u32 = 0x801F_9C20;
/// Arm 2's card faces by the table's value `1..=4` (`0x801F7EC4..0x801F7F04`).
pub const CARD_RECORDS: [u32; 4] = [0x801F_9C64, 0x801F_9CE0, 0x801F_9D5C, 0x801F_9DD8];
/// The back each card turns into on the pick (`0x801F8B78`).
pub const PICK_RECORD: u32 = 0x801F_A020;
/// The cursor's left stop and its step (`0x801F87D8`, `0x801F8910`).
const CURSOR_HOME: i16 = 0x28;
const CURSOR_STEP: i16 = 0x4C;
/// A card's column: `x = -0x120 + 0xC0 * i` (`0x801F7E78`, `0x801F7F20`).
const CARD_FIRST_X: i16 = -0x120;
const CARD_STEP_X: i16 = 0xC0;
/// The shuffle's depth axis (`li v1,0x280` at `0x801F83AC`).
const CARD_Z: i16 = 0x280;
/// The neutral tint word (`lui 0x2008; ori 0x200`).
const SHOWN_COLOUR: u32 = 0x2008_0200;
/// The rate the closing arms restore.
const FULL_RATE: u8 = 8;
/// The caster monsters that deal a stacked table (`li v0,0x9d` at `0x801F7C4C`,
/// `li v0,0x9e` at `0x801F7CA0`).
const STACKED_ONCE: u8 = 0x9D;
const STACKED_ALL: u8 = 0x9E;

/// What the picked card does. The table holds `1..=4`; the card's value picks
/// the arm at `0x801F91EC..0x801F9220`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathGameOutcome {
    /// `1`: the victim's whole bar, standing before the caster (`0x801F922C`).
    VictimFalls,
    /// `2`: the caster's whole bar, captured (`0x801F935C`).
    CasterFalls,
    /// `3`: a quarter of each bar, rounded up (`0x801F9390`).
    BothBleed,
    /// `4`: a quarter of each maximum, rounded up, back (`0x801F95D0`).
    BothHeal,
}

/// The pad reads of arm 5, fed by the host (`_DAT_8007B874` against `0x2000`,
/// `0x8000` and `_DAT_800846D0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeathGamePad {
    pub right: bool,
    pub left: bool,
    pub confirm: bool,
}

/// The body's module words. Every word is `0x801FA2xx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeathGame {
    /// `0x801FA288`: each card's value.
    pub table: [u8; 4],
    /// `0x801FA2B8`'s parts, as `+0x14` / `+0x16` / `+0x18`.
    pub cards: [[i16; 3]; 4],
    /// Which handles still hold a part (arm 9 zeroes the picked one's).
    pub live: [bool; 4],
    /// `0x801FA27C`: the swap's frame, then arm 6's frozen timer, then arm 7's.
    pub counter: i32,
    /// `0x801FA280`: the swaps made, then the pointer's blink counter.
    pub blink: i32,
    /// `0x801FA298` / `0x801FA29C`: the swap's two cards. The first holds the
    /// pick from arm 6 on.
    pub pair: [usize; 2],
    /// `0x801FA2A0` / `0x801FA2A2`: the second and first card's `x` at the swap.
    swap_x: [i16; 2],
    /// `0x801FA2A8` / `0x801FA2AA`: their `y`.
    swap_y: [i16; 2],
    /// `0x801FA2A4`: the pair crosses on the depth axis alone.
    arc: bool,
    /// `0x801FA2AC` / `0x801FA2AE`.
    pub cursor: i16,
    pub target: i16,
    /// The picked card's `+0x78` and `+0x72` climb and fall from arm 9 on.
    pub pick_level: u16,
    pub pick_fade: i16,
    /// Host feeds for this pass.
    pub pad: DeathGamePad,
    pub caster_faded: bool,
    /// What the pass did, for the host to carry out.
    pub cues: [u16; 3],
    pub kills: [bool; 4],
    pub seats_parts: bool,
    pub turns_pick: bool,
    pub outcome: Option<DeathGameOutcome>,
}

impl DeathGame {
    /// The pass's dealt table (`0x801F7BB8..0x801F7CE4`): `rand() % 4` until
    /// every value `1..=4` has found a free card, then a stacked deal for two
    /// casters: `0x9D` turns the first card of `3` or `4` into a `1`, `0x9E`
    /// every one of them.
    pub fn deal(&mut self, caster_monster: u8, mut rand: impl FnMut() -> u32) {
        self.table = [0; 4];
        let mut placed = 0u8;
        while placed < 4 {
            let i = (rand() % 4) as usize;
            if self.table[i] == 0 {
                placed += 1;
                self.table[i] = placed;
            }
        }
        match caster_monster {
            STACKED_ONCE => {
                if let Some(v) = self.table.iter_mut().find(|v| **v >= 3) {
                    *v = 1;
                }
            }
            STACKED_ALL => {
                for v in self.table.iter_mut().filter(|v| **v >= 3) {
                    *v = 1;
                }
            }
            _ => {}
        }
    }

    /// A swap's opening draws (`0x801F816C..0x801F81C4`): two `rand() % 4`,
    /// both redrawn until they differ, and one more whose low bit picks the
    /// crossing.
    pub fn draw_swap(&mut self, mut rand: impl FnMut() -> u32) {
        loop {
            self.pair = [(rand() % 4) as usize, (rand() % 4) as usize];
            if self.pair[0] != self.pair[1] {
                break;
            }
        }
        self.arc = rand() & 1 != 0;
        let [a, b] = self.pair;
        self.swap_x = [self.cards[b][0], self.cards[a][0]];
        self.swap_y = [self.cards[b][1], self.cards[a][1]];
    }

    /// One pass of arm 4's crossing at `self.counter` (`0x801F8274..0x801F879C`).
    fn cross(&mut self) {
        let [a, b] = self.pair;
        let [xb, xa] = self.swap_x.map(i32::from);
        let t = (xb - xa).abs() * self.counter / 16;
        if xb < xa {
            self.cards[a][0] = (xa - t) as i16;
            self.cards[b][0] = (xb + t) as i16;
        } else {
            self.cards[a][0] = (xa + t) as i16;
            self.cards[b][0] = (xb - t) as i16;
        }
        // `sll 8` onto the halfword table: `0x80` trig steps a frame, half a
        // turn over the swap's sixteen.
        let (sin, _) = legaia_engine_system::trig::trig12((self.counter * 0x80) as u16);
        let sin = i32::from(sin);
        if self.arc {
            self.cards[a][2] = CARD_Z - (sin / 32) as i16;
            self.cards[b][2] = CARD_Z + (sin / 32) as i16;
        } else {
            self.cards[a][2] = CARD_Z - (sin / 64) as i16;
            self.cards[b][2] = CARD_Z + (sin / 64) as i16;
            let [yb, ya] = self.swap_y;
            self.cards[a][1] = ya - (sin / 32) as i16;
            self.cards[b][1] = yb + (sin / 32) as i16;
        }
    }

    /// The cursor's third-of-the-way ease (`0x801F8984..0x801F89F8`).
    fn ease(&mut self) {
        let (c, t) = (i32::from(self.cursor), i32::from(self.target));
        if c < t {
            self.cursor = (c + (t - c + 3) / 3) as i16;
        }
        let c = i32::from(self.cursor);
        if t < c {
            self.cursor = (c - (c - t + 3) / 3) as i16;
        }
    }

    /// The column the target stands on, as a card's `x` (`0x801F8E70..0x801F8EAC`).
    fn target_x(&self) -> i16 {
        ((i32::from(self.target) - i32::from(CURSOR_HOME)) * 192 / i32::from(CURSOR_STEP)
            + i32::from(CARD_FIRST_X)) as i16
    }

    fn cue(&mut self, id: u16) {
        if let Some(c) = self.cues.iter_mut().find(|c| **c == 0) {
            *c = id;
        }
    }

    /// The picked card's value.
    pub fn picked_value(&self) -> u8 {
        self.table[self.pair[0]]
    }
}

fn drift(tr_z: i16, tr_y: i16) -> Option<CaptureDrift> {
    Some(CaptureDrift {
        tr_z,
        tr_y,
        ..Default::default()
    })
}

/// Death Game's arms. The word `0x801FA278` drains `8 * delta`:
///
/// | arm | what it does | word |
/// |---|---|---|
/// | 0 (`0x801F7ADC`) | cue `0x153`; the caster turned on its victim, every live seat hidden, the caster shown at rate `2`; the deal; a cut behind the caster | `= 0x700` |
/// | 1 (`0x801F7D14`) | rate `2`, TR z `+8`, TR y `-1`; then four backdrop records | `= 0x200` |
/// | 2 (`0x801F7E04`) | rate `0`, the same drift; then the four card faces | `= 0x600` |
/// | 3 (`0x801F7F50`) | the labels under the cards; then the swap counters zeroed | - |
/// | 4 (`0x801F8130`) | thirteen swaps of sixteen frames, each opening on `FUN_8004FE5C(0x44, caster)` and three or more draws | `= 0x960` |
/// | 5 (`0x801F87F0`) | the timer and pointer sprites; the pad, cue `0x22` a step; on confirm or a spent word cue `0x21` and the cards turned over | `= 0x200` |
/// | 6 (`0x801F8BEC`) | both sprites, the frozen digit; then the pick | - |
/// | 7 (`0x801F8F0C`) | thirty-two frames: the pick to centre, the others away and off | `= 0x100` |
/// | 8 (`0x801F9054`) | cue `0x154`, the pick's blend bits | `+= 0x200` |
/// | 9 (`0x801F90C4`) | TR z `-8`, the pick grows and fades off; then the card's result | see below |
/// | 10 (`0x801F9800`) | victim fell: TR z `-8` until its clip `+0x1D9` is `8` | - |
/// | 11 (`0x801F9838`) | caster fell: TR z `-8` until its colour word is `0` | - |
/// | 12 (`0x801F9870`) | both bled: the focus between them, TR z `+8`, the victim's rate `4`, until both settle | - |
/// | 13 (`0x801F9918`) | both healed: TR z `+8` | `+= 0x800` |
/// | `0xFF` (`0x801F9964`) | every live seat shown at full rate; done | - |
///
/// The host draws ahead of a pass ([`DeathGame::deal`], [`DeathGame::draw_swap`]),
/// feeds the pad, and carries out the cues, the parts and the outcome. This
/// director owns the phase. Arms 3, 5 and 6's packets are the host's
/// (`DeathGame::cursor`, the word).
///
/// PORT: FUN_801F798C, overlay_summon_effect_table_0957_801f798c (PROT 0957 `0x76`; the phase walk, deal, shuffle, pad, pick and results; the labels, the two sprites and arm 4's heal flash `FUN_801E22C8` are not drawn)
pub fn death_game_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let v = seats.victim;
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    let g = &mut st.death_game;
    g.cues = [0; 3];
    g.kills = [false; 4];
    g.seats_parts = false;
    g.turns_pick = false;
    g.outcome = None;
    let word = &mut st.countdown;
    match phase {
        0 => {
            g.cue(0x153);
            let h = super::heading(v, c).wrapping_add(0x800) & 0xFFF;
            arm.others_write = HIDDEN;
            arm.others_live_only = true;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(h),
                colour: Some(SHOWN_COLOUR),
                render_flag: Some(9),
                rate: Some(2),
                ..Default::default()
            };
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, h), 0],
                tr: [0, 0x500, (i32::from(seats.depth_raw as i16) / 4) as i16],
                focus: focus_on(st.caster_body.unwrap_or(c)),
                frames: 1,
            });
            word.0 = 0x700;
            arm.skips_fold = true;
            arm.next = Some(1);
        }
        1 => {
            arm.caster_write.rate = Some(2);
            arm.drift = drift(8, -1);
            if word.drain_above(0) {
                arm.hold = true;
            } else {
                for (i, x) in [-0x180, -0x80, 0x80, 0x180].into_iter().enumerate() {
                    arm.charge_spawns[i] =
                        Some(seat(BACKDROP_RECORD, [x, 0x200, 0x400], [0, 0, 0]));
                }
                word.0 = 0x200;
                arm.next = Some(2);
            }
        }
        2 => {
            arm.caster_write.rate = Some(0);
            arm.drift = drift(8, -1);
            if word.drain_above(0) {
                arm.hold = true;
            } else {
                for i in 0..4 {
                    let x = CARD_FIRST_X + CARD_STEP_X * i as i16;
                    g.cards[i] = [x, -0x1C0, CARD_Z];
                    g.live[i] = true;
                    let face = CARD_RECORDS[usize::from(g.table[i].clamp(1, 4)) - 1];
                    arm.charge_spawns[i] = Some(seat(face, g.cards[i], [0, 0, 0]));
                }
                word.0 = 0x600;
                arm.next = Some(3);
            }
        }
        3 => {
            if word.drain_above(0) {
                arm.hold = true;
            } else {
                g.counter = 0;
                g.blink = 0;
                arm.next = Some(4);
            }
        }
        4 => {
            arm.hold = true;
            let opening = g.counter == 0;
            g.counter = (g.counter + 1).min(16);
            g.cross();
            // The opening pass leaves ahead of the end test (`j 0x801F99C0`
            // at `0x801F83E0` / `0x801F84C4`).
            if !opening && g.counter == 16 {
                g.counter = 0;
                g.blink += 1;
                if g.blink == 13 {
                    word.0 = 0x960;
                    g.cursor = CURSOR_HOME;
                    g.target = CURSOR_HOME;
                    g.blink = 0;
                    arm.hold = false;
                    arm.next = Some(5);
                }
            }
        }
        5 => {
            word.drain();
            if word.0 < 0 {
                word.0 = 0;
            }
            if g.pad.right {
                g.cue(0x22);
                g.target += CURSOR_STEP;
                if g.target >= 0x158 {
                    g.target = 0x10C;
                }
            }
            if g.pad.left {
                g.cue(0x22);
                g.target -= CURSOR_STEP;
                if g.target < -0x23 {
                    g.target = CURSOR_HOME;
                }
            }
            g.ease();
            g.blink += 1;
            if g.pad.confirm || word.0 == 0 {
                g.cue(0x21);
                for i in 0..4 {
                    arm.charge_spawns[i] = Some(seat(PICK_RECORD, g.cards[i], [0, 0, 0]));
                    g.kills[i] = true;
                }
                g.seats_parts = true;
                g.counter = word.0;
                word.0 = 0x200;
                arm.next = Some(6);
            } else {
                arm.hold = true;
            }
        }
        6 => {
            g.ease();
            g.blink += 1;
            if word.drain_above(0) {
                arm.hold = true;
            } else {
                let x = g.target_x();
                if let Some(i) = g.cards.iter().position(|card| card[0] == x) {
                    g.pair[0] = i;
                }
                g.counter = 0;
                g.swap_x[0] = g.cards[g.pair[0]][0];
                arm.next = Some(7);
            }
        }
        7 => {
            g.counter += 1;
            if g.counter >= 0x20 {
                word.0 = 0x100;
                g.counter = 0x20;
                arm.next = Some(8);
            } else {
                arm.hold = true;
            }
            for i in 0..4 {
                if i == g.pair[0] {
                    let x = i32::from(g.swap_x[0]);
                    g.cards[i][0] = (x - x * g.counter / 32) as i16;
                    g.cards[i][1] += 1;
                    g.cards[i][2] -= 8;
                } else {
                    g.cards[i][1] -= 16;
                    g.cards[i][2] += 8;
                    if g.counter == 0x20 {
                        g.kills[i] = true;
                        g.live[i] = false;
                    }
                }
            }
        }
        8 => {
            if word.drain_above(0) {
                arm.hold = true;
            } else {
                g.cue(0x154);
                g.turns_pick = true;
                g.pick_fade = 0;
                g.pick_level = 0;
                word.0 += 0x200;
                arm.next = Some(9);
            }
        }
        9 => {
            arm.drift = drift(-8, 0);
            g.pick_level = g.pick_level.wrapping_add(32);
            g.pick_fade = g.pick_fade.wrapping_sub(32);
            word.drain();
            if g.pick_level >= 0x1000 && g.live[g.pair[0]] {
                g.kills[g.pair[0]] = true;
                g.live[g.pair[0]] = false;
            }
            if word.0 > 0 || g.live[g.pair[0]] {
                arm.hold = true;
            } else {
                let (sin, cos) = legaia_engine_system::trig::trig12(c.facing);
                let vf = c.facing.wrapping_add(0x800) & 0xFFF;
                // The victim before the caster, a twelfth of the unit circle
                // out (`0x2AAAAAAB`, then `sra 1`).
                let (vx, vz) = (c.x.wrapping_add(sin / 12), c.z.wrapping_add(cos / 12));
                let before = CaptureSeatWrite {
                    x: Some(vx),
                    z: Some(vz),
                    facing: Some(vf),
                    colour: Some(SHOWN_COLOUR),
                    render_flag: Some(0),
                    ..Default::default()
                };
                let mid = [
                    ((i32::from(vx) + i32::from(c.x)) / 2) as i16,
                    0,
                    ((i32::from(vz) + i32::from(c.z)) / 2) as i16,
                ];
                let cut = |tr_z: i16, yaw: i32, focus: [i16; 3]| {
                    Some(ModuleShot {
                        angles: [0, yaw_from(yaw, vf), 0],
                        tr: [0, 0x280, tr_z],
                        focus: [focus[0].wrapping_neg(), 0, focus[2].wrapping_neg()],
                        frames: 1,
                    })
                };
                let quiet_caster = CaptureSeatWrite {
                    rate: Some(FULL_RATE),
                    stage: Some(0),
                    render_flag: Some(0),
                    ..Default::default()
                };
                match g.picked_value() {
                    1 => {
                        g.outcome = Some(DeathGameOutcome::VictimFalls);
                        arm.victim_write = before;
                        arm.caster_write = quiet_caster;
                        arm.shot = cut(0x600, 0x800, [vx, 0, vz]);
                        arm.next = Some(10);
                    }
                    2 => {
                        g.outcome = Some(DeathGameOutcome::CasterFalls);
                        arm.caster_write.render_flag = Some(2);
                        arm.next = Some(11);
                    }
                    3 => {
                        g.outcome = Some(DeathGameOutcome::BothBleed);
                        arm.victim_write = CaptureSeatWrite {
                            rate: Some(4),
                            ..before
                        };
                        arm.caster_write.render_flag = Some(0);
                        arm.shot = cut(0xC00, 0x400, mid);
                        arm.next = Some(12);
                    }
                    _ => {
                        g.outcome = Some(DeathGameOutcome::BothHeal);
                        g.cue(0x20F);
                        arm.victim_write = before;
                        arm.caster_write = quiet_caster;
                        arm.shot = cut(0x800, 0x400, mid);
                        word.0 += 0x800;
                        arm.next = Some(13);
                    }
                }
            }
        }
        10 => {
            arm.drift = drift(-8, 0);
            if st.victim_settled {
                arm.next = Some(0xFF);
            } else {
                arm.hold = true;
            }
        }
        11 => {
            arm.drift = drift(-8, 0);
            if g.caster_faded {
                arm.next = Some(0xFF);
            } else {
                arm.hold = true;
            }
        }
        12 => {
            arm.drift = drift(8, 0);
            arm.victim_write.rate = Some(4);
            let body = |s: Option<super::ModuleSeat>, or: super::ModuleSeat| s.unwrap_or(or);
            let (cb, vb) = (body(st.caster_body, c), body(st.victim_body, v));
            arm.pose_set = Some(CapturePoseSet {
                focus_xz: Some([
                    ((i32::from(vb.x) + i32::from(cb.x)) / 2) as i16,
                    ((i32::from(vb.z) + i32::from(cb.z)) / 2) as i16,
                ]),
                ..Default::default()
            });
            let caster_done = st.caster_settled || g.caster_faded;
            if st.victim_settled && caster_done {
                arm.next = Some(0xFF);
            } else {
                arm.hold = true;
            }
        }
        13 => {
            arm.drift = drift(8, 0);
            if word.drain_above(0) {
                arm.hold = true;
            } else {
                arm.next = Some(0xFF);
            }
        }
        _ => {
            arm.others_write = CaptureSeatWrite {
                colour: Some(SHOWN_COLOUR),
                render_flag: Some(0),
                rate: Some(FULL_RATE),
                ..Default::default()
            };
            arm.others_live_only = true;
        }
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(values: &'static [u32]) -> impl FnMut() -> u32 {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    #[test]
    fn the_deal_places_every_value_and_retries_a_taken_card() {
        let mut g = DeathGame::default();
        // `2` lands twice: the second draw deals nothing.
        g.deal(0, stream(&[2, 2, 0, 3, 1]));
        assert_eq!(g.table, [2, 4, 1, 3]);
    }

    #[test]
    fn two_casters_deal_a_stacked_table() {
        let mut g = DeathGame::default();
        g.deal(STACKED_ONCE, stream(&[0, 1, 2, 3]));
        assert_eq!(g.table, [1, 2, 1, 4]);
        g.deal(STACKED_ALL, stream(&[0, 1, 2, 3]));
        assert_eq!(g.table, [1, 2, 1, 1]);
    }

    #[test]
    fn a_swap_redraws_both_cards_until_they_differ_and_ends_exchanged() {
        let mut g = DeathGame::default();
        for i in 0..4 {
            g.cards[i] = [CARD_FIRST_X + CARD_STEP_X * i as i16, -0x1C0, CARD_Z];
        }
        g.draw_swap(stream(&[1, 1, 0, 3, 7]));
        assert_eq!(g.pair, [0, 3]);
        assert!(g.arc);
        g.counter = 16;
        g.cross();
        assert_eq!(g.cards[0][0], 0x120);
        assert_eq!(g.cards[3][0], -0x120);
        // Back on the depth axis: the half turn has come round.
        assert_eq!(g.cards[0][2], CARD_Z);
    }

    #[test]
    fn the_target_stops_at_both_ends_and_names_a_column() {
        let mut g = DeathGame {
            target: CURSOR_HOME,
            ..Default::default()
        };
        assert_eq!(g.target_x(), CARD_FIRST_X);
        g.target = 0x10C;
        assert_eq!(g.target_x(), 0x120);
    }

    #[test]
    fn the_cast_walks_its_arms_to_the_picked_cards_result() {
        for (value, branch) in [(1u8, 10u8), (2, 11), (3, 12), (4, 13)] {
            let mut st = ModuleCamState {
                victim_settled: true,
                caster_settled: true,
                ..Default::default()
            };
            st.death_game.caster_faded = true;
            let seats = ModuleCamSeats::default();
            let mut phase = 0u8;
            let mut seq = vec![0u8];
            for _ in 0..40_000 {
                if phase == 2 {
                    st.death_game.table = [value; 4];
                }
                if phase == 4 && st.death_game.counter == 0 {
                    st.death_game.draw_swap(stream(&[0, 1, 2]));
                }
                let a = death_game_camera(&mut st, phase, seats);
                if a.hold {
                    continue;
                }
                let Some(n) = a.next else { break };
                phase = n;
                seq.push(n);
            }
            let mut want = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, branch, 0xFF];
            if branch == 0xFF {
                want.pop();
            }
            assert_eq!(seq, want, "value {value}");
        }
    }

    /// Arm 4 is thirteen swaps of sixteen frames, and arm 5 runs out on `0x960`:
    /// `battle_jou_puera_v3_death_game_mid_cast` holds the arms `210` and `300`
    /// vsyncs.
    #[test]
    fn the_shuffle_and_the_timer_run_their_vsyncs() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        let mut passes = 0;
        loop {
            if st.death_game.counter == 0 {
                st.death_game.draw_swap(stream(&[0, 1, 2]));
            }
            passes += 1;
            if !death_game_camera(&mut st, 4, seats).hold {
                break;
            }
        }
        assert_eq!(passes, 13 * 16);
        let mut passes = 0;
        loop {
            passes += 1;
            if !death_game_camera(&mut st, 5, seats).hold {
                break;
            }
        }
        assert_eq!(passes, 300);
    }
}
