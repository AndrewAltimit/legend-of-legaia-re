//! The slot-B cast and summon modules' controller rumble writes.
//!
//! Every write sits inside a tick body's phase arm (`ctx+0x279`) behind the
//! Battles vibration word (`lw ..,0x45c8(..)` / `bnez` past the stores) and
//! stores the five words the battle overlay's writers do: the pulse and
//! level motor words `_DAT_8007B92C` / `_DAT_8007B930`, a decay rate for
//! each (`gp+0xA30` / `gp+0xA34`) and a zero hold (`gp+0xA1C`). The battle
//! frame driver then runs the words down
//! ([`legaia_engine_system::rumble::BattleRumbleDecay`]).
//!
//! A write fires on one of three edges of its arm: on the arm's **pass**
//! (the tick its countdown or poll lets it through, once), on **every
//! pass** (the store sits ahead of the arm's gate, so it repeats each tick
//! the phase holds the arm), or on **held passes only** (Ozma's arm 9, whose
//! store sits on the countdown's still-waiting branch).
//!
//! A negative rate grows a word under the decay, up to `0xFF`; a zero rate
//! holds it flat. Nothing inside Gilium (arm 20), Puera (arm 22) or Chaos
//! Flare (arm 7) brings the words back down: the next writer does.
//!
//! The table is read off each module image's disassembly. One writer is not
//! carried: Puera's arm 20 (`0x801F8664`) stores `FF / FF / 0x10 / 0x10` once
//! per falling part as each of its 23 parts' `+0x16` lands, a per-part model
//! the port does not run. PROT 0924's `0x801F7F68` block is a byte copy of
//! PROT 0923's arm-14 block in its inherited tail, reached by no 0924
//! dispatch, and is not a writer.

use legaia_engine_system::rumble::BattleRumbleWrite;

/// When an arm's rumble store runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RumbleEdge {
    /// The tick the arm's gate lets it through.
    Pass,
    /// Every tick the phase holds the arm, the pass included.
    EveryPass,
    /// Every tick the arm's countdown still holds it, never the pass.
    HeldOnly,
    /// Every tick while the module countdown, read before this tick's
    /// drain, lies in `lo..=hi` (Gilium's arm 11, `addiu v0,v0,-0x201` /
    /// `sltiu v0,v0,0x1ff` at `0x801F7B68..0x801F7B6C`).
    CountdownWindow {
        /// Lowest countdown value that stores.
        lo: i32,
        /// Highest countdown value that stores.
        hi: i32,
    },
}

/// What an arm stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RumbleStore {
    /// All five words.
    Write(BattleRumbleWrite),
    /// The two rates and the hold only; the motor words keep running.
    Rates {
        /// `gp+0xA30`.
        pulse_rate: i32,
        /// `gp+0xA34`.
        level_rate: i32,
    },
    /// PROT 0934's spray arms: `0xE0 - rand % 64` and `0xC0 - rand % 64`,
    /// rates `8` - two `rand()` draws, made only while the Battles row is on.
    OzmaSpray,
    /// PROT 0922's alternating jolt: the module word `0x801FA0BC` (zeroed on
    /// arm 13's way out, `0x801F7988`) is read and bumped every pass; an odd
    /// old value stores `odd`, an even one `even`, both words, rates `4`.
    PueraAlternate {
        /// The value an odd counter stores.
        odd: i32,
        /// The value an even counter stores.
        even: i32,
    },
}

/// One rumble write in a module tick body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleRumble {
    /// The module's PROT entry.
    pub entry: u32,
    /// The tick body's VA, for a module that switches bodies on the queued
    /// action id; [`crate::cast_module_camera::SINGLE_BODY`] otherwise.
    pub body: u32,
    /// The phase arm (`ctx+0x279`).
    pub arm: u8,
    /// When the store runs.
    pub edge: RumbleEdge,
    /// What it stores.
    pub store: RumbleStore,
    /// The Battles-gate load that guards the store.
    pub site: u32,
}

const SINGLE: u32 = crate::cast_module_camera::SINGLE_BODY;

