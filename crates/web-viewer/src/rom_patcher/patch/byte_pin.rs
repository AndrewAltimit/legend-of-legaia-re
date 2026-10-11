//! Disc-gated byte pin for the browser patch run: a fixed seed and option set
//! produces one exact image and one exact summary.
//!
//! Stage order is part of a seed's meaning (later stages read what earlier
//! ones wrote and share the free-space ledger), so a reordering or a dropped
//! stage shows up here as a changed digest. A deliberate change to what a
//! stage writes moves the digests too; re-pin them from the failure output.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use super::*;
use sha2::{Digest, Sha256};

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

/// One run through the page's own stage driver, with no progress callback
/// (so no JS value is ever touched and the future never parks).
fn run(image: Vec<u8>, opts: &PatchOptions<'_>) -> (Vec<u8>, String) {
    let mut prog = Progress::new(None, PATCH_ROM_STAGES);
    let run = block_on(run_stages(image, opts, &mut prog)).expect("patch run");
    assert_eq!(prog.index + 1, PATCH_ROM_STAGES, "stage count drifted");
    (run.patcher.into_image(), run.summary)
}

fn disc() -> Option<Vec<u8>> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    std::fs::read(path).ok()
}

fn hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// The option sets. The arena features are mutually exclusive, so each gets
/// its own run; between them every stage of the run does real work at least
/// once.
fn cases() -> Vec<(&'static str, PatchOptions<'static>)> {
    // Every seeded randomizer, every tuning knob and the arena-free hooks.
    let everything = PatchOptions {
        seed: "byte-pin",
        drops: "random",
        encounters: "random",
        encounter_scope: "world",
        chests: "random",
        shops: "shuffle",
        casino: "random",
        steals: "random",
        arts: "shuffle",
        doors: "shuffle",
        door_coupling: "coupled",
        house_doors: "shuffle",
        starting_items: 10,
        door_of_wind: 1,
        incense: 2,
        speed_chain: 1,
        chicken_heart: 1,
        good_luck_bell: 1,
        all_warps: true,
        unused_enemies: true,
        unused_items: true,
        equipment_drops: true,
        monster_stats: "shuffle",
        move_power: "random",
        element_affinity: "random",
        spell_cost: "shuffle",
        equip_bonus: "shuffle",
        weapon_specialty: true,
        starting_level: 5,
        solo_strong_encounters: true,
        flee_exp: true,
        seru_trade: true,
        enemy_ally: true,
        shiny_seru: true,
        jewel_fix: true,
        approach_softlock_fix: true,
        custom_items: true,
        fishing_prices: "0x6F=500, bogus",
        location_renames: "3=Ancient Fire Cave\nnonsense",
        earth_egg_price: "50000",
        arts_powers: "RDLDL=0x16 nope",
        spirit_ap: "50",
        damage_ap: "150",
        enemy_stat_scale: "regular:0.75|boss:2",
        exp_scale: "2",
        seru_catch_rate: "50",
        super_art_powers: "Tri-Somersault=0x1A, junk",
        enemy_attack_count: "1.5",
        swing_costs: "Vahn:default=30",
        equip_owners: "0xBA=VNG",
        enemy_hp_bar: true,
        ..Default::default()
    };
    // The Delilas Challenge with its custom-item reward, over the other
    // randomizer modes; shiny Seru yields to the challenge.
    let challenge = PatchOptions {
        seed: "12345",
        drops: "shuffle",
        encounters: "shuffle",
        encounter_scope: "kingdom",
        chests: "shuffle",
        shops: "random",
        casino: "shuffle",
        steals: "shuffle",
        arts: "random",
        doors: "random",
        door_coupling: "decoupled",
        house_doors: "random",
        starting_items: 3,
        monster_stats: "random",
        move_power: "shuffle",
        element_affinity: "shuffle",
        spell_cost: "random",
        equip_bonus: "random",
        shiny_seru: true,
        delilas_challenge: true,
        custom_items: true,
        spirit_ap: "-20",
        damage_ap: "999",
        enemy_stat_scale: "hp=2,attack=1.5",
        ..Default::default()
    };
    let party = PatchOptions {
        seed: "party",
        delilas_party: "gi,lu,che",
        delilas_arts_voice: "",
        delilas_moves: "",
        encounters: "random",
        doors: "shuffle",
        ..Default::default()
    };
    let party_arena_taken = PatchOptions {
        seed: "party",
        delilas_party: "lu,che,gi",
        show_super_arts: true,
        ..Default::default()
    };
    let pack = PatchOptions {
        seed: "pack",
        super_arts_pack: true,
        arts: "shuffle",
        ..Default::default()
    };
    let arts_ap = PatchOptions {
        seed: "ap",
        arts_ap_grants: "Vahn:RDLDL=10",
        arts_ap_costs: "Noa:LLRR=30 junk",
        ..Default::default()
    };
    let oscillating = PatchOptions {
        seed: "osc",
        oscillating_ap: "60",
        starting_items: 0,
        door_of_wind: 3,
        ..Default::default()
    };
    let untouched = PatchOptions {
        seed: "none",
        ..Default::default()
    };
    vec![
        ("everything", everything),
        ("challenge", challenge),
        ("party", party),
        ("party_arena_taken", party_arena_taken),
        ("pack", pack),
        ("arts_ap", arts_ap),
        ("oscillating", oscillating),
        ("untouched", untouched),
    ]
}

