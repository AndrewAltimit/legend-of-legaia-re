//! A field frame's passes broken into the units they repeat, each priced by
//! the instructions and accesses it runs (PCSX-Redux cycles), off one
//! resumed library state.
//!
//! The runner logs the clock at the head of every loop of the decoration
//! pass `FUN_801F7088`, the ground emitter `FUN_801F6D48`, the actor render
//! pass `FUN_8001D140` and the fog record walk `FUN_8003F3FC`, at the prim
//! dispatcher `FUN_80043390` and at each prim leaf's loop head. The time
//! between two arrivals at a head is one iteration's cost; the marks passed
//! on the way say which path it took (its class); a nested routine's time is
//! taken out of the iteration that called it and priced on its own. What is
//! left of a pass once every loop, dispatch and leaf is priced is its
//! per-call fixed cost. The units, their classes and the engine counts each
//! needs are in `docs/tooling/retail-frame-cost.md`.
//!
//! A prim leaf iteration's class is what became of the primitive: `linked`
//! when the leaf's packet cursor (`t1`) moved, `+offtest` when the leaf ran
//! the off-screen reject `0x80044798` on it, `rejected` otherwise (a back
//! face, or a near-plane or off-screen reject).

use crate::frame_runner::{FRAME_HANDLER, FrameRunner, Routine};
use crate::pcsx_state::PcsxState;
use anyhow::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};

const DISP: u32 = 0x8004_3390;
const GROUP: u32 = 0x8004_3580;
const DEND: u32 = 0x8004_3650;
const OFFSCREEN: u32 = 0x8004_4798;
const T1: usize = 9;

/// One prim leaf: its entry, its loop head (the `bgtz $t3` target), its
/// primitive kind (`flags >> 1`), its handler bank, and whether it draws
/// quads. The table is the dispatcher's (`0x8007657C`), see
/// `docs/subsystems/renderer.md`.
#[derive(Debug, Clone, Copy)]
pub struct Leaf {
    pub entry: u32,
    pub head: u32,
    pub kind: u8,
    pub bank: &'static str,
    pub quad: bool,
}

const fn leaf(entry: u32, head: u32, kind: u8, bank: &'static str, quad: bool) -> Leaf {
    Leaf {
        entry,
        head,
        kind,
        bank,
        quad,
    }
}

pub const LEAVES: &[Leaf] = &[
    leaf(0x8004_3658, 0x8004_3694, 12, "0", false),
    leaf(0x8004_3768, 0x8004_37B0, 13, "0", true),
    leaf(0x8004_38B8, 0x8004_38F4, 16, "0", false),
    leaf(0x8004_39E4, 0x8004_3A2C, 17, "0", true),
    leaf(0x8004_3B58, 0x8004_3B94, 14, "0", false),
    leaf(0x8004_3C6C, 0x8004_3CB4, 15, "0", true),
    leaf(0x8004_3DD4, 0x8004_3E10, 18, "0", false),
    leaf(0x8004_3F10, 0x8004_3F58, 19, "0", true),
    leaf(0x8004_409C, 0x8004_40E0, 8, "lit", false),
    leaf(0x8004_423C, 0x8004_4284, 9, "lit", true),
    leaf(0x8004_4434, 0x8004_447C, 10, "lit", false),
    leaf(0x8004_45B0, 0x8004_45F8, 11, "lit", true),
    leaf(0x8004_48B0, 0x8004_48EC, 12, "cued", false),
    leaf(0x8004_4A3C, 0x8004_4A84, 13, "cued", true),
    leaf(0x8004_4C14, 0x8004_4C50, 16, "cued", false),
    leaf(0x8004_4DC8, 0x8004_4E10, 17, "cued", true),
    leaf(0x8004_4FDC, 0x8004_5018, 14, "cued", false),
    leaf(0x8004_5194, 0x8004_51DC, 15, "cued", true),
    leaf(0x8004_53BC, 0x8004_53F8, 18, "cued", false),
    leaf(0x8004_5584, 0x8004_55CC, 19, "cued", true),
    leaf(0x8004_57C4, 0x8004_5800, 18, "b2", false),
    leaf(0x8004_5988, 0x8004_59D0, 19, "b2", true),
    leaf(0x8004_5BB4, 0x8004_5BE8, 19, "b3", true),
];

