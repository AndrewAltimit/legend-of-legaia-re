//! `FUN_801EC3E4`, the melee hit resolver, against `World::land_melee_hit`.
//!
//! Both sides run one seated fight, drawn from one `rand()` stream: the block
//! roll, the damage roll pair with its underdog rewrite and chip floor, the
//! equipment fold, the guard ladder, the Spirit accrual, the kill compare and
//! the Seru absorb roll, the status arms of the clip's `+0x7A` class, the
//! reaction entry and the HP write.

use crate::common::{Rng, disc, scus_cpu, split_statuses};
use legaia_actor_formats::monster_archive::{MonsterAnimation, PartPose};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_battle::battle_anim::MonsterAnimPlayer;
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_battle_vm::battle_action::HitEvent;
use legaia_engine_core::world::World;
use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_game_tables::element_affinity;
use legaia_save::{AutoCommandBand, Party};
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const RESOLVER: u32 = 0x801E_C3E4;
const RAND: u32 = 0x8005_6798;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const OVERLAY_BASE: u32 = 0x801C_E818;

const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MONSTER: u32 = 0x8015_0000;
const CLIP: u32 = 0x8016_0000;
const ANIM_NODE: u32 = 0x8016_1000;
const MYSTIC_WORD: u32 = 0x8007_BD84;
const VIBRATION_OFF: u32 = 0x8008_45C8;
const XA_DRIVE: u32 = 0x8007_BC20;
const RUMBLE_PULSE: u32 = 0x8007_B92C;
const RUMBLE_LEVEL: u32 = 0x8007_B930;
const FORMATION: u32 = 0x8007_BD0C;
const B3_LINK: u32 = 0x8015_2000;
const ENTRY_TABLES: u32 = 0x801C_9360;
const ENTRY_TABLE: u32 = 0x8017_0000;
const ENTRIES: u32 = 0x8017_1000;

/// One generated hit: what both sides are seeded with.
#[derive(Debug, Clone)]
struct Hit {
    attacker: u8,
    target: u8,
    power: u8,
    committed: u8,
    class: u8,
    atk: [u16; 8],
    udf: [u16; 8],
    ldf: [u16; 8],
    spd: [u16; 8],
    hp: [u16; 8],
    max_hp: [u16; 8],
    status: [u16; 8],
    spirit: [u16; 8],
    accum: u32,
    attack_ramp: i16,
    guard_ramp: i16,
    juggle: u8,
    juggle_window: bool,
    in_block: bool,
    guarding: bool,
    fleeing: bool,
    ability: [[u32; 2]; 3],
    equip: [[u8; 5]; 3],
    element: u8,
    target_element: u8,
    absorb_seru: u8,
    absorb_pct: u8,
    has_block: bool,
    has_getup: bool,
    mystic: bool,
    drain_clip: bool,
    seed: u32,
    look: Look,
    /// The Battles vibration row is Off (`0x800845C8 != 0`).
    vibration_off: bool,
    /// The field XA one-shot's drive word `0x8007BC20`.
    xa_drive: u32,
    /// The first formation cell is `0xB3`.
    b3: bool,
    /// The acting attacker's Ra-Seru marker `ctx[+0x25F + slot]`.
    marker: bool,
    /// The monster's flinch tags: the high (`2`), the low (`3`), both, or
    /// neither.
    flinch: u8,
    /// Seat 2 holds Terra (character id `4`, record 3); only drawn while
    /// seat 2 swings at a monster.
    terra: bool,
}

/// The hit's place in its action: what the head guards, the limb-vs-height
/// miss and the apply mode's look-ahead read.
#[derive(Debug, Clone)]
struct Look {
    /// `ctx[+0x07]`, the action state.
    state: u8,
    /// The attacker's `+0x1F4`, the hit being resolved.
    hit_index: u8,
    /// The clip entry's power run `+0x00..+0x04`.
    run: [u8; 4],
    /// Its event-frame list `+0x10..+0x14`.
    frames: [u8; 4],
    /// `ctx[+0x15]`, the strike cursor (`0xFF` parked).
    cursor: u8,
    /// The attacker's stream `+0x1DF..`.
    queue: [u8; 16],
    /// The power run of the entry each stream byte below `0x10` names.
    entries: [[u8; 4]; 16],
    /// The target monster record's `+0x1E` swing class.
    class: u8,
    /// `ctx[+0x16]`, the Attack x2 pass.
    x2_pass: u8,
}

