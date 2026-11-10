//! The phase-chain tick bodies (`cast_module_ticks::chain_bodies`), each run
//! off its owning image from arm 0 to its Done, pass by pass, against the
//! port's pass: `run_chain_body`, the body's camera director
//! (`capture_camera_director`) with its seat writes, the band's caster
//! pre-pass (`CAPTURE_CASTER_STAGES`), its party-row rate (`party_rate_sweep`)
//! and the fold's hit-side rate (`ChainArm::hit_rate`).
//!
//! Every call out of the image returns `0` but `rand()` (a scripted stream),
//! the bearing (`bearing_12bit_approx`, its own differential) and the damage
//! wrappers, which return a small roll and log the seat they were handed. The
//! battle frame's scalar and step are seeded wide, so a countdown gate drains
//! in a pass or two, and every seat's playing clip `+0x1D9` reads idle before
//! each pass, so a settle wait passes. A pass that holds is not compared; on
//! each pass that leaves its arm the whole context and all eight actor records
//! are, but for the bytes another owner writes ([`owner`]): the fold's HP side
//! and the reaction it stages, the presentation stores (tint, hide / show,
//! render flags), the seats' poses, the cue byte and PROT 0963's lighting.
//! What is left - the phase walk, the Done, `ctx[+0x278]` / `+0x0D`, every
//! seat's `+0x1DA` / `+0x1DC` / `+0x21D` and any other byte - must match,
//! less the gaps [`KNOWN`] names with their reason.

use crate::cast_view::{read_actor, read_ctx, write_actor, write_ctx};
use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_module_camera::{
    self as cam, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleSeat,
};
use legaia_engine_cast::cast_module_ticks::{
    self as t, CastActorState, CasterClipRule, ChainActor, ChainBody, ChainClip, ChainRestage,
};
use psx_retail_exec::{A0, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const RECORDS: u32 = 0x801C_9348;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: usize = 0x1100;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: usize = 0x400;
const NODES: u32 = 0x8014_0000;
const MONSTER_RECORD: u32 = 0x8015_0000;
const RAND: u32 = 0x8005_6798;
const BEARING: u32 = 0x8001_9B28;
const WRAPPERS: [u32; 3] = [0x801D_D0AC, 0x801D_D4B0, 0x801D_D6B4];
const SCALAR: u32 = 0x1F80_037D;
const STEP: u32 = 0x1F80_0393;
const FORMATION: u32 = 0x8007_BD0C;
const ROSTER: u32 = 0x8007_BD10;
/// The roll PROT 0964's `0xAF` body last accepted, which its strike forks on.
const ELEMENT_ROLL: u32 = 0x801C_8FE4;
/// What every damage wrapper returns: a hit that lands and kills no one.
const ROLL: u32 = 7;
const CASTER: usize = 3;
const CROSS_BEAM_PACKETS: u32 = 0x801F_726C;

/// Which owner a record byte belongs to when it is not the tick body's port.
fn owner(off: usize, struck: bool) -> Option<&'static str> {
    match off {
        // The fold lands the hit: HP, the bar delta, the combo total, the
        // status word, and on a struck seat the reaction and its restage.
        0x00..=0x03 | 0x10..=0x13 | 0x14C..=0x14D | 0x16E..=0x16F if struck => Some("fold"),
        0x1DA | 0x1DC if struck => Some("fold"),
        // Tint, blend, hide / show, the render flags.
        0x04..=0x0F | 0x21C | 0x21F | 0x225 => Some("presentation"),
        // The live and body pairs, the height and the heading.
        0x34..=0x41 | 0x46..=0x47 => Some("pose"),
        _ => None,
    }
}

/// The context bytes another owner writes.
fn ctx_owner(entry: u32, off: usize) -> Option<&'static str> {
    match (entry, off) {
        (_, 0x18) => Some("the cue byte"),
        (963, 0x27A..=0x27D | 0x286 | 0x32C) => Some("PROT 0963's lighting"),
        _ => None,
    }
}

