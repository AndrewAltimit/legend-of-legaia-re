//! Disc-gated: a pack art and a pack spell in a real battle. Vahn knows the
//! pack's Ember Cascade (Pyro Pummel's and Somersault's clips under pack
//! power bytes) and its Cinder Wyrm (cast as Gimard); in a pack fight on the
//! disc the battle form's art bank carries the borrowed clips, the entered
//! arrows build the byte-exact pack queue, the commit plays the pack clip
//! under the retail constant's latch and the art lands damage, and the
//! spell's menu id casts the Gimard module with the pack's MP and trains the
//! pack spell's own level. Skips and passes without `LEGAIA_DISC_BIN`.

use std::sync::Arc;

use legaia_engine_battle::target_picker::CursorRow;
use legaia_engine_battle_loop::round_flow::PendingPartyAction;
use legaia_engine_core::live_loop::LiveLoopOpts;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::{ExpansionPack, PackFiles};
use legaia_save::EquipmentSlots;
use legaia_save::character::LiveStats;

const CARUBAN: u16 = 0x49;
const CHAOS_BREAKER: u8 = 0x27;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"disctest\"\nname = \"Disc Test\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert(
        "arts.toml".into(),
        br#"
[arts.ember]
character = "vahn"
name = "Ember Cascade"
commands = "RURDU"
ap = 30
segments = [{ art = "Pyro Pummel", power = ["U22", "L22"] }, { art = "Somersault", power = ["L28"] }]
damage_pct = 150
"#
        .to_vec(),
    );
    files.insert(
        "spells.toml".into(),
        br#"
[spells.wyrm]
name = "Cinder Wyrm"
base = 0x81
mp = 34
power = 200

[spells.storm]
name = "Cinder Storm"
base = 0x81
mp = 20
target = "all_enemies"
"#
        .to_vec(),
    );
    files.insert(
        "battle.toml".into(),
        format!(
            "[monsters.drake]\nbase = {{ kind = \"monster\", id = {CARUBAN} }}\nhp = 3000\n\
             [formations.f]\nmonsters = [\"drake\"]\n[formations.pair]\nmonsters = [\"drake\", \"drake\"]\n\
             [bosses.b]\nformation = \"f\"\nonce = false\n"
        )
        .into_bytes(),
    );
    Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

fn host() -> Option<SceneHost> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let mut host = SceneHost::open_disc(&disc).expect("open the disc");
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world
        .arm_live_loop("town01", &LiveLoopOpts::playable());
    host.world.toggles.live_gameplay_loop = true;
    let mut party = legaia_save::Party::zeroed(1);
    party.members[0].set_live_stats(LiveStats {
        agl: 300,
        atk: 120,
        udf: 300,
        ldf: 300,
        spd: 80,
        int: 120,
    });
    let mut slots = [0u8; 8];
    slots[2] = CHAOS_BREAKER;
    party.members[0].set_equipment(EquipmentSlots { slots });
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_max = 9000;
    hms.hp_cur = 9000;
    hms.mp_max = 500;
    hms.mp_cur = 500;
    party.members[0].set_hp_mp_sp(hms);
    host.world.load_party(party);
    // The disc's spell catalog, as the play session installs it at boot.
    {
        use legaia_engine_scene::vfs::Vfs;
        let scus = legaia_engine_scene::vfs::DiscVfs::open(std::path::Path::new(&disc))
            .expect("open the disc")
            .read("SCUS_942.54")
            .expect("SCUS_942.54 present");
        host.world.set_spell_catalog(
            legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(&scus)
                .expect("the spell table parses"),
        );
    }
    host.world
        .activate_expansion(pack())
        .expect("pack activates");
    host.merge_expansion_monsters();
    Some(host)
}

/// Tick until the battle opens the command ring for party slot 0.
fn until_command(host: &mut SceneHost, frames: usize) -> bool {
    for _ in 0..frames {
        if host.world.mode == SceneMode::Battle
            && host
                .world
                .battle
                .command
                .as_ref()
                .is_some_and(|c| c.actor == 0)
        {
            return true;
        }
        let _ = host.tick();
    }
    false
}

fn monster_hp(host: &SceneHost) -> u32 {
    let pc = usize::from(host.world.party.party_count);
    host.world.actors[pc..]
        .iter()
        .map(|a| u32::from(a.battle.hp))
        .sum()
}

