//! Disc-gated: Gaza's **Astral Slash** (`0xB8`, PROT 0952) deals its damage
//! through the melee kernel, not through a capture-class wrapper.
//!
//! The module's tick body `0x801F6A0C` has no `jal` into `FUN_801DD0AC` /
//! `FUN_801DD4B0` / `FUN_801DD6B4` and no HP store. Its arm 2 stages clip
//! `0x0A` on the caster (`sb v0,0x1da(s0)` at `0x801F6D88`) and parks the
//! battle's strike cursor (`ctx[+0x15] = 0xFF` at `0x801F6E78`). The anim
//! tick `FUN_80047430` calls `FUN_801EC3E4(actor, playing entry, cursor >> 4)`
//! every frame (`0x800478A0`), and Gaza's archive entry `0x0A` is a four-beat
//! attack-band entry (power bytes `0D 0E 0F 10`, beats `14 15 16 17`), so the
//! kernel rolls four melee hits and, the cursor parked, writes the total into
//! live HP on the last beat (`0x801EE9A4..0x801EE9EC`).
//!
//! What this pins on the live battle path: the party seat's HP loss is the sum
//! of the four clip hits and nothing more (no wrapper roll folded on top), and
//! it lands on the fourth hit's frame, not at the band's exit.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_battle::monster_catalog::{
    catalog_from_monster_archive, vanilla_formation_table,
};
use legaia_engine_battle::move_power::MovePowerCatalog;
use legaia_engine_battle::spells::{
    SpellCatalog, SpellDef, SpellEffect, SpellElement, SpellTarget,
};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use std::path::PathBuf;

/// Gaza, first fight (light element) - the one record whose magic list holds
/// Astral Slash.
const GAZA: u16 = 165;
const ASTRAL_SLASH: u8 = 0xB8;
/// Battle-action state `0x70`: the capture band ticking the module.
const CAPTURE_TICK_STATE: u8 = 0x70;
const PARTY: u8 = 3;
const CASTER: usize = 3;

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

fn prot_entry(dir: &std::path::Path, index: usize) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries[index].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .unwrap_or_else(|_| panic!("read PROT {index}"));
    bytes
}

#[test]
fn astral_slash_lands_the_clip_melee_total_on_the_last_beat() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let overlay = prot_entry(
        &dir,
        legaia_game_tables::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
    );
    let archive = prot_entry(&dir, 867);
    let ids: Vec<u16> = (1u16..=0x1FF).collect();
    let catalog = catalog_from_monster_archive(&archive, &ids);
    assert_eq!(
        catalog.get(GAZA).map(|d| d.magic_attacks.clone()),
        Some(vec![ASTRAL_SLASH]),
        "monster {GAZA} is the Astral Slash caster"
    );
    let clips = legaia_actor_formats::monster_archive::animations_by_entry(&archive, GAZA)
        .expect("archive decodes")
        .expect("Gaza has action entries");
    let clip = clips[0x0A].as_ref().expect("entry 0x0A decodes");
    assert_eq!(clip.entry_power_run(), Some([0x0D, 0x0E, 0x0F, 0x10]));
    assert_eq!(clip.entry_event_frames(), Some([0x14, 0x15, 0x16, 0x17]));

    let mut w = World::new();
    while w.actors.len() < CASTER + 1 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = PARTY;
    w.load_party(legaia_save::Party::zeroed(PARTY as usize));
    w.set_formation_table(vanilla_formation_table(), catalog.clone());
    w.enter_battle(PARTY, 1);
    w.install_menu_text(&scus);
    w.tables.move_power =
        Some(MovePowerCatalog::from_overlay_0898(&overlay).expect("move-power table"));
    w.tables.monster_catalog = catalog;
    let mut spells = SpellCatalog::new();
    spells.insert(SpellDef {
        id: ASTRAL_SLASH,
        name: "Astral Slash".into(),
        mp_cost: 0,
        target: SpellTarget::OneEnemy,
        element: SpellElement::Neutral,
        effect: SpellEffect::Damage {
            base_power: 50,
            element: SpellElement::Neutral,
        },
        ..Default::default()
    });
    w.tables.spell_catalog = spells;
    for i in 0..=CASTER {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 9000;
        w.actors[i].battle.max_hp = 9000;
        w.actors[i].battle.mp = 200;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.actors[CASTER].battle_monster_id = Some(GAZA);
    w.set_actor_battle_action_clips(CASTER, std::sync::Arc::new(clips));
    w.mode = SceneMode::Battle;
    w.toggles.live_gameplay_loop = true;
    w.battle.no_escape = true;
    w.rng_state = 0x00C0_FFEE;

    let mut hp: Vec<u16> = (0..PARTY as usize).map(|i| w.actors[i].battle.hp).collect();
    let mut fx_seen = 0usize;
    let mut cast_hits: Vec<(u8, u16)> = Vec::new();
    for _ in 0..8000 {
        w.tick();
        assert_eq!(w.mode, SceneMode::Battle, "the fight ended before the cast");
        let casting = w.actors[CASTER].battle.params[0] == ASTRAL_SLASH
            && w.battle_ctx.active_actor == CASTER as u8;
        let state = w.battle_ctx.action_state;
        let new_fx: Vec<(u8, u16)> = w.battle.hit_fx[fx_seen..]
            .iter()
            .map(|h| (h.target_slot, h.amount))
            .collect();
        fx_seen = w.battle.hit_fx.len();
        if casting && state == CAPTURE_TICK_STATE {
            cast_hits.extend(new_fx.iter().filter(|(t, _)| *t < PARTY));
        }
        for (i, was) in hp.iter_mut().enumerate() {
            let now = w.actors[i].battle.hp;
            if now >= *was {
                continue;
            }
            let taken = *was - now;
            *was = now;
            if !casting {
                // An ordinary swing before Gaza's first cast: not measured.
                cast_hits.clear();
                continue;
            }
            assert_eq!(
                state, CAPTURE_TICK_STATE,
                "Astral Slash's HP landed in state {state:#04X}, not on the clip's last beat \
                 inside the capture band"
            );
            assert_eq!(
                cast_hits.len(),
                4,
                "one melee hit per beat of entry 0x0A: {cast_hits:?}"
            );
            assert!(cast_hits.iter().all(|(t, _)| usize::from(*t) == i));
            let total: u16 = cast_hits.iter().map(|(_, a)| *a).sum();
            assert_eq!(
                taken, total,
                "the HP loss is the clip's melee total and nothing more - no wrapper roll on top"
            );
            eprintln!("[ok] Astral Slash on seat {i}: hits {cast_hits:?} -> {taken} HP");
            return;
        }
    }
    panic!("Gaza never landed Astral Slash");
}
