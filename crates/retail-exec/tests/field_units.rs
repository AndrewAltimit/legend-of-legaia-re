//! The field passes' per-unit cost table (`legaia_engine_core::world::
//! field_frame_units`) held against the runner: each of ten library field
//! states broken into its units (`psx_retail_exec::field_units`), every
//! pass priced from the table off that state's own unit counts, within 5%
//! of what the pass ran; and the units whose path is fixed land on the table
//! within 2% on every state.

use crate::common::library_state;
use legaia_engine_core::world::field_frame_units::unit_cycles;
use psx_retail_exec::field_units::field_units;

const STATES: &[(&str, &str)] = &[
    (
        "son_field_door_hop",
        "6f1f22770d047956493bed7c4d576080a04fbb533471f642fd6d906ed81672cb",
    ),
    (
        "stone_field_door_hop",
        "fbe63e8b7e3649cba7d93322a86964d16446bf22627147b9688bae5cdb87c342",
    ),
    (
        "jou_field_card_walk",
        "60879599cbddc6900a17d654bd7dc3a7bf907c7cdefc827b5c78663aed119d0c",
    ),
    (
        "retona_field_card_boot",
        "cb69ed5a9435041e9dabc83d29627aa7b7111446896305e9494178ffa63fcca3",
    ),
    (
        "bubu2_field_door_hop",
        "f6506aa81bb8087c133c9acfd57d2c1dafd3a61d7c17b2e35debbad520e20bc9",
    ),
    (
        "kor3_field_route_walk",
        "5653b2518f2bbb278bcae02674e2000871e28859de1f61836bebd6d90bd9e0b3",
    ),
    (
        "uru_field_run",
        "6889b1c9d1ecf990b30a8cc24c6a2aefef2835ee6baffb0fc77c5329284f4719",
    ),
    (
        "town01_field_card_boot",
        "6acd2d93d83e0f24a03d794f99485bea11820083ad07c8598923d4f9f03a3b34",
    ),
    (
        "doman_field_card_boot",
        "0ef4316a9350a4300ff142ef94143cb574746283378f0ea6b523c9dcb03962a2",
    ),
    (
        "korb2_field_card_boot",
        "a598c9057396248355089ec287132f3bc39b912e801de4efd7dbe548ff455ba4",
    ),
];

/// Units on one fixed path: an iteration costs the same whatever the data.
const FIXED: &[&str] = &[
    "decoration.cell[inbox]",
    "decoration.row[-]",
    "ground.cell[-]",
    "ground.cell[near+walk]",
    "ground.row[-]",
    "dispatch.group decode",
    "actor.track[-]",
    "actor.morph vertex[-]",
    "prim k14 0 tri linked",
    "prim k15 0 quad rejected",
    "prim k19 cued quad rejected",
];

#[test]
fn the_unit_table_prices_the_field_passes() {
    not_a_differential!(
        "prices retail passes off library states from a derived unit table; compares no port"
    );
    let mut priced = 0;
    for (label, fp) in STATES {
        let Some(state) = library_state(fp) else {
            continue;
        };
        let passes = field_units(&state).unwrap_or_else(|e| panic!("{label}: {e}"));
        for (name, pu) in &passes {
            if pu.total == 0 {
                continue;
            }
            let got = pu.priced(&unit_cycles).unwrap_or_else(|| {
                panic!(
                    "{label} {name}: a unit the table lacks: {:?}",
                    pu.units.keys()
                )
            });
            let err = (got - pu.total as f64) / pu.total as f64;
            assert!(
                err.abs() <= 0.05,
                "{label} {name}: table {got:.0} cycles, run {} ({:+.2}%)",
                pu.total,
                err * 100.0
            );
            for (u, (n, c)) in &pu.units {
                if *n > 0 && FIXED.contains(&u.as_str()) {
                    let per = *c as f64 / *n as f64;
                    let want = unit_cycles(u).expect("in the table");
                    assert!(
                        (per - want).abs() / want <= 0.02,
                        "{label} {u}: {per:.1} cycles against the table's {want}"
                    );
                }
            }
            priced += 1;
        }
    }
    eprintln!("{priced} passes priced");
}
