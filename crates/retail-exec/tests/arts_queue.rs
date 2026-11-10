//! `FUN_801EED1C`, the queue builder, with `FUN_801EFBFC` and `FUN_801EF9E4`,
//! against `World::build_arts_action_queue` and `charge_art_spirit`: what an
//! entered arrow string becomes, over the real art banks, with the Spirit gauge
//! at the boundaries of each art's price and the learned lists at both extremes.

use crate::common::{Rng, disc, scus_cpu};
use legaia_actor_formats::battle_char_assembly::{art_animation_bank, decode_record0};
use legaia_art::Character;
use legaia_art::queue::{ActionConstant, Command};
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const BUILDER: u32 = 0x801E_ED1C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const RECORD0_TABLE: u32 = 0x801C_9360;
const SEATS: u32 = 0x8007_BD10;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const RECORD0_BASE: u32 = 0x8016_0000;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;

#[test]
fn an_arrow_string_builds_the_queue_retail_builds() {
    differential!(prot(898, BUILDER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let mut w = World::new();
    let mut combos: [Vec<Vec<Command>>; 3] = Default::default();
    let mut banks = Vec::new();
    for (ch, combo_list) in combos.iter_mut().enumerate() {
        let file = disc.read_entry(863 + ch).expect("player file");
        let rec0 = decode_record0(&file).expect("record0");
        let bank = art_animation_bank(&rec0).expect("art bank");
        let character = Character::all()[ch];
        for r in &bank {
            let Some(action) = ActionConstant::from_byte(0x10 + r.index as u8) else {
                continue;
            };
            let commands: Vec<Command> = r
                .combo
                .iter()
                .filter_map(|&b| Command::from_byte(b))
                .collect();
            if !commands.is_empty() {
                combo_list.push(commands.clone());
            }
            let mut extra = vec![r.stream_source];
            extra.extend_from_slice(&r.chain_sources);
            w.set_art_record(
                character,
                action,
                legaia_art::ArtRecord {
                    action,
                    commands,
                    anim_index: 0,
                    anim_extra: extra,
                    name: None,
                    power: vec![],
                    dmg_timing: vec![],
                    effect_cues: Default::default(),
                    hit_cues: vec![],
                    identifier: 0,
                    anim_speed: 0,
                    enemy_effect: legaia_art::record::EnemyEffect::Toxic,
                    repeat_frames: Default::default(),
                    background: 0,
                    runtime_address: None,
                },
            );
        }
        // Retail's image: the same bytes, the `+0x58` word rebased.
        let base = RECORD0_BASE + ch as u32 * 0x2_0000;
        cpu.mem.load(base, &rec0);
        let off = u32::from_le_bytes(rec0[0x58..0x5C].try_into().unwrap());
        cpu.mem.wr32(base + 0x58, base + off);
        cpu.mem.wr32(RECORD0_TABLE + ch as u32 * 4, base);
        banks.push(rec0);
    }
    let mut rng = Rng(0x5155_4555);
    let (mut arts, mut plain, mut refused) = (0, 0, 0);
    let mut failures: Vec<String> = Vec::new();
    for round in 0..20000 {
        let ch = rng.below(3) as usize;
        // Arrows that hit the bank: one or two of its combos, or noise.
        let mut arrows: Vec<Command> = Vec::new();
        for _ in 0..1 + rng.below(3) {
            match rng.below(4) {
                0 => arrows.push(Command::from_byte(1 + rng.below(4) as u8).unwrap()),
                _ => {
                    let list = &combos[ch];
                    arrows.extend_from_slice(&list[rng.below(list.len() as u32) as usize]);
                }
            }
        }
        arrows.truncate(16);
        let spirit: u16 = [
            0,
            1,
            5,
            6,
            11,
            18,
            20,
            30,
            33,
            44,
            55,
            100,
            rng.below(101) as u16,
        ][rng.below(13) as usize];
        let halved = rng.below(4) == 0;
        let armed = rng.below(2) == 0;
        let know_all = rng.below(2) == 0;
        let known: Vec<u8> = if know_all {
            (0..15).collect()
        } else {
            (0..15u8).filter(|_| rng.below(3) == 0).collect()
        };
        let status: u16 = [0, 0, 0, 0, 8, 0x10, 0x20][rng.below(7) as usize];

        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        for b in 0..0x900 {
            cpu.mem.wr8(CTX + b, 0);
        }
        cpu.mem.wr8(CTX, 3);
        cpu.mem.wr8(CTX + 1, 2);
        cpu.mem.wr8(CTX + 0x25F + ch as u32, u8::from(armed));
        let seat = ch as u32;
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            for b in 0..STRIDE {
                cpu.mem.wr8(a + b, 0);
            }
            cpu.mem.wr8(SEATS + s, if s < 3 { s as u8 + 1 } else { 0 });
            cpu.mem.wr16(a + 0x14C, 100);
            cpu.mem.wr16(a + 0x14E, 100);
        }
        let a = ACTOR + seat * STRIDE;
        cpu.mem.wr8(a + 0x1DE, 3);
        cpu.mem.wr16(a + 0x170, spirit);
        cpu.mem.wr16(a + 0x16E, status);
        for (i, c) in arrows.iter().enumerate() {
            cpu.mem.wr8(a + 0x1DF + i as u32, c.as_action().as_byte());
        }
        let rec = CHAR_RECORDS + seat * CHAR_STRIDE;
        for b in 0..CHAR_STRIDE {
            cpu.mem.wr8(rec + b, 0);
        }
        cpu.mem.wr32(rec + 0xF8, if halved { 0x800 } else { 0 });
        cpu.mem.wr8(rec + 0x185, known.len() as u8);
        cpu.mem.load(rec + 0x186, &known);
        // No innate cap on any character.
        for c in 0..4 {
            cpu.mem.wr8(0x801F_686C + c, 0);
        }
        let seed = rng.next();
        state.set(seed);
        assert_eq!(cpu.call(BUILDER, &[seat]), Stop::Returned, "round {round}");
        let retail_queue = cpu.mem.read(a + 0x1DF, 16);
        let retail_spirit = cpu
            .mem
            .rd16(a + 0x170)
            .wrapping_sub(u16::from(cpu.mem.rd8(a + 0x224)));
        let retail_known = cpu.mem.read(rec + 0x185, 17);

        // Port.
        let mut party = Party::zeroed(3);
        let mut list = party.members[ch].displayed_skills();
        list.count = known.len() as u8;
        list.ids = [0; 16];
        list.ids[..known.len()].copy_from_slice(&known);
        party.members[ch].set_displayed_skills(list);
        let mut bits = [0u8; legaia_save::ABILITY_BITS_LEN];
        if halved {
            bits[5] = 0x08;
        }
        party.members[ch].set_ability_bits(bits);
        let mut eq = party.members[ch].equipment();
        // The marker is "the Ra-Seru slot is occupied".
        let ra_seru_slot = if ch == 1 { 2 } else { 3 };
        eq.slots[ra_seru_slot] = u8::from(armed);
        party.members[ch].set_equipment(eq);
        w.load_party(party);
        w.enter_battle(3, 2);
        w.party.party_count = 3;
        w.battle.status_effects = Default::default();
        w.actors[ch].battle.spirit_gauge = spirit;
        w.actors[ch].battle.field_flags = status;
        w.rng_state = seed;
        let (queue, actions, _) = w.build_arts_action_queue(ch as u8, &arrows);
        w.charge_art_spirit(ch as u8, &actions);
        let port_queue = queue[..16].to_vec();
        let port_spirit = w.actors[ch]
            .battle
            .spirit_gauge
            .wrapping_sub(u16::from(w.actors[ch].battle.action_recoil));
        let port_known = {
            let l = w.party.roster.members[ch].displayed_skills();
            let mut v = vec![l.count];
            v.extend_from_slice(&l.ids);
            v
        };

        // Sixteen ids is the list's room. The insert has no bound in retail and
        // runs on into the equipment bytes; the port refuses it, disclosed at
        // `check_and_learn_art`. Noa alone has seventeen arts to know.
        if retail_known[0] > 16 {
            continue;
        }
        let matched = retail_queue.iter().any(|&b| b == 0x19 || b == 0x1A);
        if matched {
            arts += 1;
        } else {
            plain += 1;
        }
        if retail_spirit == spirit && !matched && arrows.len() >= 3 {
            refused += 1;
        }
        if (&retail_queue, retail_spirit) != (&port_queue, port_spirit)
            || retail_known != port_known
        {
            failures.push(format!(
                "round {round} char {ch} arrows {:02x?} spirit {spirit} halved {halved} armed {armed} known {known:?} status {status:#x}:\n retail {retail_queue:02x?} spirit {retail_spirit} known {retail_known:02x?}\n port   {port_queue:02x?} spirit {port_spirit} known {port_known:02x?}",
                arrows.iter().map(|c| c.as_action().as_byte()).collect::<Vec<_>>()
            ));
            if failures.len() >= 5 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        arts > 2000 && plain > 500 && refused > 100,
        "{arts} {plain} {refused}"
    );
}
