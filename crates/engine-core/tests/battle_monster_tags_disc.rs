//! Disc-gated: two battle mechanics that read a monster record's own bytes,
//! driven through `World::tick` on real PROT 0867 records.
//!
//! 1. **Tagged approach clips.** Out of reach, the action SM stages a
//!    monster's tag-`0x20` pre-approach entry (`FUN_80050E2C` at
//!    `0x801E3268`), falls back to its tag-`1` walk and the `0x19` short step
//!    when it carries none, and stages the tag-`0x21` close-in on arrival -
//!    each an entry INDEX found by tag, not a literal id.
//! 2. **The killing-blow Seru absorb.** A party member's killing blow on a
//!    monster whose record `+0x3E` names a Seru rolls record `+0x3F`
//!    (`FUN_801EC3E4` `0x801EE1C0..0x801EE2E8`), and the Done band teaches the
//!    spell in the same action (`FUN_801E92DC`).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (disc-gated convention).

use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, catalog_from_monster_archive,
};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::ActionState;
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

fn archive() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    let disc = std::fs::read(path).ok()?;
    DiscPatcher::open(disc)
        .ok()?
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .ok()
}

/// A three-member party against one real monster `id`, with its action clips
/// installed positionally the way both play hosts install them.
fn world_vs(
    archive: &[u8],
    id: u16,
    party_hp: u16,
    prep: impl FnOnce(&mut legaia_save::Party, &mut legaia_engine_core::monster_catalog::MonsterCatalog),
) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.party.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = party_hp;
        hms.hp_max = party_hp;
        rec.set_hp_mp_sp(hms);
    }
    let mut cat = catalog_from_monster_archive(archive, &[id]);
    assert!(cat.get(id).is_some(), "monster {id} decodes");
    prep(&mut party, &mut cat);
    w.load_party(party);
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = party_hp;
        w.actors[i].battle.max_hp = party_hp;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
        w.set_battle_defense(i as u8, 30);
    }
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(id)]));
    w.set_formation_table(table, cat);
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(1));
    for _ in 0..300 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    let clips = legaia_asset::monster_archive::animations_by_entry(archive, id)
        .expect("clips decode")
        .expect("clips present");
    w.set_actor_battle_action_clips(3, std::sync::Arc::new(clips));
    w
}

fn tags(archive: &[u8], id: u16) -> Vec<u8> {
    legaia_asset::monster_archive::action_tags(archive, id)
        .expect("tags decode")
        .expect("tags present")
}

/// Monster 73 carries its pre-approach (`0x20`) and close-in (`0x21`) at
/// entries well past `1`; the windup chain must stage those indices.
#[test]
fn a_monster_with_a_pre_approach_stages_its_tagged_entries() {
    let Some(archive) = archive() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    const ID: u16 = 73;
    let t = tags(&archive, ID);
    let pre = t.iter().position(|&x| x == 0x20).expect("tag 0x20") as u8;
    let close = t.iter().position(|&x| x == 0x21).expect("tag 0x21") as u8;
    assert!(pre > 1 && close > 1, "indices past the literal walk id");
    let mut w = world_vs(&archive, ID, 9000, |_, _| {});
    let (mut saw_pre, mut saw_close) = (false, false);
    for _ in 0..60_000 {
        w.tick();
        if w.mode != SceneMode::Battle || (saw_pre && saw_close) {
            break;
        }
        if w.battle_ctx.active_actor != 3 {
            continue;
        }
        let q = w.actors[3].battle.queued_anim;
        match ActionState::from_byte(w.battle_ctx.action_state) {
            Some(ActionState::AttackWindup) if q == pre => saw_pre = true,
            Some(ActionState::AttackCloseRange) if q == close => saw_close = true,
            _ => {}
        }
    }
    eprintln!(
        "[ok] monster {ID}: pre-approach entry {pre} staged={saw_pre}, close-in entry {close} staged={saw_close}"
    );
    assert!(saw_pre, "the tag-0x20 entry was never staged in the windup");
    assert!(saw_close, "the tag-0x21 entry was never staged on arrival");
}

/// A record with no tag-`0x20` entry never enters the windup chain.
#[test]
fn a_monster_without_a_pre_approach_takes_the_short_step() {
    let Some(archive) = archive() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    const ID: u16 = 4;
    let t = tags(&archive, ID);
    assert!(!t.contains(&0x20), "monster {ID} carries no pre-approach");
    let walk = t.iter().position(|&x| x == 1).expect("tag 1") as u8;
    let mut w = world_vs(&archive, ID, 9000, |_, _| {});
    let (mut short_step, mut windup) = (false, false);
    for _ in 0..40_000 {
        w.tick();
        if w.mode != SceneMode::Battle || short_step {
            break;
        }
        if w.battle_ctx.active_actor != 3 {
            continue;
        }
        match ActionState::from_byte(w.battle_ctx.action_state) {
            Some(ActionState::AttackShortStep) => {
                assert_eq!(w.actors[3].battle.queued_anim, walk, "the tag-1 walk");
                short_step = true;
            }
            Some(ActionState::AttackWindup) => windup = true,
            _ => {}
        }
    }
    eprintln!("[ok] monster {ID}: short step with walk entry {walk} = {short_step}");
    assert!(short_step && !windup);
}

/// A killing blow on a Seru monster, at a certain chance, teaches its spell
/// in the same battle to the member wearing a Ra-Seru.
#[test]
fn a_killing_blow_absorbs_the_monsters_seru() {
    let Some(archive) = archive() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let records = legaia_asset::monster_archive::records(&archive).expect("archive");
    let rec = records
        .iter()
        .find(|r| r.seru_id != 0 && r.hp > 0)
        .expect("a Seru-carrying record");
    let (id, seru) = (rec.id, rec.seru_id);
    let mut w = world_vs(&archive, id, 9000, |party, cat| {
        // Every party member wears a Ra-Seru, so whoever lands the blow can
        // absorb: roster id 2 (Noa) reads equipment slot 2, the others slot 3
        // (`FUN_80053CB8`).
        for (slot, member) in party.members.iter_mut().enumerate() {
            let mut eq = member.equipment();
            eq.slots[if slot == 1 { 2 } else { 3 }] = 0x30;
            member.set_equipment(eq);
        }
        // A certain roll, and one hit kills.
        let mut def = cat.get(id).cloned().expect("def");
        def.absorb_chance_pct = 100;
        def.hp = 1;
        cat.insert(def);
    });
    let spell = seru.wrapping_add(0x80);
    let knows = |w: &World| {
        w.party.roster.members.iter().any(|m| {
            let l = m.spell_list();
            l.ids[..usize::from(l.count)].contains(&spell)
        })
    };
    assert!(!knows(&w));
    let mut learned = false;
    for _ in 0..60_000 {
        w.tick();
        if knows(&w) {
            learned = true;
            break;
        }
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    eprintln!("[ok] monster {id} (Seru {seru}): spell {spell:#04x} learned = {learned}");
    assert!(learned, "the killing blow's absorbed Seru was never taught");
}