#[test]
fn a_pack_art_and_a_pack_spell_run_in_a_real_battle() {
    let Some(mut host) = host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert!(host.world.pack_art_known("ember"));
    assert!(host.world.pack_spell_progress().knows(0, "wyrm"));
    // A formation fight, not the boss: a boss fight banks no spell XP
    // (`special_battle_word`), as retail.
    assert!(host.world.start_expansion_battle("f"));
    host.world.battle.player_driven = true;
    assert!(until_command(&mut host, 4_000), "the command ring opens");

    // The battle form's bank carries the borrowed clips at 0x33 / 0x34.
    let bank = host.world.actors[0]
        .battle_art_bank
        .clone()
        .expect("Vahn's art bank");
    let pyro = bank[0x23 - 0x10].as_ref().expect("Pyro Pummel's clip");
    let seg0 = bank[0x33 - 0x10].as_ref().expect("segment 0 clip");
    let seg1 = bank[0x34 - 0x10].as_ref().expect("segment 1 clip");
    assert_eq!(seg0.frames, pyro.frames, "segment 0 plays Pyro Pummel");
    let run = seg0.entry_power_run().unwrap();
    assert_eq!(&run[..2], &[0x19, 0x1E], "U22 L22");
    assert_eq!(seg1.entry_power_run().unwrap()[0], 0x1F, "L28");
    assert_eq!(
        seg0.entry_event_frames(),
        pyro.entry_event_frames(),
        "the clip's own beats"
    );

    // Enter R U R D U and commit it on the monster.
    let spirit = host.world.actors[0].battle.spirit_gauge.max(30);
    host.world.actors[0].battle.spirit_gauge = spirit;
    let hp0 = monster_hp(&host);
    // A battle no pad drives begins at the commit (no Begin confirm).
    host.world.battle.player_driven = false;
    host.world.commit_party_command(
        0,
        PendingPartyAction::Art {
            sequence: vec![2, 4, 2, 3, 4],
            target_row: CursorRow::Enemy,
            target_slot: 0,
        },
    );
    host.world.battle.player_driven = true;
    let mut queue = None;
    let mut played_pack_clip = false;
    let mut pack_hits = 0usize;
    for _ in 0..3_000 {
        let _ = host.tick();
        let a = &host.world.actors[0];
        if queue.is_none() && a.battle.params[..2] == [0x0D, 0x0F] && a.battle.params[4] != 0x0F {
            queue = Some(a.battle.params);
        }
        if let Some(&(_, seg)) = host.world.expansion.arts.playing.get(&0) {
            played_pack_clip = true;
            assert_eq!(
                a.battle.latched_anim,
                [0x23, 0x27][seg],
                "a pack clip latches the retail constant it borrows"
            );
        }
        pack_hits += host
            .world
            .battle
            .hit_events
            .iter()
            .filter(|e| e.attacker_slot == 0 && e.is_art && e.damage > 0)
            .count();
        if host
            .world
            .battle
            .command
            .as_ref()
            .is_some_and(|c| c.actor == 0)
            && played_pack_clip
        {
            break;
        }
    }
    let queue = queue.expect("the art armed its queue");
    assert_eq!(
        &queue[..8],
        &[0x0D, 0x0F, 0x0D, 0x0E, 0x19, 0x33, 0x34, 0x00],
        "byte-exact pack queue"
    );
    assert!(played_pack_clip, "the pack clip committed");
    assert!(pack_hits > 0, "the pack art's beats landed damage");
    let hp1 = monster_hp(&host);
    assert!(hp1 < hp0, "the monster lost HP to the art ({hp0} -> {hp1})");
    assert!(
        host.world.actors[0].battle.spirit_gauge < spirit,
        "the art's AP came off the Spirit gauge"
    );

    // Cast the pack spell under its menu id.
    assert!(until_command(&mut host, 4_000), "the next round opens");
    let menu_id = host
        .world
        .expansion_pack_spell_ids(0)
        .into_iter()
        .find(|&id| {
            host.world
                .expansion_pack_spell(id)
                .is_some_and(|s| s.key == "wyrm")
        })
        .expect("Cinder Wyrm is known");
    let entry = host
        .world
        .tables
        .spell_catalog
        .get(menu_id)
        .expect("the menu entry is installed from the disc's Gimard");
    assert_eq!(entry.mp_cost, 34);
    // The spell XP is the struck share of max HP (`summon_spell_xp_gain`):
    // a monster near full keeps it measurable.
    let pc = usize::from(host.world.party.party_count);
    let full = host.world.actors[pc].battle.max_hp;
    host.world.actors[pc].battle.hp = full;
    let hp1 = monster_hp(&host);
    let mp0 = host.world.actors[0].battle.mp;
    host.world.battle.player_driven = false;
    host.world.commit_party_command(
        0,
        PendingPartyAction::Spell {
            spell_id: menu_id,
            target_row: CursorRow::Enemy,
            target_slot: 0,
        },
    );
    host.world.battle.player_driven = true;
    let mut cast_seen = false;
    for _ in 0..4_000 {
        let _ = host.tick();
        if host.world.expansion.spells.casting.is_some() && !cast_seen {
            cast_seen = true;
            assert_eq!(
                host.world.actors[0].battle.params[0], 0x81,
                "cast as Gimard"
            );
        }
        if cast_seen
            && host
                .world
                .battle
                .command
                .as_ref()
                .is_some_and(|c| c.actor == 0)
        {
            break;
        }
        if host.world.mode != SceneMode::Battle {
            break;
        }
    }
    assert!(cast_seen, "the pack cast was in flight");
    let mp1 = host.world.actors[0].battle.mp;
    assert_eq!(mp0 - mp1, 34, "the pack MP, not Gimard's 10");
    let hp2 = monster_hp(&host);
    assert!(hp2 < hp1, "the cast damaged the monster ({hp1} -> {hp2})");
    let xp = host
        .world
        .pack_spell_progress()
        .level(0, "wyrm")
        .unwrap()
        .xp;
    assert!(xp > 0, "the cast trained the pack spell");
    host.world.abandon_live_battle();
    host.world.deactivate_expansion();
}

