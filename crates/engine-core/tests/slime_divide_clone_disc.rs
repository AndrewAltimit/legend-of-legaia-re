//! Disc-gated: a Green Slime's Divide (spell `0x50`, PROT 0940 `0x801F78B8`)
//! seats its clone where retail seats it.
//!
//! One synthetic fight, a sturdy three-member party against a lone Green
//! Slime (monster `0x07`) whose first turn is seeded to cast Divide. Retail's
//! split arm 0 saves the caster's ground pair into the module word
//! `0x801F865C`; arm 2 shakes the caster off it every pass and, once its
//! countdown runs out, pushes the word an eighth of the trig scale along the
//! caster's side axis and hands it to `FUN_80024C88` as the clone's spawn
//! point. So the two halves end up apart, side by side, both facing the
//! party, and neither carries the jiggle's render flag.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` or the extracted archive.

use std::path::PathBuf;

use legaia_actor_formats::monster_archive;
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, catalog_from_monster_archive,
};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;

/// Green Slime, the first monster whose AI picks Divide.
const GREEN_SLIME: u16 = 0x07;
/// Divide.
const DIVIDE: u8 = 0x50;

fn prot(name: &str) -> Option<Vec<u8>> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        dirs.push(PathBuf::from(d).join("PROT"));
    }
    dirs.push(PathBuf::from("../../extracted/PROT"));
    dirs.push(PathBuf::from("extracted/PROT"));
    dirs.into_iter()
        .find_map(|d| std::fs::read(d.join(name)).ok())
}

fn scus() -> Option<Vec<u8>> {
    use legaia_engine_scene::vfs::Vfs;
    let path = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    legaia_engine_scene::vfs::DiscVfs::open(&path)
        .ok()?
        .read("SCUS_942.54")
        .ok()
}

/// A booted fight against one Green Slime whose first pick is Divide.
fn setup() -> Option<(World, usize)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let Some(entry) = prot("0867_battle_data.BIN") else {
        eprintln!("[skip] extracted PROT 0867 missing");
        return None;
    };
    let scus = scus()?;
    let id = GREEN_SLIME;
    let cat = catalog_from_monster_archive(&entry, &[id]);
    cat.get(id)?;
    let clips = monster_archive::animations_by_entry(&entry, id)
        .ok()
        .flatten()?;
    let mut w = World::new();
    w.set_spell_catalog(legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(&scus)?);
    w.install_menu_text(&scus);
    if let Some(overlay) = prot("0898_xxx_dat.BIN") {
        w.tables.move_power =
            legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&overlay);
    }
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.party.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 9999;
        hms.hp_max = 9999;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 9999;
        w.actors[i].battle.max_hp = 9999;
        w.actors[i].battle.liveness = 1;
        w.battle.set_battle_attack(i as u8, 1);
        w.battle.set_battle_defense(i as u8, 20);
    }
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(id)]));
    w.set_formation_table(table, cat);
    let seat = usize::from(w.party.party_count);
    w.actors[seat].battle_monster_id = Some(id);
    w.set_actor_battle_action_clips(seat, std::sync::Arc::new(clips));
    w.battle.forced_monster_cast = Some((seat as u8, DIVIDE));
    w.mode = SceneMode::Field;
    if !w.trigger_scripted_battle(1) {
        return None;
    }
    for _ in 0..300 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    (w.mode == SceneMode::Battle && w.actors[seat].battle_monster_id == Some(id))
        .then_some((w, seat))
}

/// What the cast left behind.
#[derive(Debug)]
struct Split {
    /// The caster's ground pair when its turn opened.
    start: (i32, i32),
    /// The caster's ground pair once the cast ended.
    caster: (i32, i32),
    /// The clone's ground pair as it was seated.
    clone_seated: (i32, i32),
    /// The clone's ground pair once the cast ended.
    clone: (i32, i32),
    caster_facing: u16,
    clone_facing: u16,
    clone_render_flag: u8,
    caster_render_flag: u8,
    /// The farthest the caster strayed from `start` while the band shook it.
    shake: i32,
    /// Ticks the caster spent under the shake flag with a zero colour word -
    /// the engine's reading of retail's hidden, blacked-out body.
    black_ticks: u32,
    /// The clone's colour word on the tick it was seated.
    clone_colour: u32,
}

