//! The op-`0x20` stagers of the capture-class cells whose parts call back
//! into their module: PROT 0938 (Chaos Breath / Mystic Circle), PROT 0944
//! (Guilty Cross / Curse All), PROT 0946 (Zeto's Call Wave / Big Wave) and
//! PROT 0962 (the Blade Breath cell, whose
//! `0xA5` body is Cort's Ultra Charge). Each seats more records off the
//! calling part, the shape [`super::stager_spawns`] documents for the summon
//! band.

use super::stager_spawns::{StagerCaller, StagerSpawn};

fn on_caller(record: u32, caller: StagerCaller, scale: u16) -> StagerSpawn {
    StagerSpawn {
        record,
        pos: caller.pos,
        rot: caller.rot,
        pokes: Vec::new(),
        patches: Vec::new(),
        halve_speed: false,
        scale,
    }
}

/// PROT 0938's stager `FUN_801F7AB8`, reached through `gp[+0x714]` from its
/// parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F7BE0` on the caller (`FUN_80021B04`, `0x801F7B1C`) |
/// | 1 | eight `0x801F7C60` on the caller's position, each turned a further `0x200` about Y from the caller's `+0x26` (`0x801F7B2C..0x801F7B74`) |
/// | 2 | `0x801F7DF4` on the caller (`FUN_80050ED4`, `0x801F7B80`): Mystic Circle's rays, called every eleven frames by `0x801F7DC4` |
/// | 3 | `0x801F7F28` on the caller |
///
/// Every arm forwards the caller's `+0x72` as the seat's scale (`lhu` /
/// `lh a3,0x72(s1)`).
///
/// PORT: FUN_801F7AB8 (PROT 0938 stager)
pub fn chaos_breath_cell_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let scale = caller.scale;
    match arm {
        0 => vec![on_caller(0x801F_7BE0, caller, scale)],
        1 => (1..=8u16)
            .map(|k| {
                let mut s = on_caller(0x801F_7C60, caller, scale);
                s.rot = [
                    0,
                    (caller.rot[1] as u16).wrapping_add(k * 0x200) as i16 & 0xFFF,
                    0,
                ];
                s
            })
            .collect(),
        2 => vec![on_caller(0x801F_7DF4, caller, scale)],
        3 => vec![on_caller(0x801F_7F28, caller, scale)],
        _ => Vec::new(),
    }
}

/// PROT 0962's stager `FUN_801F813C`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 1 | `0x801F82AC` on the caller (`0x801F8174..0x801F81CC`) |
/// | 2 | `0x801F86D4`, `0x801F8720` and `0x801F88FC` on the caller (`0x801F8188..0x801F81CC`): Ultra Charge's three rings, called every fifteen frames by its caster-seated `0x801F86B8` |
///
/// Each through `FUN_80050ED4` at `0x1000`.
///
/// PORT: FUN_801F813C (PROT 0962 stager)
pub fn blade_breath_cell_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    match arm {
        1 => vec![on_caller(0x801F_82AC, caller, 0x1000)],
        2 => [0x801F_86D4, 0x801F_8720, 0x801F_88FC]
            .into_iter()
            .map(|r| on_caller(r, caller, 0x1000))
            .collect(),
        _ => Vec::new(),
    }
}

/// PROT 0944's stager `FUN_801F7F2C`, reached through `gp[+0x714]` from its
/// parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F7FE8` (`FUN_80021B04`, `0x801F7F80`) |
/// | 1 | `0x801F8244`, then falls into arm 2's call (`0x801F7FA0..0x801F7FB8`) |
/// | 2 | `0x801F8290` |
///
/// Every call sits on the caller's `+0x14` / `+0x24` and forwards its `+0x72`
/// as the scale. Guilty Cross's arm-3 record `0x801F8210` calls arms 1 and 2
/// in a loop.
///
/// PORT: FUN_801F7F2C (PROT 0944 stager)
pub fn guilty_cross_cell_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let scale = caller.scale;
    match arm {
        0 => vec![on_caller(0x801F_7FE8, caller, scale)],
        1 => vec![
            on_caller(0x801F_8244, caller, scale),
            on_caller(0x801F_8290, caller, scale),
        ],
        2 => vec![on_caller(0x801F_8290, caller, scale)],
        _ => Vec::new(),
    }
}

