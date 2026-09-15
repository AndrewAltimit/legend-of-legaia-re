//! **Every ported cast-band body, once, with its denominator.**
//!
//! The three cast-band modules carry a body per `// PORT:` marker, and each
//! lane that added them shipped its own ladder over its own rows. What none of
//! them can answer is the question the reach measurement actually asks: *is
//! there a body in the band that no ladder enters?* A per-lane ladder cannot
//! see a body a later lane added, and a ladder keyed on a hand-written row
//! list cannot see one nobody wrote a row for.
//!
//! So this ladder takes its denominator from the **sources**, not from its own
//! table: `tagged_bodies` scrapes every `// PORT:` address out of the three
//! modules, which `include_str!` pulls in at compile time, and the row table
//! below has to account for every `(file, address)` pair it finds. Adding a
//! body without adding a row fails `the_row_table_accounts_for_every_ported_body`
//! with no disc present.
//!
//! The walk itself is disc-gated: it seats the real module pool out of
//! `PROT.DAT` and drives each row through `World::run_cast_module_code` until
//! the body reports done, asserting the body was entered and left an
//! observable state change. Skips (and passes) without `LEGAIA_DISC_BIN` /
//! `extracted/`.

use legaia_asset::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use legaia_engine_core::world::{Actor, SceneMode, World};
use std::path::PathBuf;

/// The three module sources, by the name the row table uses.
const MODULE_SOURCES: [(&str, &str); 3] = [
    (
        "cast_seru_ticks_a",
        include_str!("../../engine-vm/src/cast_seru_ticks_a.rs"),
    ),
    (
        "cast_seru_ticks_b",
        include_str!("../../engine-vm/src/cast_seru_ticks_b.rs"),
    ),
    (
        "cast_arm_ticks",
        include_str!("../../engine-vm/src/cast_arm_ticks.rs"),
    ),
];

/// How a row is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The band dispatch enters this body for `(entry, id)`.
    Dispatch,
    /// The body is a leaf the row's tick body calls; entering the tick enters
    /// it. Named so the accounting does not silently drop it.
    Inner,
}

/// Every ported body in the band: `(module, body VA, PROT entry, action id,
/// how it is reached)`.
///
/// The module name doubles as the **band**: the two `cast_seru_ticks_*` bands
/// are cast by a party seat and driven with the stager arm their own ladders
/// use, and `cast_arm_ticks` is cast by a monster seat. Driving one band with
/// the other's seating makes its bodies early-out before the first write.
///
/// Two shapes are not one row per body. PROT 0940's split body is named by two
/// action ids, so it takes two rows sharing one VA; and PROT 0910's per-slash
/// leaf is reached from inside its own tick rather than by the dispatch, so it
/// is an [`Reach::Inner`] row on 0910's id.
const ROWS: [(&str, u32, u32, u8, Reach); 27] = [
    // The player-Seru band, spell ids 0x81..=0x86.
    ("cast_seru_ticks_a", 0x801F_69D8, 903, 0x81, Reach::Dispatch),
    ("cast_seru_ticks_a", 0x801F_69D8, 904, 0x82, Reach::Dispatch),
    ("cast_seru_ticks_a", 0x801F_69D8, 905, 0x83, Reach::Dispatch),
    ("cast_seru_ticks_a", 0x801F_69F4, 906, 0x84, Reach::Dispatch),
    ("cast_seru_ticks_a", 0x801F_69E8, 907, 0x85, Reach::Dispatch),
    ("cast_seru_ticks_a", 0x801F_69D8, 908, 0x86, Reach::Dispatch),
    // The rest of the player-Seru band, 0x87..=0x8B.
    ("cast_seru_ticks_b", 0x801F_69F4, 909, 0x87, Reach::Dispatch),
    ("cast_seru_ticks_b", 0x801F_69EC, 910, 0x88, Reach::Dispatch),
    ("cast_seru_ticks_b", 0x801F_81DC, 910, 0x88, Reach::Inner),
    ("cast_seru_ticks_b", 0x801F_69D8, 911, 0x89, Reach::Dispatch),
    ("cast_seru_ticks_b", 0x801F_69D8, 912, 0x8A, Reach::Dispatch),
    ("cast_seru_ticks_b", 0x801F_69F0, 913, 0x8B, Reach::Dispatch),
    // The trampoline-reached capture-class arms.
    ("cast_arm_ticks", 0x801F_7240, 940, 0xAC, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_78B8, 940, 0x50, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_78B8, 940, 0xAE, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_730C, 941, 0x51, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_6A04, 941, 0xB9, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_6EF4, 943, 0x40, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_6A04, 943, 0xB5, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_6A04, 944, 0x37, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_7470, 944, 0x53, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_79F8, 950, 0x5A, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_6A24, 950, 0xAB, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_7298, 956, 0x71, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_7AE4, 962, 0xA2, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_74A0, 962, 0xA3, Reach::Dispatch),
    ("cast_arm_ticks", 0x801F_6D54, 962, 0xA4, Reach::Dispatch),
];

