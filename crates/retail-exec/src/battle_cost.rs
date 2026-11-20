//! Battle frames priced by unit off PCSX-Redux library states.

use crate::frame_runner::{FrameCost, FrameRunner, Routine, linked_primitives};
use crate::pcsx_state::PcsxState;
use crate::{Memory, Spent};
use anyhow::Result;
use std::cell::RefCell;
use std::rc::Rc;

/// Every routine `frames` battle frames reach by `jal`.
pub fn reached(state: &PcsxState, frames: usize) -> Result<Vec<u32>> {
    let mut probe = FrameRunner::with_routines(state, &[]);
    probe.settle()?;
    let mut vas: Vec<u32> = Vec::new();
    for _ in 0..frames {
        probe.frame()?;
        vas.extend(probe.cpu.calls.iter().copied());
        vas.sort_unstable();
        vas.dedup();
    }
    Ok(vas)
}

/// The effect pool `_DAT_8007BD30`: its live children (`0x80` slots of
/// `0x20` bytes from `+0x10`, live while byte `0` is set, `lbu v0,0(s0)` at
/// `0x801E0174`) and live masters (`0x20` slots of `0x1C` bytes from
/// `+0x1010`, `lbu v0,0(a0)` at `0x801E0120`).
pub fn pool_live(m: &mut Memory) -> (u32, u32) {
    let pool = m.rd32(0x8007_BD30);
    if pool & 0x1FFF_FFFF >= 0x20_0000 || pool == 0 {
        return (0, 0);
    }
    let children = (0..0x80)
        .filter(|i| m.rd8(pool + 0x10 + 0x20 * i) != 0)
        .count();
    let masters = (0..0x20)
        .filter(|i| m.rd8(pool + 0x1010 + 0x1C * i) != 0)
        .count();
    (children as u32, masters as u32)
}

/// Every routine a battle frame reaches by `jal`, priced inclusive over
/// `frames` frames after the state's frame in progress.
pub fn reached_routines(state: &PcsxState, frames: usize) -> Result<Vec<Vec<(u32, Spent, i64)>>> {
    let vas = reached(state, frames)?;
    let routines: Vec<Routine> = vas.iter().map(|&va| Routine { va, name: "" }).collect();
    let mut run = FrameRunner::with_routines(state, &routines);
    run.settle()?;
    let mut out = Vec::new();
    for _ in 0..frames {
        let f = run.frame()?;
        out.push(
            f.rows
                .iter()
                .filter(|r| r.spent.calls > 0)
                .map(|r| (r.routine.va, r.spent, r.linked))
                .collect(),
        );
    }
    Ok(out)
}

/// The per-primitive dispatcher `FUN_80043390`: its group head (`lw s7,0(a1)`),
/// its jump into a leaf (`jr t8`) and its exit.
pub const DISPATCHER: u32 = 0x8004_3390;
const GROUP_HEAD: u32 = 0x8004_3580;
const GROUP_JUMP: u32 = 0x8004_3614;
const DISPATCH_EXIT: u32 = 0x8004_361C;
/// The dispatcher's copy of its colour word (`sw a1,-0x2e8(t2)`) and its
/// winding mask (`sw t9,-0x2d8(t2)`, `0x7FFFFFFF` for a double-sided word).
const WORD: u32 = 0x1F80_002C;
const WINDING_MASK: u32 = 0x1F80_003C;
const CALL_TOP: u32 = 0x1F80_0043;

/// One primitive group a leaf walked.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    pub leaf: u32,
    /// Primitives in the group (the header's low half).
    pub prims: u32,
    /// Of them, linked into the frame's ordering table.
    pub linked: u32,
    /// From the `jr` into the leaf to its jump back to the group head.
    pub cycles: u64,
    pub word: u32,
    /// The top byte of the call's colour word (`sb s6,-0x2d1(t2)`,
    /// `0x1F800043`): its `0x10` bit picks the far-corner depth, its `0x40`
    /// bit the quad leaves' off-screen test.
    pub call_top: u8,
    pub double_sided: bool,
    /// The clock at the `jr`.
    pub at: u64,
}

/// A profiled call with its clock span.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub entry: u32,
    pub start: u64,
    pub end: u64,
    pub spent: Spent,
}

