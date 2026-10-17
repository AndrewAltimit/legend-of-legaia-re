//! The op-`0x20` stagers of two capture-class cells whose parts call back
//! into their module: PROT 0938 (Chaos Breath / Mystic Circle) and PROT 0962
//! (the Blade Breath cell, whose `0xA5` body is Cort's Ultra Charge). Each
//! seats more records off the calling part, the shape
//! [`super::stager_spawns`] documents for the summon band.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