const fn write(
    entry: u32,
    arm: u8,
    edge: RumbleEdge,
    value: i32,
    rate: i32,
    site: u32,
) -> ModuleRumble {
    ModuleRumble {
        entry,
        body: SINGLE,
        arm,
        edge,
        store: RumbleStore::Write(BattleRumbleWrite::even(value, rate)),
        site,
    }
}

const fn in_body(m: ModuleRumble, body: u32) -> ModuleRumble {
    ModuleRumble { body, ..m }
}

use RumbleEdge::{EveryPass, HeldOnly, Pass};

/// PROT 0944's Guilty Cross body (action `0x37`).
const GUILTY_CROSS: u32 = crate::cast_module_camera::GUILTY_CROSS_BODY;
/// PROT 0951's Chaos Flare body (action `0x36`).
const CHAOS_FLARE: u32 = crate::cast_module_ticks::CHAOS_FLARE_TICK;
/// PROT 0963's Genocidal Cannon body (action `0xB3`).
const GENOCIDAL_CANNON: u32 = crate::cast_module_ticks::GENOCIDAL_CANNON_CHAIN.body;

/// Every carried module rumble write, by module.
///
/// PORT: overlay_summon_nighto_0907_801f69e8 (PROT 0907; the rumble stores)
/// PORT: overlay_summon_zenoir_0908_801f69d8 (PROT 0908; the rumble stores)
/// PORT: overlay_summon_kemaro_0918_801f6c70 (PROT 0918; the rumble stores)
/// PORT: overlay_summon_puera_0922_801f6a3c (PROT 0922; arms 14, 15 and 22, not arm 20; the rumble stores)
/// PORT: overlay_summon_gilium_0923_801f69d8 (PROT 0923; the rumble stores)
/// PORT: overlay_summon_palma_0928_801f69f4 (PROT 0928; the rumble stores)
/// PORT: overlay_summon_meta_0932_801f6a34 (PROT 0932; the rumble stores)
/// PORT: overlay_summon_ozma_0934_801f6a40 (PROT 0934; the rumble stores)
/// PORT: overlay_cast_earthquake_0935_801f69d8 (PROT 0935; the rumble stores)
/// PORT: overlay_cast_hyper_crush_0936_801f69d8 (PROT 0936; the rumble stores)
/// PORT: overlay_cast_guilty_cross_0944_801f6a04 (PROT 0944; the rumble stores)
/// PORT: overlay_cast_chaos_flare_0951_801f6a20 (PROT 0951; the rumble stores)
/// PORT: overlay_cast_genocidal_cannon_0963_801f6a20 (PROT 0963; the rumble stores)
pub const MODULE_RUMBLE: &[ModuleRumble] = &[
    // PROT 0907 Nighto: arm 6's pass (`ctx+0x6D8 < 0`), after the creature seat.
    write(907, 6, Pass, 0xFF, 4, 0x801F_77AC),
    // PROT 0908 Zenoir: arm 4's pass (the `FUN_8003F2B8(1)` stream poll).
    write(908, 4, Pass, 0xFF, 3, 0x801F_71CC),
    // PROT 0918 Kemaro: arm 3's pass (stream poll); rate `s1 = 2`.
    write(918, 3, Pass, 0xFF, 2, 0x801F_700C),
    // PROT 0922 Puera: arms 14 / 15 every pass, alternating; arm 22's pass
    // grows both words from `0x40`.
    ModuleRumble {
        entry: 922,
        body: SINGLE,
        arm: 14,
        edge: EveryPass,
        store: RumbleStore::PueraAlternate {
            odd: 0xE0,
            even: 0xC0,
        },
        site: 0x801F_7A30,
    },
    ModuleRumble {
        entry: 922,
        body: SINGLE,
        arm: 15,
        edge: EveryPass,
        store: RumbleStore::PueraAlternate {
            odd: 0xFF,
            even: 0xE0,
        },
        site: 0x801F_7C00,
    },
    write(922, 22, Pass, 0x40, -1, 0x801F_8A48),
    // PROT 0923 Gilium.
    write(923, 4, Pass, 0xFF, 6, 0x801F_72A0),
    write(
        923,
        11,
        RumbleEdge::CountdownWindow {
            lo: 0x201,
            hi: 0x3FF,
        },
        0xC0,
        3,
        0x801F_7B7C,
    ),
    // Rate `t7 = 1` (`sp+0x38`, stored at `0x801F6A1C`).
    write(923, 14, Pass, 0xFF, 1, 0x801F_7F68),
    write(923, 20, Pass, 0x01, -1, 0x801F_876C),
    // PROT 0928 Palma: arm 4's pass (clock at `0x69 << 7`) holds `0x60` flat.
    write(928, 4, Pass, 0x60, 0, 0x801F_753C),
    write(928, 5, Pass, 0xFF, 2, 0x801F_76E4),
    // Rate `fp = 1` (`0x801F69FC`).
    write(928, 18, Pass, 0xFF, 1, 0x801F_8820),
    // PROT 0932 Meta: arm 6's pass.
    write(932, 6, Pass, 0xFF, 4, 0x801F_7144),
    // PROT 0934 Ozma: the spray arms 5..=8 every pass, arm 9 on its held passes.
    ModuleRumble {
        entry: 934,
        body: SINGLE,
        arm: 5,
        edge: EveryPass,
        store: RumbleStore::OzmaSpray,
        site: 0x801F_6F00,
    },
    ModuleRumble {
        entry: 934,
        body: SINGLE,
        arm: 6,
        edge: EveryPass,
        store: RumbleStore::OzmaSpray,
        site: 0x801F_70F0,
    },
    ModuleRumble {
        entry: 934,
        body: SINGLE,
        arm: 7,
        edge: EveryPass,
        store: RumbleStore::OzmaSpray,
        site: 0x801F_7370,
    },
    ModuleRumble {
        entry: 934,
        body: SINGLE,
        arm: 8,
        edge: EveryPass,
        store: RumbleStore::OzmaSpray,
        site: 0x801F_7570,
    },
    ModuleRumble {
        entry: 934,
        body: SINGLE,
        arm: 9,
        edge: HeldOnly,
        store: RumbleStore::OzmaSpray,
        site: 0x801F_7820,
    },
    // PROT 0935 Earthquake: arms 4, 5 (the hit) and 6 (the settle) every pass.
    write(935, 4, EveryPass, 0xC0, 0x10, 0x801F_73FC),
    write(935, 5, EveryPass, 0xC0, 0x10, 0x801F_77A0),
    write(935, 6, EveryPass, 0xC0, 0x10, 0x801F_7DE8),
    // PROT 0936 Hyper Crush: arms 4, 5 and 6 (the hit) every pass.
    write(936, 4, EveryPass, 0xC0, 8, 0x801F_7288),
    write(936, 5, EveryPass, 0xC0, 8, 0x801F_72FC),
    write(936, 6, EveryPass, 0xC0, 8, 0x801F_7618),
    // PROT 0944 Guilty Cross: arm 0's one run clears all five words, after
    // the hit roll; arm 3's pass jolts.
    in_body(write(944, 0, EveryPass, 0, 0, 0x801F_6AE8), GUILTY_CROSS),
    in_body(write(944, 3, Pass, 0xFF, 4, 0x801F_714C), GUILTY_CROSS),
    // PROT 0951 Chaos Flare: arm 6's pass jolts, arm 7's pass turns both
    // rates to `-1` so the words climb back to `0xFF`.
    in_body(write(951, 6, Pass, 0xFF, 1, 0x801F_7248), CHAOS_FLARE),
    ModuleRumble {
        entry: 951,
        body: CHAOS_FLARE,
        arm: 7,
        edge: Pass,
        store: RumbleStore::Rates {
            pulse_rate: -1,
            level_rate: -1,
        },
        site: 0x801F_731C,
    },
    // PROT 0963 Genocidal Cannon.
    in_body(write(963, 3, Pass, 0xFF, 2, 0x801F_70C4), GENOCIDAL_CANNON),
    in_body(
        write(963, 0x0B, Pass, 0xFF, 4, 0x801F_77F4),
        GENOCIDAL_CANNON,
    ),
    // After the arm's hit loop; rates `v1 = 1` through the `j 0x801F8084`.
    in_body(
        write(963, 0x0E, Pass, 0xFF, 1, 0x801F_7D54),
        GENOCIDAL_CANNON,
    ),
    in_body(
        write(963, 0x0F, Pass, 0x01, -1, 0x801F_7EA0),
        GENOCIDAL_CANNON,
    ),
    ModuleRumble {
        entry: 963,
        body: GENOCIDAL_CANNON,
        arm: 0x10,
        edge: Pass,
        store: RumbleStore::Rates {
            pulse_rate: 4,
            level_rate: 4,
        },
        site: 0x801F_8070,
    },
];