impl Span {
    pub fn holds(&self, at: u64) -> bool {
        self.start <= at && at < self.end
    }
}

/// One battle frame, by group and by call.
#[derive(Debug, Clone)]
pub struct BattleFrame {
    pub cost: FrameCost,
    pub groups: Vec<Group>,
    pub spans: Vec<Span>,
}

impl BattleFrame {
    /// The groups walked inside calls of `va`.
    pub fn groups_in(&self, va: u32) -> impl Iterator<Item = &Group> {
        let spans: Vec<Span> = self
            .spans
            .iter()
            .filter(|s| s.entry == va)
            .copied()
            .collect();
        self.groups
            .iter()
            .filter(move |g| spans.iter().any(|s| s.holds(g.at)))
    }
    /// The innermost call of a [`CONTEXTS`] routine running at `at`.
    pub fn context_of(&self, at: u64) -> Option<&Span> {
        self.spans
            .iter()
            .filter(|s| {
                s.holds(at)
                    && CONTEXTS
                        .iter()
                        .any(|r| r.va == s.entry && r.va != DISPATCHER)
            })
            .min_by_key(|s| s.end - s.start)
    }
    pub fn spans_of(&self, va: u32) -> impl Iterator<Item = &Span> {
        self.spans.iter().filter(move |s| s.entry == va)
    }
}

fn clock_mark(m: &mut Memory) -> u32 {
    m.clock as u32
}

#[derive(Default)]
struct Pending {
    open: Option<(u64, u32, u32, u32, u32, bool, u8)>,
    done: Vec<Group>,
}

/// A battle state resumed frame by frame, every dispatcher group and every
/// call of `routines` priced on its own.
pub struct BattleRunner {
    pub run: FrameRunner,
    groups: Rc<RefCell<Pending>>,
}

impl BattleRunner {
    pub fn new(state: &PcsxState, routines: &[Routine]) -> Result<Self> {
        let mut run = FrameRunner::with_routines(state, routines);
        run.cpu.profile_mark = Some(clock_mark);
        let groups = Rc::new(RefCell::new(Pending::default()));
        let g = groups.clone();
        run.cpu.probe(GROUP_JUMP, move |r, m, _| {
            let linked = linked_primitives(m);
            let word = m.rd32(WORD);
            let ds = m.rd32(WINDING_MASK) == 0x7FFF_FFFF;
            let top = m.rd8(CALL_TOP);
            g.borrow_mut().open = Some((m.clock, r.r[24], r.r[11] + 1, linked, word, ds, top));
        });
        for at in [GROUP_HEAD, DISPATCH_EXIT] {
            let g = groups.clone();
            run.cpu.probe(at, move |_, m, _| {
                let mut p = g.borrow_mut();
                if let Some((t0, leaf, prims, l0, word, double_sided, call_top)) = p.open.take() {
                    let linked = linked_primitives(m).wrapping_sub(l0);
                    p.done.push(Group {
                        leaf,
                        prims,
                        linked,
                        cycles: m.clock - t0,
                        word,
                        call_top,
                        double_sided,
                        at: t0,
                    });
                }
            });
        }
        run.settle()?;
        groups.borrow_mut().done.clear();
        Ok(Self { run, groups })
    }

    pub fn frame(&mut self) -> Result<BattleFrame> {
        let base = self.run.cpu.mem.clock;
        let cost = self.run.frame()?;
        let groups = std::mem::take(&mut self.groups.borrow_mut().done);
        // The marks are the clock's low word; the frame is far shorter.
        let full = |m: u32| -> u64 {
            let lo = base as u32;
            base + u64::from(m.wrapping_sub(lo))
        };
        let spans = cost
            .calls
            .iter()
            .map(|c| Span {
                entry: c.entry,
                start: full(c.mark.0),
                end: full(c.mark.1),
                spent: c.spent,
            })
            .collect();
        Ok(BattleFrame {
            cost,
            groups,
            spans,
        })
    }
}

const fn r(va: u32, name: &'static str) -> Routine {
    Routine { va, name }
}

