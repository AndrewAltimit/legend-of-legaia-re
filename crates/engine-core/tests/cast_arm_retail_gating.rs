//! Retail arm walk for the trampoline-reached capture-class tick bodies,
//! measured under PCSX-Redux and pinned here.
//!
//! The rows are the output of `scripts/pcsx-redux/autorun_capture_arm_gating.lua`
//! reduced by `scripts/pcsx-redux/analyze_capture_arm_gating.py`: one cast per
//! action id, driven by converting a monster seat's queued action on a pre-turn
//! battle state, with dwell counted in **module ticks** off the band's single
//! tick dispatcher `FUN_801F2160` (`jal` at `0x801E50C8`, the only one in PROT
//! 0898). The method and the dwell discussion live in
//! `docs/subsystems/cast-module.md`.
//!
//! What this file asserts is the leg a capture settles and a static read
//! cannot: **which body VA retail actually entered** for each action id. The
//! port keys these arms on `(PROT entry, body VA)` because `0x801F6A04` is a
//! body in three different images at three different sizes, so a table whose VA
//! came from a dump's printed address rather than from execution would key the
//! wrong routine with every gate green.
//!
//! The dwell column is recorded, not asserted against the port: the ported
//! bodies advance one phase per tick and carry no frame budget, so retail's
//! dwell is an input to a future timing layer rather than a current invariant.
//! The arm *set* is asserted, because that is the dispatch bound walked.
//!
//! Two of the fourteen arms are absent: PROT 0943's `0x40` and PROT 0944's
//! `0x53` reach battle phase `0x70` with their module paged and then fault
//! before the first tick in the fight this corpus drives, so no walk exists to
//! record. Their siblings in the same two images complete from the same state.
//!
//! REF: FUN_801F2160

use legaia_engine_vm::cast_arm_ticks as arms;

/// One measured cast: action id, owning PROT entry, the body VA retail
/// entered, the phase bytes its arms walked in order, the dwell in module
/// ticks of each, and whether the walk reached a terminal arm (a `false` row's
/// last dwell is a floor - the capture window closed on it).
struct MeasuredCast {
    action: u8,
    prot: u16,
    body: u32,
    phases: &'static [u8],
    ticks: &'static [u32],
    complete: bool,
}

const fn cast(
    action: u8,
    prot: u16,
    body: u32,
    phases: &'static [u8],
    ticks: &'static [u32],
    complete: bool,
) -> MeasuredCast {
    MeasuredCast {
        action,
        prot,
        body,
        phases,
        ticks,
        complete,
    }
}

/// Driven from `party_basic_attack_vs_gobu_gobu` (one party seat, one monster
/// seat), one cast per row.
const MEASURED: &[MeasuredCast] = &[
    cast(
        0xAC,
        940,
        0x801F_7240,
        &[0, 1, 2, 3, 4, 5, 6, 7],
        &[1, 30, 64, 16, 16, 48, 18, 27],
        true,
    ),
    cast(
        0x50,
        940,
        0x801F_78B8,
        &[0, 1, 2, 3, 255],
        &[1, 3, 64, 19, 1],
        true,
    ),
    cast(
        0xAE,
        940,
        0x801F_78B8,
        &[0, 1, 2, 3, 255],
        &[1, 4, 64, 16, 1],
        true,
    ),
    cast(
        0x51,
        941,
        0x801F_730C,
        &[0, 1, 2, 3, 255],
        &[1, 21, 16, 32, 1],
        true,
    ),
    cast(
        0xB9,
        941,
        0x801F_6A04,
        &[0, 1, 2, 3, 4],
        &[1, 65, 32, 64, 25],
        true,
    ),
    cast(
        0xB5,
        943,
        0x801F_6A04,
        &[0, 1, 2, 3, 4],
        &[1, 65, 32, 64, 32],
        true,
    ),
    cast(
        0x37,
        944,
        0x801F_6A04,
        &[0, 1, 2, 3, 4, 5],
        &[1, 33, 32, 32, 64, 82],
        true,
    ),
    cast(
        0x5A,
        950,
        0x801F_79F8,
        &[0, 1, 2, 3, 4],
        &[1, 13, 1, 32, 13],
        true,
    ),
    cast(
        0xAB,
        950,
        0x801F_6A24,
        &[0, 1, 2, 3, 4, 5, 6],
        &[1, 65, 21, 64, 8, 40, 15],
        false,
    ),
    cast(
        0x71,
        956,
        0x801F_7298,
        &[0, 1, 2, 3, 255],
        &[1, 33, 32, 32, 1],
        true,
    ),
    cast(
        0xA2,
        962,
        0x801F_7AE4,
        &[0, 1, 2, 3],
        &[1, 21, 1, 1862],
        false,
    ),
    cast(
        0xA3,
        962,
        0x801F_74A0,
        &[0, 1, 2, 3, 4],
        &[1, 21, 1, 42, 684],
        false,
    ),
    cast(
        0xA4,
        962,
        0x801F_6D54,
        &[0, 1, 2, 3, 4, 255],
        &[1, 21, 1, 18, 42, 1],
        true,
    ),
];

