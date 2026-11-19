//! The field frame's passes priced off five library states and held against
//! PCSX-Redux's own profile of the same frame.
//!
//! `FrameRunner` resumes a state where its CPU was saved, runs the frame in
//! progress out and then one whole field frame, collecting each pass's
//! instructions and data accesses (`2 * instructions + accesses` cycles, the
//! emulator's charge). The figures below are
//! `scripts/pcsx-redux/autorun_field_frame_profile.lua` on the same states
//! (first whole frame after the load, `cycles - irq_cycles`: the interrupt
//! handler's time inside a call is taken out on both sides, see
//! `frame_runner.rs`). A pass that is priced with the wrong arguments, camera
//! or lists misses them by far more than the 3% this allows.

use crate::common::{library_state, resident_is_retail};
use psx_retail_exec::frame_runner::FrameRunner;

const DECORATION: u32 = 0x801F_7088;
const GROUND: u32 = 0x801F_6D48;
const GROUND_ALT: u32 = 0x801F_69EC;
const ACTOR_DRAW: u32 = 0x8001_D140;
const ACTOR_TICK: u32 = 0x8002_519C;
const FOG_WALK: u32 = 0x8003_F3FC;
const FRAME_DRIVER: u32 = 0x8001_6444;
/// The slot-B image the decoration pass and the ground emitter live in.
const SLOT_B: (usize, u32) = (900, 0x801F_69D8);

/// `(label, fingerprint, [decoration less ground, ground, actor draw, actor
/// tick, fog walk, frame driver])` in PCSX-Redux cycles, interrupts out.
const PROFILE: &[(&str, &str, [u64; 6])] = &[
    (
        "son_field_door_hop",
        "6f1f22770d047956493bed7c4d576080a04fbb533471f642fd6d906ed81672cb",
        [725_164, 47_816, 444_154, 25_585, 0, 1_245_726],
    ),
    (
        "stone_field_door_hop",
        "fbe63e8b7e3649cba7d93322a86964d16446bf22627147b9688bae5cdb87c342",
        [551_345, 664_432, 146_473, 34_083, 23_460, 1_424_923],
    ),
    (
        "jou_field_card_walk",
        "60879599cbddc6900a17d654bd7dc3a7bf907c7cdefc827b5c78663aed119d0c",
        [257_661, 323_236, 456_897, 240_931, 0, 1_281_744],
    ),
    (
        "retona_field_card_boot",
        "cb69ed5a9435041e9dabc83d29627aa7b7111446896305e9494178ffa63fcca3",
        [642_547, 101_228, 122_695, 43_372, 30_523, 945_553],
    ),
    (
        "bubu2_field_door_hop",
        "f6506aa81bb8087c133c9acfd57d2c1dafd3a61d7c17b2e35debbad520e20bc9",
        [434_414, 257_997, 428_848, 95_728, 23_532, 1_245_663],
    ),
];

#[test]
fn the_field_passes_cost_what_pcsx_redux_measures() {
    not_a_differential!(
        "prices retail passes off library states against PCSX-Redux's profile; compares no port"
    );
    let mut ran = 0;
    for (label, fp, want) in PROFILE {
        let Some(state) = library_state(fp) else {
            continue;
        };
        for (va, prot) in [
            (DECORATION, Some(SLOT_B)),
            (GROUND, Some(SLOT_B)),
            (ACTOR_DRAW, None),
            (FRAME_DRIVER, None),
        ] {
            assert!(
                resident_is_retail(&state, va, prot),
                "{label}: FUN_{va:08X} is not the retail image's"
            );
        }
        let mut run = FrameRunner::new(&state);
        run.settle().unwrap_or_else(|e| panic!("{label}: {e}"));
        let f = run.frame().unwrap_or_else(|e| panic!("{label}: {e}"));
        let c = |va: u32| f.of(va).cycles_pcsx();
        let ground = c(GROUND) + c(GROUND_ALT);
        let got = [
            c(DECORATION) - ground,
            ground,
            c(ACTOR_DRAW),
            c(ACTOR_TICK),
            c(FOG_WALK),
            c(FRAME_DRIVER),
        ];
        let names = [
            "decoration less ground",
            "ground",
            "actor draw",
            "actor tick",
            "fog walk",
            "frame driver",
        ];
        for i in 0..6 {
            let (g, w) = (got[i] as f64, want[i] as f64);
            let ok = if w == 0.0 {
                g == 0.0
            } else {
                (g - w).abs() / w <= 0.03
            };
            assert!(
                ok,
                "{label} {}: runner {} cycles, profile {} ({:+.2}%)",
                names[i],
                got[i],
                want[i],
                (g - w) / w * 100.0
            );
        }
        // The decoration pass links what it draws into the frame's table.
        assert!(f.row(DECORATION).is_some_and(|r| r.linked > 0), "{label}");
        ran += 1;
    }
    eprintln!("{ran} library states priced");
}