/// The kind-4 node's procedural mesh builder `FUN_80028158(out, mode,
/// count | phase << 8, src)`.
pub const MESH_BUILDER: u32 = 0x8002_8158;

/// The battle frame's parts a unit belongs to.
pub const CONTEXTS: &[Routine] = &[
    r(0x8002_5EEC, "frame handler"),
    r(0x8001_6444, "master frame driver"),
    r(0x8001_6B6C, "frame pacer"),
    r(0x8001_D140, "actor render pass"),
    r(0x8001_ADA4, "actor draw"),
    r(0x8004_80D8, "battle draw tick"),
    r(0x8004_8A08, "body draw"),
    r(0x8004_9348, "ghost walk"),
    r(0x8004_998C, "TRS keyframe decode"),
    r(DISPATCHER, "prim dispatcher"),
    r(0x8002_735C, "TMD renderer"),
    r(0x8001_C204, "floor clip"),
    r(0x8002_6CE4, "field render"),
    r(0x801D_02C0, "ground grid"),
    r(0x8004_DA00, "battle template tick"),
    r(0x8002_519C, "actor tick pass"),
    r(0x8004_6A20, "battle frame driver"),
    r(0x8002_1DF4, "move-VM part tick"),
    r(0x801E_0080, "effect pool walker"),
    r(0x8002_B28C, "ME keyframe decode"),
    r(0x8002_8158, "kind-4 mesh builder"),
];

/// The effect pool walker `FUN_801E0080` (battle overlay), visit by visit.
pub mod walker {
    /// The walker's entry.
    pub const ENTRY: u32 = 0x801E_0080;
    /// A sweep of the first pass begins (`0x801E0104`, once per vsync of the
    /// frame step while the pool is up).
    pub const SWEEP: u32 = 0x801E_0104;
    /// A live child's first-pass visit: from past its live test (`0x801E049C`)
    /// to the slot advance (`0x801E0694`).
    pub const STEP_OPEN: u32 = 0x801E_049C;
    pub const STEP_CLOSE: u32 = 0x801E_0694;
    /// One frame advance of a child's anim cursor (`0x801E0594`): reached
    /// once by a child whose countdown ran out, and again for every delay
    /// byte of zero it reads on (the overrun).
    pub const ADVANCE: u32 = 0x801E_0594;
    /// A live child's draw in the second pass: past its live test
    /// (`0x801E06F4`) to the slot advance (`0x801E09B0`), the projector
    /// `FUN_800195A8` and the link `FUN_8003D2C4` included.
    pub const DRAW_OPEN: u32 = 0x801E_06F4;
    pub const DRAW_CLOSE: u32 = 0x801E_09B0;
}

/// One live child's visit by the walker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visit {
    /// A first-pass step: its cycles and the frame advances it made (`0`:
    /// its countdown branch).
    Step { cycles: u64, advances: u32 },
    /// A second-pass draw.
    Draw { cycles: u64 },
}

#[derive(Default)]
struct WalkLog {
    open: Option<(u64, u32)>,
    sweeps: u32,
    visits: Vec<Visit>,
    timed: Vec<(u64, u64, Visit)>,
}

/// The walker's visits over a frame, collected beside a [`FrameRunner`].
pub struct WalkerProbe {
    log: Rc<RefCell<WalkLog>>,
}

impl WalkerProbe {
    pub fn arm(run: &mut FrameRunner) -> Self {
        use walker::*;
        let log = Rc::new(RefCell::new(WalkLog::default()));
        let l = log.clone();
        run.cpu
            .probe(SWEEP, move |_, _, _| l.borrow_mut().sweeps += 1);
        for at in [STEP_OPEN, DRAW_OPEN] {
            let l = log.clone();
            run.cpu
                .probe(at, move |_, m, _| l.borrow_mut().open = Some((m.clock, 0)));
        }
        let l = log.clone();
        run.cpu.probe(ADVANCE, move |_, _, _| {
            if let Some((_, n)) = &mut l.borrow_mut().open {
                *n += 1;
            }
        });
        let l = log.clone();
        run.cpu.probe(STEP_CLOSE, move |_, m, _| {
            let mut w = l.borrow_mut();
            if let Some((t, advances)) = w.open.take() {
                let v = Visit::Step {
                    cycles: m.clock - t,
                    advances,
                };
                w.visits.push(v);
                w.timed.push((t, m.clock - t, v));
            }
        });
        let l = log.clone();
        run.cpu.probe(DRAW_CLOSE, move |_, m, _| {
            let mut w = l.borrow_mut();
            if let Some((t, _)) = w.open.take() {
                let v = Visit::Draw {
                    cycles: m.clock - t,
                };
                w.visits.push(v);
                w.timed.push((t, m.clock - t, v));
            }
        });
        Self { log }
    }

