//! The slot-B cast and summon modules' hit-tint stores: the colour word
//! `+0x04`, its blend `+0x0C` and the impact selector `+0x21F` a tick body
//! writes into a struck seat beside its damage.
//!
//! The selector is what makes a struck body burn: `FUN_8004998C` sheds an
//! effect-`0x0B` puff off a selector-`1` body once its red lane reaches
//! `0xB0`, and an effect-`0x10` spark off a selector-`2` one. The table holds
//! each store as the disassembly has it, keyed on the arm that makes it, so
//! the host can apply it on the same edge. PROT 0903 and 0904 write theirs
//! from their tick ports and PROT 0962 from its chain
//! (`cast_arm_ticks::BladeBreathShape::strike_sub_flag`), so neither is here.

use crate::cast_module_camera::{SINGLE_BODY, SPEED_SCALAR};

/// Which seats an entry writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactSeat {
    /// `actor_table[caster + 0x1DD]`, the seat the cast latched.
    Victim,
    /// `actor_table[ctx + 0x13]`.
    Caster,
    /// `actor_table[7]`, the summoned creature.
    Creature,
    /// `actor_table[3..7]` with non-zero HP: the loops the row hits run.
    LiveMonsters,
    /// `actor_table[0..ctx[+0]]`: the party row.
    Party,
    /// The party row's seats with HP `+0x14C`: the loops that skip a fallen
    /// seat ahead of its hit. The host reads the HP after the tick's hits, so
    /// a seat this hit felled goes untinted, and it does not read the
    /// `+0x16E & 4` skip.
    LiveParty,
}

/// The edge of its arm a store sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactEdge {
    /// The pass the arm's gate lets through.
    Pass,
    /// Every pass of the arm, held or not.
    EveryPass,
    /// Every pass whose module countdown reads at most this.
    CountdownAtMost(i32),
}

/// One store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleImpact {
    pub entry: u32,
    pub body: u32,
    pub arm: u8,
    pub edge: ImpactEdge,
    pub seat: ImpactSeat,
    /// `+0x04`, where the site writes it.
    pub colour: Option<u32>,
    /// Whether the site also stores `+0x0C = 0x1000`.
    pub full_blend: bool,
    /// `+0x21F`, where the site writes it.
    pub selector: Option<u8>,
    /// `+0x21C`, the tint state machine's state, where the site writes it.
    pub tint_state: Option<u8>,
}

/// `0x3FF`: the colour word's red lane at its top, no green or blue.
const RED: u32 = 0x3FF;
/// `+0x21C = 7`: the tint state that ramps a body's red lane up.
const SOFT_RED_TINT_STATE: u8 = 7;
/// `0x3FF00000`: the colour word's blue lane at its top.
const BLUE: u32 = 0x3FF0_0000;
/// `0x3FF003FF`: red and blue.
const MAGENTA: u32 = 0x3FF0_03FF;

const fn tint(entry: u32, arm: u8, seat: ImpactSeat, colour: u32, selector: u8) -> ModuleImpact {
    ModuleImpact {
        entry,
        body: SINGLE_BODY,
        arm,
        edge: ImpactEdge::Pass,
        seat,
        colour: Some(colour),
        full_blend: true,
        selector: Some(selector),
        tint_state: None,
    }
}

const fn latch(
    entry: u32,
    arm: u8,
    edge: ImpactEdge,
    seat: ImpactSeat,
    selector: u8,
) -> ModuleImpact {
    ModuleImpact {
        entry,
        body: SINGLE_BODY,
        arm,
        edge,
        seat,
        colour: None,
        full_blend: false,
        selector: Some(selector),
        tint_state: None,
    }
}