/// A power byte the look-ahead classifies: a terminator, the low and high
/// swing bands, or one at `0x16` and above that connects with everything.
fn look_byte(rng: &mut Rng) -> u8 {
    match rng.below(10) {
        0..=1 => 0,
        2 => 1 + rng.below(0x0B) as u8,
        3..=5 => 0x0C + rng.below(5) as u8,
        6..=7 => 0x11 + rng.below(5) as u8,
        _ => 0x16 + rng.below(0x0A) as u8,
    }
}

fn gen_look(rng: &mut Rng, power: u8, party_attacks: bool) -> Look {
    let plain = rng.below(3) == 0;
    let hit_index = if plain { 0 } else { rng.below(4) as u8 };
    let mut run = [0u8; 4];
    let mut frames = [0u8; 4];
    for i in 0..4 {
        run[i] = look_byte(rng);
        frames[i] = if rng.below(3) == 0 {
            0
        } else {
            1 + rng.below(3) as u8
        };
    }
    // Every hit up to this one has its beat, this one on frame 1.
    for f in frames.iter_mut().take(usize::from(hit_index)) {
        *f = 1;
    }
    frames[usize::from(hit_index)] = 1;
    run[0] = 0x0C + rng.below(0x14) as u8;
    run[usize::from(hit_index)] = power;
    if plain {
        run[1..].fill(0);
        frames[1..].fill(0);
    }
    let mut queue = [0u8; 16];
    let mut entries = [[0u8; 4]; 16];
    for q in queue.iter_mut() {
        *q = match rng.below(8) {
            0 => 0,
            1 => 0x10 + rng.below(0x10) as u8,
            _ => 1 + rng.below(0x0F) as u8,
        };
    }
    for e in entries.iter_mut() {
        for b in e.iter_mut() {
            *b = look_byte(rng);
        }
    }
    Look {
        state: match rng.below(8) {
            0 => 0x6E + rng.below(10) as u8,
            1 => 0x64 + rng.below(0x0A) as u8,
            _ => 3,
        },
        hit_index,
        run,
        frames,
        cursor: match rng.below(5) {
            0 | 1 => 0xFF,
            2 => 0,
            _ => 1 + rng.below(0x11) as u8,
        },
        queue,
        entries,
        class: if plain || !party_attacks {
            0
        } else {
            [0, 1, 2, 2, 3, 3][rng.below(6) as usize]
        },
        x2_pass: rng.below(3) as u8,
    }
}

/// A value with its boundaries over-represented.
fn stat(rng: &mut Rng, top: u32) -> u16 {
    match rng.below(8) {
        0 => 0,
        1 => 1,
        2 => top as u16,
        3 => (7 + 8 * rng.below(top / 8 + 1)).min(top) as u16,
        _ => rng.below(top + 1) as u16,
    }
}

