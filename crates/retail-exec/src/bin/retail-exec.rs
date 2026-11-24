//! `retail-exec`: run retail code in the R3000 interpreter off a save state.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use psx_retail_exec::PCSX_CYCLES_PER_HBLANK;
use psx_retail_exec::frame_runner::{FIELD_ROUTINES, FrameRunner, Routine};
use psx_retail_exec::pcsx_state::PcsxState;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Run retail code in the R3000 interpreter off a save state")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Break the first whole frame's field passes into their repeated units
    /// (loop iterations by path, dispatcher draws and groups, prim-leaf
    /// iterations by kind and outcome), each with its count and its
    /// PCSX-Redux cycles: `pass,unit,count,cycles,cycles_per_unit`.
    FieldUnits(UnitsArgs),
    /// Every entry the field actor render lists walk on the first whole
    /// frame: record, draw kind, flags, origin, view depth, and the
    /// dispatcher draws, groups, primitives and cycles its drawing ran.
    ActorDraws(UnitsArgs),
    /// Every entry the field actor tick passes walk on the first whole
    /// frame: record, tick handler, draw kind, sub-mode, flags and the
    /// cycles its tick ran.
    ActorTicks(UnitsArgs),
    /// Print where a PCSX-Redux state's CPU stood and the interrupt and
    /// counter registers it saved.
    StateInfo {
        #[command(flatten)]
        state: StateArg,
    },
    /// Resume a PCSX-Redux state (field or battle: both run the per-frame
    /// handler 0x80025EEC) and price each frame's routines on PCSX-Redux's
    /// scale (2 cycles an instruction, 1 a data access): calls,
    /// instructions, accesses by region, cycles, hblanks and the primitives
    /// each linked into the frame's ordering table.
    #[command(alias = "field-frame")]
    FrameCost {
        #[command(flatten)]
        state: StateArg,
        /// Whole frames to run after the state's frame in progress.
        #[arg(long, default_value_t = 4)]
        frames: usize,
        /// Take the vblank interrupt (kernel dispatch and libetc callbacks;
        /// the SIO pad exchange returns at once).
        #[arg(long)]
        interrupts: bool,
        /// One CSV row per routine per frame instead of the table.
        #[arg(long)]
        csv: bool,
        /// Also list every call of a listed routine with its own cost.
        #[arg(long)]
        calls: bool,
        /// Write `pc,hits,accesses` for every instruction run inside this
        /// routine (hex entry) on the first frame to `--hist-out`.
        #[arg(long)]
        hist: Option<String>,
        #[arg(long, default_value = "hist.csv")]
        hist_out: PathBuf,
        /// Log every arrival at these addresses (hex, comma separated) on the
        /// first frame, `pc,clock` to `--marks-out`.
        #[arg(long, value_delimiter = ',')]
        marks: Vec<String>,
        #[arg(long, default_value = "marks.csv")]
        marks_out: PathBuf,
        /// The register each mark records beside the clock.
        #[arg(long, default_value_t = 9)]
        mark_reg: usize,
        /// More routines to price (hex entries, comma separated), beside the
        /// field passes.
        #[arg(long, value_delimiter = ',')]
        funcs: Vec<String>,
        /// RAM words (hex, comma separated) printed at each frame's end,
        /// `name=ADDR` or `ADDR`, read as `u32` (`ADDR:u8` / `ADDR:s16`
        /// for a narrower kind).
        #[arg(long, value_delimiter = ',')]
        watch: Vec<String>,
        /// RAM halfwords written once the state's frame in progress has
        /// run out, `ADDR=VALUE` (hex) or `*PTR+OFF=VALUE` (the word at
        /// `PTR` plus `OFF`): a what-if on a retail state, such as raising
        /// an actor's `+0x42` (`*8007C364+42=1`, the player). Probe only.
        #[arg(long, value_delimiter = ',')]
        poke: Vec<String>,
    },
    /// Price a battle frame by unit (see `battle_cost`).
    BattleCost {
        #[command(flatten)]
        state: StateArg,
        /// Whole frames to run after the state's frame in progress.
        #[arg(long, default_value_t = 2)]
        frames: usize,
        /// `parts` (every unit of each frame, its own cycles), `reached`
        /// (every routine a frame reaches, inclusive), `series` (those per
        /// frame with the pool's live children), `walker`, `prims`,
        /// `groups`, `groupcsv`, `bodies`, `builds`, `spans`.
        #[arg(long, default_value = "reached")]
        mode: String,
    },
}