/// The residuals the port carries on purpose, as `(entry, arm, seat, offset,
/// why)`; `seat` is `CASTER`, or `0` for any party seat.
const KNOWN: &[(u32, u8, usize, usize, &str)] = &[
    (
        964,
        7,
        7,
        0x14C,
        "the creature's HP zero as it retires (not ported)",
    ),
    (
        964,
        7,
        7,
        0x14D,
        "the creature's HP zero as it retires (not ported)",
    ),
    (964, 0x37, 7, 0x14C, "the creature's HP zero"),
    (964, 0x37, 7, 0x14D, "the creature's HP zero"),
    (964, 0x69, 7, 0x14C, "the creature's HP zero"),
    (964, 0x69, 7, 0x14D, "the creature's HP zero"),
    (
        960,
        4,
        CASTER,
        0x21D,
        "the rate rule: the caster's 0 is never restored in the body",
    ),
    (
        963,
        6,
        CASTER,
        0x21D,
        "the rate rule: arms 6..0xB alternate the caster 3 / 4",
    ),
    (963, 7, CASTER, 0x21D, "the rate rule"),
    (963, 8, CASTER, 0x21D, "the rate rule"),
    (963, 9, CASTER, 0x21D, "the rate rule"),
    (963, 0x0A, CASTER, 0x21D, "the rate rule"),
    (963, 0x0B, CASTER, 0x21D, "the rate rule"),
    (
        961,
        1,
        CASTER,
        0x21D,
        "the rate rule: arm 1's caster 3 is never restored in the body",
    ),
    (
        963,
        0x10,
        0,
        0x21D,
        "the party freeze beside the hide the port does not carry",
    ),
    (963, 0x11, 0, 0x21D, "the restore of that freeze"),
];

/// A director's seat write onto one record's bytes, as
/// `World::apply_capture_seat_writes` makes it.
fn apply_seat_write(rec: &mut [u8], w: &CaptureSeatWrite) {
    if let Some(r) = w.rate {
        rec[0x21D] = r;
    }
    if w.loop_release {
        rec[0x21B] = 0;
        rec[0x176..0x178].fill(0);
    }
    if let Some(c) = w.stage {
        rec[0x1DA] = c;
        if w.restage {
            rec[0x1DC] = rec[0x1DC].wrapping_add(1);
        }
    }
}

fn seat_of(rec: &[u8], s: usize) -> ModuleSeat {
    let o = s * STRIDE;
    let h = |k: usize| i16::from_le_bytes([rec[o + k], rec[o + k + 1]]);
    ModuleSeat {
        x: h(0x34),
        y: h(0x36),
        z: h(0x38),
        facing: h(0x46) as u16 & 0xFFF,
    }
}

/// Every `jal` target the image's words encode, outside the image.
fn jal_targets(module: &[u8]) -> Vec<u32> {
    let mut out: Vec<u32> = module
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(BASE..BASE + 0x8000).contains(&t))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The clip literals the band's pre-pass stages for this cast, if the body
/// has a row.
fn pre_pass_clips(entry: u32, action: u8, formation: u8) -> Vec<u8> {
    t::CAPTURE_CASTER_STAGES
        .iter()
        .filter(|r| {
            r.prot_entry == entry && (r.action_ids.is_empty() || r.action_ids.contains(&action))
        })
        .flat_map(|r| match r.rule {
            CasterClipRule::Fixed(c) => c.to_vec(),
            CasterClipRule::SeatMonster { id, hit, miss } => {
                vec![if formation == id { hit } else { miss }]
            }
            CasterClipRule::SeatMonsterBands(b) => b
                .iter()
                .filter(|&&(lo, hi, _)| (lo..=hi).contains(&formation))
                .map(|&(_, _, c)| c)
                .collect(),
            CasterClipRule::FirstMonster { id, hit, miss } => {
                vec![if formation == id { hit } else { miss }]
            }
            #[allow(unreachable_patterns)]
            _ => Vec::new(),
        })
        .collect()
}

struct Run {
    /// The arms the body left, in order.
    path: Vec<u8>,
    gaps: Vec<String>,
    known: usize,
    hits: usize,
}

