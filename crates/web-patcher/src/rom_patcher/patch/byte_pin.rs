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
        fortress_encounter_fix: true,
        conkram_freeze_fix: true,
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
        "b31d2d19c4bb7617846a57c1d0638c560351962e048d3a971e963e234b452235",
        "edad85629b6338dd3a8fac49a21ddeb70c079b4a44c8f3626f288dbfa9469b57",
    ),
    (
        "challenge",
        "bf4f9a36c831eefc3b3e1b7a4b577bc6a0933dd58fcdf3e05f9b7d295119c3cb",
        "0a2da7d07cb4f35441eb966dbac85b49a0805e222b29d87b1fe78cf3b2156225",
    ),
    (
        "party",
        "d296295ad2040947d9a1da7cd9290f26f28c609738abdb5ea2417fe940de2a0d",
        "80ff46489e9aaa69e765ef196e3d917b33161e716acb8d385eb6e445e2654a2b",
    ),
    (
        "party_arena_taken",
        "3e90183f0610ad62d02c55273b798eb95b88fb1b4959f38432a62baa960d9bf2",
        "bdf3f423eab0fc38df81abf5da582514b41027823eeb4047962223b33d56794a",
    ),
    (
        "pack",
        "edd9ebee1b01eb0f8e10039158627093df35d38a1c6b8ac0511632d5c0284aba",
        "95be6887b15ab7b520ee233ea8a9cba16e0f8f70c28fb8c0c47dc5c0e2637597",
    ),
    (
        "arts_ap",
        "cd44254c017bc499cbce39dde8643f908d344e3d8de432b00b3a8ae653506086",
        "1c714f2302453e15cc1efa5b80ff59e5c3f2d5984ddf03302151e0bb116817df",
    ),
    (
        "oscillating",
        "ccb7988a1b62945639ba39ccc75be7627c24c178c4662c1f29f5cab6d84f2ac8",
        "9906d19b5655a3b2f023f608402feb4fc77b1816a72d40c04adde0de51a58c74",
    ),
    (
        "untouched",
        "e6120a5d70716dd2f026a2da32d0171d52651971b52c4347a68541299f75258c",
        "31d7f42fff596f7e9dbe96a3920437e5660f279a3f21c15aec3ab3d789eb70e1",
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
