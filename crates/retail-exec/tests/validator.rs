//! `FUN_8003FB10`, the gate the menus and the battle UI ask before offering an
//! action, against `validate_action`, both the return value and the validity
//! byte the greying reads (`gp + 0x9A8`), in and out of battle.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_action::{
    ActionValidatorHost, RecordStat, SlotResources, validate_action,
};
use psx_retail_exec::{GP, Stop, V0};

const VALIDATOR: u32 = 0x8003_FB10;
const MODE: u32 = 0x8007_B83C;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const PARTY_COUNT: u32 = 0x8008_4594;
const PARTY_SLOTS: u32 = 0x8008_4598;
const ENGINE_FLAGS: u32 = 0x1F80_0394;
const SYSTEM_FLAGS: u32 = 0x8008_5758;
const GATE_WORD: u32 = 0x8007_B600;

#[derive(Debug)]
struct Host {
    in_battle: bool,
    quad: [SlotResources; 7],
    status: [u16; 7],
    stats: [[u16; 8]; 7],
    party: Vec<u8>,
    engine: u32,
    system: [bool; 2],
    gate: i32,
}

impl ActionValidatorHost for Host {
    fn in_battle(&self) -> bool {
        self.in_battle
    }
    fn slot_resources(&self, slot: u8) -> Option<SlotResources> {
        self.quad.get(usize::from(slot)).copied()
    }
    fn status_word(&self, slot: u8) -> u16 {
        self.status[usize::from(slot)]
    }
    fn record_stat(&self, slot: u8, stat: RecordStat) -> u16 {
        self.stats[usize::from(slot)][stat as usize]
    }
    fn party_count(&self) -> u8 {
        self.party.len() as u8
    }
    fn party_member_slot(&self, index: u8) -> u8 {
        self.party[usize::from(index)]
    }
    fn engine_flag_word(&self) -> u32 {
        self.engine
    }
    fn system_flag(&self, idx: u16) -> bool {
        self.system[usize::from(idx) - 5]
    }
    fn inventory_count(&self) -> i32 {
        self.gate
    }
}

/// `RecordStat`'s fields in declaration order, as record offsets and caps.
const STAT_OFFSETS: [(u32, u32); 8] = [
    (0x104, 9999),
    (0x108, 999),
    (0x110, 280),
    (0x112, 999),
    (0x114, 999),
    (0x116, 999),
    (0x118, 999),
    (0x11A, 999),
];

fn stat(rng: &mut Rng, cap: u32) -> u16 {
    match rng.below(4) {
        0 => cap as u16,
        1 => cap as u16 - 1,
        2 => (cap + 1) as u16,
        _ => rng.below(cap + 2) as u16,
    }
}

#[test]
fn the_validator_answers_what_retail_answers() {
    differential!(scus(VALIDATOR));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let validity = cpu.regs.r[GP] + 0x9A8;
    let mut rng = Rng(0x5641_4C49);
    let mut failures = Vec::new();
    let mut valid = 0;
    for round in 0..60000u32 {
        let in_battle = rng.below(2) == 0;
        let mut h = Host {
            in_battle,
            quad: [SlotResources::default(); 7],
            status: [0; 7],
            stats: [[0; 8]; 7],
            party: (0..rng.below(4) as u8).collect(),
            engine: [0, 0x10_0000, 0x20_0000][rng.below(3) as usize],
            system: [rng.below(3) == 0, rng.below(3) == 0],
            gate: [0, 0xDF, 0xE0, 0xE1, -1][rng.below(5) as usize],
        };
        cpu.mem
            .wr32(MODE, if in_battle { 0x15 } else { rng.below(0x15) });
        cpu.mem.wr8(PARTY_COUNT, h.party.len() as u8);
        for (i, s) in h.party.iter().enumerate() {
            cpu.mem.wr8(PARTY_SLOTS + i as u32, *s);
        }
        cpu.mem.wr32(ENGINE_FLAGS, h.engine);
        cpu.mem.wr32(GATE_WORD, h.gate as u32);
        // Flags 5 and 6 of the system bank, bit 7 down from the first byte.
        let flags = (u8::from(h.system[0]) << 2) | (u8::from(h.system[1]) << 1);
        cpu.mem.wr8(SYSTEM_FLAGS, flags);
        let slots = if in_battle { 7 } else { 3 };
        for s in 0..slots {
            let hp_max = 1 + rng.below(9999) as u16;
            let mp_max = rng.below(999) as u16;
            let q = SlotResources {
                hp: [0, hp_max, hp_max - 1, rng.below(u32::from(hp_max)) as u16]
                    [rng.below(4) as usize],
                hp_max,
                mp: [0, mp_max, mp_max.saturating_sub(1)][rng.below(3) as usize],
                mp_max,
            };
            h.quad[s] = q;
            let status = [0u16, 1, 2, 3, 4, 8, 0x400, 0x8000, 0xFFFF][rng.below(9) as usize];
            h.status[s] = status;
            if in_battle {
                let a = ACTOR + s as u32 * 0x400;
                cpu.mem.wr32(ACTORS + s as u32 * 4, a);
                cpu.mem.wr16(a + 0x14C, q.hp);
                cpu.mem.wr16(a + 0x14E, q.hp_max);
                cpu.mem.wr16(a + 0x150, q.mp);
                cpu.mem.wr16(a + 0x152, q.mp_max);
                cpu.mem.wr16(a + 0x16E, status);
            } else {
                let r = RECORDS + s as u32 * STRIDE;
                cpu.mem.wr16(r + 0x106, q.hp);
                cpu.mem.wr16(r + 0x104, q.hp_max);
                cpu.mem.wr16(r + 0x10A, q.mp);
                cpu.mem.wr16(r + 0x108, q.mp_max);
                cpu.mem.wr16(r + 0x12E, status);
            }
        }
        for s in 0..7 {
            let r = RECORDS + s as u32 * STRIDE;
            for (k, (off, cap)) in STAT_OFFSETS.iter().enumerate() {
                // In the field the quad's own maxima are these words.
                let v = if !in_battle && s < 3 && *off == 0x104 {
                    h.quad[s].hp_max
                } else if !in_battle && s < 3 && *off == 0x108 {
                    h.quad[s].mp_max
                } else {
                    stat(&mut rng, *cap)
                };
                h.stats[s][k] = v;
                cpu.mem.wr16(r + off, v);
            }
        }
        let arm = match rng.below(4) {
            0 => rng.below(0x100) as u8,
            1 => 0x80 + rng.below(4) as u8,
            _ => rng.below(0x0E) as u8,
        };
        let sub = rng.below(9) as u8;
        let slot = rng.below(slots as u32) as u8;
        let before = rng.below(256) as u8;
        cpu.mem.wr8(validity, before);
        cpu.regs.r[V0] = 0;
        let stop = cpu.call(
            VALIDATOR,
            &[u32::from(arm), u32::from(sub), u32::from(slot)],
        );
        assert_eq!(stop, Stop::Returned, "arm {arm:#x}");
        let retail = (cpu.regs.r[V0] != 0, cpu.mem.rd8(validity));
        let mut bits = before;
        let port = (validate_action(&mut h, arm, sub, slot, &mut bits), bits);
        valid += u32::from(retail.0);
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} arm {arm:#x} sub {sub} slot {slot} battle {in_battle}: retail {retail:?} port {port:?}\n {h:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(valid > 5000, "{valid} valid");
}
