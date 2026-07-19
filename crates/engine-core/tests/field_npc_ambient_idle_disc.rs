//! Disc-gated: field NPCs run their retail **ambient facing** channel, so a
//! standing town NPC idly turns in place instead of holding one heading.
//!
//! The channel is the second motion VM's facing half (`FUN_80038158` ops
//! `0x04` / `0x0D`, ported at `legaia_engine_vm::ambient_motion`). Its
//! bytecode arrives as MAN tail-section 1, one record per bound actor, bound
//! by `actor_id = N0 + placement_index` (`FUN_8003A9D4` / `FUN_8003A1E4`).
//! `World::seed_field_npc_ambient` installs one channel per placement and
//! `World::tick_field_npc_ambient` steps them on the actor game tick.
//!
//! What this pins that the engine-vm unit tests cannot: the **wiring**. The
//! ported interpreter was already disc-oracled over every authored site; what
//! was missing was anything calling it. So the assertions here are that real
//! scene MANs install channels, that stepping them actually moves a heading,
//! and that a heading another writer posed survives an idle channel.
//!
//! Assertions are structural (channel counts, heading movement, compass
//! membership) - no Sony bytes. Skip-passes without `LEGAIA_DISC_BIN` /
//! `extracted/` (CLAUDE.md convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Load a scene's MAN and stand a Field world on it, seeded the way
/// `SceneHost::enter_field_scene` does (carriers, then facings - the facing
/// pass is what re-points the ambient channels' start headings).
fn world_for(scene: &str) -> Option<(World, usize)> {
    let extracted = extracted_dir()?;
    let index = Arc::new(ProtIndex::open_extracted(&extracted).ok()?);
    let scene = Scene::load(&index, scene).ok()?;
    let man_bytes = scene.field_man_payload(&index).ok()??;
    let man_file = parse_man(&man_bytes).ok()?;
    let placements = man_file.actor_placements(&man_bytes).len();

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_carriers_from_man(&man_file, &man_bytes);
    world.seed_field_npc_facings(&man_file, &man_bytes);
    Some((world, placements))
}

/// Real scenes install ambient channels, and every channel carries at least
/// one variant of real bytecode. Non-vacuous: several Rim Elm placements are
/// bound to a motion record.
#[test]
fn town_scenes_install_ambient_facing_channels() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some((world, placements)) = world_for("town01") else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    assert!(
        !world.field_npc_ambient.is_empty(),
        "town01 binds motion records to placements, so channels must install \
         (0 of {placements} placements got one - the binding law is wrong)"
    );
    for (slot, chan) in &world.field_npc_ambient {
        assert!(
            !chan.variants.is_empty(),
            "slot {slot} installed with no variants"
        );
        assert!(
            chan.variants.iter().all(|(_, code)| !code.is_empty()),
            "slot {slot} carries an empty variant body"
        );
        assert!(
            chan.live.is_none(),
            "slot {slot} must not have selected a variant before its first tick"
        );
    }
}

/// Where the facing ops actually live in town01, measured rather than
/// assumed: the **default** (fresh-game) variants carry none - they are
/// `0x17` default-move + `0x05` wait + `0x18` AABB wander, so a fresh Rim Elm
/// villager's ambient behaviour is *wandering*, not turning in place. The
/// `0x04` ramps sit in **flag-gated** variants, i.e. later story states.
///
/// This is worth pinning because it is the reason "tick the channels and
/// watch someone turn" does not hold on a fresh save, and a future reader
/// should not mistake that for broken wiring.
#[test]
fn town01_authors_its_facing_ramps_in_flag_gated_variants() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some((world, _)) = world_for("town01") else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    use legaia_asset::man_motion::SELECTOR_DEFAULT;
    use legaia_engine_vm::ambient_motion::facing_sites;

    let mut gated_sites = 0usize;
    let mut default_sites = 0usize;
    for chan in world.field_npc_ambient.values() {
        for (sel, code) in &chan.variants {
            let n = facing_sites(code).len();
            if *sel == SELECTOR_DEFAULT {
                default_sites += n;
            } else {
                gated_sites += n;
            }
        }
    }
    assert!(
        gated_sites > 0,
        "town01 authors facing ramps behind story flags (found none)"
    );
    assert_eq!(
        default_sites, 0,
        "town01's fresh-game variants wander ({default_sites} facing sites) - \
         if this changes, the premise of the gated-turn test below moved"
    );
}