/// Frames a row is driven before the ladder gives up. The deepest chain in the
/// band names twenty-odd arms plus the terminal.
const MAX_FRAMES: usize = 96;

/// Scrape `(module, address)` for every `// PORT:` marker in the three
/// sources.
///
/// Reads the marker's **opening line** only, which is the same rule
/// `scripts/ci/port_tag_reader.py` follows - a tag whose address list wraps
/// loses everything past the wrap there too, so agreeing with the scraper is
/// the point.
fn tagged_bodies() -> Vec<(&'static str, u32)> {
    let mut out = Vec::new();
    for (module, src) in MODULE_SOURCES {
        for line in src.lines() {
            let Some(tail) = line.split_once("PORT:").map(|(_, t)| t) else {
                continue;
            };
            let mut rest = tail;
            while let Some(at) = rest.find("FUN_") {
                rest = &rest[at + 4..];
                let hex: String = rest.chars().take(8).collect();
                if hex.len() == 8
                    && let Ok(va) = u32::from_str_radix(&hex, 16)
                {
                    out.push((module, va));
                }
            }
        }
    }
    out
}

/// The row table must account for every tagged body, and name no body that is
/// not tagged. Disc-free: this is the denominator, and it has to hold whether
/// or not a disc is present.
#[test]
fn the_row_table_accounts_for_every_ported_body() {
    let mut tagged = tagged_bodies();
    tagged.sort_unstable();
    // PROT 0940's split body is named by two action ids; the tag site is one,
    // so the second row must not count twice against the denominator.
    let mut rows: Vec<(&str, u32)> = Vec::new();
    let mut seen_split = false;
    for &(m, va, _, _, _) in &ROWS {
        if m == "cast_arm_ticks" && va == 0x801F_78B8 {
            if seen_split {
                continue;
            }
            seen_split = true;
        }
        rows.push((m, va));
    }
    rows.sort_unstable();
    assert_eq!(
        rows, tagged,
        "the ladder's row table and the modules' `// PORT:` markers disagree - \
         a body was added or removed without a row"
    );
    assert!(
        !tagged.is_empty(),
        "no `// PORT:` marker found in the three module sources - the scrape is broken"
    );
    eprintln!(
        "[ok] cast-band ladder denominator: {} ported bodies across {} modules, \
         {} rows ({} of them driven)",
        tagged.len(),
        MODULE_SOURCES.len(),
        ROWS.len(),
        ROWS.iter().filter(|(.., r)| *r == Reach::Dispatch).count()
    );
}

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

/// A live battle seated the way the **player-Seru** bodies need it: the caster
/// is a party seat aiming into the monster row, every seat up to and including
/// the summon seat is alive, and the summon slot is declared. Those bodies read
/// all three groups, so an under-populated table makes the interesting legs
/// unreachable.
fn seru_world() -> World {
    let mut world = seated_world(3);
    world.actors[0].battle.active_target = 3;
    world.battle_ctx.active_actor = 0;
    world.casting.summon_actor_slot = Some(legaia_engine_vm::cast_module_ticks::SUMMON_SEAT);
    world
}

/// The same battle seated the way the **capture-class arms** need it: the
/// caster is a monster seat aiming at the party row, which is what a
/// capture-class cast is in retail.
fn arm_world() -> World {
    let mut world = seated_world(3);
    world.battle_ctx.active_actor = 3;
    world.actors[3].battle.active_target = 0;
    world.party.inventory.insert(0x20, 3);
    world.party.inventory.insert(0x21, 1);
    world
}