/// Every store the table carries, in PROT order.
pub const MODULE_IMPACTS: &[ModuleImpact] = &[
    // PROT 0908 Zenoir. Arm 7's pass lights the creature (`0x801F761C`, the
    // delay slot of the arm's exit); arm 8's opener strikes the victim
    // (`0x801F77DC..0x801F7800`); arm 11's finisher strikes the victim
    // (`0x801F7D58..0x801F7D6C`) and its splash every other live monster
    // (`0x801F7F60..0x801F7F7C`).
    latch(908, 7, ImpactEdge::Pass, ImpactSeat::Creature, 1),
    tint(908, 8, ImpactSeat::Victim, RED, 1),
    tint(908, 11, ImpactSeat::Victim, RED, 1),
    tint(908, 11, ImpactSeat::LiveMonsters, RED, 1),
    // PROT 0909 Viguro. Arm 11 marks the live row (`0x801F7248..0x801F7268`,
    // beside `+0x21C = 4`), the sweep strikes it (`0x801F76DC..0x801F76F8`).
    latch(909, 11, ImpactEdge::Pass, ImpactSeat::LiveMonsters, 2),
    tint(909, 13, ImpactSeat::LiveMonsters, BLUE, 2),
    // PROT 0914 Gola Gola. Arm 12's pass marks the live row
    // (`0x801F7458..0x801F7488`, its colour word cleared and the row hidden),
    // arm 13 strikes it (`0x801F77E8..0x801F7804`).
    latch(914, 12, ImpactEdge::Pass, ImpactSeat::LiveMonsters, 1),
    tint(914, 13, ImpactSeat::LiveMonsters, RED, 1),
    // PROT 0916 Aluru: arm 18's victim (`0x801F8684`, beside its rate `2`).
    latch(916, 18, ImpactEdge::Pass, ImpactSeat::Victim, 2),
    // PROT 0923 Gilium: the finale's row strike (`0x801F8A08..0x801F8A24`).
    tint(923, 20, ImpactSeat::LiveMonsters, BLUE, 1),
    // PROT 0932 Meta: the row strike (`0x801F7C5C..0x801F7C78`).
    tint(932, 18, ImpactSeat::LiveMonsters, RED, 1),
    // PROT 0934 Ozma. Arms 17 and 18 hold the creature's sparks every pass
    // (`0x801F7F38`, `0x801F7F58`, `s4 = actor_table[7]`), arms 21..=24 flash
    // the live row magenta every time they run (`0x801F85B0..0x801F85D4`,
    // `0x801F86B4..0x801F86CC`, `0x801F8948`, `0x801F8D4C`).
    latch(934, 17, ImpactEdge::EveryPass, ImpactSeat::Creature, 2),
    latch(934, 18, ImpactEdge::EveryPass, ImpactSeat::Creature, 2),
    tint(934, 21, ImpactSeat::LiveMonsters, MAGENTA, 2),
    tint(934, 22, ImpactSeat::LiveMonsters, MAGENTA, 2),
    tint(934, 23, ImpactSeat::LiveMonsters, MAGENTA, 2),
    tint(934, 24, ImpactSeat::LiveMonsters, MAGENTA, 2),
    // PROT 0937 Hyper Lightning: arm 7's victim sparks, every pass
    // (`0x801F7580`, `s2 = actor_table[caster + 0x1DD]`, `s3` the caster).
    latch(937, 7, ImpactEdge::EveryPass, ImpactSeat::Victim, 2),
    // PROT 0948 Cross Beam: arm 3's hit (`0x801F7030..0x801F7044`).
    tint(948, 3, ImpactSeat::Victim, RED, 1),
    // PROT 0950 Rolling Flare, body `0x801F79F8`: the last `0x60` scalars of
    // arm 3's countdown turn both seats red (`+0x21C = 7`, the soft-red tint
    // state, `0x801F7F00..0x801F7F28`), and the last `0x40` burn them
    // (`sll v0,v0,0x6` / `slt` at `0x801F7F34..0x801F7F48`). The burn sheds
    // fire only once the red lane the tint state ramps reaches `0xB0`.
    ModuleImpact {
        entry: 950,
        body: crate::cast_arm_ticks::ROLLING_FLARE_TICK,
        arm: 3,
        edge: ImpactEdge::CountdownAtMost(SPEED_SCALAR * 0x60),
        seat: ImpactSeat::Victim,
        colour: None,
        full_blend: false,
        selector: None,
        tint_state: Some(SOFT_RED_TINT_STATE),
    },
    ModuleImpact {
        entry: 950,
        body: crate::cast_arm_ticks::ROLLING_FLARE_TICK,
        arm: 3,
        edge: ImpactEdge::CountdownAtMost(SPEED_SCALAR * 0x60),
        seat: ImpactSeat::Caster,
        colour: None,
        full_blend: false,
        selector: None,
        tint_state: Some(SOFT_RED_TINT_STATE),
    },
    ModuleImpact {
        entry: 950,
        body: crate::cast_arm_ticks::ROLLING_FLARE_TICK,
        arm: 3,
        edge: ImpactEdge::CountdownAtMost(SPEED_SCALAR << 6),
        seat: ImpactSeat::Victim,
        colour: None,
        full_blend: false,
        selector: Some(1),
        tint_state: None,
    },
    ModuleImpact {
        entry: 950,
        body: crate::cast_arm_ticks::ROLLING_FLARE_TICK,
        arm: 3,
        edge: ImpactEdge::CountdownAtMost(SPEED_SCALAR << 6),
        seat: ImpactSeat::Caster,
        colour: None,
        full_blend: false,
        selector: Some(1),
        tint_state: None,
    },
    // Arm 3's pass, after its hit, puts both seats' tint state back to `0`
    // (`sb zero,0x21c` at `0x801F800C` / `0x801F8010`), past the stores
    // above on the same pass: the red ramps back down and the fire stops
    // with it, the selector left standing.
    ModuleImpact {
        entry: 950,
        body: crate::cast_arm_ticks::ROLLING_FLARE_TICK,
        arm: 3,
        edge: ImpactEdge::Pass,
        seat: ImpactSeat::Caster,
        colour: None,
        full_blend: false,
        selector: None,
        tint_state: Some(0),
    },
    ModuleImpact {
        entry: 950,
        body: crate::cast_arm_ticks::ROLLING_FLARE_TICK,
        arm: 3,
        edge: ImpactEdge::Pass,
        seat: ImpactSeat::Victim,
        colour: None,
        full_blend: false,
        selector: None,
        tint_state: Some(0),
    },
    // PROT 0958 Blazing Slash: arms 6 and 7 hold the victim's fire every pass
    // (`0x801F7440`, `0x801F7574`, `s1` the victim).
    ModuleImpact {
        entry: 958,
        body: crate::cast_module_ticks::BLAZING_SLASH_TICK,
        arm: 6,
        edge: ImpactEdge::EveryPass,
        seat: ImpactSeat::Victim,
        colour: None,
        full_blend: false,
        selector: Some(1),
        tint_state: None,
    },
    ModuleImpact {
        entry: 958,
        body: crate::cast_module_ticks::BLAZING_SLASH_TICK,
        arm: 7,
        edge: ImpactEdge::EveryPass,
        seat: ImpactSeat::Victim,
        colour: None,
        full_blend: false,
        selector: Some(1),
        tint_state: None,
    },
    // PROT 0960 Plasma Strike's burst, arm `0x0D`
    // (`sw 0x3FF00000, 0x1000` then `sb v1,0x21f(s3)` at `0x801F81D4..0x801F8200`).
    ModuleImpact {
        entry: 960,
        body: crate::cast_module_ticks::PLASMA_STRIKE_TICK,
        arm: 0x0D,
        edge: ImpactEdge::Pass,
        seat: ImpactSeat::Victim,
        colour: None,
        full_blend: true,
        selector: Some(2),
        tint_state: None,
    },
    // PROT 0963 Genocidal Cannon: arm 14's party row (`0x801F7D10..0x801F7D20`).
    ModuleImpact {
        entry: 963,
        // `(0xB3, 0x801F6A20)`: Genocidal Cannon's trampoline arm.
        body: 0x801F_6A20,
        arm: 14,
        edge: ImpactEdge::Pass,
        seat: ImpactSeat::Party,
        colour: Some(RED),
        full_blend: false,
        selector: Some(1),
        tint_state: None,
    },
    // PROT 0964's Element Strike body (`0xB0..=0xB2`), whose arm 1 forks into
    // three six-arm runs: Flame on phases `2..=7`, Wind on `0x32..=0x37`,
    // Thunder on `0x64..=0x69`. The standing-in creature is seat 7
    // (`lw v1,0x1C(0x801C9370)`). `k = 3`'s pass gives it its element's tint
    // state and the selector; `k = 4` restores `+0x21F` from the prologue's
    // stack word `1` every pass (`lbu t3,0x28(sp)`), and its pass tints and
    // burns each party seat the hit loop reaches.
    element(
        2 + 3,
        ImpactEdge::Pass,
        ImpactSeat::Creature,
        None,
        Some(1),
        Some(3),
    ),
    element(
        2 + 4,
        ImpactEdge::EveryPass,
        ImpactSeat::Creature,
        None,
        Some(1),
        None,
    ),
    element(
        2 + 4,
        ImpactEdge::Pass,
        ImpactSeat::LiveParty,
        Some(RED),
        Some(1),
        None,
    ),
    element(
        0x32 + 3,
        ImpactEdge::Pass,
        ImpactSeat::Creature,
        None,
        None,
        Some(8),
    ),
    element(
        0x64 + 3,
        ImpactEdge::Pass,
        ImpactSeat::Creature,
        None,
        Some(2),
        Some(4),
    ),
    element(
        0x64 + 4,
        ImpactEdge::EveryPass,
        ImpactSeat::Creature,
        None,
        Some(1),
        None,
    ),
    element(
        0x64 + 4,
        ImpactEdge::Pass,
        ImpactSeat::LiveParty,
        Some(BLUE),
        Some(2),
        None,
    ),
];