/// The `(PROT entry, body VA)` pair the port keys each action id's arm on.
fn ported_body_for(action: u8) -> Option<(u16, u32)> {
    Some(match action {
        0xAC => (940, arms::GLARE_DIVIDE_BLIND_TICK),
        0x50 | 0xAE => (940, arms::GLARE_DIVIDE_SPLIT_TICK),
        0x51 => (941, arms::STEAL_TICK),
        0xB9 => (941, arms::STEAL_SWEEP_TICK),
        0x40 => (943, arms::CURSE_SINGLE_TICK),
        0xB5 => (943, arms::CURSE_MP_DRAIN_TICK),
        0x37 => (944, arms::GUILTY_CROSS_TICK),
        0x53 => (944, arms::GUILTY_CROSS_CURSE_TICK),
        0x5A => (950, arms::ROLLING_FLARE_TICK),
        0xAB => (950, arms::ROLLING_FLARE_SWEEP_TICK),
        0x71 => (956, arms::WATER_HAZARD_TICK),
        0xA2 => (962, arms::BLADE_BREATH_A_TICK),
        0xA3 => (962, arms::BLADE_BREATH_B_TICK),
        0xA4 => (962, arms::BLADE_BREATH_C_TICK),
        _ => return None,
    })
}

#[test]
fn measured_body_vas_match_the_ported_arm_keys() {
    assert!(!MEASURED.is_empty(), "the measured table must not be empty");
    for m in MEASURED {
        let (prot, body) = ported_body_for(m.action)
            .unwrap_or_else(|| panic!("action 0x{:02X} has no ported arm", m.action));
        assert_eq!(
            prot, m.prot,
            "action 0x{:02X}: retail paged PROT {}, the port keys PROT {prot}",
            m.action, m.prot
        );
        assert_eq!(
            body, m.body,
            "action 0x{:02X}: retail entered 0x{:08X}, the port keys 0x{body:08X}",
            m.action, m.body
        );
    }
}

/// `0x801F6A04` is measured as a live body entry in three different images, so
/// a port keyed on the VA alone would run one routine for all three.
#[test]
fn the_shared_body_va_is_measured_in_three_images() {
    let mut owners: Vec<u16> = MEASURED
        .iter()
        .filter(|m| m.body == arms::STEAL_SWEEP_TICK)
        .map(|m| m.prot)
        .collect();
    owners.sort_unstable();
    owners.dedup();
    assert_eq!(owners, vec![941, 943, 944]);
}

#[test]
fn measured_arm_walks_are_well_formed() {
    for m in MEASURED {
        assert_eq!(
            m.phases.len(),
            m.ticks.len(),
            "action 0x{:02X}: one dwell per arm",
            m.action
        );
        assert!(!m.phases.is_empty(), "action 0x{:02X}: no arms", m.action);
        assert_eq!(
            m.phases[0], 0,
            "action 0x{:02X}: starts at phase 0",
            m.action
        );
        for d in m.ticks {
            assert!(*d > 0, "action 0x{:02X}: a walked arm dwells", m.action);
        }
        // A phase byte either counts up by one or latches the terminal
        // `0xFF`; a gap would mean the walk skipped an arm.
        for pair in m.phases.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                b == a + 1 || b == arms::LATCHED_DONE_PHASE,
                "action 0x{:02X}: phase {a} -> {b} is neither an advance nor the latch",
                m.action
            );
        }
        // The chain-dispatched bodies latch `0xFF` and the terminal arm clears
        // the busy register on its single tick; the table-dispatched ones stop
        // on their last numbered arm instead.
        if m.phases.last() == Some(&arms::LATCHED_DONE_PHASE) {
            assert!(
                m.complete,
                "action 0x{:02X}: a latched walk is complete",
                m.action
            );
            assert_eq!(
                *m.ticks.last().unwrap(),
                1,
                "action 0x{:02X}: the latched arm runs one tick",
                m.action
            );
        }
    }
}
