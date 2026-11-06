//! What every differential shares: the disc, and a CPU with the executable in.

use legaia_retail_exec::Cpu;
use std::sync::OnceLock;

/// The retail executable and PROT reader, once per test binary. `None` skips.
pub fn disc() -> Option<&'static legaia_disc_patch::disc::DiscPatcher> {
    static DISC: OnceLock<Option<legaia_disc_patch::disc::DiscPatcher>> = OnceLock::new();
    DISC.get_or_init(|| {
        let path = std::env::var_os("LEGAIA_DISC_BIN")?;
        let image = std::fs::read(path).ok()?;
        legaia_disc_patch::disc::DiscPatcher::open(image).ok()
    })
    .as_ref()
}

/// The `SCUS_942.54` file off the disc.
pub fn scus() -> Option<Vec<u8>> {
    disc()?.read_named_file("SCUS_942.54")
}

/// A CPU with the executable loaded, or `None` without a disc.
pub fn scus_cpu() -> Option<Cpu> {
    let mut cpu = Cpu::new();
    cpu.load_psx_exe(&scus()?);
    // The header's `gp` is zero; the boot code loads the real one (`0x80026CA8`).
    cpu.regs.r[legaia_retail_exec::GP] = 0x8007_B318;
    Some(cpu)
}

/// A small deterministic generator for test inputs.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u32 {
        // SplitMix64.
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// First byte offsets where two buffers differ, for a failure message.
pub fn diff_offsets(a: &[u8], b: &[u8]) -> Vec<String> {
    a.iter()
        .zip(b)
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .take(16)
        .map(|(i, (x, y))| format!("+{i:#x}: retail {x:#04x} port {y:#04x}"))
        .collect()
}

/// Tracked kinds and the `+0x16E` bit each packs to.
pub const TRACKED_STATUS: [(legaia_engine_battle_vm::status_effects::StatusKind, u16); 7] = {
    use legaia_engine_battle_vm::status_effects::StatusKind as K;
    [
        (K::Venom, 0x0001),
        (K::Toxic, 0x0002),
        (K::Stone, 0x0004),
        (K::Numb, 0x0400),
        (K::Sleep, 0x0800),
        (K::Curse, 0x1000),
        // The tracker holds the delegation group whole.
        (K::Confuse, 0x0380),
    ]
};

/// Seat a retail `+0x16E` word on the port's actor `slot`, split the way the
/// port can hold one: with `split`, each tracked kind's bit goes into the status
/// tracker, as a hit's status arm leaves it, and the rest stays in the raw
/// half a cast writes. Every reader of the word has to see both halves.
pub fn seat_status(w: &mut legaia_engine_core::world::World, slot: u8, word: u16, split: bool) {
    let mut raw = word;
    if split {
        for (kind, bit) in TRACKED_STATUS {
            if word & bit == bit {
                w.battle.status_effects.apply(slot, kind);
                raw &= !bit;
            }
        }
    }
    w.actors[usize::from(slot)].battle.field_flags = raw;
}