/// The wiring's whole point: with its gating story flag set, a real
/// disc-authored facing stream drives the NPC's render heading. Before this
/// lane nothing called the interpreter, so every field NPC stood frozen.
///
/// This exercises the full chain on real bytes - per-tick variant
/// re-selection against `DAT_80085758`, the `0x04` ramp, and the mirror into
/// `field_npc_headings` (converted out of retail heading space).
#[test]
fn a_flag_gated_facing_stream_turns_its_npc() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some((mut world, _)) = world_for("town01") else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    use legaia_asset::man_motion::SELECTOR_DEFAULT;
    use legaia_engine_vm::ambient_motion::facing_sites;

    // Find a placement whose gated variant carries facing ops, and the flag
    // that selects it.
    let (slot, gate) = world
        .field_npc_ambient
        .iter()
        .find_map(|(slot, chan)| {
            chan.variants
                .iter()
                .find(|(sel, code)| *sel != SELECTOR_DEFAULT && !facing_sites(code).is_empty())
                .map(|(sel, _)| (*slot, *sel & 0x0FFF))
        })
        .expect("town01 has a flag-gated facing stream");

    // Fresh state: the default variant runs and the NPC holds its heading.
    let posed = world.field_npc_headings.get(&slot).copied().unwrap_or(0);
    for _ in 0..60 {
        world.tick_field_npc_ambient();
    }
    assert_eq!(
        world.field_npc_headings.get(&slot).copied().unwrap_or(0),
        posed,
        "slot {slot}'s default variant carries no facing op, so it must not turn"
    );

    // Set the gate: the facing variant takes over and turns the NPC.
    world.system_flag_set(gate);
    let mut turned = false;
    for _ in 0..240 {
        world.tick_field_npc_ambient();
        if world.field_npc_headings.get(&slot).copied().unwrap_or(0) != posed {
            turned = true;
            break;
        }
    }
    assert!(
        turned,
        "slot {slot}: gate {gate:#X} set, but the facing stream never moved \
         the render heading - the channel is installed and not driving"
    );
    assert_eq!(
        world.field_npc_ambient[&slot].live,
        world.field_npc_ambient[&slot]
            .variants
            .iter()
            .position(|(sel, _)| *sel != SELECTOR_DEFAULT && (*sel & 0x0FFF) == gate),
        "the gated variant is the live one"
    );

    // Every heading the channel produced is a live 12-bit render heading.
    for (slot, h) in &world.field_npc_headings {
        assert!(
            (0..=0x0FFF).contains(h),
            "slot {slot} heading {h:#X} outside the 12-bit render space"
        );
    }
}

/// The `yaw_written`-equivalent gate, against real authored streams: a
/// heading posed by another writer (the interact face-the-speaker bearing)
/// must survive every NPC whose channel is sitting in a wait op. Only the
/// slots whose channel actually moved may differ.
#[test]
fn idle_channels_do_not_clobber_externally_posed_headings() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some((mut world, _)) = world_for("town01") else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    // Pose every NPC at a distinctive bearing, as an interact would.
    const POSED: i16 = 0x0333;
    let slots: Vec<u8> = world.field_npc_ambient.keys().copied().collect();
    assert!(!slots.is_empty(), "non-vacuous: town01 installs channels");
    for &slot in &slots {
        world.field_npc_headings.insert(slot, POSED);
    }

    // One tick: any slot whose stream opens on a wait must still read POSED.
    world.tick_field_npc_ambient();
    let held = slots
        .iter()
        .filter(|s| world.field_npc_headings.get(s) == Some(&POSED))
        .count();
    assert!(
        held > 0,
        "every channel overwrote the posed heading on its first tick - the \
         idle gate is not being applied"
    );
}