/// The records a capture-cell stager seats with words rolled off `rand()`
/// stored over the seat: PROT 0946's Call Wave rays (`0x801F7970`, arm 0 of
/// [`call_wave_stager_spawns`]) and PROT 0961's shards (`0x801F7E80`, arm 2
/// of [`dead_end_crisis_stager_spawns`]). Each part's wait, lean rate
/// `+0x84`, spread, length and roll are draws on the stream's history since
/// the fight began, which a replay does not re-deal; a capture reads them
/// back off the part.
pub const ROLLED_STAGER_RECORDS: [u32; 2] = [0x801F_7970, 0x801F_7E80];
/// The rolled words of a [`ROLLED_STAGER_RECORDS`] part beyond a seated
/// part's position, banks, wait and channel block: the lean rate `+0x84`.
pub const ROLLED_STAGER_EXTRA_OFFSETS: [u16; 1] = [0x84];

/// PROT 0946's wave record the stager's arm 1 seats.
pub const CALL_WAVE_WAVE_RECORD: u32 = 0x801F_7BF8;
/// The handle table `0x801F7F30` arm 1 fills: sixteen slots, and arm 1 seats
/// only into a null one (`0x801F7894..0x801F78A8`). Nothing in the body
/// clears a slot as its part dies, so a cast seats sixteen waves at most.
pub const CALL_WAVE_WAVE_SLOTS: usize = 16;

/// PROT 0946's stager `FUN_801F76C4`, reached through `gp[+0x714]` from its
/// parts' op `0x20` (`a1` the arm):
///
/// | Arm | Caller | Seats |
/// |---:|---|---|
/// | 0 | `0x801F78CC`, every eleven frames (Call Wave) | `0x801F7928`, `0x801F79C0`, `0x801F7A08` on the caller (`0x801F76F8..0x801F7744`), then eight rays `0x801F7970`, ray `k` stored after its seat `+0x54 = k + r % 3`, `+0x84 = r % 256 - 0x80`, `+0xBA = r % 128`, `+0xB6 = r % 256 + 0x1000`, `+0x28 = 0x200 k + r % 512 - 0x100`, one `rand()` each in that order (`0x801F774C..0x801F7840`) |
/// | 1 | `0x801F7C44`, every five frames (Big Wave) | while the phase `ctx[+0x279]` is below `7`, one wave `0x801F7BF8` into the first null slot of the handle table ([`CALL_WAVE_WAVE_SLOTS`]), none once it is full (`0x801F7870..0x801F78AC`) |
///
/// Every call is `FUN_80021B04(caller + 0x14, caller + 0x24, record, 0x1000)`.
/// `rand()` is `FUN_80056798`, never negative, so each `%` is the plain
/// remainder the arm's `mult` / shift sequences compute.
///
/// PORT: FUN_801F76C4 (PROT 0946 stager)
pub fn call_wave_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    phase: u8,
    waves_seated: usize,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    match arm {
        0 => {
            let mut out: Vec<StagerSpawn> = [0x801F_7928, 0x801F_79C0, 0x801F_7A08]
                .into_iter()
                .map(|r| on_caller(r, caller, 0x1000))
                .collect();
            for k in 0..8u16 {
                let mut s = on_caller(0x801F_7970, caller, 0x1000);
                let wait = k + (rand() % 3) as u16;
                let lean = (rand() % 256) as i32 - 0x80;
                let ba = (rand() % 128) as u16;
                let b6 = (rand() % 256) as u16 + 0x1000;
                let roll = (i32::from(k) << 9) + (rand() % 512) as i32 - 0x100;
                s.pokes = vec![
                    (0x54, wait),
                    (0x84, lean as u16),
                    (0xBA, ba),
                    (0xB6, b6),
                    (0x28, roll as u16),
                ];
                out.push(s);
            }
            out
        }
        1 if phase < 7 && waves_seated < CALL_WAVE_WAVE_SLOTS => {
            vec![on_caller(CALL_WAVE_WAVE_RECORD, caller, 0x1000)]
        }
        _ => Vec::new(),
    }
}

