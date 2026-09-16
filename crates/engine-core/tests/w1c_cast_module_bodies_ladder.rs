//! Disc-gated **cast-module body ladder**: every ported body in
//! `legaia_engine_vm::cast_module_ticks`, reached the way its own dispatch
//! reaches it, with the denominator taken from the sources.
//!
//! Why it exists. `cast_module_ticks.rs` is the one cast-band module whose
//! bodies no union member accounts for by name.
//! `w2c_cast_band_body_ladder` scrapes the other **three** band modules
//! (`cast_seru_ticks_a` / `cast_seru_ticks_b` / `cast_arm_ticks`) and this file
//! is deliberately not among them; `w4d_cast_band_ladder` walks the band one
//! frame deep but seats **one representative id per PROT entry**, which is
//! blind by construction to a module holding several choreographies - PROT
//! 0955 holds six, and five of them sit behind ids the representative is not.
//! So those bodies never run, however far a route ladder plays.
//!
//! Three reach kinds, because the module half has three dispatch seams and a
//! row that names the wrong one passes without entering anything:
//!
//! * `Reach::Trampoline` - `World::run_cast_module_code(action_id, arm)` with
//!   the caster's queued action id seeded, which is the byte
//!   `capture_tick_body` switches on (`caster[+0x1DF]`);
//! * `Reach::Entry` - the same call for a module with **no** trampoline, where
//!   the `0x801CF4EC` / `0x801CF56C` arm points straight at the body;
//! * `Reach::Aoe` - `World::run_cast_module_aoe`, the seam
//!   `fold_pending_cast` takes for PROT 0927 and PROT 0966, whose damage lands
//!   inside the module's own sweep stager rather than in the generic fold.
//!
//! The denominator is the **sources**: `tagged_addresses` scrapes every
//! `// PORT:` address out of `cast_module_ticks.rs`, which `include_str!`
//! pulls in at compile time, and `ROWS` plus `TRAMPOLINE_TABLE` have to
//! account for every one. Adding a body without adding a row fails
//! `the_row_table_accounts_for_every_ported_body` with no disc present - the
//! same property that makes `w2c_cast_band_body_ladder` unable to go stale.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::cast_module_ticks as ticks;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// The module source, scraped for its `// PORT:` addresses at compile time.
const MODULE_SOURCE: &str = include_str!("../../engine-vm/src/cast_module_ticks.rs");

/// How a row's body is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// `run_cast_module_code`, with `action_id` seeded into `caster[+0x1DF]`;
    /// the module's trampoline picks the body.
    Trampoline,
    /// `run_cast_module_code` for a module with no trampoline - the band arm
    /// names the body directly, so any id that pages the entry reaches it.
    Entry,
    /// `run_cast_module_aoe` - the whole-row sweep stager, the seam
    /// `fold_pending_cast` uses for the two modules that own their own damage.
    Aoe,
    /// `run_cast_module_code`'s **staging** call, which runs before the tick
    /// match and unconditionally for its entry. A stager is not a tick body -
    /// it reports no `CastTickStep`, so `tick_ported` stays false for a module
    /// that carries only a stager (PROT 0923), and "entered" has to be the
    /// state the stager itself writes.
    Stager,
}

/// One dispatch site of the module half.
struct Row {
    /// Every address the site's `// PORT:` marker names. A routine ported as a
    /// table walk carries its arm VAs on the same marker (PROT 0949's freeze
    /// ramp is eight addresses on one body), so a row is a site, not an
    /// address.
    addrs: &'static [u32],
    /// The band entry the site belongs to.
    entry: u32,
    /// The queued action id the trampoline reads, for a `Trampoline` row. A
    /// `Entry` / `Aoe` row leaves it `None` and the ladder asks the engine's
    /// own resolver which spell ids page the entry, so the row tracks the
    /// disc's spell table rather than a copy of it.
    action_id: Option<u8>,
    /// The body VA the trampoline is expected to name - checked against
    /// `capture_tick_body` before the row is driven, so a row that names the
    /// wrong pair fails loudly instead of entering some other choreography.
    body: Option<u32>,
    reach: Reach,
}