/// One loop of a pass: its head, the marks that end its last iteration,
/// the marks that name an iteration's path, and the nested `(start, end)`
/// intervals taken out of it.
#[derive(Debug, Clone, Copy)]
pub struct LoopSpec {
    pub name: &'static str,
    pub head: u32,
    pub exits: &'static [u32],
    pub classes: &'static [(u32, &'static str)],
    pub nested: &'static [(u32, u32)],
}

const fn lp(
    name: &'static str,
    head: u32,
    exits: &'static [u32],
    classes: &'static [(u32, &'static str)],
    nested: &'static [(u32, u32)],
) -> LoopSpec {
    LoopSpec {
        name,
        head,
        exits,
        classes,
        nested,
    }
}

const DISP_SPAN: (u32, u32) = (DISP, DEND);
const POSE_SPAN: (u32, u32) = (0x8001_C604, 0x8001_C798);

/// A pass: its entry, the mark its call ends on, its loops.
#[derive(Debug, Clone, Copy)]
pub struct PassSpec {
    pub name: &'static str,
    pub entry: u32,
    pub exit: u32,
    pub loops: &'static [LoopSpec],
}

pub const PASSES: &[PassSpec] = &[
    PassSpec {
        name: "decoration",
        entry: 0x801F_7088,
        exit: 0x8002_6E9C,
        loops: &[
            lp(
                "row",
                0x801F_74C8,
                &[0x801F_7904],
                &[],
                &[(0x801F_74DC, 0x801F_78EC)],
            ),
            lp(
                "cell",
                0x801F_74DC,
                &[0x801F_78EC],
                &[
                    (0x801F_7548, "inbox"),
                    (0x801F_7578, "record"),
                    (0x801F_78CC, "drawn"),
                ],
                &[DISP_SPAN],
            ),
        ],
    },
    PassSpec {
        name: "ground",
        entry: 0x801F_6D48,
        exit: 0x801F_7080,
        loops: &[
            lp(
                "row",
                0x801F_6DE4,
                &[0x801F_705C],
                &[],
                &[(0x801F_6DF0, 0x801F_7040)],
            ),
            lp(
                "cell",
                0x801F_6DF0,
                &[0x801F_7040],
                &[
                    (0x801F_6F28, "walk"),
                    (0x801F_6F74, "near"),
                    (0x801F_6FA0, "sorted"),
                ],
                &[],
            ),
        ],
    },
    PassSpec {
        name: "actor",
        entry: 0x8001_D140,
        exit: 0x8001_D17C,
        loops: &[
            lp(
                "actor",
                0x8001_ADF4,
                &[0x8001_B70C],
                &[
                    (0x8001_AE60, "gate1"),
                    (0x8001_AE78, "gate2"),
                    (0x8001_AE8C, "kind"),
                    (0x8001_B198, "model"),
                    (0x8001_B2C8, "placed"),
                ],
                &[
                    (0x8001_B964, 0x8001_BE78),
                    (0x8001_B430, 0x8001_B5D8),
                    DISP_SPAN,
                ],
            ),
            lp(
                "placed mesh",
                0x8001_B430,
                &[0x8001_B5D8],
                &[(0x8001_B474, "drawn")],
                &[DISP_SPAN, POSE_SPAN],
            ),
            lp(
                "model",
                0x8001_B964,
                &[0x8001_BE78],
                &[],
                &[(0x8001_BB14, 0x8001_BDCC)],
            ),
            lp(
                "object",
                0x8001_BB14,
                &[0x8001_BDCC],
                &[(0x8001_BC58, "posed")],
                &[DISP_SPAN, POSE_SPAN],
            ),
            lp(
                "pose",
                0x8001_C604,
                &[0x8001_C798],
                &[],
                &[(0x8001_C680, 0x8001_C6A4), (0x8005_B038, 0x8005_B0B0)],
            ),
            lp("track", 0x8001_C680, &[0x8001_C6A4], &[], &[]),
            lp(
                "morph lane",
                0x8005_B038,
                &[0x8005_B0B0],
                &[],
                &[(0x8005_B044, 0x8005_B0B0)],
            ),
            lp("morph vertex", 0x8005_B044, &[0x8005_B0B0], &[], &[]),
        ],
    },
    PassSpec {
        name: "fog",
        entry: 0x8003_F3FC,
        exit: 0x8003_F830,
        loops: &[lp(
            "record",
            0x8003_F3FC,
            &[0x8003_F830],
            &[(0x8003_F66C, "near1"), (0x8003_F7F8, "wind")],
            &[(HALF_SHEET, SHEET_KEPT), (HALF_SHEET, SHEET_CULLED)],
        )],
    },
];