fn run_body(body: &ChainBody, action: u8, formation: u8, selector: u8, seed: u64) -> Option<Run> {
    let disc = disc()?;
    let mut cpu = scus_cpu()?;
    cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
    let module = disc.read_entry(body.prot_entry as usize).ok()?;
    cpu.mem.load(BASE, &module);
    cpu.mem.wr32(CTX_PTR, CTX);
    let stream = Rc::new(std::cell::Cell::new(seed as u32));
    let hits = Rc::new(RefCell::new(Vec::<usize>::new()));
    for target in jal_targets(&module) {
        if target == RAND {
            let stream = stream.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
            });
        } else if target == BEARING {
            cpu.hook(BEARING, |r, _| {
                let a = |i: usize| r.r[A0 + i] as i16;
                r.r[V0] = u32::from(
                    legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                        a(0),
                        a(1),
                        a(2),
                        a(3),
                    ),
                );
            });
        } else if WRAPPERS.contains(&target) {
            let hits = hits.clone();
            cpu.hook(target, move |r, _| {
                hits.borrow_mut().push((r.r[A2] as usize).min(7));
                r.r[V0] = ROLL;
            });
        } else {
            cpu.stub(target, 0);
        }
    }
    // PROT 0948's beam packet builder, reached by a `jal` inside the image:
    // GPU packets only (`legaia_render_kernels::cast_beam`).
    if body.prot_entry == 948 {
        cpu.stub(CROSS_BEAM_PACKETS, 0);
    }
    let mut rng = Rng(seed);
    for b in 0..CTX_LEN as u32 {
        cpu.mem.wr8(CTX + b, 0);
    }
    // Three party seats, three monsters, the first of them casting.
    let victim = rng.below(3) as usize;
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 3);
    cpu.mem.wr8(CTX + 0x13, CASTER as u8);
    cpu.mem.wr16(CTX + 0x6D0, 0x900);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE as u32;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE as u32).step_by(2) {
            cpu.mem.wr16(a + o, 0);
        }
        cpu.mem.wr16(a + 0x14C, 100 + rng.below(400) as u16);
        cpu.mem.wr16(a + 0x172, 1000);
        cpu.mem.wr8(a + 0x1DA, rng.below(0x10) as u8);
        cpu.mem.wr8(a + 0x1DC, rng.below(0x10) as u8);
        for (k, o) in (0x1EFu32..=0x1F3).enumerate() {
            cpu.mem
                .wr8(a + o, 0x10 + k as u8 * 0x10 + rng.below(0x10) as u8);
        }
        cpu.mem.wr8(a + 0x21D, 8);
        cpu.mem.wr8(a + 0x1DF, action);
        cpu.mem.wr16(a + 0x46, rng.below(0x1000) as u16);
        for o in [0x34u32, 0x38, 0x3C, 0x40] {
            cpu.mem
                .wr16(a + o, (rng.below(0x1000) as u16).wrapping_sub(0x800));
        }
        let node = NODES + s * 0x100;
        cpu.mem.wr32(a + 0x22C, node);
        cpu.mem.wr16(node + 0x58, 0x80);
        cpu.mem.wr16(node + 0x72, 0x1000);
    }
    let caster_rec = ACTOR + (CASTER * STRIDE) as u32;
    cpu.mem.wr8(caster_rec + 0x1DD, victim as u8);
    cpu.mem.wr8(caster_rec + 0x1DE, 2);
    for i in 0..5 {
        cpu.mem.wr32(RECORDS + i * 4, MONSTER_RECORD);
        cpu.mem.wr8(FORMATION + i, formation);
    }
    for i in 0..3 {
        cpu.mem.wr8(ROSTER + i, 1 + i as u8);
    }
    cpu.mem.wr32(ELEMENT_ROLL, u32::from(selector));
    // One vsync a pass at the speed scalar, the clock the directors keep.
    cpu.mem.wr8(SCALAR, cam::SPEED_SCALAR as u8);
    cpu.mem.wr8(STEP, 1);

    let key = if t::capture_trampoline_for(body.prot_entry).is_some() {
        body.body
    } else {
        cam::SINGLE_BODY
    };
    let director = cam::capture_camera_director(body.prot_entry, key);
    let pre_pass = if body.caster_stages_replayed() {
        pre_pass_clips(body.prot_entry, action, formation)
    } else {
        Vec::new()
    };
    let mut st = ModuleCamState {
        lead_roster: 1,
        opens_pass: true,
        frame_step: 1,
        victim_settled: true,
        caster_in_reach: true,
        party_settled: true,
        target_code: Some(victim as u8),
        ..ModuleCamState::default()
    };
    let mut out = Run {
        path: Vec::new(),
        gaps: Vec::new(),
        known: 0,
        hits: 0,
    };
    let mut held = 0;
    for _ in 0..40_000 {
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE as u32;
            cpu.mem.wr8(a + 0x1D9, 0);
            cpu.mem.wr8(a + 0x21B, 0);
            cpu.mem.wr16(NODES + s * 0x100 + 0x68, 0x7FFF);
        }
        let phase = cpu.mem.rd8(CTX + 0x279);
        let (ctx0, rec0) = (cpu.mem.read(CTX, CTX_LEN), cpu.mem.read(ACTOR, 8 * STRIDE));
        let vi = usize::from(rec0[CASTER * STRIDE + 0x1DD].min(7));
        // The director runs on every pass, held or not, as the host's does.
        let yaw0 = st.yaw_base;
        let dir = director.map(|d| {
            st.countdown.0 = 0;
            st.victim_on_gate_clip = rec0[vi * STRIDE + 0x1D9] == rec0[vi * STRIDE + 0x1F2];
            let seats = ModuleCamSeats {
                caster: seat_of(&rec0, CASTER),
                victim: seat_of(&rec0, vi),
                action,
                depth_raw: 0x900,
                victim_radius: 0x80,
                caster_scale: 0x1000,
                caster_monster: formation,
                first_monster: formation,
                ..ModuleCamSeats::default()
            };
            d(&mut st, phase, seats)
        });
        hits.borrow_mut().clear();
        let stop = cpu.call(body.body, &[]);
        let c0 = format!("PROT {:04} {action:#04x} arm {phase:#x}", body.prot_entry);
        if stop != Stop::Returned {
            out.gaps.push(format!("{c0}: {stop:?}"));
            return Some(out);
        }
        let busy = cpu.regs.r[V0] != 0;
        let after = cpu.mem.rd8(CTX + 0x279);
        if busy && after == phase {
            held += 1;
            if held > 5000 {
                out.gaps.push(format!("{c0}: retail holds 5000 passes"));
                return Some(out);
            }
            continue;
        }
        held = 0;
        out.path.push(phase);
        let mut struck: Vec<usize> = hits.borrow().clone();
        out.hits += struck.len();
        // The fold lands on the cast's victim whichever seat a wrapper was
        // handed (PROT 0959 aims its own at seat 0), and PROT 0919's arm 7 is
        // the party row's heal.
        if !struck.is_empty() {
            struck.push(vi);
        }
        if body.prot_entry == 919 && phase == 7 {
            struck.extend(0..3);
        }

        // The port's pass: the body, then the director's seat writes.
        let Some(arm) = body.arm(phase) else {
            out.gaps.push(format!("{c0}: the port names no such arm"));
            return Some(out);
        };
        let mut v: Vec<CastActorState> = (0..8)
            .map(|s| read_actor(&mut cpu.mem, ACTOR + (s * STRIDE) as u32))
            .collect();
        for (s, a) in v.iter_mut().enumerate() {
            *a = read_actor_from(&rec0, s, *a);
        }
        let mut cview = read_ctx(&mut cpu.mem, CTX);
        cview = read_ctx_from(&ctx0, cview);
        let si = usize::from(t::SUMMON_SEAT);
        let (mut cc, mut vv, mut ss) = (v[CASTER], v[vi], v[si]);
        let tick = t::run_chain_body(body, &mut cview, &mut cc, &mut vv, &mut ss, selector);
        v[si] = ss;
        v[vi] = vv;
        v[CASTER] = cc;
        let mut ctx = ctx0.clone();
        write_ctx(&mut ctx, &cview);
        let mut want = rec0.clone();
        for (s, a) in v.iter().enumerate() {
            write_actor(&mut want[s * STRIDE..(s + 1) * STRIDE], a);
        }
        let rec = |s: usize| s * STRIDE..(s + 1) * STRIDE;
        // The band's pre-pass stages the caster's literals.
        for s in arm.stages.iter().filter(|s| s.actor == ChainActor::Caster) {
            if let ChainClip::Literal(k) = s.clip
                && pre_pass.contains(&k)
            {
                let r = &mut want[rec(CASTER)];
                r[0x1DA] = k;
                match s.restage {
                    ChainRestage::Bump => r[0x1DC] = r[0x1DC].wrapping_add(1),
                    ChainRestage::One => r[0x1DC] = 1,
                    ChainRestage::Untouched => {}
                }
            }
        }
        if let Some(rate) = arm.hit_rate {
            for &s in &struck {
                want[s * STRIDE + 0x21D] = rate;
            }
        }
        if let Some(rate) = t::party_rate_sweep(body.prot_entry, Some(body.body), phase) {
            for s in 0..3 {
                want[s * STRIDE + 0x21D] = rate;
            }
        }
        if let Some(a) = dir {
            if a.hold {
                out.gaps
                    .push(format!("{c0}: the director holds where retail passed"));
            }
            if st.yaw_base != yaw0 {
                ctx[0x6DA..0x6DC].copy_from_slice(&(st.yaw_base as u16).to_le_bytes());
            }
            if let Some(b) = a.ctx_278 {
                ctx[0x278] = b;
            }
            apply_seat_write(&mut want[rec(CASTER)], &a.caster_write);
            apply_seat_write(&mut want[rec(vi)], &a.victim_write);
            let hp = |w: &[u8], s: usize| {
                u16::from_le_bytes([w[s * STRIDE + 0x14C], w[s * STRIDE + 0x14D]])
            };
            for s in 0..3 {
                if !a.party_write.is_empty()
                    && (!a.party_live_only || hp(&want, s) > 0)
                    && !(a.party_skip_victim && s == vi)
                {
                    apply_seat_write(&mut want[rec(s)], &a.party_write);
                }
            }
            for s in 0..8 {
                let live = hp(&want, s) > 0;
                if !a.others_write.is_empty()
                    && ((s < 3 && (!a.others_live_only || live)) || (s >= 3 && live))
                {
                    apply_seat_write(&mut want[rec(s)], &a.others_write);
                }
            }
        }
        // The finishing arm's store into the yaw counter (`capture_exit_yaw_base`).
        if !busy && let Some(y) = cam::capture_exit_yaw_base(body.prot_entry, key) {
            ctx[0x6DA..0x6DC].copy_from_slice(&(y as u16).to_le_bytes());
        }
        if (tick.step == t::CastTickStep::Done) == busy {
            out.gaps
                .push(format!("{c0}: port {:?}, retail busy {busy}", tick.step));
        }
        if busy && cview.phase != after {
            out.gaps.push(format!(
                "{c0}: port leaves to {:#x}, retail {after:#x}",
                cview.phase
            ));
        }
        let got = cpu.mem.read(ACTOR, 8 * STRIDE);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            let (s, off) = (i / STRIDE, i % STRIDE);
            if g == w || owner(off, struck.contains(&s)).is_some() {
                continue;
            }
            let seat_kind = if s < 3 { 0 } else { s };
            if KNOWN
                .iter()
                .any(|k| (k.0, k.1, k.2, k.3) == (body.prot_entry, phase, seat_kind, off))
            {
                out.known += 1;
                continue;
            }
            out.gaps.push(format!(
                "{c0}: seat {s} +{off:#05x} retail {g:#04x} port {w:#04x} (was {:#04x})",
                rec0[i]
            ));
        }
        let got = cpu.mem.read(CTX, CTX_LEN);
        for (i, (g, w)) in got.iter().zip(&ctx).enumerate() {
            if g != w && i != 0x279 && ctx_owner(body.prot_entry, i).is_none() {
                out.gaps.push(format!(
                    "{c0}: ctx +{i:#05x} retail {g:#04x} port {w:#04x} (was {:#04x})",
                    ctx0[i]
                ));
            }
        }
        if !busy {
            return Some(out);
        }
    }
    out.gaps.push("no Done in 40000 passes".into());
    Some(out)
}

