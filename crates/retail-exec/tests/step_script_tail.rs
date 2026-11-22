//! The shared step-script tail of the window opener `FUN_801D388C` (PROT
//! 0898, `0x801D4BA0..`), run on from every case body: the step record
//! `PTR_DAT_801F4D34[case]` (count, kind, chip count, then `(record, mode)`
//! pairs) spawns its records through `FUN_801D8DE8`, here hooked to hand back
//! one scratch record per id.
//!
//! - **Kind 3** (cases `5`, `7`, `0x13`, `0x17`, `0x1A`, `0x1C`, `0x1E`: a list
//!   opening, or a target cancel back onto it) zeroes the list kernel's scroll
//!   top `_DAT_8007BB90` and selection `_DAT_8007BB98` (`0x801D4C14..
//!   0x801D4C28`); the other kinds leave them. The port's lists reopen on row
//!   0 on exactly those steps (`BattleSpellSession::new`, its target cancels,
//!   and the item window's cancel in `World`'s item flow).
//! - **The cross-button stash** (`_DAT_800846C8` set, `0x801D4CC0..
//!   0x801D4D78`): the first `chip count` spawned records are stored at
//!   `ctx[+0x1114..]`, and the one whose direction the highlight word
//!   `ctx[+0x880]` names takes frame style `2`, against
//!   `battle_chip_highlight` under `BattleCommandOpt::CrossButton` (and no
//!   highlight under the directional default).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_commit_log::LAUNCH_FIRST_RECORD;
use legaia_engine_system::option_values::BattleCommandOpt;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const WINDOW: u32 = 0x801D_388C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const STEPS: u32 = 0x801F_4D34;
const SCRATCH: u32 = 0x8017_0000;
const CROSS_ENTRY: u32 = 0x8008_46C8;
const LIST_TOP: u32 = 0x8007_BB90;
const LIST_ROW: u32 = 0x8007_BB98;
const LEFT: u32 = 0x8000;
const UP: u32 = 0x1000;
const RIGHT: u32 = 0x2000;
const DOWN: u32 = 0x4000;
/// The highlight word each chip pair carries in the stash's tests, pair `k`
/// (`0x801D4CF0..0x801D4D74`): the ring's Attack, Magic, Item, Spirit arms;
/// a pair prompt's Left and Right chips.
const PAIR_WORDS: [u32; 4] = [LEFT, RIGHT, UP, DOWN];

fn scratch(rec: u32) -> u32 {
    SCRATCH + 0x40 * rec
}