    /// The visits since the last take, each with the clock it opened on.
    pub fn take_timed(&self) -> Vec<(u64, u64, Visit)> {
        std::mem::take(&mut self.log.borrow_mut().timed)
    }

    /// The sweeps and visits since the last take.
    pub fn take(&self) -> (u32, Vec<Visit>) {
        let mut w = self.log.borrow_mut();
        let s = std::mem::take(&mut w.sweeps);
        (s, std::mem::take(&mut w.visits))
    }
}

/// The dispatcher's leaves (`0x8007657C`, kinds `8..=19`, banks `0..=3`):
/// `(entry, per-primitive loop head, back edge)`. Every leaf loads its first
/// primitive, runs its loop from the head to the `bgtz t3` back edge once a
/// primitive, and jumps back to the dispatcher's group head
/// (`j 0x80043580`).
pub const LEAVES: &[(u32, u32, u32)] = &[
    (0x8004_409C, 0x8004_40E0, 0x8004_422C),
    (0x8004_423C, 0x8004_4284, 0x8004_4424),
    (0x8004_4434, 0x8004_447C, 0x8004_45A0),
    (0x8004_45B0, 0x8004_45F8, 0x8004_4788),
    (0x8004_3658, 0x8004_3694, 0x8004_3758),
    (0x8004_3768, 0x8004_37B0, 0x8004_38A8),
    (0x8004_3B58, 0x8004_3B94, 0x8004_3C5C),
    (0x8004_3C6C, 0x8004_3CB4, 0x8004_3DC4),
    (0x8004_38B8, 0x8004_38F4, 0x8004_39D4),
    (0x8004_39E4, 0x8004_3A2C, 0x8004_3B48),
    (0x8004_3DD4, 0x8004_3E10, 0x8004_3F00),
    (0x8004_3F10, 0x8004_3F58, 0x8004_408C),
    (0x8004_48B0, 0x8004_48EC, 0x8004_4A2C),
    (0x8004_4A3C, 0x8004_4A84, 0x8004_4C04),
    (0x8004_4FDC, 0x8004_5018, 0x8004_5184),
    (0x8004_5194, 0x8004_51DC, 0x8004_53AC),
    (0x8004_4C14, 0x8004_4C50, 0x8004_4DB8),
    (0x8004_4DC8, 0x8004_4E10, 0x8004_4FCC),
    (0x8004_53BC, 0x8004_53F8, 0x8004_5574),
    (0x8004_57C4, 0x8004_5800, 0x8004_5978),
    (0x8004_5584, 0x8004_55CC, 0x8004_57B4),
    (0x8004_5988, 0x8004_59D0, 0x8004_5BA4),
    (0x8004_5BB4, 0x8004_5BE8, 0x8004_5C80),
];

/// The leaves' off-screen reject.
pub const OFFSCREEN_REJECT: u32 = 0x8004_4798;

/// How one primitive left its leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrimFate {
    /// A packet written and linked (the packet cursor `t1` moved).
    Linked,
    /// Past the winding and near tests, dropped by `FUN_80044798`.
    OffScreen,
    /// Dropped before the off-screen test: back-facing, or under the near
    /// floor (or the leaf carries no off-screen test).
    Culled,
}

/// One primitive's pass through a leaf.
#[derive(Debug, Clone, Copy)]
pub struct PrimVisit {
    pub leaf: u32,
    /// The clock at the loop head.
    pub at: u64,
    pub fate: PrimFate,
    /// From the loop head to the back edge.
    pub cycles: u64,
}

#[derive(Default)]
struct PrimLog {
    open: Option<(u32, u64, u32, bool)>,
    visits: Vec<PrimVisit>,
}