/// The view of seat `s` as the pass opened.
fn read_actor_from(rec: &[u8], s: usize, _now: CastActorState) -> CastActorState {
    let mut m = psx_retail_exec::Memory::default();
    m.load(ACTOR, &rec[s * STRIDE..(s + 1) * STRIDE]);
    read_actor(&mut m, ACTOR)
}

/// The context view as the pass opened.
fn read_ctx_from(ctx: &[u8], _now: t::CastModuleCtx) -> t::CastModuleCtx {
    let mut m = psx_retail_exec::Memory::default();
    m.load(CTX, ctx);
    read_ctx(&mut m, CTX)
}

#[test]
fn the_chain_bodies_walk_as_retail_does() {
    differential!(
        prot(919, SPOON),
        prot(935, EARTHQUAKE),
        prot(936, HYPER_CRUSH),
        prot(937, HYPER_LIGHTNING),
        prot(939, SPORE_GAS),
        prot(942, POWER_UP_AA),
        prot(947, V_WINDHASH),
        prot(948, CROSS_BEAM),
        prot(956, PARALYZING_WAVE),
        prot(959, MEGATON_PRESS),
        prot(960, NEO_STAR_SLASH),
        prot(961, DEAD_END_CRISIS),
        prot(963, GENOCIDAL_CANNON),
        prot(964, ELEMENT_STRIKE)
    );
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut failures = Vec::new();
    let (mut casts, mut arms, mut hits, mut known) = (0, 0, 0, 0);
    for body in t::chain_bodies() {
        // PROT 0961 rolls its sweep in the evolved Cort's formation only; PROT
        // 0964 forks on the last Element Change roll.
        let formation = if body.prot_entry == 961 { 0xB5 } else { 0x2A };
        let selectors: &[u8] = if body.prot_entry == 964 {
            &[0, 1, 2]
        } else {
            &[0]
        };
        for &action in body.action_ids {
            for &sel in selectors {
                for seed in [0x4348_4149u64, 0x5449_434B] {
                    let run = run_body(body, action, formation, sel, seed).expect("disc");
                    casts += 1;
                    arms += run.path.len();
                    hits += run.hits;
                    known += run.known;
                    let terminal = run.path.last().and_then(|&p| body.arm(p));
                    if !terminal.is_some_and(|a| a.exit == t::ChainExit::Finish) {
                        failures.push(format!(
                            "PROT {:04} {action:#04x}: path {:x?} ends off the terminal arm",
                            body.prot_entry, run.path
                        ));
                    }
                    failures.extend(run.gaps);
                }
            }
        }
    }
    eprintln!("{casts} casts, {arms} arms left, {hits} wrapper hits, {known} known gaps hit");
    assert!(hits > 20, "{hits} hits");
    failures.dedup();
    let n = failures.len();
    failures.truncate(200);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}