#[test]
fn the_step_script_tail_spawns_and_stashes_as_retail() {
    differential!(prot(898, WINDOW));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let spawned: Rc<RefCell<Vec<u32>>> = Rc::default();
    let modes: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let log = spawned.clone();
    let mlog = modes.clone();
    cpu.hook(0x801D_8DE8, move |r, _| {
        log.borrow_mut().push(r.r[A0]);
        mlog.borrow_mut().push((r.r[A0], r.r[A1]));
        r.r[V0] = scratch(r.r[A0] & 0xFF);
    });
    for leaf in [
        0x8003_5F04u32, // the text measure
        0x801D_5854,    // the camera
        0x801D_32BC,    // the member step
        0x801D_5778,    // the log row copy
        0x801D_99BC,    // the handle table reset
        0x801D_9AE8,
        0x801D_B318,
        0x8003_19A8,
    ] {
        cpu.stub(leaf, 0);
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + 4 * s, ACTOR + 0x400 * s);
    }
    // Every step record's kind byte is 0..=3, which the tail's kind
    // dispatch (`0x801D4BB8..0x801D4BE8`) and its scope row rest on.
    for case in 0..0x32 {
        let step = cpu.mem.rd32(STEPS + 4 * case);
        assert!(cpu.mem.rd8(step + 1) <= 3, "case {case:#x} kind");
    }
    let clean = cpu.mem.clone();
    let mut world = legaia_engine_core::world::World::new();
    let mut rng = Rng(0x5441_494C);
    let (mut resets, mut lit, mut cases_run) = (0u32, 0u32, [0u32; 0x32]);
    let (mut launches, mut swaps) = (0u32, 0u32);
    for n in 0..6000 {
        let case = rng.below(0x32);
        cpu.mem = clean.clone();
        spawned.borrow_mut().clear();
        modes.borrow_mut().clear();
        let rows = rng.below(4);
        cpu.mem.wr8(CTX + 0x1F, rows as u8);
        let mode = rng.below(2);
        let cross = rng.below(2) == 0;
        cpu.mem.wr32(CROSS_ENTRY, u32::from(cross));
        let word = [LEFT, RIGHT, UP, DOWN, 0][rng.below(5) as usize];
        cpu.mem.wr32(CTX + 0x880, word);
        // A chip count other than this step's, so the panel-swap reset
        // (`0x801D4C38..0x801D4C74`) cannot rewrite the word: 2 against 4
        // sums to 6, so the previous count is kept equal or zero.
        let step = cpu.mem.rd32(STEPS + 4 * case);
        let (count, kind, chips) = (
            cpu.mem.rd8(step),
            cpu.mem.rd8(step + 1),
            cpu.mem.rd8(step + 2),
        );
        let prev = [chips, 0, 2, 4][rng.below(4) as usize];
        cpu.mem.wr8(CTX + 0x275, prev);
        if rng.below(6) == 0 {
            cpu.mem.wr8(CTX + 6, 0x6E);
        }
        let (top, row) = (1 + rng.below(9), 1 + rng.below(9));
        cpu.mem.wr32(LIST_TOP, top);
        cpu.mem.wr32(LIST_ROW, row);
        let stop = cpu.call(WINDOW, &[case, mode]);
        assert_eq!(stop, Stop::Returned, "n {n} case {case:#x}");
        cases_run[case as usize] += 1;
        let at =
            format!("n {n} case {case:#x} kind {kind} chips {chips} cross {cross} word {word:#x}");

        // The spawn order is the step record's.
        let want: Vec<u32> = (0..u32::from(count))
            .map(|i| u32::from(cpu.mem.rd8(step + 3 + 2 * i)))
            .collect();
        // The case body may spawn ahead of the tail and the second switch
        // after it; the tail's run is one contiguous stretch.
        let got = spawned.borrow().clone();
        assert!(
            want.is_empty() || got.windows(want.len()).any(|w| w == want.as_slice()),
            "spawns {got:?} want {want:?}: {at}"
        );

        // The launch tails clone the log's `3 * rows` elements into
        // `LAUNCH_FIRST_RECORD + i` in the caller's mode (`0x801D5080..
        // 0x801D5124`); the Begin exit `0x24` re-spawns the rows after the
        // first, `0x2E + k`, in mode 1 (`0x801D56A0..0x801D56E4`).
        let launch = matches!(case, 5..=8 | 0xA | 0x2A | 0x2B | 0x30 | 0x31) || case == 9;
        let tail: Vec<(u32, u32)> = if launch {
            (0..3 * rows)
                .map(|i| (LAUNCH_FIRST_RECORD as u32 + i, mode))
                .collect()
        } else if case == 0x24 {
            (0..3 * rows).map(|k| (0x2E + k, 1)).collect()
        } else {
            Vec::new()
        };
        let got_modes = modes.borrow().clone();
        assert!(
            tail.is_empty() || got_modes.ends_with(&tail) || case == 0xA,
            "tail spawns {got_modes:?} want {tail:?}: {at}"
        );
        if case == 0xA {
            assert!(
                tail.is_empty() || got_modes.windows(tail.len()).any(|w| w == tail.as_slice()),
                "tail spawns {got_modes:?} want {tail:?}: {at}"
            );
        }
        launches += u32::from(launch && rows > 0);

        // Kind 3 sends the list back to row 0.
        let reset = kind == 3;
        assert_eq!(
            (cpu.mem.rd32(LIST_TOP), cpu.mem.rd32(LIST_ROW)),
            if reset { (0, 0) } else { (top, row) },
            "list words: {at}"
        );
        resets += u32::from(reset);

        // The highlight word (`0x801D4C30..0x801D4C88`): the commit confirm
        // and the 2 <-> 4 chip swap send it back to the left arm, which is
        // where `World::tick_battle_command` drops the ring's remembered arm
        // (`BattleState::ring_highlight`, the two-chip prompts and `0x6E`);
        // the new count is kept for the next step.
        let flow = cpu.mem.rd8(CTX + 6);
        let swap = prev != chips && u32::from(prev) + u32::from(chips) == 6;
        let want_word = if flow == 0x6E || swap { LEFT } else { word };
        assert_eq!(
            cpu.mem.rd32(CTX + 0x880),
            want_word,
            "highlight word: {at} prev {prev}"
        );
        assert_eq!(cpu.mem.rd8(CTX + 0x275), chips, "chip count: {at}");
        let word = want_word;
        swaps += u32::from(swap || flow == 0x6E);

        // The stash and the lit chip, against the port's highlight.
        world.toggles.battle_command = if cross {
            BattleCommandOpt::CrossButton
        } else {
            BattleCommandOpt::default()
        };
        if chips == 0 {
            continue;
        }
        let pairs = usize::from(chips);
        let records: Vec<u32> = (0..pairs as u32)
            .map(|k| u32::from(cpu.mem.rd8(step + 3 + 2 * k)))
            .collect();
        for (k, &rec) in records.iter().enumerate() {
            let stored = cpu.mem.rd32(CTX + 0x1114 + 4 * k as u32);
            assert_eq!(
                stored,
                if cross { scratch(rec) } else { 0 },
                "stash {k}: {at}"
            );
        }
        // The port's cursor on the word, the chip it lights.
        let cursor = PAIR_WORDS.iter().take(pairs).position(|&w| w == word);
        let port = cursor.and_then(|c| legaia_engine_battle_hud::battle_chip_highlight(&world, c));
        for (k, &rec) in records.iter().enumerate() {
            let style = cpu.mem.rd8(scratch(rec) + 0x1D);
            assert_eq!(
                style == 2,
                port == Some(k),
                "pair {k} record {rec:#x}: {at}"
            );
        }
        lit += u32::from(port.is_some());
    }
    assert!(
        resets > 500 && lit > 500 && launches > 500 && swaps > 500,
        "{resets} {lit} {launches} {swaps}"
    );
    assert!(cases_run.iter().all(|&c| c > 0), "{cases_run:?}");
}
