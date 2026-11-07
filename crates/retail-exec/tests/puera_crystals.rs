//! PROT 0922 (Puera) arm 19's crystal deal (`0x801F8318..0x801F85BC`) run off
//! the disc against `puera_crystal_seats`: the height deal's rejection draws,
//! then each of the 23 parts' five draws, its ring point, height and angles.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_module_camera::puera_crystal_seats;
use psx_retail_exec::{RA, RETURN_SENTINEL, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const SLICE: u32 = 0x801F_8318;
const SLICE_END: u32 = 0x801F_85BC;
const MODULE_BASE: u32 = 0x801F_69D8;
const RAND: u32 = 0x8005_6798;
const HANDLES: u32 = 0x801F_9F90;
const PARTS: u32 = 0x8016_0000;
const CTX: u32 = 0x8012_0000;
const S2: usize = 18;

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

#[test]
fn the_crystals_land_where_retail_lands_them() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(MODULE_BASE, &disc.read_entry(922).expect("PROT 0922"));
    let stream = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    {
        let (stream, draws) = (stream.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            let next = lcg(stream.get());
            stream.set(next);
            draws.set(draws.get() + 1);
            r.r[V0] = (next >> 16) & 0x7FFF;
        });
    }
    // The slice's own `jal`s have moved `ra`; its end returns to the caller.
    cpu.hook(SLICE_END, |r, _| r.r[RA] = RETURN_SENTINEL);
    cpu.mem.wr32(0x8007_BD24, CTX);
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    for i in 0..23u32 {
        cpu.mem.wr32(HANDLES + (0x16 + i) * 4, PARTS + i * 0x100);
    }
    let mut rng = Rng(0x5055_4552);
    for round in 0..300 {
        let seed = rng.next();
        stream.set(seed);
        draws.set(0);
        cpu.regs.r[S2] = 0;
        let stop = cpu.call(SLICE, &[0x8008_0000, 0]);
        assert_eq!(stop, Stop::Returned, "round {round}");

        let mut s = seed;
        let mut n = 0u32;
        let seats = puera_crystal_seats(|| {
            n += 1;
            s = lcg(s);
            (s >> 16) & 0x7FFF
        });
        assert_eq!(draws.get(), n, "draws: round {round}");
        for (i, seat) in seats.iter().enumerate() {
            let p = PARTS + i as u32 * 0x100;
            let rd = |cpu: &mut psx_retail_exec::Cpu, o: u32| cpu.mem.rd16(p + o) as i16;
            let retail = (
                [rd(&mut cpu, 0x14), rd(&mut cpu, 0x16), rd(&mut cpu, 0x18)],
                [rd(&mut cpu, 0x24), rd(&mut cpu, 0x26), rd(&mut cpu, 0x28)],
            );
            assert_eq!(retail, (seat.pos, seat.rot), "part {i}: round {round}");
        }
    }
}