const SPOON: u32 = 0x801F_69D8;
const EARTHQUAKE: u32 = 0x801F_69D8;
const HYPER_CRUSH: u32 = 0x801F_69D8;
const HYPER_LIGHTNING: u32 = 0x801F_69D8;
const SPORE_GAS: u32 = 0x801F_69D8;
const POWER_UP_AA: u32 = 0x801F_69F4;
const V_WINDHASH: u32 = 0x801F_69F0;
const CROSS_BEAM: u32 = 0x801F_69F0;
const PARALYZING_WAVE: u32 = 0x801F_69D8;
const MEGATON_PRESS: u32 = 0x801F_69F0;
const NEO_STAR_SLASH: u32 = 0x801F_69D8;
const DEAD_END_CRISIS: u32 = 0x801F_69D8;
const GENOCIDAL_CANNON: u32 = 0x801F_6A20;
const ELEMENT_STRIKE: u32 = 0x801F_69D8;

/// A summon creature's tick body, by its port.
type SummonTick =
    fn(&mut t::CastModuleCtx, &mut CastActorState, &mut CastActorState) -> t::CastTickStep;

/// Walk one summon tick body off its image: the arms it left, each as
/// `(phase, phase after, busy)`, and whether it reached its Done.
/// Each arm a walk left: `(phase, phase after, busy)`.
type Walk = Vec<(u8, u8, bool)>;