/// Every primitive a frame's leaves walk, timed head to back edge.
pub struct LeafProbe {
    log: Rc<RefCell<PrimLog>>,
}

impl LeafProbe {
    pub fn arm(run: &mut FrameRunner) -> Self {
        let log = Rc::new(RefCell::new(PrimLog::default()));
        for &(leaf, head, back) in LEAVES {
            let l = log.clone();
            run.cpu.probe(head, move |r, m, _| {
                l.borrow_mut().open = Some((leaf, m.clock, r.r[9], false));
            });
            let l = log.clone();
            run.cpu.probe(back, move |r, m, _| {
                let mut p = l.borrow_mut();
                if let Some((leaf, t, t1, rejected)) = p.open.take() {
                    let fate = if r.r[9] != t1 {
                        PrimFate::Linked
                    } else if rejected {
                        PrimFate::OffScreen
                    } else {
                        PrimFate::Culled
                    };
                    p.visits.push(PrimVisit {
                        leaf,
                        at: t,
                        fate,
                        cycles: m.clock - t,
                    });
                }
            });
        }
        let l = log.clone();
        run.cpu.probe(OFFSCREEN_REJECT, move |_, _, _| {
            if let Some(o) = &mut l.borrow_mut().open {
                o.3 = true;
            }
        });
        Self { log }
    }

    pub fn take(&self) -> Vec<PrimVisit> {
        std::mem::take(&mut self.log.borrow_mut().visits)
    }
}

/// What a stretch of a battle frame's time is spent on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// A call of a [`CONTEXTS`] routine (a body draw inside the ghost walk is
    /// [`Part::GhostDraw`]).
    Call(u32),
    GhostDraw,
    /// A dispatcher group, from the `jr` into its leaf to the group head,
    /// less its primitives: the leaf's set-up and the first primitive's loads.
    Group,
    /// One primitive's loop pass in a leaf.
    Prim(u32, PrimFate),
    /// The walker's first-pass visit of a live child: on its countdown or one
    /// frame advance, or reading on through a retired batch (more than one
    /// advance).
    ChildStep,
    ChildOverrun,
    /// The walker's second-pass draw of a live child.
    ChildDraw,
}

/// One frame's time by part, each part's own (its nested parts taken out).
#[derive(Debug, Clone, Default)]
pub struct FrameParts {
    pub ring: u16,
    pub step: u8,
    /// Live effect-pool children and masters as the frame opened
    /// ([`pool_live`]).
    pub children: u32,
    pub masters: u32,
    /// The battle's action state as the frame opened (`ctx[+7]`, the context
    /// at `*0x8007BD24`).
    pub state: u8,
    /// `(visits, own cycles)` a part; for [`Part::ChildOverrun`] the visits'
    /// advances are in [`Self::overrun_advances`].
    pub parts: std::collections::BTreeMap<Part, (u64, u64)>,
    /// Each part's cycles with its nested parts in.
    pub inclusive: std::collections::BTreeMap<Part, u64>,
    pub overrun_advances: u64,
    pub sweeps: u32,
    /// Dispatcher calls made directly under each part (a body draw's
    /// objects).
    pub objects: std::collections::BTreeMap<Part, u64>,
    /// Each kind-4 mesh build: `(shape, segments)` (`(mode >> 3) & 0xF`,
    /// `count & 0xFF`).
    pub builds: Vec<(u32, u32)>,
    /// Primitives linked, per routine, off a second run with link marks.
    pub linked: std::collections::BTreeMap<u32, i64>,
}

impl FrameParts {
    pub fn get(&self, p: Part) -> (u64, u64) {
        self.parts.get(&p).copied().unwrap_or_default()
    }
    /// The ring entry in cycles.
    pub fn ring_cycles(&self) -> f64 {
        f64::from(self.ring) * crate::PCSX_CYCLES_PER_HBLANK
    }
}

/// A battle state resumed with every part of its frames timed.
pub struct PartRunner {
    runner: BattleRunner,
    builds: Rc<RefCell<Vec<(u32, u32)>>>,
    leaves: LeafProbe,
    walker: WalkerProbe,
    linked: FrameRunner,
}