const fn row(
    addrs: &'static [u32],
    entry: u32,
    action_id: Option<u8>,
    body: Option<u32>,
    reach: Reach,
) -> Row {
    Row {
        addrs,
        entry,
        action_id,
        body,
        reach,
    }
}

/// The trampoline table itself: a `const` anchor whose six addresses are the
/// trampolines every [`Reach::Trampoline`] row is dispatched through. It has no
/// dispatch site of its own - `capture_tick_body` reads it once per row - so it
/// is credited when any trampoline row entered.
const TRAMPOLINE_TABLE: [u32; 6] = [
    0x801F_7A40,
    0x801F_7B1C,
    0x801F_7B28,
    0x801F_816C,
    0x801F_8E60,
    0x801F_92A4,
];

/// The state each inline stager alone produces, keyed on the row's first
/// address - so a `Stager` row is credited on an observed write rather than on
/// the call having been made.
///
/// PROT 0906's Gizam stager is the one with no probe: both of its state arms
/// write fields the cast-band seam does not carry back out of the view
/// (`+0x21C` is already the default and `+0x0C` has no actor mirror), so its
/// credit is the unconditional call site - the staging match runs before the
/// tick match with no branch between entering the seam and the call.
type StagerProbe = (u32, fn(&World) -> bool);

const STAGER_PROBES: [StagerProbe; 4] = [
    // PROT 0949's freeze ramp writes the victim's animation rate to `7 - arm`.
    (0x801F_75BC, |w| w.actors[0].battle.anim_rate.0 == 7),
    // PROT 0922 / PROT 0923 both write `ctx[+0x278] = 3` on arm 0.
    (0x801F_90E4, |w| w.casting.module_ctx_278 == 3),
    (0x801F_8B90, |w| w.casting.module_ctx_278 == 3),
    // PROT 0909's arm 0 puts the summon seat on the enemy-row group code.
    (0x801F_7AF4, |w| {
        w.actors[ticks::SUMMON_SEAT as usize].battle.active_target == ticks::TARGET_CODE_ENEMY_ROW
    }),
];

/// Every ported dispatch site in `cast_module_ticks.rs`.
const ROWS: [Row; 34] = [
    // --- the five state-touching stagers the band arm runs inline ----------
    row(
        &[
            0x801F_75BC,
            0x801F_7630,
            0x801F_7644,
            0x801F_7658,
            0x801F_766C,
            0x801F_7680,
            0x801F_7694,
            0x801F_76A8,
        ],
        949,
        None,
        None,
        Reach::Stager,
    ),
    row(&[0x801F_90E4], 922, None, None, Reach::Stager),
    row(&[0x801F_8B90], 923, None, None, Reach::Stager),
    row(&[0x801F_7740], 906, None, None, Reach::Stager),
    row(&[0x801F_7AF4], 909, None, None, Reach::Stager),
    // --- the two whole-row AoE sweep stagers -------------------------------
    row(&[0x801F_85A8], 927, None, None, Reach::Aoe),
    row(&[0x801F_8D64], 966, None, None, Reach::Aoe),
    // --- the six bodies a module with no trampoline names directly ---------
    row(&[0x801F_6A00], 925, None, None, Reach::Entry),
    row(&[0x801F_6A18], 924, None, None, Reach::Entry),
    row(&[0x801F_6A3C], 922, None, None, Reach::Entry),
    row(&[0x801F_6A84], 927, None, None, Reach::Entry),
    row(&[0x801F_6C70], 918, None, None, Reach::Entry),
    row(&[0x801F_6A10], 949, None, None, Reach::Entry),
    // --- the twenty-one trampoline-reached bodies --------------------------
    row(
        &[0x801F_726C],
        938,
        Some(0x4E),
        Some(0x801F_726C),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_69EC],
        938,
        Some(0xB7),
        Some(0x801F_69EC),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_7D34],
        942,
        Some(0x52),
        Some(0x801F_7D34),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_6EDC],
        945,
        Some(0x54),
        Some(0x801F_6EDC),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_69F8],
        945,
        Some(0xBA),
        Some(0x801F_69F8),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_6A20],
        951,
        Some(0x36),
        Some(0x801F_6A20),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_77E8],
        951,
        Some(0x5B),
        Some(0x801F_77E8),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_7118],
        952,
        Some(0x5C),
        Some(0x801F_7118),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_6A0C],
        952,
        Some(0xB8),
        Some(0x801F_6A0C),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_8F0C],
        955,
        Some(0x60),
        Some(0x801F_8F0C),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_86A4],
        955,
        Some(0x6E),
        Some(0x801F_86A4),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_7FA4],
        955,
        Some(0x6F),
        Some(0x801F_7FA4),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_767C],
        955,
        Some(0x70),
        Some(0x801F_767C),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_7158],
        955,
        Some(0x72),
        Some(0x801F_7158),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_6A28],
        955,
        Some(0x73),
        Some(0x801F_6A28),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_798C],
        957,
        Some(0x76),
        Some(0x801F_798C),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_6A14],
        957,
        Some(0x77),
        Some(0x801F_6A14),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_6DD8],
        958,
        Some(0x79),
        Some(0x801F_6DD8),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_74E4],
        960,
        Some(0x7B),
        Some(0x801F_74E4),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_88EC],
        964,
        Some(0xAF),
        Some(0x801F_88EC),
        Reach::Trampoline,
    ),
    row(
        &[0x801F_69D8],
        965,
        Some(0xB6),
        Some(0x801F_69D8),
        Reach::Trampoline,
    ),
];

