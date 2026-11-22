//! PROT 0918 (Kemaro)'s arm 17 against `kemaro_spear_height`.
//!
//! The arm frames the victim, stores `5 h / 4` of the victim's display height
//! `+0x3E` on the three spear parts (`0x801F8408..0x801F8438`), then switches
//! on the victim's monster id through the jump table at the image head
//! (`addiu -1`, `sltiu 0xA6`, `jr` at `0x801F8474`) to restore a species'
//! own fraction. The run takes every id the formation byte can hold and
//! heights of both signs, and compares each part's `+0x16`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use psx_retail_exec::Stop;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const PARTS: u32 = 0x8016_0000;
const FORMATION: u32 = 0x8007_BD0C;
const KEMARO: u32 = 0x801F_6C70;
/// The module's part handles; arm 17 stores through `+0x8`, `+0xC`, `+0x10`.
const HANDLES: u32 = 0x801F_92C4;

#[test]
fn kemaros_spear_stands_where_retail_stands_it_for_every_monster() {
    differential!(prot(918, KEMARO));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let module = disc.read_entry(918).expect("PROT 0918");
    let mut targets: Vec<u32> = module
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| 0x8000_0000 | ((w & 0x03FF_FFFF) << 2))
        .collect();
    targets.sort_unstable();
    targets.dedup();
    let mut rng = Rng(0x4B45_4D41);
    let mut failures = Vec::new();
    let mut runs = 0usize;
    for id in 0..=0xFFu16 {
        let h =
            [0x40i16, -0x123, 0x3FF, -7][usize::from(id) % 4].wrapping_add(rng.below(0x40) as i16);
        let mut cpu = scus_cpu().expect("SCUS");
        cpu.mem.load(OVERLAY_BASE, &overlay);
        cpu.mem.load(BASE, &module);
        for &t in &targets {
            cpu.stub(t, 0);
        }
        cpu.mem.wr32(CTX_PTR, CTX);
        for b in 0..0x1100 {
            cpu.mem.wr8(CTX + b, 0);
        }
        let (caster, victim) = (0u32, 3u32);
        cpu.mem.wr8(CTX, 3);
        cpu.mem.wr8(CTX + 1, 1);
        cpu.mem.wr8(CTX + 0x13, caster as u8);
        cpu.mem.wr8(CTX + 0x279, 17);
        cpu.mem.wr16(CTX + 0x6D0, 0x1000);
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            for o in (0..STRIDE).step_by(4) {
                cpu.mem.wr32(a + o, 0);
            }
        }
        cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, victim as u8);
        cpu.mem.wr16(ACTOR + victim * STRIDE + 0x3E, h as u16);
        cpu.mem.wr8(FORMATION, id as u8);
        for k in 0..5u32 {
            let p = PARTS + k * 0x80;
            for o in (0..0x80).step_by(4) {
                cpu.mem.wr32(p + o, 0);
            }
            cpu.mem.wr32(HANDLES + 4 * k, p);
        }
        let stop = cpu.call(KEMARO, &[]);
        assert_eq!(stop, Stop::Returned, "id {id:#x}");
        let want = World::drive_kemaro_spear_height(i32::from(h), Some(id));
        for k in 2..5u32 {
            let got = cpu.mem.rd16(PARTS + k * 0x80 + 0x16) as i16;
            if got != want {
                failures.push(format!(
                    "id {id:#04x} h {h}: part {k} retail {got} port {want}"
                ));
            }
        }
        runs += 1;
    }
    assert_eq!(runs, 256);
    let n = failures.len();
    failures.truncate(40);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}