/// The fog half-sheet emitter `FUN_8003F86C` and its two returns.
const HALF_SHEET: u32 = 0x8003_F86C;
const SHEET_KEPT: u32 = 0x8003_FA8C;
const SHEET_CULLED: u32 = 0x8003_FAB4;
/// The ground emitter's twin, inside the decoration pass's time.
const GROUND_ALT: u32 = 0x801F_69EC;

/// `(pc, clock, t1)` arrivals.
pub type MarkLog = [(u32, u64, u32)];

/// Every address the analysis marks.
pub fn marks() -> BTreeSet<u32> {
    let mut m: BTreeSet<u32> = [
        DISP,
        GROUP,
        DEND,
        OFFSCREEN,
        HALF_SHEET,
        SHEET_KEPT,
        SHEET_CULLED,
    ]
    .into_iter()
    .collect();
    for l in LEAVES {
        m.insert(l.entry);
        m.insert(l.head);
    }
    for p in PASSES {
        m.insert(p.entry);
        m.insert(p.exit);
        for l in p.loops {
            m.insert(l.head);
            m.extend(l.exits);
            m.extend(l.classes.iter().map(|c| c.0));
            for (a, b) in l.nested {
                m.insert(*a);
                m.insert(*b);
            }
        }
    }
    m
}

/// `class -> (iterations, cycles)` of one loop over a log, nested spans out.
pub fn loop_costs(log: &MarkLog, spec: &LoopSpec) -> BTreeMap<String, (u64, u64)> {
    let mut out: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let n = log.len();
    let mut i = 0;
    while i < n {
        if log[i].0 != spec.head {
            i += 1;
            continue;
        }
        let t0 = log[i].1;
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut nest = 0;
        let mut j = i + 1;
        while j < n && log[j].0 != spec.head && !spec.exits.contains(&log[j].0) {
            let pc = log[j].0;
            let ends: Vec<u32> = spec
                .nested
                .iter()
                .filter(|(s, _)| *s == pc)
                .map(|(_, e)| *e)
                .collect();
            if !ends.is_empty() {
                let mut k = j + 1;
                while k < n && !ends.contains(&log[k].0) {
                    k += 1;
                }
                if k < n {
                    nest += log[k].1 - log[j].1;
                }
                j = k;
                continue;
            }
            if let Some((_, c)) = spec.classes.iter().find(|(a, _)| *a == pc) {
                seen.insert(c);
            }
            j += 1;
        }
        if j >= n {
            break;
        }
        let key = if seen.is_empty() {
            "-".to_string()
        } else {
            seen.into_iter().collect::<Vec<_>>().join("+")
        };
        let e = out.entry(key).or_default();
        e.0 += 1;
        e.1 += log[j].1 - t0 - nest;
        i = j;
    }
    out
}

