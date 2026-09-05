//! Rules-parity fixture for the VRChat slot-machine port.
//!
//! The Udon behaviour `scripts/vrchat-world/world-project/Assets/LegaiaWorld/
//! Udon/LegaiaSlotMachine.cs` is a C# translation of [`SlotMachine`]'s rules
//! kernel. Nothing ties the two implementations together at build time, so a
//! translation slip (an RNG fold, `%` semantics, an int-width difference)
//! would drift silently. This test drives the engine kernel through a scripted
//! spin/stop trace and pins the full outcome stream - strips, stop rows,
//! claimed values, feature modes, payouts, net take, balance - into a JSON
//! fixture committed next to the Unity kit. The Unity side replays the same
//! trace against the actual UdonSharp class (menu **Legaia > Verify Slot
//! Rules**, `LegaiaSlotTools.cs`) and diffs every field.
//!
//! Everything in the fixture is pure arithmetic from a committed seed constant
//! plus a synthetic payout table - no Sony bytes, so the fixture is
//! committable and the test runs disc-free.
//!
//! Two traces:
//! - `retail-seed`: the retail entry seed `0x6C0A2AF0`, normal-mode play;
//! - `feature-seed`: the first seed (deterministic search) whose trace enters
//!   a feature mode and completes a bonus round quickly, so the mode-1/2 stop
//!   plans, the display-strip swap, and the product payout are all covered.
//!
//! Regenerate after an intentional rules change with:
//!
//! ```text
//! LEGAIA_BLESS_SLOT_FIXTURE=1 cargo test -p legaia-engine-core --test slot_verify_fixture
//! ```

use legaia_asset::slot_payout::SlotPayoutTable;
use legaia_engine_core::slot_machine::{
    ENTRY_LCG_SEED, SlotMachine, SlotPhase, SlotRng, build_reel,
};

/// Synthetic per-symbol payout table (u8, index = symbol id). Deliberately
/// NOT the disc's values: the fixture tests the logic, not the data, and this
/// keeps every byte of the file non-Sony.
const PAYOUTS: [u8; 10] = [2, 4, 6, 8, 10, 20, 30, 60, 100, 200];

const START_BALANCE: i32 = 100_000;

/// Ticks inserted before stopping reel `r` of spin `i` - an arbitrary but
/// fixed schedule both sides implement verbatim, so the reels travel
/// different distances every spin.
fn stop_gap(spin: usize, reel: usize) -> usize {
    match reel {
        0 => (spin * 7 + 3) % 11,
        1 => (spin * 5 + 2) % 13,
        _ => (spin * 3 + 1) % 7,
    }
}

struct SpinRecord {
    mode: u8,
    rows: [usize; 3],
    claimed: [i32; 3],
    line: i64,
    symbol: i64,
    payout: i32,
    net: i32,
    bonus: i32,
    mode_after: u8,
    balance: i32,
}

struct Trace {
    symbol_strips: Vec<Vec<u8>>,
    bonus_strips: Vec<Vec<u8>>,
    records: Vec<SpinRecord>,
}

/// Drive one machine through `spins` scripted spins and return the strip
/// snapshots plus the per-spin outcome records. Returns `None` if a spin was
/// refused (never happens at the fixture balance - a guard, not a path).
fn run_trace(seed: u32, spins: usize) -> Option<Trace> {
    let payouts = SlotPayoutTable { payouts: PAYOUTS };
    let mut machine = SlotMachine::new(payouts, seed, START_BALANCE);

    // The strips, reproduced through the public builder - `SlotMachine::new`
    // runs exactly this loop.
    let mut rng = SlotRng::new(seed);
    let mut symbol_strips = Vec::new();
    let mut bonus_strips = Vec::new();
    for _ in 0..3 {
        let (symbols, bonus) = build_reel(&mut rng);
        symbol_strips.push(symbols.to_vec());
        bonus_strips.push(bonus.to_vec());
    }

    let mut records = Vec::new();
    for spin in 0..spins {
        if !machine.spin() {
            return None;
        }
        let mode = machine.feature_mode();
        while machine.phase() == SlotPhase::Spinning {
            machine.tick();
        }
        for reel in 0..3 {
            for _ in 0..stop_gap(spin, reel) {
                machine.tick();
            }
            assert!(
                machine.stop_reel(reel),
                "stop refused (spin {spin} reel {reel})"
            );
        }
        let result = machine.last_result().expect("evaluated spin");
        let record = SpinRecord {
            mode,
            rows: [
                machine.payline_row(0),
                machine.payline_row(1),
                machine.payline_row(2),
            ],
            claimed: [machine.claimed(0), machine.claimed(1), machine.claimed(2)],
            line: result.line.map_or(-1, |l| l as i64),
            symbol: result.symbol.map_or(-1, |s| s as i64),
            payout: result.payout,
            net: machine.net_take(),
            bonus: machine.bonus_spins(),
            mode_after: machine.feature_mode(),
            balance: {
                machine.collect();
                machine.balance()
            },
        };
        records.push(record);
    }
    Some(Trace {
        symbol_strips,
        bonus_strips,
        records,
    })
}

