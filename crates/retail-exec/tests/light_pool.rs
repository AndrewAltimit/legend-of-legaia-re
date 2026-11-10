//! The attached light's keyframe script `FUN_801E3E00` (PROT 0897), frame by
//! frame against `attached_sprite_script_tick`: the record walk (`0x00`
//! retire, `0x01` restart, `0x02` keyframe, `0x40` header, anything else a
//! restart), the first-frame capture, the per-channel interpolation and the
//! snap at the keyframe's end, over generated scripts.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_vm::field_actor_billboard::{AttachedSprite, attached_sprite_script_tick};
use psx_retail_exec::Stop;

const SCRIPT_TICK: u32 = 0x801E_3E00;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FRAME_STEP: u32 = 0x1F80_0393;
const ACTOR: u32 = 0x8013_0000;
const SCRIPT: u32 = 0x8014_0000;

fn script(rng: &mut Rng) -> Vec<u8> {
    let mut s = Vec::new();
    for _ in 0..1 + rng.below(5) {
        match rng.below(8) {
            0 => s.extend([0x40, rng.next() as u8]),
            1 if rng.below(3) == 0 => s.push(0x30 + rng.below(0x10) as u8),
            _ => {
                s.push(2);
                let dur = [1i16, 2, 8, 30, 0, -3][rng.below(6) as usize];
                s.extend(dur.to_le_bytes());
                for _ in 0..2 {
                    s.extend((rng.next() as i16 >> rng.below(8)).to_le_bytes());
                }
                for _ in 0..6 {
                    s.push(rng.next() as u8);
                }
                let y = if rng.below(16) == 0 {
                    i16::MIN
                } else {
                    rng.next() as i16 >> rng.below(8)
                };
                s.extend(y.to_le_bytes());
            }
        }
    }
    s.push(rng.below(2) as u8);
    s
}

fn read(cpu: &mut psx_retail_exec::Cpu, s: &AttachedSprite) -> AttachedSprite {
    let h = |cpu: &mut psx_retail_exec::Cpu, o: u32| cpu.mem.rd16(ACTOR + o) as i16;
    let mut r = s.clone();
    r.offset.1 = h(cpu, 0x16);
    r.half_extent = (h(cpu, 0x3C), h(cpu, 0x3E));
    r.color_a = cpu.mem.rd32(ACTOR + 0x74);
    r.color_b = cpu.mem.rd32(ACTOR + 0x88);
    r.cursor = h(cpu, 0x9E);
    r.frame = h(cpu, 0x9C);
    r.from_extent = (h(cpu, 0x40), h(cpu, 0x58));
    r.from_y = h(cpu, 0x6A);
    r.from_a = [0x80, 0x81, 0x82].map(|o| cpu.mem.rd8(ACTOR + o));
    r.from_b = [0x83, 0x84, 0x85].map(|o| cpu.mem.rd8(ACTOR + o));
    r.torn_down = cpu.mem.rd32(ACTOR + 0x10) & 8 != 0;
    r
}