fn summon_walk(entry: u32, va: u32, action: u8) -> Option<(Walk, bool)> {
    let disc = disc()?;
    let mut cpu = scus_cpu()?;
    cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
    let module = disc.read_entry(entry as usize).ok()?;
    cpu.mem.load(BASE, &module);
    cpu.mem.wr32(CTX_PTR, CTX);
    let stream = Rc::new(std::cell::Cell::new(0x1234_5679u32));
    for target in jal_targets(&module) {
        if target == RAND {
            let stream = stream.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
            });
        } else if WRAPPERS.contains(&target) {
            cpu.stub(target, ROLL);
        } else {
            cpu.stub(target, 0);
        }
    }
    for b in 0..CTX_LEN as u32 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 3);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE as u32;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE as u32).step_by(2) {
            cpu.mem.wr16(a + o, 0);
        }
        cpu.mem.wr16(a + 0x14C, 300);
        cpu.mem.wr8(a + 0x21D, 8);
        cpu.mem.wr8(a + 0x1DF, action);
        cpu.mem.wr8(a + 0x1DD, 4);
        let node = NODES + s * 0x100;
        cpu.mem.wr32(a + 0x22C, node);
        cpu.mem.wr16(node + 0x72, 0x1000);
    }
    for i in 0..5 {
        cpu.mem.wr32(RECORDS + i * 4, MONSTER_RECORD);
    }
    cpu.mem.wr8(SCALAR, cam::SPEED_SCALAR as u8);
    cpu.mem.wr8(STEP, 1);
    let mut path = Vec::new();
    for _ in 0..40_000 {
        for s in 0..8u32 {
            cpu.mem.wr8(ACTOR + s * STRIDE as u32 + 0x1D9, 0);
            cpu.mem.wr16(NODES + s * 0x100 + 0x68, 0x7FFF);
        }
        let phase = cpu.mem.rd8(CTX + 0x279);
        if cpu.call(va, &[]) != Stop::Returned {
            return Some((path, false));
        }
        let busy = cpu.regs.r[V0] != 0;
        let after = cpu.mem.rd8(CTX + 0x279);
        if busy && after == phase {
            continue;
        }
        path.push((phase, after, busy));
        if !busy {
            return Some((path, true));
        }
    }
    Some((path, false))
}