/// The eleven addresses the reach report carries as *live but never entered*
/// for this module, which is the reason this ladder exists. Asserted as a
/// subset of what the walk entered, so the conversion claim is checked rather
/// than stated.
const NEVER_ENTERED: [u32; 11] = [
    0x801F_69F8,
    0x801F_6A0C,
    0x801F_6A14,
    0x801F_6A28,
    0x801F_7158,
    0x801F_767C,
    0x801F_77E8,
    0x801F_7FA4,
    0x801F_85A8,
    0x801F_86A4,
    0x801F_8D64,
];

/// Frames to drive one body before giving up: past every phase chain in the
/// module (the longest bound is sixteen) with room for the clip holds.
const MAX_FRAMES: usize = 96;

/// Every `FUN_801Fxxxxxxxx` address on a `// PORT:` marker in the module source.
fn tagged_addresses() -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    for line in MODULE_SOURCE.lines() {
        if !line.trim_start().starts_with("//") {
            continue;
        }
        let Some(rest) = line.split_once("PORT:").map(|(_, r)| r) else {
            continue;
        };
        let mut cursor = rest;
        while let Some(at) = cursor.find("FUN_") {
            let hex: String = cursor[at + 4..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if hex.len() == 8
                && let Ok(addr) = u32::from_str_radix(&hex, 16)
            {
                out.insert(addr);
            }
            cursor = &cursor[at + 4..];
        }
    }
    out
}

