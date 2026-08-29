//! Hand-frame calibration against the retail player files (keyed on
//! `extracted/PROT`; skips without it). Leave-one-out: fit the donor ->
//! target transform without one shared blade, re-seat the donor's copy of
//! that blade, and compare with the target's own record of it.

use std::path::PathBuf;

use legaia_asset::battle_data_pack;
use legaia_asset::equip_hand_frame::{
    self, V3, WeaponClass, fit_class_excluding, nearest_rms, shaft_axis,
};
use legaia_asset::equip_transplant::{section_bones, section_clut_cols, weapon_section};
use legaia_asset::party_swap::weapon_fuse::{BareFrame, weapon_fusion_record};

fn prot_dir() -> Option<PathBuf> {
    ["extracted/PROT", "../../extracted/PROT"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("0863_edstati3.BIN").exists())
}

fn load_all() -> Option<[Vec<u8>; 3]> {
    let d = prot_dir()?;
    Some([
        std::fs::read(d.join("0863_edstati3.BIN")).ok()?,
        std::fs::read(d.join("0864_edstati3.BIN")).ok()?,
        std::fs::read(d.join("0865_battle_data.BIN")).ok()?,
    ])
}

const NAMES: [&str; 3] = ["Vahn", "Noa", "Gala"];
const BLADES: [u32; 3] = [0x22, 0x23, 0x24];

/// The hand-channel vertices of `id`'s cut in `file`.
fn hand_points(file: &[u8], slot: usize, id: u32) -> Option<Vec<V3>> {
    let pack = battle_data_pack::parse(file).ok()?;
    let sec = weapon_section(&pack)?;
    let cols = section_clut_cols(file, &pack, sec).ok()?;
    let bare = BareFrame::new(file, &pack).ok()?;
    let bones = section_bones(file, &pack, sec).ok()?;
    let hand = *bones.last()?;
    let (per_channel, _) =
        weapon_fusion_record(file, &pack, &bare, slot, sec, id, &cols).ok()??;
    let obj = per_channel.get(&hand)?;
    let mut used = vec![false; obj.vertices.len()];
    for g in &obj.groups {
        for p in &g.prims {
            for &v in &p.vertices {
                used[v as usize] = true;
            }
        }
    }
    Some(
        obj.vertices
            .iter()
            .zip(&used)
            .filter(|(_, u)| **u)
            .map(|(v, _)| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
    )
}

#[test]
fn every_pair_calibrates_the_hand_and_forearm() {
    let Some(files) = load_all() else {
        eprintln!("[skip] extracted/PROT not present");
        return;
    };
    let packs: Vec<_> = files
        .iter()
        .map(|f| battle_data_pack::parse(f).unwrap())
        .collect();
    for (d, t) in [(0usize, 1usize), (0, 2), (1, 2), (1, 0), (2, 0), (2, 1)] {
        let fit = equip_hand_frame::fit(&files[d], &packs[d], d, &files[t], &packs[t], t).unwrap();
        assert_eq!(fit.channels.len(), 3, "{} -> {}", NAMES[d], NAMES[t]);
        for (k, c) in fit.channels.iter().enumerate() {
            match c {
                Some(c) => eprintln!(
                    "{}->{} channel {k} (bone {} -> {}): {:5.1} deg, rms {:5.1}, from {:?}",
                    NAMES[d],
                    NAMES[t],
                    fit.donor_bones[k],
                    fit.target_bones[k],
                    c.xf.angle_deg(),
                    c.rms,
                    c.weapons
                ),
                None => eprintln!("{}->{} channel {k}: no calibration", NAMES[d], NAMES[t]),
            }
        }
        // The hand (last channel) always calibrates - every shared weapon
        // touches it; the forearm does through the claws.
        let hand = fit.channels[2].as_ref().expect("hand channel fit");
        assert!(
            hand.weapons.len() >= 3,
            "{} -> {}: {:?}",
            NAMES[d],
            NAMES[t],
            hand.weapons
        );
        assert!(
            hand.rms < 20.0,
            "{} -> {} hand rms {}",
            NAMES[d],
            NAMES[t],
            hand.rms
        );
        // Per class: the pool a real transplant uses.
        for (class, probe) in [
            (WeaponClass::Blade, 0x24u32),
            (WeaponClass::Claw, 0x2A),
            (WeaponClass::Club, 0x31),
        ] {
            let cf =
                equip_hand_frame::fit_for(&files[d], &packs[d], d, &files[t], &packs[t], t, probe)
                    .unwrap();
            let h = cf.channels[2].as_ref().unwrap();
            eprintln!(
                "{}->{} {class:?} hand: {:5.1} deg, rms {:5.1}, from {:?}",
                NAMES[d],
                NAMES[t],
                h.xf.angle_deg(),
                h.rms,
                h.weapons
            );
            assert!(
                h.rms < 20.0,
                "{} -> {} {class:?} rms {}",
                NAMES[d],
                NAMES[t],
                h.rms
            );
        }
        let fore = fit.channels[1].as_ref().expect("forearm channel fit");
        assert!(!fore.weapons.is_empty());
    }
}

#[test]
fn leave_one_out_blade_lands_on_the_targets_own_record() {
    let Some(files) = load_all() else {
        eprintln!("[skip] extracted/PROT not present");
        return;
    };
    let packs: Vec<_> = files
        .iter()
        .map(|f| battle_data_pack::parse(f).unwrap())
        .collect();
    for (d, t) in [(0usize, 1usize), (0, 2), (1, 2)] {
        for held in BLADES {
            let fit = fit_class_excluding(
                &files[d],
                &packs[d],
                d,
                &files[t],
                &packs[t],
                t,
                Some(WeaponClass::Blade),
                &[held],
            )
            .unwrap();
            let hand = fit.channels[2].as_ref().unwrap();
            let pd = hand_points(&files[d], d, held).unwrap();
            let pt = hand_points(&files[t], t, held).unwrap();
            let moved: Vec<V3> = pd.iter().map(|p| hand.xf.apply(*p)).collect();
            let raw_rms = nearest_rms(&pd, &pt);
            let rms = nearest_rms(&moved, &pt);
            let ax = shaft_axis(&moved).unwrap();
            let at = shaft_axis(&pt).unwrap();
            let dot = equip_hand_frame::dot(ax, at);
            let cm = equip_hand_frame::centroid(&moved);
            let ct = equip_hand_frame::centroid(&pt);
            let gap = equip_hand_frame::norm(equip_hand_frame::sub(cm, ct));
            eprintln!(
                "{}->{} held-out {held:#04x}: verbatim rms {raw_rms:5.1} -> re-seated rms {rms:5.1}, axis dot {dot:+.3}, centroid gap {gap:5.1}",
                NAMES[d], NAMES[t]
            );
            assert!(
                dot > 0.98,
                "{}->{} {held:#04x} axis dot {dot}",
                NAMES[d],
                NAMES[t]
            );
            assert!(
                rms < 12.0 && rms < raw_rms / 2.0,
                "{}->{} {held:#04x}: re-seated rms {rms} (verbatim {raw_rms})",
                NAMES[d],
                NAMES[t]
            );
            assert!(
                gap < 15.0,
                "{}->{} {held:#04x}: centroid gap {gap}",
                NAMES[d],
                NAMES[t]
            );
        }
    }
}