fn gen_hit(rng: &mut Rng) -> Hit {
    let party_attacks = rng.below(2) == 0;
    // Now and then a monster strikes the other monster.
    let monster_on_monster = !party_attacks && rng.below(6) == 0;
    let (attacker, target) = if party_attacks {
        (rng.below(3) as u8, 3 + rng.below(2) as u8)
    } else if monster_on_monster {
        let a = 3 + rng.below(2) as u8;
        (a, 7 - a)
    } else {
        (3 + rng.below(2) as u8, rng.below(3) as u8)
    };
    let top = [100, 400, 1200, 9999][rng.below(4) as usize];
    let mut h = Hit {
        attacker,
        target,
        power: 0x0C + rng.below(0x14) as u8,
        committed: if party_attacks {
            0x0C + rng.below(6) as u8
        } else {
            // A monster's clip index is a plain entry, any of them.
            1 + rng.below(0x18) as u8
        },
        class: if rng.below(2) == 0 {
            0
        } else {
            rng.below(8) as u8
        },
        atk: [0; 8],
        udf: [0; 8],
        ldf: [0; 8],
        spd: [0; 8],
        hp: [0; 8],
        max_hp: [0; 8],
        status: [0; 8],
        spirit: [0; 8],
        accum: 0,
        attack_ramp: [0, 0, 0x400, 0x800][rng.below(4) as usize],
        guard_ramp: [0, 0, 16, rng.below(0x200) as i16][rng.below(4) as usize],
        juggle: rng.below(6) as u8,
        juggle_window: rng.below(4) == 0,
        in_block: rng.below(8) == 0,
        guarding: target < 3 && rng.below(6) == 0,
        fleeing: target < 3 && rng.below(8) == 0,
        ability: [[0; 2]; 3],
        equip: [[0; 5]; 3],
        element: rng.below(8) as u8,
        target_element: rng.below(8) as u8,
        absorb_seru: if rng.below(2) == 0 {
            0
        } else {
            1 + rng.below(0x20) as u8
        },
        absorb_pct: rng.below(101) as u8,
        has_block: rng.below(2) == 0,
        has_getup: rng.below(2) == 0,
        mystic: rng.below(16) == 0,
        drain_clip: rng.below(6) == 0,
        seed: rng.next(),
        look: Look {
            state: 3,
            hit_index: 0,
            run: [0; 4],
            frames: [0; 4],
            cursor: 0,
            queue: [0; 16],
            entries: [[0; 4]; 16],
            class: 0,
            x2_pass: 0,
        },
        vibration_off: rng.below(3) == 0,
        xa_drive: [0u32, 0, 1, 2, 3][rng.below(5) as usize],
        b3: rng.below(8) == 0,
        marker: rng.below(2) == 0,
        flinch: rng.below(4) as u8,
        terra: attacker == 2 && target >= 3 && rng.below(3) == 0,
    };
    // The Mystic Shield's rumble skips the vibration test the port keeps
    // (a deliberate deviation), so an Off row is not drawn with it.
    if h.mystic {
        h.vibration_off = false;
    }
    h.look = gen_look(rng, h.power, party_attacks);
    if monster_on_monster {
        h.look.class = [0, 2, 3][rng.below(3) as usize];
    }
    for s in 0..8 {
        h.atk[s] = stat(rng, top);
        h.udf[s] = stat(rng, top);
        h.ldf[s] = stat(rng, top);
        // A zero sum traps retail's `divu`; every record a game seats has SPD.
        h.spd[s] = 1 + stat(rng, top.min(999)).min(998);
        h.max_hp[s] = 1 + rng.below(9999) as u16;
        h.hp[s] = 1 + rng.below(u32::from(h.max_hp[s])) as u16;
        h.spirit[s] = rng.below(101) as u16;
        h.status[s] = match rng.below(6) {
            0 => [
                1, 2, 3, 4, 0x80, 0x400, 0x1000, 0x1003, 0x08, 0x10, 0x20, 0x38,
            ][rng.below(12) as usize],
            _ => 0,
        };
    }
    for m in 0..3 {
        if rng.below(2) == 0 {
            // The passives the resolver reads; `0x2000` is the War God
            // Icon's Attack x2 carry in the apply mode.
            let w0 = [
                0x2000u32,
                0x4000,
                0x8_0000,
                0x10_0000,
                0x20_0000,
                0x2000_0000,
                0x4000_0000,
                0x8000_0000,
                // The Rot wards.
                0x0100_0000,
                0x1000_0000,
            ][rng.below(10) as usize];
            let w1 = [1u32, 2, 4, 8, 0x10, 0x100, 0x200, 0x1000, 0x4000, 0x20_0000]
                [rng.below(10) as usize];
            h.ability[m] = [w0, w1];
        }
        for slot in 0..5 {
            h.equip[m][slot] = rng.below(0x80) as u8;
        }
    }
    h.accum = match rng.below(3) {
        0 => 0,
        _ => rng.below(u32::from(h.hp[target as usize]) + 20),
    };
    h
}

/// The monster id seated on `slot`: `0xB3` on the first monster seat for the
/// arm keyed on it.
fn monster_id(h: &Hit, slot: u8) -> u16 {
    if h.b3 && slot == 3 {
        0xB3
    } else {
        0x40 + u16::from(slot)
    }
}

fn actor(slot: u8) -> u32 {
    ACTOR + u32::from(slot) * ACTOR_STRIDE
}

/// `+0x1EF..+0x1F3`, as `FUN_80054CB0` builds it for the clips [`clips`] seats.
fn reaction_map(h: &Hit, slot: u8) -> [u8; 5] {
    if slot < 3 {
        return [2, 3, 4, 5, 0x0B];
    }
    let tags = monster_tags(h);
    legaia_engine_battle_loop::clip_ladder::monster_reaction_map(&tags)
}

fn monster_tags(h: &Hit) -> Vec<u8> {
    let mut tags = match h.flinch {
        0 => vec![0u8, 2, 4],
        1 => vec![0u8, 3, 4],
        2 => vec![0u8, 2, 3, 4],
        _ => vec![0u8, 4],
    };
    if h.has_getup {
        tags.push(5);
    }
    if h.has_block {
        tags.push(0x0B);
    }
    tags
}