/// Whether `seed` reaches a completed bonus round within `spins` scripted
/// spins (used by the deterministic feature-seed search).
fn completes_bonus_round(seed: u32, spins: usize) -> bool {
    let payouts = SlotPayoutTable { payouts: PAYOUTS };
    let mut machine = SlotMachine::new(payouts, seed, START_BALANCE);
    let mut saw_bonus = false;
    for spin in 0..spins {
        if !machine.spin() {
            return false;
        }
        while machine.phase() == SlotPhase::Spinning {
            machine.tick();
        }
        for reel in 0..3 {
            for _ in 0..stop_gap(spin, reel) {
                machine.tick();
            }
            if !machine.stop_reel(reel) {
                return false;
            }
        }
        if machine
            .last_result()
            .is_some_and(|r| r.bonus_spin || r.bonus_triggered)
        {
            saw_bonus = true;
        }
        machine.collect();
        // Completed: a bonus round ran and the machine is back to normal.
        if saw_bonus && machine.feature_mode() == 0 && machine.bonus_spins() <= 0 {
            return true;
        }
    }
    false
}

fn trace_json(name: &str, seed: u32, spins: usize) -> serde_json::Value {
    let trace = run_trace(seed, spins).expect("fixture trace refused a spin");
    serde_json::json!({
        "name": name,
        "seed": seed,
        "spins": spins,
        "start_balance": START_BALANCE,
        "symbol_strips": trace.symbol_strips,
        "bonus_strips": trace.bonus_strips,
        "results": trace.records.iter().map(|r| serde_json::json!({
            "mode": r.mode,
            "rows": r.rows,
            "claimed": r.claimed,
            "line": r.line,
            "symbol": r.symbol,
            "payout": r.payout,
            "net": r.net,
            "bonus": r.bonus,
            "mode_after": r.mode_after,
            "balance": r.balance,
        })).collect::<Vec<_>>(),
    })
}

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/vrchat-world/world-project/Assets/LegaiaWorld/Editor/slot-verify.json")
}

#[test]
fn udon_slot_fixture_matches_engine_kernel() {
    // The deterministic feature-seed search: first seed under the int31 bound
    // (the Unity side parses it into a C# int) whose scripted trace completes
    // a bonus round within 40 spins. The search is part of the contract - the
    // fixture records the winner, and re-running the search proves it stable.
    let mut feature_seed = None;
    for candidate in 0x1000u32.. {
        assert!(candidate < 0x7FFF_FFFF, "feature-seed search ran away");
        if completes_bonus_round(candidate, 40) {
            feature_seed = Some(candidate);
            break;
        }
    }
    let feature_seed = feature_seed.expect("feature seed");

    let fixture = serde_json::json!({
        "comment": "Generated by engine-core tests/slot_verify_fixture.rs - do not hand-edit. \
                    Pure arithmetic from the seeds below and a synthetic payout table; no disc data. \
                    Consumed by Unity menu Legaia > Verify Slot Rules (LegaiaSlotTools.cs).",
        "payouts": PAYOUTS,
        "traces": [
            trace_json("retail-seed", ENTRY_LCG_SEED, 60),
            trace_json("feature-seed", feature_seed, 40),
        ],
    });
    let generated = serde_json::to_string_pretty(&fixture).expect("serialize fixture") + "\n";

    let path = fixture_path();
    if std::env::var_os("LEGAIA_BLESS_SLOT_FIXTURE").is_some() {
        std::fs::write(&path, &generated).expect("write fixture");
        println!("blessed {}", path.display());
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e} (bless with LEGAIA_BLESS_SLOT_FIXTURE=1)",
            path.display()
        )
    });
    assert_eq!(
        committed.replace("\r\n", "\n"),
        generated,
        "slot-verify.json is stale - regenerate with LEGAIA_BLESS_SLOT_FIXTURE=1 \
         cargo test -p legaia-engine-core --test slot_verify_fixture"
    );
}
