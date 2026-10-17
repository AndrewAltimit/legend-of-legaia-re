//! PROT 0927 (Juggernaut)'s stager `FUN_801F85A8` as its own parts reach
//! it: the op-`0x20` calls of the records the body seats. Nine arms behind
//! `sltiu a1, 9` through the table at `0x801F6A60`; arm 4 is also the
//! monster-row sweep [`super::juggernaut_stager`] ports.

use super::{StagerCaller, StagerSpawn};
use legaia_engine_battle_vm::battle_action::SummonFadeTemplate;

fn at(record: u32, caller: StagerCaller) -> StagerSpawn {
    StagerSpawn {
        record,
        pos: caller.pos,
        rot: caller.rot,
        pokes: Vec::new(),
        patches: Vec::new(),
        halve_speed: false,
        scale: 0x1000,
    }
}

/// The records stager arm `arm` seats on its caller (`a0 = part + 0x14`,
/// `a1 = part + 0x24`, every call through the pool wrapper
/// `FUN_80050ED4`):
///
/// | arm | records |
/// |---|---|
/// | 0 (`0x801F85F8`) | `0x801F8D60` |
/// | 1 (`0x801F860C`) | `0x801F8BC8` |
/// | 2 (`0x801F8620`) | `0x801F8F08`, `0x801F8FA0`, `0x801F8FE4`, `0x801F9030` |
/// | 6 (`0x801F8844`) | `0x801F90F8` |
/// | 8 (`0x801F892C`) | `0x801F9B24`, `0x801F9B68` |
///
/// Arm 7 (`0x801F8858`) jitters the caller's own `+0x84` / `+0x28` off
/// `rand()` and seats nothing; arms 3..5 are [`juggernaut_stager_fade`] /
/// [`juggernaut_stager_shake`].
///
/// PORT: FUN_801F85A8 (PROT 0927 stager; the spawn arms 0, 1, 2, 6 and 8)
pub fn juggernaut_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let records: &[u32] = match arm {
        0 => &[0x801F_8D60],
        1 => &[0x801F_8BC8],
        2 => &[0x801F_8F08, 0x801F_8FA0, 0x801F_8FE4, 0x801F_9030],
        6 => &[0x801F_90F8],
        8 => &[0x801F_9B24, 0x801F_9B68],
        _ => &[],
    };
    records.iter().map(|&r| at(r, caller)).collect()
}

/// The fade stager arm `arm` spawns (`FUN_80024E80(0x801C9070, 1)`): arm 4
/// (`0x801F86EC`, which arm 3 falls into) a `0x40`-vsync salmon
/// `(0xFF, 0x80, 0x60)` flash to black, arm 5 (`0x801F8834`) the same over
/// `0x100` vsyncs.
pub fn juggernaut_stager_fade(arm: u8) -> Option<SummonFadeTemplate> {
    let duration = match arm {
        3 | 4 => 0x40,
        5 => 0x100,
        _ => return None,
    };
    Some(SummonFadeTemplate {
        kind: 1,
        duration,
        start_rgb: [0xFF, 0x80, 0x60],
        end_rgb: [0; 3],
        delay: 0,
        hold: 0,
    })
}

/// What stager arm `arm` adds to the module's shake word `0x801F9C90`:
/// arm 3 `0xC` (`0x801F8690`) and then arm 4's `0x14` (`0x801F86E8`), which
/// it falls into.
pub fn juggernaut_stager_shake(arm: u8) -> i32 {
    match arm {
        3 => 0xC + 0x14,
        4 => 0x14,
        _ => 0,
    }
}

/// One pass of PROT 0927's epilogue over the shake word
/// (`0x801F82D4..0x801F8328`): while it is non-zero, TR y `0x800840BC`
/// moves by `8 * word`, up on an odd word and down on an even one, and the
/// word counts down by one. Returns the TR y delta.
///
/// PORT: overlay_summon_juggernaut_0927_801f6a84 (PROT 0927; the epilogue's camera shake)
pub fn juggernaut_shake_step(word: &mut i32) -> i32 {
    if *word == 0 {
        return 0;
    }
    let d = if *word & 1 != 0 {
        *word * 8
    } else {
        -*word * 8
    };
    *word -= 1;
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_2_seats_four_records_on_the_caller() {
        let caller = StagerCaller {
            pos: [1, 2, 3],
            ..Default::default()
        };
        let s = juggernaut_stager_spawns(2, caller);
        assert_eq!(s.len(), 4);
        assert!(s.iter().all(|s| s.pos == [1, 2, 3]));
        assert!(juggernaut_stager_spawns(7, caller).is_empty());
    }

    #[test]
    fn the_shake_alternates_and_settles() {
        let mut w = juggernaut_stager_shake(3);
        assert_eq!(w, 0x20);
        let mut sum = 0;
        let mut n = 0;
        while w != 0 {
            sum += juggernaut_shake_step(&mut w);
            n += 1;
        }
        assert_eq!(n, 0x20);
        // Even word first: down 8 * 32, up 8 * 31, ... nets -8 * 16.
        assert_eq!(sum, -8 * 16);
    }
}