fn run() -> Option<Split> {
    let (mut w, seat) = setup()?;
    let clone = seat + 1;
    let pair = |a: &Actor| {
        (
            i32::from(a.move_state.world_x),
            i32::from(a.move_state.world_z),
        )
    };
    let mut start = None;
    let mut clone_seated = None;
    let mut shake = 0;
    let mut acted = false;
    let mut black_ticks = 0;
    let mut clone_colour = 0;
    for _ in 0..4000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
        let acting =
            usize::from(w.battle_ctx.active_actor) == seat && w.battle_ctx.queued_action == 2;
        if acting && start.is_none() {
            start = Some(pair(&w.actors[seat]));
        }
        if let Some(s) = start
            && clone_seated.is_none()
        {
            let p = pair(&w.actors[seat]);
            shake = shake.max((p.0 - s.0).abs() + (p.1 - s.1).abs());
        }
        if clone_seated.is_none() && w.actors[clone].battle_monster_id == Some(GREEN_SLIME) {
            clone_seated = Some(pair(&w.actors[clone]));
            clone_colour = w.actors[clone].battle.render_color;
        }
        let c = &w.actors[seat].battle;
        if c.render_flag != 0 && c.render_color == 0 {
            black_ticks += 1;
        }
        acted |= acting;
        if acted && !acting {
            break;
        }
    }
    let (c, k) = (&w.actors[seat], &w.actors[clone]);
    Some(Split {
        start: start?,
        caster: pair(c),
        clone_seated: clone_seated?,
        clone: pair(k),
        caster_facing: c.battle.facing_angle,
        clone_facing: k.battle.facing_angle,
        clone_render_flag: k.battle.render_flag,
        caster_render_flag: c.battle.render_flag,
        shake,
        black_ticks,
        clone_colour,
    })
}

#[test]
fn a_divide_clone_stands_beside_its_caster() {
    let Some(s) = run() else {
        return;
    };
    eprintln!("{s:?}");
    // The synthetic fight seats the slime facing the party (`0x800`), whose
    // side axis is `-x`: the cosine view reads `-4095` there, so the push is
    // `4095 / 8 = 511` units along x and nothing along z.
    assert_eq!(s.caster_facing, 0x800, "{s:?}");
    assert_eq!(s.clone_seated.1, s.start.1, "{s:?}");
    assert_eq!((s.clone_seated.0 - s.start.0).abs(), 511, "{s:?}");
    // The caster is left on its last shake, at the far side of its old seat.
    let (caster_dx, clone_dx) = (s.caster.0 - s.start.0, s.clone.0 - s.start.0);
    assert!(
        caster_dx.signum() == -clone_dx.signum() && caster_dx.abs() >= 0x1F0,
        "the halves stand apart across the old seat: {s:?}"
    );
    assert_eq!(s.caster.1, s.start.1, "{s:?}");
    assert_eq!(s.clone_facing, s.caster_facing, "{s:?}");
    assert_eq!(s.clone_render_flag, 0, "{s:?}");
    assert_eq!(s.caster_render_flag, 0, "{s:?}");
    // Seated full white, eased at most a step or two toward neutral.
    assert!(
        s.clone_colour & 0x3FF > 0x300,
        "the clone is seated white: {:#X}",
        s.clone_colour
    );
    assert!(s.shake >= 0x1F0, "the band shakes the caster: {s:?}");
    assert_eq!(
        s.black_ticks, 0,
        "the shake whitens, never blacks out: {s:?}"
    );
}