/// A pack spell whose `target` widens its base's shape strikes every
/// monster: Cinder Storm rides Gimard (one enemy) as `all_enemies`, and
/// casts as a fire Seru whose own shape is every enemy, so its creature
/// strikes the whole row the damage folds on.
#[test]
fn a_widened_pack_spell_strikes_every_monster() {
    let Some(mut host) = host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert!(host.world.start_expansion_battle("pair"));
    host.world.battle.player_driven = true;
    assert!(until_command(&mut host, 4_000), "the command ring opens");
    let pc = usize::from(host.world.party.party_count);
    let menu_id = host
        .world
        .expansion_pack_spell_ids(0)
        .into_iter()
        .find(|&id| {
            host.world
                .expansion_pack_spell(id)
                .is_some_and(|s| s.key == "storm")
        })
        .expect("Cinder Storm is known");
    let before: Vec<u16> = (pc..pc + 2)
        .map(|s| host.world.actors[s].battle.hp)
        .collect();
    assert!(before.iter().all(|&h| h > 0), "{before:?}");
    host.world.battle.player_driven = false;
    host.world.commit_party_command(
        0,
        PendingPartyAction::Spell {
            spell_id: menu_id,
            target_row: CursorRow::Enemy,
            target_slot: 0,
        },
    );
    host.world.battle.player_driven = true;
    let mut cast_seen = false;
    let mut cast_as = None;
    for _ in 0..4_000 {
        let _ = host.tick();
        cast_seen |= host.world.expansion.spells.casting.is_some();
        if cast_seen && cast_as.is_none() {
            cast_as = Some(host.world.actors[0].battle.params[0]);
        }
        if cast_seen
            && host
                .world
                .battle
                .command
                .as_ref()
                .is_some_and(|c| c.actor == 0)
        {
            break;
        }
        if host.world.mode != SceneMode::Battle {
            break;
        }
    }
    assert!(cast_seen, "the pack cast was in flight");
    let cast_as = cast_as.expect("the cast id");
    assert_ne!(cast_as, 0x81, "not Gimard's one-enemy module");
    let shape = host
        .world
        .tables
        .spell_catalog
        .get(cast_as)
        .map(|d| (d.target, d.element));
    let gimard = host.world.tables.spell_catalog.get(0x81).map(|d| d.element);
    assert_eq!(
        shape,
        Some((
            legaia_engine_battle::spells::SpellTarget::AllEnemies,
            gimard.unwrap()
        )),
        "cast {cast_as:#04x} as an all-enemies Seru of Gimard's element"
    );
    let after: Vec<u16> = (pc..pc + 2)
        .map(|s| host.world.actors[s].battle.hp)
        .collect();
    for k in 0..2 {
        assert!(
            after[k] < before[k],
            "monster {k} was struck: {before:?} -> {after:?}"
        );
    }
    host.world.abandon_live_battle();
    host.world.deactivate_expansion();
}

/// Rebirth's combos keep clear of every retail combo on the disc's arts
/// table: none equals one, and none contains one (the retail walk would take
/// those arrows first).
#[test]
fn rebirth_combos_avoid_the_discs_retail_combos() {
    use legaia_engine_scene::vfs::Vfs;
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = legaia_engine_scene::vfs::DiscVfs::open(std::path::Path::new(&disc))
        .expect("open the disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let oracle = legaia_art::ArtsOracle::from_scus(&scus).expect("the arts table");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let pack = ExpansionPack::from_dir(&root).expect("Rebirth loads");
    let mut checked = 0;
    for (key, art) in &pack.arts.arts {
        let mine = art.command_bytes().unwrap();
        if mine.is_empty() {
            continue;
        }
        let ch = legaia_expansion_format::arts::art_character(art.character);
        for e in oracle
            .entries()
            .iter()
            .filter(|e| e.character == ch && e.commands.len() >= 2)
        {
            let theirs: Vec<u8> = e.commands.iter().map(|c| c.as_byte()).collect();
            assert!(
                !mine.windows(theirs.len()).any(|w| w == theirs.as_slice()),
                "{key} holds {}'s combo",
                e.name
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 3, "one input art per hero");
}