/// The dispatcher's and the leaves' units over a log:
/// `dispatch.draw`, `dispatch.group decode`, `leaf group ...`, `prim ...`.
pub fn leaf_units(log: &MarkLog, units: &mut BTreeMap<String, (u64, u64)>) {
    let n = log.len();
    let mut add = |k: String, c: u64| {
        let e = units.entry(k).or_default();
        e.0 += 1;
        e.1 += c;
    };
    let mut draw_tail = Vec::new();
    let mut i = 0;
    while i < n {
        let (pc, clk, _) = log[i];
        if pc == DISP && i + 1 < n && log[i + 1].0 == GROUP {
            add("dispatch.draw".into(), log[i + 1].1 - clk);
        } else if pc == GROUP && i + 1 < n {
            if LEAVES.iter().any(|l| l.entry == log[i + 1].0) {
                add("dispatch.group decode".into(), log[i + 1].1 - clk);
            } else if log[i + 1].0 == DEND {
                draw_tail.push(log[i + 1].1 - clk);
            }
        } else if let Some(l) = LEAVES.iter().find(|l| l.entry == pc) {
            let mut j = i + 1;
            while j < n && log[j].0 != GROUP {
                j += 1;
            }
            let heads: Vec<usize> = (i + 1..j).filter(|&k| log[k].0 == l.head).collect();
            let name = format!(
                "k{} {} {}",
                l.kind,
                l.bank,
                if l.quad { "quad" } else { "tri" }
            );
            if let Some(&h) = heads.first() {
                add(format!("leaf group {name}"), log[h].1 - clk);
            }
            for (a, &k) in heads.iter().enumerate() {
                let end = heads.get(a + 1).copied().unwrap_or(j);
                if end >= n {
                    break;
                }
                let off = (k + 1..end).any(|q| log[q].0 == OFFSCREEN);
                let linked = log[end].2 != log[k].2;
                let class = format!(
                    "{}{}",
                    if linked { "linked" } else { "rejected" },
                    if off { "+offtest" } else { "" }
                );
                add(format!("prim {name} {class}"), log[end].1 - log[k].1);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    // The terminal group test and the epilogue belong to the draw.
    if let Some(e) = units.get_mut("dispatch.draw") {
        e.1 += draw_tail.iter().sum::<u64>();
    }
}

/// One pass's units on one frame: `unit -> (count, cycles)`, and the pass's
/// own cost they add up to.
#[derive(Debug, Clone, Default)]
pub struct PassUnits {
    pub calls: u64,
    pub total: u64,
    pub units: BTreeMap<String, (u64, u64)>,
}

impl PassUnits {
    /// The pass's cost priced from a `unit -> cycles` table instead.
    pub fn priced(&self, table: &dyn Fn(&str) -> Option<f64>) -> Option<f64> {
        let mut sum = 0.0;
        for (u, (n, _)) in &self.units {
            if *n > 0 {
                sum += *n as f64 * table(u)?;
            }
        }
        Some(sum)
    }
}

/// The whole frame's units by pass, off the first whole frame after the
/// state.
pub fn field_units(state: &PcsxState) -> Result<BTreeMap<&'static str, PassUnits>> {
    let routines: Vec<Routine> = PASSES
        .iter()
        .map(|p| Routine {
            va: p.entry,
            name: p.name,
        })
        .chain([
            Routine {
                va: FRAME_HANDLER,
                name: "field frame handler",
            },
            Routine {
                va: GROUND_ALT,
                name: "ground emitter (alt)",
            },
        ])
        .collect();
    let mut run = FrameRunner::with_routines(state, &routines);
    run.settle()?;
    run.cpu.marks = marks().into_iter().collect();
    run.cpu.mark_reg = T1;
    run.cpu.mark_log.clear();
    let f = run.frame()?;
    let log = std::mem::take(&mut run.cpu.mark_log);
    let ground_in_decoration = f.of(0x801F_6D48).cycles_pcsx() + f.of(GROUND_ALT).cycles_pcsx();
    let mut out = BTreeMap::new();
    for p in PASSES {
        let mut slice = Vec::new();
        let mut calls = 0;
        let mut i = 0;
        while i < log.len() {
            if log[i].0 == p.entry {
                let mut j = i;
                while j < log.len() && log[j].0 != p.exit {
                    j += 1;
                }
                slice.extend_from_slice(&log[i..(j + 1).min(log.len())]);
                calls += 1;
                i = j;
            }
            i += 1;
        }
        let mut pu = PassUnits {
            calls,
            total: f.of(p.entry).cycles_pcsx()
                - if p.name == "decoration" {
                    ground_in_decoration
                } else {
                    0
                },
            units: BTreeMap::new(),
        };
        for l in p.loops {
            for (c, v) in loop_costs(&slice, l) {
                pu.units.insert(format!("{}.{}[{}]", p.name, l.name, c), v);
            }
        }
        leaf_units(&slice, &mut pu.units);
        if p.name == "fog" {
            // Each half-sheet call, by the return it takes.
            let mut k = 0;
            while k < slice.len() {
                if slice[k].0 == HALF_SHEET {
                    let mut q = k + 1;
                    while q < slice.len() && ![SHEET_KEPT, SHEET_CULLED].contains(&slice[q].0) {
                        q += 1;
                    }
                    if q < slice.len() {
                        let class = if slice[q].0 == SHEET_KEPT {
                            "kept"
                        } else {
                            "culled"
                        };
                        let e = pu
                            .units
                            .entry(format!("fog.half-sheet[{class}]"))
                            .or_default();
                        e.0 += 1;
                        e.1 += slice[q].1 - slice[k].1;
                    }
                    k = q;
                }
                k += 1;
            }
        }
        let accounted: u64 = pu.units.values().map(|v| v.1).sum();
        if calls > 0 {
            if accounted > pu.total {
                bail!(
                    "{}: units {accounted} past the pass's {} cycles",
                    p.name,
                    pu.total
                );
            }
            pu.units
                .insert(format!("{}.call", p.name), (calls, pu.total - accounted));
        }
        out.insert(p.name, pu);
    }
    Ok(out)
}

/// One actor the render lists walked on a frame: its record, the fields the
/// walk reads to pick its arm, and what drawing it ran.
#[derive(Debug, Clone, Default)]
pub struct ActorDraw {
    /// The actor record (`s6` at the list walk's head `0x8001ADF4`).
    pub actor: u32,
    /// `+0x56`, the draw kind the walk's jump table takes (`1..=11`).
    pub kind: u16,
    /// `+0x10`, the flags the walk gates on (`& 0x80` transform, `& 0xA`
    /// skip).
    pub flags: u32,
    /// `+0x52`.
    pub flags52: u16,
    /// The origin the walk transforms (`+0x14..+0x18`).
    pub pos: [i16; 3],
    /// The view depth the walk stored (`+0x34`).
    pub depth: i32,
    /// Dispatcher draws, primitive groups and primitives (leaf iterations)
    /// its iteration ran, and the iteration's cycles, everything nested in.
    pub draws: u32,
    pub groups: u32,
    pub prims: u32,
    pub cycles: u64,
}

/// What each actor the five render lists walk draws on the first whole
/// frame after `state`: one row per list entry, in walk order.
pub fn actor_draws(state: &PcsxState) -> Result<Vec<ActorDraw>> {
    const HEAD: u32 = 0x8001_ADF4;
    const EXIT: u32 = 0x8001_B70C;
    const S6: usize = 22;
    let mut run = FrameRunner::with_routines(state, &[]);
    run.settle()?;
    let mut m: BTreeSet<u32> = [HEAD, EXIT, DISP, GROUP].into_iter().collect();
    m.extend(LEAVES.iter().map(|l| l.head));
    run.cpu.marks = m.into_iter().collect();
    run.cpu.mark_reg = S6;
    run.frame()?;
    let log = std::mem::take(&mut run.cpu.mark_log);
    let mut out = Vec::new();
    let mut i = 0;
    while i < log.len() {
        if log[i].0 != HEAD {
            i += 1;
            continue;
        }
        let a = log[i].2;
        let mut d = ActorDraw {
            actor: a,
            ..ActorDraw::default()
        };
        let mut j = i + 1;
        while j < log.len() && log[j].0 != HEAD && log[j].0 != EXIT {
            match log[j].0 {
                DISP => d.draws += 1,
                GROUP => d.groups += 1,
                _ => d.prims += 1,
            }
            j += 1;
        }
        if j >= log.len() {
            break;
        }
        // The group test runs once more than the groups, at each draw's end.
        d.groups = d.groups.saturating_sub(d.draws);
        d.cycles = log[j].1 - log[i].1;
        let mem = &mut run.cpu.mem;
        d.kind = mem.rd16(a + 0x56);
        d.flags = mem.rd32(a + 0x10);
        d.flags52 = mem.rd16(a + 0x52);
        d.pos = [0x14, 0x16, 0x18].map(|o| mem.rd16(a + o) as i16);
        d.depth = mem.rd32(a + 0x34) as i32;
        out.push(d);
        i = j;
    }
    Ok(out)
}