fn clip(action_id: u8, impact_class: u8) -> MonsterAnimation {
    MonsterAnimation {
        variant_track: Default::default(),
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: 2,
        frames: vec![vec![PartPose::default()]; 2],
    }
}

fn seed_retail(cpu: &mut Cpu, h: &Hit) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x900 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 0x0A, h.juggle);
    cpu.mem.wr8(CTX + 0x07, h.look.state);
    cpu.mem.wr8(CTX + 0x13, h.attacker);
    // The Ra-Seru markers are party seats' (`ctx[+0x25F..+0x262]`).
    if h.attacker < 3 {
        cpu.mem
            .wr8(CTX + 0x25F + u32::from(h.attacker), u8::from(h.marker));
    }
    cpu.mem.wr32(VIBRATION_OFF, u32::from(h.vibration_off));
    cpu.mem.wr32(XA_DRIVE, h.xa_drive);
    cpu.mem.wr32(RUMBLE_PULSE, 0);
    cpu.mem.wr32(RUMBLE_LEVEL, 0);
    // The formation's first cell: the monster seated first, `0xB3` for the
    // arm that forces its knockdown entry.
    cpu.mem.wr8(FORMATION, if h.b3 { 0xB3 } else { 0x43 });
    // The apply gate's strike cursor and the Attack x2 pass.
    cpu.mem.wr8(CTX + 0x15, h.look.cursor);
    cpu.mem.wr8(CTX + 0x16, h.look.x2_pass);
    cpu.mem.wr16(CTX + 0x6D2, h.attack_ramp as u16);
    cpu.mem.wr16(CTX + 0x6D4, h.guard_ramp as u16);
    for m in 0..3u32 {
        let char_id = if h.terra && m == 2 { 4 } else { m as u8 + 1 };
        cpu.mem.wr8(SEATS + m, char_id);
        let rec = CHAR_RECORDS + (u32::from(char_id) - 1) * CHAR_STRIDE;
        cpu.mem.wr32(rec + 0xF4, h.ability[m as usize][0]);
        cpu.mem.wr32(rec + 0xF8, h.ability[m as usize][1]);
        cpu.mem.wr8(rec + 0x1A7, 0x0C);
        cpu.mem.wr8(rec + 0x1B7, 0x0D);
        for s in 0..5 {
            cpu.mem
                .wr8(rec + 0x196 + s, h.equip[m as usize][s as usize]);
        }
    }
    for b in 0..0x100 {
        cpu.mem.wr8(ANIM_NODE + b, 0);
        cpu.mem.wr8(MONSTER + b, 0);
        cpu.mem.wr8(MONSTER + 0x100 + b, 0);
    }
    for s in 0..8u8 {
        let a = actor(s);
        cpu.mem.wr32(ACTORS + u32::from(s) * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
        let i = usize::from(s);
        cpu.mem.wr16(a + 0x14C, h.hp[i]);
        cpu.mem.wr16(a + 0x14E, h.max_hp[i]);
        cpu.mem.wr16(a + 0x158, h.atk[i]);
        cpu.mem.wr16(a + 0x15C, h.udf[i]);
        cpu.mem.wr16(a + 0x160, h.ldf[i]);
        cpu.mem.wr16(a + 0x164, h.spd[i]);
        cpu.mem.wr16(a + 0x16E, h.status[i]);
        cpu.mem.wr16(a + 0x170, h.spirit[i]);
        cpu.mem.wr16(a + 0x172, h.hp[i]);
        cpu.mem.wr32(a + 0x22C, ANIM_NODE);
        cpu.mem.wr32(ANIM_NODE + 0x4C, ANIM_NODE);
        for (k, e) in reaction_map(h, s).into_iter().enumerate() {
            cpu.mem.wr8(a + 0x1EF + k as u32, e);
        }
    }
    for m in 0..2u32 {
        let rec = MONSTER + m * 0x80;
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, rec);
    }
    // The `0xB3` arm's side store runs through record 0's `+0x54` -> `+0x88`.
    cpu.mem.wr32(MONSTER + 0x54, B3_LINK);
    cpu.mem.wr32(B3_LINK + 0x88, B3_LINK + 0x100);
    let att = actor(h.attacker);
    let tgt = actor(h.target);
    cpu.mem.wr32(MYSTIC_WORD, u32::from(h.mystic));
    cpu.mem.wr32(MONSTER + 0x4C, ANIM_NODE);
    if h.drain_clip {
        cpu.mem.wr8(ANIM_NODE + 0x77, 0x3B);
    }
    let monster_record = |slot: u8| MONSTER + u32::from(slot - 3) * 0x80;
    if h.attacker >= 3 {
        cpu.mem.wr8(monster_record(h.attacker) + 0x1D, h.element);
    }
    if h.target >= 3 {
        let r = monster_record(h.target);
        cpu.mem.wr8(r + 0x1D, h.target_element);
        cpu.mem.wr8(r + 0x3E, h.absorb_seru);
        cpu.mem.wr8(r + 0x3F, h.absorb_pct);
    }
    cpu.mem.wr8(att + 0x1D9, h.committed);
    cpu.mem.wr8(att + 0x1DD, h.target);
    cpu.mem.wr32(tgt, h.accum);
    if h.juggle_window {
        cpu.mem.wr8(tgt + 0x1F7, 1);
        // The window runs only while a reaction plays.
        cpu.mem.wr8(tgt + 0x1D9, 1);
    }
    if h.in_block && reaction_map(h, h.target)[4] != 0 {
        cpu.mem.wr8(tgt + 0x1D9, reaction_map(h, h.target)[4]);
    }
    if h.guarding {
        cpu.mem.wr8(tgt + 0x1DE, 4);
    } else if h.fleeing {
        cpu.mem.wr8(tgt + 0x1DE, 5);
    }
    // The clip record: the power run, the event frames, the impact class.
    for b in 0..0x100 {
        cpu.mem.wr8(CLIP + b, 0);
    }
    for i in 0..4u32 {
        cpu.mem.wr8(CLIP + i, h.look.run[i as usize]);
        cpu.mem.wr8(CLIP + 0x10 + i, h.look.frames[i as usize]);
    }
    cpu.mem.wr8(CLIP + 0x7A, h.class);
    // The look-ahead: the hit index, the stream and the entry table
    // `0x801C9360[slot][byte]` its swing bytes resolve through.
    cpu.mem.wr8(att + 0x1F4, h.look.hit_index);
    for (i, &q) in h.look.queue.iter().enumerate() {
        cpu.mem.wr8(att + 0x1DF + i as u32, q);
    }
    // Four words ahead of the actor table: party seats only, and only a
    // party attacker's look-ahead reads it.
    if h.attacker < 3 {
        cpu.mem
            .wr32(ENTRY_TABLES + u32::from(h.attacker) * 4, ENTRY_TABLE);
    }
    for (b, run) in h.look.entries.iter().enumerate() {
        let entry = ENTRIES + b as u32 * 0x20;
        cpu.mem.wr32(ENTRY_TABLE + b as u32 * 4, entry);
        for (i, &p) in run.iter().enumerate() {
            cpu.mem.wr8(entry + i as u32, p);
        }
    }
    if h.target >= 3 {
        cpu.mem.wr8(monster_record(h.target) + 0x1E, h.look.class);
    }
}