fn seated_world(party_count: u8) -> World {
    let mut world = World {
        party: legaia_engine_core::world::PartyState {
            party_count,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for slot in 0usize..8 {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 4000;
        world.actors[slot].battle.hp = 4000;
        world.actors[slot].battle.mp = 99;
        world.actors[slot].battle.liveness = 1;
        world.actors[slot].battle.anim_rate = legaia_engine_vm::battle_anim_rate::AnimRate(8);
    }
    world
}

/// Everything the band's bodies write, cheaply. "Entered" is then an observed
/// state change rather than a returned flag.
fn fingerprint(w: &World) -> Vec<u64> {
    let mut out = vec![
        w.casting.module_phase as u64,
        w.casting.module_ctx_278 as u64,
        w.battle_ctx.turn_cursor as u64,
    ];
    for a in &w.actors {
        out.push(
            (a.active as u64)
                | (a.battle.hp as u64) << 1
                | (a.battle.mp as u64) << 17
                | (a.battle.field_flags as u64) << 33
                | (a.battle.render_flag as u64) << 49
                | (a.battle.queued_anim as u64) << 57,
        );
        out.push(a.battle.init_key as u64 | (a.battle.anim_rate.0 as u64) << 16);
    }
    out
}

#[test]
fn every_ported_cast_band_body_is_entered_once() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = legaia_asset::cast_effect_pool::CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let Some(entry) = archive.entries.get(idx as usize).cloned() else {
            continue;
        };
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        pool.insert(idx, &bytes);
    }
    let pool = std::sync::Arc::new(pool);
    let mut menu_src = seated_world(3);
    menu_src.install_menu_text(&scus);

    let mut entered = 0usize;
    let mut finished = 0usize;
    let mut changed = 0usize;
    let mut misseated: Vec<(u32, u8)> = Vec::new();
    let mut unfinished: Vec<(u32, u8)> = Vec::new();

    for (module, body, entry, id, reach) in ROWS {
        if reach == Reach::Inner {
            // Reached from inside its row-mate's tick; the dispatch row above
            // covers it, and counting it again would double the numerator.
            continue;
        }
        let seru = module != "cast_arm_ticks";
        let mut w = if seru { seru_world() } else { arm_world() };
        w.menu.text = menu_src.menu.text.clone();
        w.casting.effect_pool = Some(pool.clone());
        if !seru {
            // The byte the capture-class trampoline switches on
            // (`caster[+0x1DF]`). The player-Seru band pages off the spell id
            // argument instead, and seeding the queued-action byte there feeds
            // its phase chain an action it never queued.
            let caster = w.battle_ctx.active_actor as usize;
            w.actors[caster].battle.params[0] = id;
        }
        w.casting.module_phase = 0;
        w.casting.module_ctx_278 = 0;

        // The disc's own spell table has to seat this module for this id; a
        // disagreement is a finding, not a reason to skip the row.
        if w.cast_module_for(id) != Some(entry) {
            misseated.push((entry, id));
            continue;
        }

        let before = fingerprint(&w);
        let mut saw_port = false;
        let mut saw_done = false;
        for _ in 0..MAX_FRAMES {
            // Only the second player-Seru band takes a non-zero stager arm,
            // and only off its rendezvous phases: PROT 0909 parks on two
            // phases until its move-VM stager bumps them, and every other arm
            // of that band's stagers only hands spawn records to the pool.
            // The first band and the capture-class arms take `0` throughout.
            let rendezvous = legaia_engine_vm::cast_seru_ticks_b::VIGURO_RENDEZVOUS_PHASES
                .contains(&w.casting.module_phase);
            let arm = u8::from(module == "cast_seru_ticks_b" && !rendezvous) * 2;
            let Some(run) = w.run_cast_module_code(id, arm) else {
                break;
            };
            assert_eq!(
                run.prot_entry, entry,
                "{module} {body:#010X}: id {id:#04X} paged PROT {} instead",
                run.prot_entry
            );
            // A frame where the band runs a stager arm rather than the tick
            // reports `tick_ported == false` mid-chain; that is a step of the
            // walk, not its end, so only a ported frame that reports not-busy
            // terminates it.
            saw_port |= run.tick_ported;
            if run.tick_ported && !run.busy {
                saw_done = true;
                break;
            }
        }
        assert!(
            saw_port,
            "{module} {body:#010X} (PROT {entry}, id {id:#04X}) never entered a ported body"
        );
        entered += 1;
        if saw_done {
            finished += 1;
        } else {
            unfinished.push((entry, id));
        }
        if fingerprint(&w) != before {
            changed += 1;
        }
    }

    let driven = ROWS.iter().filter(|(.., r)| *r == Reach::Dispatch).count();
    assert!(
        misseated.is_empty(),
        "the disc's spell table seated no module for {misseated:?}"
    );
    assert_eq!(entered, driven, "every dispatch row must enter its body");
    assert_eq!(
        finished, driven,
        "every body must reach a terminal step within {MAX_FRAMES} frames; \
         these did not: {unfinished:?}"
    );
    assert_eq!(
        changed, driven,
        "every body must leave an observable state change (non-vacuity)"
    );
    eprintln!(
        "[ok] cast-band body ladder: {entered}/{driven} dispatch rows entered their body, \
         {finished} reached a terminal step, {changed} changed observable state; \
         {} ported bodies accounted for in {} modules",
        tagged_bodies().len(),
        MODULE_SOURCES.len()
    );
}