#[test]
fn the_light_pool_script_matches_retail() {
    differential!(prot(897, SCRIPT_TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let mut rng = Rng(0x4C41_4D50);
    let (mut frames, mut retired, mut traps, mut lerped, mut spins) = (0, 0, 0, 0, 0);
    cpu.step_budget = 20_000;
    'run: for run in 0..3000 {
        let bytes = script(&mut rng);
        let mut s = AttachedSprite {
            offset: (rng.next() as i16, rng.next() as i16, rng.next() as i16),
            half_extent: (rng.next() as i16 >> 4, rng.next() as i16 >> 4),
            color_a: rng.next() & 0xFF_FFFF,
            color_b: rng.next() & 0xFF_FFFF,
            abr: 1,
            script: bytes.clone(),
            cursor: 0,
            frame: 0,
            from_extent: (0, 0),
            from_y: 0,
            from_a: [0; 3],
            from_b: [0; 3],
            torn_down: false,
        };
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        cpu.mem.load(SCRIPT, &bytes);
        cpu.mem.wr32(ACTOR + 0x10, rng.next() & !8);
        cpu.mem.wr32(ACTOR + 0x94, SCRIPT);
        cpu.mem.wr16(ACTOR + 0x16, s.offset.1 as u16);
        cpu.mem.wr16(ACTOR + 0x3C, s.half_extent.0 as u16);
        cpu.mem.wr16(ACTOR + 0x3E, s.half_extent.1 as u16);
        cpu.mem.wr32(ACTOR + 0x74, s.color_a);
        cpu.mem.wr32(ACTOR + 0x88, s.color_b);
        for frame in 0..200 {
            let dt = [1u8, 1, 2, 3][rng.below(4) as usize];
            cpu.mem.wr8(FRAME_STEP, dt);
            let before = s.clone();
            let stop = cpu.call(SCRIPT_TICK, &[ACTOR]);
            if let Stop::DivideByZero { .. } = stop {
                // A non-positive duration the snap test does not catch
                // (`frame < dur` with `dur < 0` cannot hold, so only a
                // wrapped frame reaches it); retail traps.
                traps += 1;
                continue 'run;
            }
            if let Stop::StepLimit { .. } = stop {
                // A walk that never consumes a record (a restart byte, or a
                // `0x40` chain back onto one): retail spins for ever, the
                // port gives up after one pass over the script.
                spins += 1;
                continue 'run;
            }
            assert_eq!(stop, Stop::Returned, "run {run} frame {frame}");
            attached_sprite_script_tick(&mut s, dt);
            frames += 1;
            lerped += usize::from(before.frame > 0 && s.frame > before.frame);
            let retail = read(&mut cpu, &s);
            assert_eq!(
                retail, s,
                "run {run} frame {frame} dt {dt} script {bytes:02x?}\nbefore {before:?}"
            );
            if s.torn_down {
                retired += 1;
                continue 'run;
            }
        }
    }
    eprintln!(
        "{frames} frames, {lerped} interpolated, {retired} retired, {traps} traps, {spins} spins"
    );
    assert!(
        frames > 100_000 && lerped > 20_000 && retired > 500,
        "{frames} {lerped} {retired}"
    );
}

const POOL: u32 = 0x801E_3984;
const FAN: u32 = 0x801E_3658;
const RING: u32 = 0x801E_3764;
const SIDE: u32 = 0x801E_3894;
const OT_LINK: u32 = 0x8003_D2C4;
const PACKET_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8015_0000;
const RECT: u32 = 0x8013_8000;

/// One linked packet decoded back into the port's shape: `POLY_G3`
/// (the fan), `POLY_G4` (the ring) or `POLY_F4` (the fills).
fn decode(
    cpu: &mut psx_retail_exec::Cpu,
    p: u32,
) -> legaia_engine_vm::field_actor_billboard::LightPoly {
    use legaia_engine_vm::field_actor_billboard::LightPoly;
    let rgb = |cpu: &mut psx_retail_exec::Cpu, at: u32| cpu.mem.rd32(at) & 0xFF_FFFF;
    let xy = |cpu: &mut psx_retail_exec::Cpu, at: u32| {
        (cpu.mem.rd16(at) as i16, cpu.mem.rd16(at + 2) as i16)
    };
    let swap = |c: u32| (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16);
    let cmd = cpu.mem.rd8(p + 7);
    assert_eq!(cmd & 2, 2, "{cmd:#x}: semi-transparent");
    match cmd & 0xFC {
        0x30 => {
            let v: [(i16, i16); 3] = std::array::from_fn(|i| xy(cpu, p + 8 + 8 * i as u32));
            let c: [u32; 3] = std::array::from_fn(|i| swap(rgb(cpu, p + 4 + 8 * i as u32)));
            LightPoly {
                xy: [v[0], v[1], v[2], v[2]],
                rgb: [c[0], c[1], c[2], c[2]],
                verts: 3,
            }
        }
        0x38 => LightPoly {
            xy: std::array::from_fn(|i| xy(cpu, p + 8 + 8 * i as u32)),
            rgb: std::array::from_fn(|i| swap(rgb(cpu, p + 4 + 8 * i as u32))),
            verts: 4,
        },
        0x28 => {
            let c = swap(rgb(cpu, p + 4));
            LightPoly {
                xy: std::array::from_fn(|i| xy(cpu, p + 8 + 4 * i as u32)),
                rgb: [c; 4],
                verts: 4,
            }
        }
        other => panic!("unexpected command {other:#x}"),
    }
}

/// The light pool's draw `FUN_801E3984` with its three emitters - the fan
/// `FUN_801E3658`, the ring `FUN_801E3764` and the side fill `FUN_801E3894`
/// - against `light_pool_polys`: every primitive it links, in link order,
/// decoded back from the packet bytes (vertices and per-vertex colours),
/// after the draw-mode packet it links first. The emitters are also called
/// on their own, each against the matching primitive of the port's list.
#[test]
fn the_light_pool_draws_what_retail_draws() {
    differential!(
        prot(897, POOL),
        prot(897, FAN),
        prot(897, RING),
        prot(897, SIDE)
    );
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    use legaia_engine_vm::field_actor_billboard::{SpriteRect, light_pool_polys};
    let links = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    {
        let links = links.clone();
        cpu.hook(OT_LINK, move |r, _| {
            links
                .borrow_mut()
                .push((r.r[psx_retail_exec::A0], r.r[psx_retail_exec::A1]));
        });
    }
    let mut rng = Rng(0x504F_4F4C);
    let ot = 0x8016_0000;
    cpu.mem.wr32(OT_PTR, ot);
    let (mut polys, mut filled, mut alone) = (0, 0, 0);
    for run in 0..3000 {
        let rect = SpriteRect {
            centre_x: (rng.below(0x200) as i16) - 0x60,
            centre_y: (rng.below(0x180) as i16) - 0x40,
            width: [rng.below(0x200) as i16, 0, -(rng.below(0x40) as i16)][rng.below(3) as usize],
            height: [rng.below(0x200) as i16, 0, 1][rng.below(3) as usize],
        };
        let color_a = rng.next() & 0xFF_FFFF;
        let color_b = if rng.below(3) == 0 {
            0
        } else {
            rng.next() & 0xFF_FFFF
        };
        let abr = 1 + rng.below(2);
        for (i, v) in [rect.centre_x, rect.centre_y, rect.width, rect.height]
            .iter()
            .enumerate()
        {
            cpu.mem.wr16(RECT + 2 * i as u32, *v as u16);
        }
        cpu.mem.wr32(PACKET_PTR, PACKETS);
        links.borrow_mut().clear();
        assert_eq!(
            cpu.call(POOL, &[RECT, color_a, color_b, abr]),
            Stop::Returned,
            "run {run}"
        );
        let port = light_pool_polys(&rect, color_a, color_b);
        let got: Vec<(u32, u32)> = links.borrow().clone();
        assert!(got.iter().all(|&(slot, _)| slot == ot + 8), "run {run}");
        // A draw-mode reset first and the pool's blend mode last (drawn
        // first: the slot is LIFO), the primitives between.
        assert_eq!(got[0].1, PACKETS, "run {run}");
        assert_eq!(cpu.mem.rd8(PACKETS + 7), 0xE1, "run {run}: draw mode");
        let (_, last) = *got.last().unwrap();
        assert_eq!(cpu.mem.rd8(last + 7), 0xE1, "run {run}: blend mode");
        assert_eq!(
            cpu.mem.rd32(last + 4) & 0x7F,
            (abr << 5 | 0x1E) & 0x7F,
            "run {run}: abr"
        );
        let retail: Vec<_> = got[1..got.len() - 1]
            .iter()
            .map(|&(_, p)| decode(&mut cpu, p))
            .collect();
        let label = format!("run {run} {rect:?} a {color_a:#08x} b {color_b:#08x}");
        assert_eq!(retail.len(), port.len(), "{label}");
        for (i, (r, p)) in retail.iter().zip(&port).enumerate() {
            assert_eq!(r, p, "{label}: primitive {i}");
        }
        polys += port.len();
        filled += usize::from(color_b != 0);

        // Each emitter on its own: the first primitive of its kind in the
        // port's list, rebuilt from the same quadrant step. The emitters
        // read the centre and both colours the draw staged at
        // `0x801F28F0..0x801F2900`, which the call above left in place.
        let w = i32::from(rect.width);
        let h = i32::from(rect.height);
        let scale = |k: usize, e: i32| {
            ((legaia_engine_vm::field_actor_billboard::QUARTER_SINE[k] * e) as u32 >> 13) as i32
        };
        let (x0, y0, x1, y1) = (scale(0, w), scale(4, h), scale(1, w), scale(3, h));
        let base = usize::from(rect.centre_y as i32 - y0 > 0)
            + usize::from((rect.centre_y as i32 + y0) < 0xEF);
        let mut emit = |va: u32, nth: usize| {
            cpu.mem.wr32(PACKET_PTR, PACKETS + 0x1000);
            links.borrow_mut().clear();
            assert_eq!(
                cpu.call(va, &[x0 as u32, y0 as u32, x1 as u32, y1 as u32]),
                Stop::Returned
            );
            let (_, p) = links.borrow()[0];
            assert_eq!(
                decode(&mut cpu, p),
                port[base + nth],
                "{label}: {va:#x} alone"
            );
        };
        emit(FAN, 0);
        emit(RING, 4);
        if color_b != 0 {
            emit(SIDE, 8);
        }
        alone += 1;
    }
    eprintln!("{polys} primitives, {filled} with fills, {alone} lone emits");
    assert!(polys > 100_000 && filled > 1500, "{polys} {filled}");
}