impl PartRunner {
    pub fn new(state: &PcsxState) -> Result<Self> {
        let mut runner = BattleRunner::new(state, CONTEXTS)?;
        let leaves = LeafProbe::arm(&mut runner.run);
        let walker = WalkerProbe::arm(&mut runner.run);
        let builds = Rc::new(RefCell::new(Vec::new()));
        let b = builds.clone();
        runner.run.cpu.probe(MESH_BUILDER, move |r, _, _| {
            b.borrow_mut().push(((r.r[5] >> 3) & 0xF, r.r[6] & 0xFF));
        });
        let mut linked = FrameRunner::with_routines(state, CONTEXTS);
        linked.settle()?;
        Ok(Self {
            runner,
            builds,
            leaves,
            walker,
            linked,
        })
    }

    pub fn frame(&mut self) -> Result<FrameParts> {
        let (children, masters) = pool_live(&mut self.runner.run.cpu.mem);
        let state = {
            let m = &mut self.runner.run.cpu.mem;
            let ctx = m.rd32(0x8007_BD24);
            if ctx & 0x1FFF_FFFF < 0x20_0000 && ctx != 0 {
                m.rd8(ctx + 7)
            } else {
                0
            }
        };
        let bf = self.runner.frame()?;
        let prims = self.leaves.take();
        let (sweeps, _) = self.walker.take();
        let visits = self.walker.take_timed();
        let lf = self.linked.frame()?;
        let mut iv: Vec<(u64, u64, Part)> = Vec::new();
        for s in &bf.spans {
            iv.push((s.start, s.end, Part::Call(s.entry)));
        }
        for g in &bf.groups {
            iv.push((g.at, g.at + g.cycles, Part::Group));
        }
        for p in &prims {
            iv.push((p.at, p.at + p.cycles, Part::Prim(p.leaf, p.fate)));
        }
        let mut overrun_advances = 0;
        for &(at, cycles, v) in &visits {
            let part = match v {
                Visit::Draw { .. } => Part::ChildDraw,
                Visit::Step { advances, .. } if advances > 1 => {
                    overrun_advances += u64::from(advances);
                    Part::ChildOverrun
                }
                Visit::Step { .. } => Part::ChildStep,
            };
            iv.push((at, at + cycles, part));
        }
        // Outer before inner: by start, then the longer first.
        iv.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        let mut parts: std::collections::BTreeMap<Part, (u64, u64)> = Default::default();
        // (end, part, own cycles so far)
        let mut stack: Vec<(u64, Part, u64)> = Vec::new();
        let mut objects: std::collections::BTreeMap<Part, u64> = Default::default();
        let mut inclusive: std::collections::BTreeMap<Part, u64> = Default::default();
        let close = |stack: &mut Vec<(u64, Part, u64)>,
                     upto: u64,
                     parts: &mut std::collections::BTreeMap<Part, (u64, u64)>| {
            while let Some(&(end, part, own)) = stack.last() {
                if end > upto {
                    break;
                }
                stack.pop();
                let e = parts.entry(part).or_default();
                e.0 += 1;
                e.1 += own;
            }
        };
        for (start, end, mut part) in iv {
            close(&mut stack, start, &mut parts);
            if part == Part::Call(0x8004_8A08)
                && stack.iter().any(|s| s.1 == Part::Call(0x8004_9348))
            {
                part = Part::GhostDraw;
            }
            if let Some(top) = stack.last_mut() {
                top.2 = top.2.saturating_sub(end - start);
                if part == Part::Call(DISPATCHER) {
                    *objects.entry(top.1).or_default() += 1;
                }
            }
            *inclusive.entry(part).or_default() += end - start;
            stack.push((end, part, end - start));
        }
        close(&mut stack, u64::MAX, &mut parts);
        let linked = lf.rows.iter().map(|r| (r.routine.va, r.linked)).collect();
        Ok(FrameParts {
            ring: bf.cost.ring,
            step: bf.cost.step,
            children,
            masters,
            state,
            parts,
            overrun_advances,
            sweeps,
            objects,
            inclusive,
            builds: std::mem::take(&mut *self.builds.borrow_mut()),
            linked,
        })
    }
}