/// Every address the row table plus the trampoline `const` accounts for.
fn rowed_addresses() -> BTreeSet<u32> {
    let mut rowed: BTreeSet<u32> = TRAMPOLINE_TABLE.iter().copied().collect();
    for r in &ROWS {
        rowed.extend(r.addrs.iter().copied());
    }
    rowed
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

/// A live battle with a full party row and three monster seats.
///
/// The shape matters: the whole-row sweeps bound on `ctx[+0]` (the party count)
/// and `ctx[+1]` (the live monster count), and the scoped bodies branch on the
/// caster's `+0x1DD`, so an under-populated table makes the arms this ladder
/// exists for unreachable while every assertion still passes.
fn battle_world() -> World {
    let mut world = World {
        party: legaia_engine_core::world::PartyState {
            party_count: 3,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for slot in 0usize..6 {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 4000;
        world.actors[slot].battle.hp = 4000;
        world.actors[slot].battle.mp = 77;
        world.actors[slot].battle.liveness = 1;
        world.actors[slot].battle.anim_rate = legaia_engine_vm::battle_anim_rate::AnimRate(8);
    }
    // A monster seat casting at the party row - what a capture-class cast is.
    world.battle_ctx.active_actor = 3;
    world.actors[3].battle.active_target = 0;
    world.party.inventory.insert(0x20, 3);
    world.party.inventory.insert(0x21, 1);
    world
}

/// A fingerprint of everything the module bodies write, so "entered" is an
/// observed state change rather than a returned flag.
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

/// Seat the disc's module pool and menu text on a fresh battle world.
fn seeded_base(dir: &std::path::Path) -> World {
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let mut base = battle_world();
    base.install_menu_text(&scus);
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
    base.casting.effect_pool = Some(std::sync::Arc::new(pool));
    base
}

/// `PROT entry -> every spell id the engine's own resolver pages it with`.
fn ids_by_entry(base: &World) -> BTreeMap<u32, Vec<u8>> {
    let mut map: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for id in 0u8..=0xFF {
        if let Some(entry) = base.cast_module_for(id) {
            map.entry(entry).or_default().push(id);
        }
    }
    map
}

/// Disc-free: the row table has to account for every tagged address, and no
/// row may name an address the module does not tag.
#[test]
fn the_row_table_accounts_for_every_ported_body() {
    let tagged = tagged_addresses();
    assert!(
        tagged.len() > 40,
        "the scrape found only {} addresses - it did not read the module",
        tagged.len()
    );
    let rowed = rowed_addresses();
    let missing: Vec<String> = tagged
        .difference(&rowed)
        .map(|a| format!("{a:#010X}"))
        .collect();
    assert!(
        missing.is_empty(),
        "cast_module_ticks.rs tags bodies this ladder has no row for: {missing:?}"
    );
    let extra: Vec<String> = rowed
        .difference(&tagged)
        .map(|a| format!("{a:#010X}"))
        .collect();
    assert!(
        extra.is_empty(),
        "the row table names addresses the module does not tag: {extra:?}"
    );
    for addr in NEVER_ENTERED {
        assert!(
            rowed.contains(&addr),
            "{addr:#010X} is on the never-entered list and has no row"
        );
    }
    eprintln!(
        "[ok] cast-module body accounting: {} tagged addresses over {} rows",
        tagged.len(),
        ROWS.len()
    );
}

/// The walk: every row's body entered through its own dispatch seam.
#[test]
fn every_ported_cast_module_body_is_entered_through_its_own_seam() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let base = seeded_base(&dir);
    let by_entry = ids_by_entry(&base);
    assert!(
        !by_entry.is_empty(),
        "the resolver paged no band entry at all - the spell table did not install"
    );

    let mut entered: BTreeSet<u32> = BTreeSet::new();
    let mut unseated: Vec<String> = Vec::new();
    let mut changed = 0usize;
    let mut finished = 0usize;
    let mut aoe_hits_total = 0usize;
    let mut trampoline_rows = 0usize;

    for r in &ROWS {
        let entry = r.entry;
        // Which id to cast. A trampoline row names its own; an entry / AoE row
        // takes whichever id the disc's spell table pages that module with.
        let id = match r.action_id {
            Some(id) => {
                if base.cast_module_for(id) != Some(entry) {
                    unseated.push(format!("PROT {entry} id {id:#04X} (trampoline arm)"));
                    continue;
                }
                assert_eq!(
                    ticks::capture_tick_body(entry, id),
                    r.body,
                    "PROT {entry} id {id:#04X}: the trampoline map names a different body"
                );
                id
            }
            None => match by_entry.get(&entry).and_then(|v| v.first()).copied() {
                Some(id) => id,
                None => {
                    unseated.push(format!("PROT {entry} (no id pages it)"));
                    continue;
                }
            },
        };

        let mut w = battle_world();
        w.menu.text = base.menu.text.clone();
        w.casting.effect_pool = base.casting.effect_pool.clone();
        let caster = w.battle_ctx.active_actor as usize;
        // The byte the trampoline reads.
        w.actors[caster].battle.params[0] = id;
        let before = fingerprint(&w);

        let mut saw_port = false;
        match r.reach {
            Reach::Aoe => {
                // The stager's nine arms, driven in order: the sweep lands on
                // the working arm and the rest are the ramp.
                for arm in 0u8..9 {
                    let Some(run) = w.run_cast_module_aoe(id, arm) else {
                        continue;
                    };
                    assert_eq!(
                        run.prot_entry, entry,
                        "id {id:#04X} swept PROT {} rather than {entry}",
                        run.prot_entry
                    );
                    saw_port |= run.tick_ported;
                    aoe_hits_total += run.aoe_hits.len();
                }
                finished += usize::from(saw_port);
            }
            Reach::Stager => {
                // The stager is called once per `run_cast_module_code`, before
                // the tick match, with the arm the caller passes - so the walk
                // is over arms, and each arm gets a fresh world so an earlier
                // arm's writes cannot stand in for this one's.
                let probe = STAGER_PROBES
                    .iter()
                    .find(|(a, _)| *a == r.addrs[0])
                    .map(|(_, f)| *f);
                let mut paged = false;
                for arm in 0u8..8 {
                    let mut wa = battle_world();
                    wa.menu.text = base.menu.text.clone();
                    wa.casting.effect_pool = base.casting.effect_pool.clone();
                    wa.actors[caster].battle.params[0] = id;
                    let Some(run) = wa.run_cast_module_code(id, arm) else {
                        continue;
                    };
                    assert_eq!(
                        run.prot_entry, entry,
                        "id {id:#04X} paged PROT {} rather than {entry}",
                        run.prot_entry
                    );
                    paged = true;
                    match probe {
                        Some(f) => saw_port |= f(&wa),
                        None => saw_port = true,
                    }
                    if saw_port {
                        // Carry the arm that fired into the outer world so the
                        // fingerprint comparison sees it.
                        w = wa;
                        break;
                    }
                }
                assert!(
                    paged,
                    "PROT {entry} id {id:#04X}: the module never paged, so the staging \
                     call was never reached"
                );
            }
            Reach::Trampoline | Reach::Entry => {
                for _ in 0..MAX_FRAMES {
                    let Some(run) = w.run_cast_module_code(id, 0) else {
                        break;
                    };
                    assert_eq!(
                        run.prot_entry, entry,
                        "id {id:#04X} paged PROT {} rather than {entry}",
                        run.prot_entry
                    );
                    if !run.tick_ported {
                        break;
                    }
                    saw_port = true;
                    if !run.busy {
                        finished += 1;
                        break;
                    }
                }
            }
        }

        assert!(
            saw_port,
            "PROT {entry} id {id:#04X} ({:#010X}, {:?}) never entered a ported body",
            r.addrs[r.addrs.len() - 1],
            r.reach
        );
        entered.extend(r.addrs.iter().copied());
        if r.reach == Reach::Trampoline {
            trampoline_rows += 1;
        }
        if fingerprint(&w) != before {
            changed += 1;
        }
    }

    assert!(
        unseated.is_empty(),
        "the disc spell table pages no module for {unseated:?} - the row table and \
         `cast_module_for` disagree"
    );
    // The trampoline `const` is read once per trampoline row, so it is
    // credited only when one of them ran.
    assert!(
        trampoline_rows > 0,
        "no trampoline row ran - the trampoline table was never read"
    );
    entered.extend(TRAMPOLINE_TABLE.iter().copied());

    // Non-vacuity, three ways: every row ran, most rows left a state change,
    // and the two AoE stagers actually swept seats rather than returning an
    // empty hit list.
    let rowed = rowed_addresses();
    assert_eq!(
        entered,
        rowed,
        "some rows did not enter: {:?}",
        rowed
            .difference(&entered)
            .map(|a| format!("{a:#010X}"))
            .collect::<Vec<_>>()
    );
    assert!(
        changed >= ROWS.len() / 2,
        "only {changed} of {} rows changed observable state - the walk is vacuous",
        ROWS.len()
    );
    assert!(
        aoe_hits_total > 0,
        "the two whole-row stagers swept no seat - `damage_shape_for` declined both"
    );

    // The conversion claim, checked: every address the reach report carries as
    // never-entered for this module is in the entered set.
    let missed: Vec<String> = NEVER_ENTERED
        .iter()
        .filter(|a| !entered.contains(a))
        .map(|a| format!("{a:#010X}"))
        .collect();
    assert!(
        missed.is_empty(),
        "never-entered addresses this ladder is here to convert were not entered: {missed:?}"
    );

    eprintln!(
        "[ok] cast-module body ladder: {}/{} rows entered ({} addresses), {changed} changed \
         state, {finished} reached a terminal step, {aoe_hits_total} sweep hits; \
         all {} never-entered addresses converted",
        ROWS.len(),
        ROWS.len(),
        entered.len(),
        NEVER_ENTERED.len()
    );
}