/// One store of PROT 0964's Element Strike body. Flame's `k = 3` pass:
/// `+0x21C = 3`, `+0x21F = 1` (`0x801F828C..0x801F829C`); its `k = 4` head
/// `+0x21F = 1` (`0x801F82D0`) and party loop `+0x04 = 0x3FF`, `+0x21F = 1`
/// (`0x801F8608..0x801F8618`). Wind's `k = 3` pass: `+0x21C = 8`
/// (`0x801F7AD0`). Thunder's `k = 3` pass: `+0x21C = 4`, `+0x21F = 2`
/// (`0x801F7208..0x801F7218`); its `k = 4` head `+0x21F = 1` (`0x801F7238`)
/// and party loop `+0x04 = 0x3FF00000`, `+0x21F = 2` (`0x801F7598..0x801F75A8`).
/// Each party loop skips a seat with no HP or with `+0x16E & 4` ahead of its
/// hit (`0x801F850C..0x801F8528`).
const fn element(
    arm: u8,
    edge: ImpactEdge,
    seat: ImpactSeat,
    colour: Option<u32>,
    selector: Option<u8>,
    tint_state: Option<u8>,
) -> ModuleImpact {
    ModuleImpact {
        entry: 964,
        body: crate::cast_module_ticks::ELEMENT_STRIKE_CHAIN.body,
        arm,
        edge,
        seat,
        colour,
        full_blend: false,
        selector,
        tint_state,
    }
}

