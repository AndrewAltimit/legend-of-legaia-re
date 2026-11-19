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
        } => {
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
                        "frame {f}: {:.1} hb ({} cycles, {} instructions, VSync waits included), ring {} step {}",
                        fc.cycles as f64 / PCSX_CYCLES_PER_HBLANK,
                        fc.cycles,
                        fc.steps,
                        fc.ring,
                        fc.step
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