/// `(case, image sha256, summary sha256)`.
const PINS: &[(&str, &str, &str)] = &[
    (
        "everything",
        "c5fec37aebcca3c66b99eb5fd857efbb31bb55e7204b38677dd21535f3330fd9",
        "29a4b8854111085424133b35c6debc8ff51b107c08245c8787911daa48d1c535",
    ),
    (
        "challenge",
        "bf4f9a36c831eefc3b3e1b7a4b577bc6a0933dd58fcdf3e05f9b7d295119c3cb",
        "09d37588c7a4db2034788cb0944b5f905cc25acbf518ed32dce799b3855c13b4",
    ),
    (
        "party",
        "ca223fbfe4637c27d5d3b45e5aaef92b0d700b9fa425bb4453db9771581927ef",
        "c5116a06250512dc4283b69bacd6c0734522694b931cb2d9d7b4e80719cf0b52",
    ),
    (
        "party_arena_taken",
        "a2afb150239ed1b6cfdce6741d539708a3e76654d20851e717fc979c10255a3f",
        "0bbee883761edf11256218da87ba832167ae8362b94ac1098992129a3545ea81",
    ),
    (
        "pack",
        "edd9ebee1b01eb0f8e10039158627093df35d38a1c6b8ac0511632d5c0284aba",
        "e9aab20167e02be274636aa393b50c08b07a4e401446d806f4f21dc4968e2663",
    ),
    (
        "arts_ap",
        "cd44254c017bc499cbce39dde8643f908d344e3d8de432b00b3a8ae653506086",
        "2b6b8ebfe69dc339c12aea6d5180b5a0449f827359cd531ce40ae5e25bf46e18",
    ),
    (
        "oscillating",
        "ccb7988a1b62945639ba39ccc75be7627c24c178c4662c1f29f5cab6d84f2ac8",
        "2f3ee7e0c47706217715288ee24c587dc6561a88a71b51554bec3319b70f4618",
    ),
    (
        "untouched",
        "e6120a5d70716dd2f026a2da32d0171d52651971b52c4347a68541299f75258c",
        "71b6a635ed46198575d4046b88e19b5c497b7b9c7d55c4bb2dc31212ea5e0bbb",
    ),
];

/// The refusals fire before the image is parsed, so they need no disc.
#[test]
fn conflicting_options_are_refused_before_the_image_is_read() {
    let cases = [
        PatchOptions {
            delilas_challenge: true,
            delilas_party: "gi,lu,che",
            ..Default::default()
        },
        PatchOptions {
            oscillating_ap: "50",
            shiny_seru: true,
            ..Default::default()
        },
        PatchOptions {
            arts_ap_costs: "RDLDL=10",
            delilas_challenge: true,
            ..Default::default()
        },
        PatchOptions {
            show_super_arts: true,
            shiny_seru: true,
            ..Default::default()
        },
        PatchOptions {
            super_arts_pack: true,
            show_super_arts: true,
            ..Default::default()
        },
    ];
    for opts in &cases {
        let mut prog = Progress::new(None, PATCH_ROM_STAGES);
        let got = block_on(run_stages(Vec::new(), opts, &mut prog));
        let Err(msg) = got else {
            panic!("a conflicting option set was accepted");
        };
        assert!(msg.contains("mutually exclusive"), "{msg}");
        assert_eq!(prog.index, 0, "refused before the first stage boundary");
    }
}

/// No two progress stages share a label (the page shows it beside the bar).
#[test]
fn stage_labels_are_distinct() {
    let mut labels: Vec<&str> = STAGES.iter().map(|s| s.label).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), STAGES.len());
}

#[test]
fn fixed_seed_and_options_produce_pinned_bytes() {
    let Some(image) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut got = Vec::new();
    for (name, opts) in cases() {
        let (patched, summary) = run(image.clone(), &opts);
        if name == "untouched" {
            assert!(patched == image, "an all-off run must not change the image");
        } else {
            assert!(
                patched != image,
                "{name}: the run changed nothing\n{summary}"
            );
        }
        got.push((name, hex(&patched), hex(summary.as_bytes())));
    }
    let mut table = String::new();
    for (name, img, sum) in &got {
        table.push_str(&format!("    (\"{name}\", \"{img}\", \"{sum}\"),\n"));
    }
    let matches = got.len() == PINS.len()
        && got
            .iter()
            .zip(PINS)
            .all(|(g, p)| g.0 == p.0 && g.1 == p.1 && g.2 == p.2);
    assert!(matches, "patch output moved; measured digests:\n{table}");
}