#[derive(clap::Args)]
struct UnitsArgs {
    #[command(flatten)]
    state: StateArg,
}

#[derive(clap::Args)]
struct StateArg {
    /// A `.sstate` file.
    #[arg(long, conflicts_with = "scenario")]
    state: Option<PathBuf>,
    /// A label in `scripts/scenarios.toml` with a PCSX-Redux state
    /// (its `backup_fingerprint` under `saves/library/pcsx-redux/`).
    #[arg(long)]
    scenario: Option<String>,
    /// The manifest to resolve `--scenario` against.
    #[arg(long, default_value = "scripts/scenarios.toml")]
    manifest: PathBuf,
}

impl StateArg {
    fn path(&self) -> Result<PathBuf> {
        if let Some(p) = &self.state {
            return Ok(p.clone());
        }
        let Some(label) = &self.scenario else {
            bail!("give --state or --scenario");
        };
        let m = psx_ram_image::ScenarioManifest::from_path(&self.manifest)?;
        let s = m
            .scenarios
            .iter()
            .find(|s| &s.label == label)
            .with_context(|| format!("no scenario {label}"))?;
        if let Some(fp) = &s.backup_fingerprint {
            let p = PathBuf::from(format!("saves/library/pcsx-redux/{fp}.sstate"));
            if p.exists() {
                return Ok(p);
            }
        }
        s.pcsx_redux_sstate
            .clone()
            .with_context(|| format!("{label} has no PCSX-Redux state"))
    }
    fn load(&self) -> Result<PcsxState> {
        let p = self.path()?;
        PcsxState::from_path(&p)
    }
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::ActorTicks(a) => {
            let s = a.state.load()?;
            println!("actor,handler,kind,submode,flags,cycles");
            for t in psx_retail_exec::field_units::actor_ticks(&s)? {
                println!(
                    "{:08X},{:08X},{},{},{:08X},{}",
                    t.actor, t.handler, t.kind, t.submode, t.flags, t.cycles
                );
            }
        }
        Cmd::ActorDraws(a) => {
            let s = a.state.load()?;
            println!(
                "actor,kind,flags,flags52,x,y,z,depth,draws,groups,prims,cycles,view_x,view_y,morph_vertices,colour"
            );
            for d in psx_retail_exec::field_units::actor_draws(&s)? {
                println!(
                    "{:08X},{},{:08X},{:04X},{},{},{},{},{},{},{},{},{},{},{},{:08X}",
                    d.actor,
                    d.kind,
                    d.flags,
                    d.flags52,
                    d.pos[0],
                    d.pos[1],
                    d.pos[2],
                    d.depth,
                    d.draws,
                    d.groups,
                    d.prims,
                    d.cycles,
                    d.view[0],
                    d.view[1],
                    d.morph_vertices,
                    d.colour
                );
            }
        }
        Cmd::FieldUnits(a) => {
            let s = a.state.load()?;
            println!("pass,unit,count,cycles,cycles_per_unit");
            for (p, pu) in psx_retail_exec::field_units::field_units(&s)? {
                println!("{p},total,{},{},", pu.calls, pu.total);
                for (u, (n, c)) in &pu.units {
                    let per = if *n > 0 { *c as f64 / *n as f64 } else { 0.0 };
                    println!("{p},{u},{n},{c},{per:.1}");
                }
            }
        }
        Cmd::StateInfo { state } => {
            let s = state.load()?;
            let h = |o: usize| {
                u32::from_le_bytes([
                    s.hardware[o],
                    s.hardware[o + 1],
                    s.hardware[o + 2],
                    s.hardware[o + 3],
                ])
            };
            println!(
                "pc {:08X} in_isr {} ra {:08X} sp {:08X} mode {:#x} epc {:08X} sr {:08X}",
                s.pc,
                s.in_isr,
                s.gpr[31],
                s.gpr[29],
                s.u32_at(0x8007_B83C) & 0xFFFF,
                s.cp0[14],
                s.cp0[12]
            );
            println!(
                "I_STAT {:08X} I_MASK {:08X} rcnt1 {:08X} GPUSTAT {:08X}",
                h(0x1070),
                h(0x1074),
                h(0x1110),
                h(0x1814)
            );
        }
        Cmd::BattleCost {
            state,
            frames,
            mode,
        } => battle_cost(&state.load()?, frames, &mode)?,
        Cmd::FrameCost {
            state,
            frames,
            csv,
            interrupts,
            calls,
            funcs,
            hist,
            hist_out,
            marks,
            marks_out,
            mark_reg,
            watch,
            poke,
        } => {
            let watch = parse_watch(&watch)?;
            let s = state.load()?;
            let extra: Vec<Routine> = funcs
                .iter()
                .map(|h| u32::from_str_radix(h.trim_start_matches("0x"), 16))
                .collect::<Result<Vec<u32>, _>>()?
                .into_iter()
                .filter(|va| !FIELD_ROUTINES.iter().any(|r| r.va == *va))
                .map(|va| Routine {
                    va,
                    name: "(asked)",
                })
                .collect();
            let routines: Vec<Routine> = FIELD_ROUTINES.iter().copied().chain(extra).collect();
            let hist_va = hist
                .map(|h| u32::from_str_radix(h.trim_start_matches("0x"), 16))
                .transpose()?;
            let mut routines = routines;
            if let Some(va) = hist_va
                && !routines.iter().any(|r| r.va == va)
            {
                routines.push(Routine { va, name: "(hist)" });
            }
            let mut run = FrameRunner::with_routines(&s, &routines);
            if interrupts {
                run.take_interrupts();
            }
            run.settle()?;
            for spec in &poke {
                apply_poke(&mut run.cpu.mem, spec)?;
            }
            run.cpu.hist_within = hist_va;
            run.cpu.mark_reg = mark_reg;
            for m in &marks {
                run.cpu
                    .marks
                    .insert(u32::from_str_radix(m.trim_start_matches("0x"), 16)?);
            }
            if csv {
                println!(
                    "frame,ring,step,func,name,calls,steps,ram,scratch,io,bios,cycles,hblanks,linked"
                );
            }
            for f in 0..frames {
                let fc = run.frame()?;
                if f == 0 && !run.cpu.marks.is_empty() {
                    let mut out = String::from("pc,clock,reg\n");
                    for (pc, c, r) in &run.cpu.mark_log {
                        out.push_str(&format!("{pc:08X},{c},{r:08X}\n"));
                    }
                    std::fs::write(&marks_out, out)?;
                    run.cpu.marks.clear();
                }
                if f == 0 && hist_va.is_some() {
                    let mut rows: Vec<_> = run.cpu.hist.iter().map(|(k, v)| (*k, *v)).collect();
                    rows.sort();
                    let mut out = String::from("pc,hits,accesses\n");
                    for (pc, (h, a)) in rows {
                        out.push_str(&format!("{pc:08X},{h},{a}\n"));
                    }
                    std::fs::write(&hist_out, out)?;
                    run.cpu.hist_within = None;
                }
                if !csv {
                    println!(
                        "frame {f}: {:.1} hb ({} cycles, {} instructions, VSync waits included), ring {} step {}{}",
                        fc.cycles as f64 / PCSX_CYCLES_PER_HBLANK,
                        fc.cycles,
                        fc.steps,
                        fc.ring,
                        fc.step,
                        watch_line(&mut run.cpu.mem, &watch)
                    );
                }
                for row in &fc.rows {
                    let (r, sp) = (row.routine, row.spent);
                    if sp.calls == 0 {
                        continue;
                    }
                    let a = sp.accesses;
                    if csv {
                        println!(
                            "{f},{},{},{:08X},{},{},{},{},{},{},{},{},{:.2},{}",
                            fc.ring,
                            fc.step,
                            r.va,
                            r.name,
                            sp.calls,
                            sp.steps,
                            a.ram,
                            a.scratch,
                            a.io,
                            a.bios,
                            sp.cycles_pcsx(),
                            sp.hblanks_pcsx(),
                            row.linked
                        );
                    } else {
                        println!(
                            "  {:08X} {:<20} calls {:>3} instr {:>8} ram {:>7} scratch {:>6} io {:>4} -> {:>8} cycles {:>7.1} hb, {:>5} linked",
                            r.va,
                            r.name,
                            sp.calls,
                            sp.steps,
                            a.ram,
                            a.scratch,
                            a.io,
                            sp.cycles_pcsx(),
                            sp.hblanks_pcsx(),
                            row.linked
                        );
                    }
                }
                if calls {
                    for c in &fc.calls {
                        println!(
                            "  call {:08X} {:>8} cycles {:>7.2} hb, linked {}..{}",
                            c.entry,
                            c.spent.cycles_pcsx(),
                            c.spent.hblanks_pcsx(),
                            c.mark.0,
                            c.mark.1
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn battle_cost(s: &PcsxState, frames: usize, mode: &str) -> Result<()> {
    use psx_retail_exec::battle_cost as bc;
    match mode {
        "reached" => {
            for (f, rows) in bc::reached_routines(s, frames)?.iter().enumerate() {
                println!("frame {f}");
                let mut rows = rows.clone();
                rows.sort_by_key(|r| std::cmp::Reverse(r.1.cycles_pcsx()));
                for (va, sp, linked) in rows {
                    println!(
                        "  {va:08X} calls {:>5} {:>9} cycles {:>7.1} hb linked {linked}",
                        sp.calls,
                        sp.cycles_pcsx(),
                        sp.hblanks_pcsx()
                    );
                }
            }
        }
        "series" => {
            let vas = bc::reached(s, frames)?;
            let routines: Vec<Routine> = vas.iter().map(|&va| Routine { va, name: "" }).collect();
            let mut run = FrameRunner::with_routines(s, &routines);
            run.settle()?;
            println!("frame,ring,step,children,masters,func,calls,cycles,linked");
            for f in 0..frames {
                let (c, m) = bc::pool_live(&mut run.cpu.mem);
                let fc = run.frame()?;
                for row in fc.rows.iter().filter(|r| r.spent.calls > 0) {
                    println!(
                        "{f},{},{},{c},{m},{:08X},{},{},{}",
                        fc.ring,
                        fc.step,
                        row.routine.va,
                        row.spent.calls,
                        row.spent.cycles_pcsx(),
                        row.linked
                    );
                }
            }
        }
        "groupcsv" => {
            let mut run = bc::BattleRunner::new(s, bc::CONTEXTS)?;
            println!("frame,ctx,leaf,ds,top,prims,linked,cycles");
            for f in 0..frames {
                let bf = run.frame()?;
                for g in &bf.groups {
                    let c = bf.context_of(g.at).map_or(0, |s| s.entry);
                    println!(
                        "{f},{c:08X},{:08X},{},{:02X},{},{},{}",
                        g.leaf,
                        u8::from(g.double_sided),
                        g.word >> 24,
                        g.prims,
                        g.linked,
                        g.cycles
                    );
                }
            }
        }
        "walker" => {
            let mut run = FrameRunner::with_routines(
                s,
                &[Routine {
                    va: bc::walker::ENTRY,
                    name: "walker",
                }],
            );
            run.settle()?;
            let probe = bc::WalkerProbe::arm(&mut run);
            println!("frame,ring,step,walker_cycles,sweeps,visit,cycles,advances");
            for f in 0..frames {
                let fc = run.frame()?;
                let (sweeps, visits) = probe.take();
                let w = fc.of(bc::walker::ENTRY).cycles_pcsx();
                println!("{f},{},{},{w},{sweeps},frame,0,0", fc.ring, fc.step);
                for v in visits {
                    match v {
                        bc::Visit::Step { cycles, advances } => {
                            println!(
                                "{f},{},{},{w},{sweeps},step,{cycles},{advances}",
                                fc.ring, fc.step
                            )
                        }
                        bc::Visit::Draw { cycles } => {
                            println!("{f},{},{},{w},{sweeps},draw,{cycles},0", fc.ring, fc.step)
                        }
                    }
                }
            }
        }
        "prims" => {
            let mut run = bc::BattleRunner::new(s, bc::CONTEXTS)?;
            let probe = bc::LeafProbe::arm(&mut run.run);
            println!("frame,leaf,fate,n,cycles,min,max");
            for f in 0..frames {
                run.frame()?;
                let mut agg: std::collections::BTreeMap<(u32, bc::PrimFate), (u64, u64, u64, u64)> =
                    Default::default();
                for v in probe.take() {
                    let e = agg.entry((v.leaf, v.fate)).or_insert((0, 0, u64::MAX, 0));
                    e.0 += 1;
                    e.1 += v.cycles;
                    e.2 = e.2.min(v.cycles);
                    e.3 = e.3.max(v.cycles);
                }
                for ((leaf, fate), (n, c, lo, hi)) in agg {
                    println!("{f},{leaf:08X},{fate:?},{n},{c},{lo},{hi}");
                }
            }
        }
        "parts" => {
            let mut run = bc::PartRunner::new(s)?;
            println!("frame,ring,step,sweeps,part,n,cycles,extra,children,inclusive,st7");
            for f in 0..frames {
                let fp = run.frame()?;
                let (ring, step) = (fp.ring, fp.step);
                for (p, (n, c)) in &fp.parts {
                    let name = match p {
                        bc::Part::Call(va) => format!("call {va:08X}"),
                        bc::Part::Prim(leaf, fate) => format!("prim {leaf:08X} {fate:?}"),
                        other => format!("{other:?}"),
                    };
                    let extra = match p {
                        bc::Part::ChildOverrun => fp.overrun_advances as i64,
                        // Segments of the shape-0 builds; the other shapes'
                        // builds are counted on their own row below.
                        bc::Part::Call(bc::MESH_BUILDER) => fp
                            .builds
                            .iter()
                            .filter(|b| b.0 == 0)
                            .map(|b| i64::from(b.1))
                            .sum(),
                        bc::Part::Call(va) if *va != 0x8004_8A08 => {
                            fp.linked.get(va).copied().unwrap_or(0)
                        }
                        other => fp.objects.get(other).copied().unwrap_or(0) as i64,
                    };
                    println!(
                        "{f},{ring},{step},{},{name},{n},{c},{extra},{},{},{:#04x}",
                        fp.sweeps,
                        fp.children,
                        fp.inclusive.get(p).copied().unwrap_or(0),
                        fp.state
                    );
                }
                let other = fp.builds.iter().filter(|b| b.0 != 0).count();
                if other > 0 {
                    println!(
                        "{f},{ring},{step},{},mesh builds of other shapes,{other},0,0,{},0",
                        fp.sweeps, fp.children
                    );
                }
            }
        }
        "spans" => {
            let vas = bc::reached(s, frames)?;
            let routines: Vec<Routine> = vas.iter().map(|&va| Routine { va, name: "" }).collect();
            let mut run = bc::BattleRunner::new(s, &routines)?;
            println!("frame,entry,start,end,cycles");
            for f in 0..frames {
                let bf = run.frame()?;
                for sp in &bf.spans {
                    println!(
                        "{f},{:08X},{},{},{}",
                        sp.entry,
                        sp.start,
                        sp.end,
                        sp.spent.cycles_pcsx()
                    );
                }
            }
        }
        "bodies" => {
            let mut run = bc::BattleRunner::new(s, bc::CONTEXTS)?;
            println!("frame,ring,body,cycles,prims,linked,tmd_calls,tmd_cycles");
            for f in 0..frames {
                let bf = run.frame()?;
                for (i, b) in bf.spans_of(0x8004_8A08).enumerate() {
                    let (mut p, mut l) = (0u64, 0u64);
                    for g in bf.groups.iter().filter(|g| b.holds(g.at)) {
                        p += u64::from(g.prims);
                        l += u64::from(g.linked);
                    }
                    let tmd: Vec<_> = bf
                        .spans_of(0x8002_735C)
                        .filter(|t| b.holds(t.start))
                        .collect();
                    let tc: u64 = tmd.iter().map(|t| t.spent.cycles_pcsx()).sum();
                    println!(
                        "{f},{},{i},{},{p},{l},{},{tc}",
                        bf.cost.ring,
                        b.spent.cycles_pcsx(),
                        tmd.len()
                    );
                }
            }
        }
        "builds" => {
            let mut run = bc::BattleRunner::new(s, bc::CONTEXTS)?;
            let args = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(u64, u32, u32)>::new()));
            let a = args.clone();
            run.run.cpu.probe(bc::MESH_BUILDER, move |r, m, _| {
                a.borrow_mut().push((m.clock, r.r[5], r.r[6]))
            });
            println!("frame,shape,mode,count,cycles");
            for f in 0..frames {
                let bf = run.frame()?;
                let got = std::mem::take(&mut *args.borrow_mut());
                for sp in bf.spans_of(bc::MESH_BUILDER) {
                    if let Some(&(_, mode, packed)) = got.iter().find(|g| g.0 == sp.start) {
                        println!(
                            "{f},{},{mode:#x},{},{}",
                            (mode >> 3) & 0xF,
                            packed & 0xFF,
                            sp.spent.cycles_pcsx()
                        );
                    }
                }
            }
        }
        "fates" => {
            // Each primitive's fate by the routine it was drawn under and the
            // call's colour-word top byte.
            let mut run = bc::BattleRunner::new(s, bc::CONTEXTS)?;
            let probe = bc::LeafProbe::arm(&mut run.run);
            println!("frame,ctx,call_top,leaf,linked,off_screen,culled,st7");
            for f in 0..frames {
                let st7 = {
                    let m = &mut run.run.cpu.mem;
                    let ctx = m.rd32(0x8007_BD24);
                    if ctx & 0x1FFF_FFFF < 0x20_0000 && ctx != 0 {
                        m.rd8(ctx + 7)
                    } else {
                        0
                    }
                };
                let bf = run.frame()?;
                let mut agg: std::collections::BTreeMap<(u32, u8, u32), [u64; 3]> =
                    Default::default();
                for v in probe.take() {
                    // A body draw is keyed by its order in the frame.
                    let c = match bf.context_of(v.at) {
                        Some(sp) if sp.entry == 0x8004_8A08 => {
                            0x8004_8A08
                                + bf.spans_of(0x8004_8A08)
                                    .filter(|o| o.start < sp.start)
                                    .count() as u32
                        }
                        Some(sp) => sp.entry,
                        None => 0,
                    };
                    let top = bf
                        .groups
                        .iter()
                        .find(|g| g.at <= v.at && v.at < g.at + g.cycles)
                        .map_or(0xFF, |g| g.call_top);
                    let e = agg.entry((c, top, v.leaf)).or_default();
                    e[match v.fate {
                        bc::PrimFate::Linked => 0,
                        bc::PrimFate::OffScreen => 1,
                        bc::PrimFate::Culled => 2,
                    }] += 1;
                }
                for ((c, top, leaf), e) in agg {
                    println!(
                        "{f},{c:08X},{top:02X},{leaf:08X},{},{},{},{st7}",
                        e[0], e[1], e[2]
                    );
                }
            }
        }
        "groups" => {
            let ctx: Vec<Routine> = bc::CONTEXTS.to_vec();
            let mut run = bc::BattleRunner::new(s, &ctx)?;
            for f in 0..frames {
                let bf = run.frame()?;
                println!(
                    "frame {f}: ring {} step {} groups {}",
                    bf.cost.ring,
                    bf.cost.step,
                    bf.groups.len()
                );
                let mut agg: std::collections::BTreeMap<(u32, u32, bool, u8), [u64; 4]> =
                    Default::default();
                for g in &bf.groups {
                    let c = bf.context_of(g.at).map_or(0, |s| s.entry);
                    let e = agg
                        .entry((c, g.leaf, g.double_sided, g.call_top))
                        .or_default();
                    e[0] += 1;
                    e[1] += u64::from(g.prims);
                    e[2] += u64::from(g.linked);
                    e[3] += g.cycles;
                }
                for ((c, leaf, ds, top), e) in agg {
                    println!(
                        "  ctx {c:08X} leaf {leaf:08X} ds {} top {top:02X} groups {:>3} prims {:>5} linked {:>5} cycles {:>8} ({:.1}/prim)",
                        u8::from(ds),
                        e[0],
                        e[1],
                        e[2],
                        e[3],
                        e[3] as f64 / e[1].max(1) as f64
                    );
                }
                for r in bc::CONTEXTS {
                    let sp: Vec<_> = bf.spans_of(r.va).collect();
                    if sp.is_empty() {
                        continue;
                    }
                    let cyc: u64 = sp.iter().map(|s| s.spent.cycles_pcsx()).sum();
                    let inner: u64 = bf.groups_in(r.va).map(|g| g.cycles).sum();
                    println!(
                        "  {:08X} {:<22} calls {:>4} cycles {:>8} ({:>6.1} hb) groups' {:>8}",
                        r.va,
                        r.name,
                        sp.len(),
                        cyc,
                        cyc as f64 / PCSX_CYCLES_PER_HBLANK,
                        inner
                    );
                }
            }
        }
        _ => bail!("unknown mode {mode}"),
    }
    Ok(())
}

/// One `--poke`: `ADDR=VALUE` or `*PTR+OFF=VALUE`, a halfword, hex.
fn apply_poke(mem: &mut psx_retail_exec::Memory, spec: &str) -> Result<()> {
    let hex = |t: &str| u32::from_str_radix(t.trim().trim_start_matches("0x"), 16);
    let (at, value) = spec
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("--poke {spec}: no '='"))?;
    let va = match at.strip_prefix('*') {
        Some(rest) => {
            let (ptr, off) = rest.split_once('+').unwrap_or((rest, "0"));
            let b = mem.read(hex(ptr)?, 4);
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]).wrapping_add(hex(off)?)
        }
        None => hex(at)?,
    };
    mem.wr16(va, hex(value)? as u16);
    eprintln!("poke {va:08X} = {}", value.trim());
    Ok(())
}

/// One `--watch` word: its label, address and width.
struct Watch {
    name: String,
    va: u32,
    kind: String,
}

fn parse_watch(specs: &[String]) -> Result<Vec<Watch>> {
    specs
        .iter()
        .map(|spec| {
            let (name, rest) = match spec.split_once('=') {
                Some((n, r)) => (n.to_string(), r),
                None => (spec.clone(), spec.as_str()),
            };
            let (addr, kind) = rest.split_once(':').unwrap_or((rest, "u32"));
            let va = u32::from_str_radix(addr.trim_start_matches("0x"), 16)?;
            Ok(Watch {
                name,
                va,
                kind: kind.to_string(),
            })
        })
        .collect()
}

fn watch_line(mem: &mut psx_retail_exec::Memory, watch: &[Watch]) -> String {
    let mut out = String::new();
    for w in watch {
        let b = mem.read(w.va, 4);
        let v = match w.kind.as_str() {
            "u8" => i64::from(b[0]),
            "s8" => i64::from(b[0] as i8),
            "u16" => i64::from(u16::from_le_bytes([b[0], b[1]])),
            "s16" => i64::from(i16::from_le_bytes([b[0], b[1]])),
            "s32" => i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            _ => i64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        };
        out.push_str(&format!(" {}={v:#x}", w.name));
    }
    out
}