fn seed_port(w: &mut World, h: &Hit) {
    let mut party = Party::zeroed(4);
    for m in 0..3 {
        // Seat `m`'s record: Terra's (roster 3) for seat 2 when she sits.
        let r = if h.terra && m == 2 { 3 } else { m };
        let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
        bytes[0..4].copy_from_slice(&h.ability[m][0].to_le_bytes());
        bytes[4..8].copy_from_slice(&h.ability[m][1].to_le_bytes());
        party.members[r].set_ability_bits(bytes);
        party.members[r].set_auto_command_string(AutoCommandBand::Primary, [0x0C; 16]);
        party.members[r].set_auto_command_string(AutoCommandBand::Secondary, [0x0D; 16]);
    }
    w.load_party(party);
    w.party.active_party = if h.terra {
        vec![0, 1, 3]
    } else {
        vec![0, 1, 2]
    };
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    w.battle.status_effects = Default::default();
    w.battle.juggle = h.juggle;
    w.battle.monster_ai_state.flag_bd84 = u32::from(h.mystic) as _;
    w.battle.attack_ramp = h.attack_ramp;
    w.battle.guard_ramp = h.guard_ramp;
    w.battle.guarding = [false; 3];
    for s in 0..5u8 {
        let i = usize::from(s);
        let is_party = s < 3;
        w.battle.set_battle_attack(s, h.atk[i]);
        w.battle.attack[i] = h.atk[i];
        w.battle.defense_split[i] = Some((h.udf[i], h.ldf[i]));
        w.battle.speed[i] = h.spd[i];
        let a = &mut w.actors[i];
        a.battle.liveness = 1;
        a.battle.hp = h.hp[i];
        a.battle.max_hp = h.max_hp[i];
        a.battle.spd = h.spd[i];
        a.battle.field_flags = h.status[i];
        a.battle.spirit_gauge = h.spirit[i];
        a.battle.damage_accum = 0;
        a.battle.action_category = 0;
        a.battle.current_anim = 0;
        a.battle.hp_display = None;
        a.battle.hp_bar_pending = 0;
        a.battle_juggle_window = false;
        a.battle_reaction_entry = None;
        // A stage the previous round's hit left would read as this round's.
        a.battle_reaction_stage = None;
        a.battle_animation = None;
        a.battle_monster_id = if is_party {
            None
        } else {
            Some(monster_id(h, s))
        };
        a.battle_element = (!is_party).then_some(if s == h.attacker {
            h.element
        } else {
            h.target_element
        });
        let tags = if is_party {
            (0..0x12u8).collect::<Vec<_>>()
        } else {
            monster_tags(h)
        };
        let mut clips: Vec<_> = tags.iter().map(|&t| Some(clip(t, 0))).collect();
        if s == h.attacker && is_party {
            // The entries the look-ahead resolves stream bytes through.
            for (b, c) in clips.iter_mut().enumerate().take(16) {
                let c = c.as_mut().unwrap();
                c.effect_script = vec![0; 0x14];
                c.effect_script[..4].copy_from_slice(&h.look.entries[b]);
            }
        }
        w.set_actor_battle_action_clips(i, std::sync::Arc::new(clips));
        if !is_party {
            let mut def = MonsterDef::new(monster_id(h, s), "m", 1, 10);
            def.absorb_seru = h.absorb_seru;
            def.absorb_chance_pct = h.absorb_pct;
            if s == h.target {
                def.swing_class = h.look.class;
            }
            w.tables.monster_catalog.insert(def);
        }
    }
    split_statuses(w, h.seed);
    let ti = usize::from(h.target);
    let ai = usize::from(h.attacker);
    w.actors[ti].battle.damage_accum = h.accum;
    w.actors[ti].battle_juggle_window = h.juggle_window;
    if h.juggle_window {
        w.actors[ti].battle_reaction_entry = Some(1);
    }
    if h.in_block && reaction_map(h, h.target)[4] != 0 {
        w.actors[ti].battle_reaction_entry = Some(reaction_map(h, h.target)[4]);
    }
    if h.guarding && h.target < 3 {
        w.battle.guarding[ti] = true;
    }
    if h.fleeing && !h.guarding {
        w.actors[ti].battle.action_category = 5;
    }
    w.actors[ai].battle.current_anim = h.committed;
    w.actors[ai].battle.strike_index = h.look.cursor;
    w.actors[ai].battle.params = [0; legaia_engine_battle_vm::battle_action::ACTION_PARAM_BYTES];
    w.actors[ai].battle.params[..16].copy_from_slice(&h.look.queue);
    w.battle_ctx.action_state = h.look.state;
    w.battle_ctx.attack_x2_pass = h.look.x2_pass;
    w.battle_ctx.active_actor = h.attacker;
    w.battle.art_seed_ra_seru = h.marker && h.attacker < 3;
    w.toggles.retail_options.vibration_battles = u8::from(h.vibration_off);
    // The grunt waits out a field XA one-shot still driving (`BC20 >= 2`).
    w.audio.battle_xa_busy_frames = u16::from(h.xa_drive >= 2);
    w.rumble.motors = Default::default();
    let _ = w.drain_battle_xa_cues();
    for m in 0..3 {
        w.party.character_ability_bits[m] = h.ability[m][0];
    }
    w.actors[ai].battle.active_target = h.target;
    let mut playing = clip(0x0C, h.class);
    playing.attach_key = if h.drain_clip { 0x3B } else { 0 };
    w.actors[ai].battle_animation = MonsterAnimPlayer::new_one_shot(&playing);
    w.rng_state = h.seed;
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Outcome {
    accum: u32,
    hp: u16,
    pending: i32,
    spirit: u16,
    status: u16,
    juggle: u8,
    ramps: (i16, i16),
    reaction: u8,
    attacker_hp: u16,
    attacker_pending: i32,
    auto_heads: Option<(u8, u8)>,
    absorbed: u8,
    strobe: u8,
    rumble: (i32, i32),
    grunts: usize,
    rng: u32,
}

#[test]
fn a_melee_hit_lands_what_retail_lands() {
    differential!(prot(898, RESOLVER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    // The grunt's XA start, counted.
    let grunts = Rc::new(Cell::new(0usize));
    let g = grunts.clone();
    cpu.hook(0x8003_D53C, move |_, _| g.set(g.get() + 1));
    cpu.stub(0x8004_FE5C, 0); // The Mystic Shield's sound.
    cpu.stub(0x8001_9B28, 0); // The bearing; turning is compared elsewhere.

    let scus = crate::common::scus().unwrap();
    let mut w = World::new();
    w.tables.element_affinity = element_affinity::parse(&overlay);
    w.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));

    // The attack byte of the equipment two-hop, read off the executable.
    let equip_byte = |cpu: &mut Cpu, id: u8| -> u8 {
        let row = cpu.mem.rd8(0x8007_4368 + u32::from(id) * 0xC + 1);
        cpu.mem.rd8(0x8007_4F68 + u32::from(row) * 8 + 1)
    };

    let mut rng = Rng(0x4D45_4C45);
    let mut blocked = 0;
    let mut hits = 0;
    let mut rewrites = 0;
    let mut misses = 0;
    let mut failures: Vec<String> = Vec::new();
    let rounds: u32 = std::env::var("MELEE_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20000);
    for round in 0..rounds {
        let h = gen_hit(&mut rng);
        seed_retail(&mut cpu, &h);
        state.set(h.seed);
        let stop = cpu.call(RESOLVER, &[u32::from(h.attacker), CLIP, 0, 0]);
        assert_eq!(stop, Stop::Returned, "round {round}: {stop:?} {h:?}");
        let t = actor(h.target);
        let a = actor(h.attacker);
        let retail = Outcome {
            accum: cpu.mem.rd32(t),
            hp: cpu.mem.rd16(t + 0x14C),
            pending: cpu.mem.rd32(t + 0x10) as i32,
            spirit: cpu.mem.rd16(t + 0x170),
            status: cpu.mem.rd16(t + 0x16E),
            juggle: cpu.mem.rd8(CTX + 0x0A),
            ramps: (
                cpu.mem.rd16(CTX + 0x6D2) as i16,
                cpu.mem.rd16(CTX + 0x6D4) as i16,
            ),
            reaction: cpu.mem.rd8(t + 0x1DA),
            attacker_hp: cpu.mem.rd16(a + 0x14C),
            attacker_pending: cpu.mem.rd32(a + 0x10) as i32,
            auto_heads: (h.target < 3).then(|| {
                let rec = CHAR_RECORDS + u32::from(h.target) * CHAR_STRIDE;
                (cpu.mem.rd8(rec + 0x1A7), cpu.mem.rd8(rec + 0x1B7))
            }),
            absorbed: cpu.mem.rd8(CTX + 0x269),
            strobe: cpu.mem.rd8(CTX + 0x263),
            rumble: (
                cpu.mem.rd32(RUMBLE_PULSE) as i32,
                cpu.mem.rd32(RUMBLE_LEVEL) as i32,
            ),
            grunts: grunts.replace(0),
            rng: state.get(),
        };

        seed_port(&mut w, &h);
        let commits = w.actors[usize::from(h.target)].battle_clip_commits;
        for m in 0..3 {
            w.battle.equip_atk[m] = h.equip[m].map(|id| equip_byte(&mut cpu, id));
        }
        let hit = HitEvent {
            hit_index: h.look.hit_index,
            power_byte: h.power,
            event_frame: 1,
        };
        let plan = w.melee_hit_plan(h.attacker, h.target, &hit, &h.look.run, &h.look.frames);
        let dmg = match plan {
            Some(apply) => {
                let (dmg, applied) =
                    w.land_melee_hit_planned(h.attacker, h.target, h.power, h.committed, apply);
                if applied {
                    w.apply_combo_total(h.target);
                }
                dmg
            }
            None => {
                misses += 1;
                1
            }
        };
        let ta = &w.actors[usize::from(h.target)];
        let port = Outcome {
            accum: ta.battle.damage_accum,
            hp: ta.battle.hp,
            pending: ta.battle.hp_bar_pending,
            spirit: ta.battle.spirit_gauge,
            status: w.raw_status_word(h.target),
            juggle: w.battle.juggle,
            ramps: (w.battle.attack_ramp, w.battle.guard_ramp),
            // Retail's `+0x1DA` is the staged byte: a hit stages its reaction
            // and the seat's own anim tick commits it (`FUN_80047430`).
            reaction: if let Some((entry, _)) = ta.battle_reaction_stage {
                entry
            } else if ta.battle_clip_commits != commits {
                ta.battle_reaction_entry.unwrap_or(0)
            } else {
                0
            },
            attacker_hp: w.actors[usize::from(h.attacker)].battle.hp,
            attacker_pending: w.actors[usize::from(h.attacker)].battle.hp_bar_pending,
            auto_heads: (h.target < 3).then(|| {
                let rec = &w.party.roster.members[usize::from(h.target)];
                (
                    rec.auto_command_string(AutoCommandBand::Primary)[0],
                    rec.auto_command_string(AutoCommandBand::Secondary)[0],
                )
            }),
            absorbed: w.battle_ctx.absorbed_seru,
            strobe: u8::from(plan.is_none()),
            rumble: (w.rumble.motors.pulse, w.rumble.motors.level),
            grunts: w.drain_battle_xa_cues().len(),
            rng: w.rng_state,
        };
        if dmg == 0 {
            blocked += 1;
        } else {
            hits += 1;
        }
        if cpu.steps > 0 && retail.rng != state.get() {
            rewrites += 1;
        }
        if retail != port {
            failures.push(format!(
                "round {round}: dmg {dmg} plan {plan:?}\n retail {retail:?}\n port   {port:?}\n {h:?}"
            ));
            if failures.len() >= 6 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        blocked > 100 && hits > 1000 && misses > 100,
        "{blocked} blocked, {hits} hits, {misses} limb misses, {rewrites}"
    );
}

/// The head guard chain (`0x801EC41C..0x801EC484`) against
/// `hit_event_admits`: an admitted call always reaches the epilogue's
/// `+0x1F4` bump at `0x801EECC0`, a refused one returns past it.
#[test]
fn the_head_guards_admit_what_retail_admits() {
    differential!(prot(898, RESOLVER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    cpu.stub(0x8003_D53C, 0);
    cpu.stub(0x8004_FE5C, 0);
    cpu.stub(0x8001_9B28, 0);
    let mut rng = Rng(0x4845_4144);
    let (mut admitted, mut refused) = (0, 0);
    for round in 0..4000 {
        let mut h = gen_hit(&mut rng);
        h.look.state = match rng.below(6) {
            0 => 0x5A,
            1 => rng.below(0x100) as u8,
            _ => 3,
        };
        h.look.hit_index = [0, 1, 2, 3, 4, 5][rng.below(6) as usize];
        for i in 0..4 {
            h.look.run[i] = match rng.below(4) {
                0 => rng.below(0x100) as u8,
                1 => [0x0B, 0x0C, 0x1F, 0x20][rng.below(4) as usize],
                _ => 0x0C + rng.below(0x14) as u8,
            };
            h.look.frames[i] = [0, 1, 2, 5, 0xFF][rng.below(5) as usize];
        }
        h.power = h.look.run[usize::from(h.look.hit_index.min(3))];
        let frame = [0u8, 1, 3, 4, 0xFE, 0xFF][rng.below(6) as usize];
        seed_retail(&mut cpu, &h);
        state.set(h.seed);
        let a = actor(h.attacker);
        // The fifth slot's event byte is the record's `+0x14`.
        cpu.mem.wr8(CLIP + 0x14, h.look.frames[0]);
        cpu.mem.wr8(CLIP + 0x15, h.look.frames[1]);
        let stop = cpu.call(
            RESOLVER,
            &[u32::from(h.attacker), CLIP, u32::from(frame), 0],
        );
        assert_eq!(stop, Stop::Returned, "round {round}: {stop:?} {h:?}");
        let retail = cpu.mem.rd8(a + 0x1F4) != h.look.hit_index;
        let ev4 = [
            h.look.frames[0],
            h.look.frames[1],
            h.look.frames[2],
            h.look.frames[3],
        ];
        // The port bounds the index first; past it retail reads
        // `entry[0x10 + idx]`, which the seat above mirrors.
        let port = legaia_engine_battle_vm::battle_action::hit_event_admits(
            h.look.state,
            &h.look.run,
            &ev4,
            h.look.hit_index,
            frame,
        )
        .is_some();
        assert_eq!(
            retail, port,
            "round {round}: state {:#04x} run {:02x?} frames {:02x?} idx {} frame {frame}",
            h.look.state, h.look.run, ev4, h.look.hit_index
        );
        if retail {
            admitted += 1;
        } else {
            refused += 1;
        }
    }
    assert!(
        admitted > 500 && refused > 500,
        "{admitted} admitted, {refused} refused"
    );
}