/// The stores `entry`'s `body` makes on `arm` this tick: `passed` is whether
/// the arm's gate let it through, `countdown` the module countdown before
/// this tick's drain.
pub fn module_impacts_due(
    entry: u32,
    body: u32,
    arm: u8,
    passed: bool,
    countdown: i32,
) -> impl Iterator<Item = &'static ModuleImpact> {
    MODULE_IMPACTS.iter().filter(move |m| {
        m.entry == entry
            && m.body == body
            && m.arm == arm
            && match m.edge {
                ImpactEdge::Pass => passed,
                ImpactEdge::EveryPass => true,
                ImpactEdge::CountdownAtMost(c) => countdown <= c,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hit_store_waits_for_its_arms_pass() {
        assert_eq!(module_impacts_due(948, SINGLE_BODY, 3, false, 0).count(), 0);
        let hit: Vec<_> = module_impacts_due(948, SINGLE_BODY, 3, true, 0).collect();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].colour, Some(RED));
        assert!(hit[0].full_blend);
        assert_eq!(hit[0].selector, Some(1));
    }

    #[test]
    fn rolling_flare_burns_both_seats_for_its_countdowns_last_stretch() {
        let body = crate::cast_arm_ticks::ROLLING_FLARE_TICK;
        let burns = |cd| {
            module_impacts_due(950, body, 3, false, cd)
                .filter(|m| m.selector == Some(1))
                .map(|m| m.seat)
                .collect::<Vec<_>>()
        };
        let reddens = |cd| {
            module_impacts_due(950, body, 3, false, cd)
                .filter(|m| m.tint_state == Some(7))
                .count()
        };
        let top = SPEED_SCALAR << 6;
        assert!(burns(top + 1).is_empty());
        assert_eq!(burns(top), [ImpactSeat::Victim, ImpactSeat::Caster]);
        // The red tint state goes in first, from the last `0x60` scalars.
        assert_eq!(reddens(SPEED_SCALAR * 0x60 + 1), 0);
        assert_eq!(reddens(SPEED_SCALAR * 0x60), 2);
        assert_eq!(reddens(top), 2);
    }

    #[test]
    fn a_body_the_table_does_not_name_writes_nothing() {
        assert_eq!(module_impacts_due(950, SINGLE_BODY, 3, true, 0).count(), 0);
        assert_eq!(module_impacts_due(906, SINGLE_BODY, 13, true, 0).count(), 0);
    }
}