/// The summon tick bodies the walk holds to their ports, as `(entry, body,
/// action, the port, the fewest arms the walk must leave)`. PROT 0922's and
/// 0958's walks stop where an arm reaches a part handle the stubbed seat call
/// never made; the arms before it are compared.
const SUMMON_TICKS: [(u32, u32, u8, SummonTick, usize); 7] = [
    (
        949,
        WATER_CRYSTALS,
        0x9E,
        |c, _, v| t::water_crystals_tick(c, v, None),
        6,
    ),
    (925, SPIKEFISH, 0x97, |c, k, _| t::spikefish_tick(c, k), 10),
    (
        924,
        ULTIMATE_RAVE,
        0x96,
        |c, k, v| t::ultimate_rave_tick(c, k, v, false),
        12,
    ),
    (922, PUERA, 0x94, |c, _, v| t::puera_tick(c, v, None), 8),
    (
        927,
        JUGGERNAUT,
        0x99,
        |c, _, v| t::juggernaut_tick(c, v, None),
        20,
    ),
    (
        958,
        BLAZING_SLASH,
        0x79,
        |c, _, v| t::blazing_slash_tick(c, v, None),
        24,
    ),
    (
        918,
        KEMARO,
        0x95,
        |c, _, v| t::kemaro_tick(c, v, None).unwrap_or(t::CastTickStep::Done),
        21,
    ),
];

#[test]
fn the_summon_tick_bodies_walk_their_phases_as_retail_does() {
    differential!(
        prot(949, WATER_CRYSTALS),
        prot(925, SPIKEFISH),
        prot(924, ULTIMATE_RAVE),
        prot(922, PUERA),
        prot(927, JUGGERNAUT),
        prot(958, BLAZING_SLASH),
        prot(918, KEMARO)
    );
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut failures = Vec::new();
    for (entry, va, action, port, fewest) in SUMMON_TICKS {
        let (path, done) = summon_walk(entry, va, action).expect("disc");
        eprintln!(
            "PROT {entry:04}: {:x?}{}",
            path.iter().map(|p| p.0).collect::<Vec<_>>(),
            if done { " Done" } else { "" }
        );
        if path.len() < fewest {
            failures.push(format!(
                "PROT {entry:04}: the walk left {} arms",
                path.len()
            ));
        }
        for &(p, a, busy) in &path {
            let mut ctx = t::CastModuleCtx {
                phase: p,
                ..Default::default()
            };
            let (mut k, mut v) = (CastActorState::default(), CastActorState::default());
            let step = port(&mut ctx, &mut k, &mut v);
            if (step == t::CastTickStep::Done) == busy || (busy && ctx.phase != a) {
                failures.push(format!(
                    "PROT {entry:04} arm {p:#x}: retail -> {a:#x} busy {busy}, port -> {:#x} {step:?}",
                    ctx.phase
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const WATER_CRYSTALS: u32 = 0x801F_6A10;
const SPIKEFISH: u32 = 0x801F_6A00;
const ULTIMATE_RAVE: u32 = 0x801F_6A18;
const PUERA: u32 = 0x801F_6A3C;
const JUGGERNAUT: u32 = 0x801F_6A84;
const BLAZING_SLASH: u32 = 0x801F_6DD8;
const KEMARO: u32 = 0x801F_6C70;