/// The writes `entry`'s `body` makes on `arm` this tick: `passed` is whether
/// the arm's gate let it through, `countdown` the module countdown before
/// this tick's drain.
pub fn module_rumble_due(
    entry: u32,
    body: u32,
    arm: u8,
    passed: bool,
    countdown: i32,
) -> impl Iterator<Item = &'static ModuleRumble> {
    MODULE_RUMBLE.iter().filter(move |m| {
        m.entry == entry
            && m.body == body
            && m.arm == arm
            && match m.edge {
                RumbleEdge::Pass => passed,
                RumbleEdge::EveryPass => true,
                RumbleEdge::HeldOnly => !passed,
                RumbleEdge::CountdownWindow { lo, hi } => (lo..=hi).contains(&countdown),
            }
    })
}

/// PROT 0934's spray store from its two `rand()` draws, in order.
pub fn ozma_spray(r0: u32, r1: u32) -> BattleRumbleWrite {
    BattleRumbleWrite {
        pulse: 0xE0 - (r0 % 64) as i32,
        level: 0xC0 - (r1 % 64) as i32,
        pulse_rate: 8,
        level_rate: 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_pick_the_right_ticks() {
        assert_eq!(module_rumble_due(918, SINGLE, 3, true, 0).count(), 1);
        assert_eq!(module_rumble_due(918, SINGLE, 3, false, 0).count(), 0);
        assert_eq!(module_rumble_due(935, SINGLE, 5, false, 0).count(), 1);
        assert_eq!(module_rumble_due(934, SINGLE, 9, true, 0).count(), 0);
        assert_eq!(module_rumble_due(934, SINGLE, 9, false, 0).count(), 1);
        assert_eq!(module_rumble_due(923, SINGLE, 11, false, 0x200).count(), 0);
        assert_eq!(module_rumble_due(923, SINGLE, 11, false, 0x201).count(), 1);
        assert_eq!(module_rumble_due(923, SINGLE, 11, false, 0x3FF).count(), 1);
        assert_eq!(module_rumble_due(923, SINGLE, 11, false, 0x400).count(), 0);
        // A trampoline module's write belongs to one body.
        assert_eq!(module_rumble_due(944, GUILTY_CROSS, 3, true, 0).count(), 1);
        assert_eq!(module_rumble_due(944, SINGLE, 3, true, 0).count(), 0);
    }

    #[test]
    fn every_site_is_a_slot_b_address_and_unique() {
        let mut sites: Vec<u32> = MODULE_RUMBLE.iter().map(|m| m.site).collect();
        assert!(
            sites
                .iter()
                .all(|&s| (0x801F_69D8..0x8020_0000).contains(&s))
        );
        let mut keyed: Vec<(u32, u32)> = MODULE_RUMBLE.iter().map(|m| (m.entry, m.site)).collect();
        keyed.sort_unstable();
        keyed.dedup();
        sites.sort_unstable();
        assert_eq!(keyed.len(), MODULE_RUMBLE.len());
    }

    #[test]
    fn the_ozma_spray_takes_its_two_draws() {
        let w = ozma_spray(64 + 5, 0x3F);
        assert_eq!(
            (w.pulse, w.level, w.pulse_rate, w.level_rate),
            (0xDB, 0x81, 8, 8)
        );
    }
}