/// PROT 0961's stager `FUN_801F78A4` (row `0x20 + 26` of `0x801F6734`),
/// reached through `gp[+0x714]` from its parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F7AD0`, `0x801F7B1C` (`0x801F7914`, `0x801F7A30`) |
/// | 1 | `0x801F7DB0` (`0x801F7A30`) |
/// | 2 | `0x801F7DF4`, `0x801F7E38`, then four `0x801F7E80`, part `k` stored after its seat `+0x54 = 8 k + r % 32`, `+0x84 = r % 256 - 0x80`, `+0xBA = r % 128`, `+0xB6 = r % 256 + 0x800`, `+0x28 = r % 4096`, one `rand()` each in that order (`0x801F7944..0x801F79F4`) |
/// | 3 | `0x801F8234`, `0x801F8278` (`0x801F7A18`, `0x801F7A30`) |
///
/// Every call is `FUN_80050ED4(caller + 0x14, caller + 0x24, record,
/// 0x1000)`.
///
/// PORT: FUN_801F78A4 (PROT 0961 stager)
pub fn dead_end_crisis_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    let at = |r: u32| on_caller(r, caller, 0x1000);
    match arm {
        0 => vec![at(0x801F_7AD0), at(0x801F_7B1C)],
        1 => vec![at(0x801F_7DB0)],
        2 => {
            let mut out = vec![at(0x801F_7DF4), at(0x801F_7E38)];
            for k in 0..4u16 {
                let mut s = at(0x801F_7E80);
                let wait = (k << 3) + (rand() % 32) as u16;
                let lean = (rand() % 256) as i32 - 0x80;
                let ba = (rand() % 128) as u16;
                let b6 = (rand() % 256) as u16 + 0x800;
                let roll = (rand() % 4096) as u16;
                s.pokes = vec![
                    (0x54, wait),
                    (0x84, lean as u16),
                    (0xBA, ba),
                    (0xB6, b6),
                    (0x28, roll),
                ];
                out.push(s);
            }
            out
        }
        3 => vec![at(0x801F_8234), at(0x801F_8278)],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_end_crisis_arm_two_pokes_four_shards() {
        let mut n = 0u32;
        let mut rand = || {
            n += 1;
            n * 0x123
        };
        let s = dead_end_crisis_stager_spawns(2, StagerCaller::default(), &mut rand);
        assert_eq!(s.len(), 6);
        assert_eq!(n, 20, "five draws per shard");
        assert!(s[2..].iter().all(|x| x.record == 0x801F_7E80));
        // Shard 1: draws 6..=10.
        assert_eq!(s[3].pokes[0], (0x54, 8 + (6 * 0x123) % 32));
        assert_eq!(s[3].pokes[3], (0xB6, ((9 * 0x123) % 256 + 0x800) as u16));
        assert_eq!(s[3].pokes[4], (0x28, 10 * 0x123));
    }

    #[test]
    fn mystic_circle_fan_turns_by_a_sixteenth() {
        let caller = StagerCaller {
            rot: [0, 0xF00, 0],
            scale: 0x800,
            ..Default::default()
        };
        let s = chaos_breath_cell_stager_spawns(1, caller);
        assert_eq!(s.len(), 8);
        assert_eq!(s[0].rot, [0, 0x100, 0]);
        assert_eq!(s[7].rot, [0, 0xF00, 0]);
        assert!(
            s.iter()
                .all(|x| x.scale == 0x800 && x.record == 0x801F_7C60)
        );
    }

    #[test]
    fn ultra_charge_rings_are_the_three_arm_two_records() {
        let s = blade_breath_cell_stager_spawns(2, StagerCaller::default());
        let recs: Vec<u32> = s.iter().map(|x| x.record).collect();
        assert_eq!(recs, [0x801F_86D4, 0x801F_8720, 0x801F_88FC]);
        assert!(blade_breath_cell_stager_spawns(0, StagerCaller::default()).is_empty());
    }

    #[test]
    fn call_wave_rays_take_five_draws_each() {
        let mut n = 0u32;
        let mut rand = || {
            n += 1;
            n * 0x101
        };
        let s = call_wave_stager_spawns(0, StagerCaller::default(), 0, 0, &mut rand);
        assert_eq!(s.len(), 11);
        // Ray 0's draws are 0x101, 0x202, 0x303, 0x404, 0x505.
        assert_eq!(
            s[3].pokes,
            vec![
                (0x54, (0x101 % 3) as u16),
                (0x84, (0x202 % 256 - 0x80) as u16),
                (0xBA, (0x303 % 128) as u16),
                (0xB6, (0x404 % 256 + 0x1000) as u16),
                (0x28, (0x505 % 512 - 0x100) as u16),
            ]
        );
    }

    #[test]
    fn call_wave_waves_stop_at_phase_seven_or_a_full_table() {
        let mut rand = || 0;
        let c = StagerCaller::default();
        assert_eq!(call_wave_stager_spawns(1, c, 6, 15, &mut rand).len(), 1);
        assert!(call_wave_stager_spawns(1, c, 7, 0, &mut rand).is_empty());
        assert!(call_wave_stager_spawns(1, c, 0, 16, &mut rand).is_empty());
    }
}
