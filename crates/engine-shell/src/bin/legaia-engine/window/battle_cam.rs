//! Native adapter over the shared phase-scripted retail battle camera.
//!
//! The whole model - phases, poses, measured glides, formation law,
//! projection - lives in [`legaia_engine_vm::battle_cam_script`], the ONE
//! implementation the native play-window and the browser play page both
//! drive (host-drift rule: same model, not parallel reimplementations).
//! This module re-exports it under the window's historical path and keeps
//! the native-only tests: the retail seat-table cross-checks (which need
//! `legaia_engine_core::battle_seats`) and the disc-gated per-character
//! height check (which needs `legaia_prot` + `extracted/`).
//!
//! Provenance, the trace-pinned laws and the full doc live on the shared
//! module. REF: FUN_801D5854 (framing cases), FUN_801D829C (angle-tween
//! builder).

pub(crate) use legaia_engine_vm::battle_cam_script::{BattleCamera, FormationBox};

#[cfg(test)]
mod tests {
    use legaia_engine_vm::battle_cam_script::{
        BattleCamActor, BattleCamPose, FormationBox, SUBMENU_HEIGHT_FALLBACK, menu_framing,
        prescale_tr_z,
    };

    /// The formation behind the traced Tetsu fight (span 1600 - see the
    /// shared module's `traced_formation`).
    fn traced_formation() -> FormationBox {
        FormationBox {
            min: [-800.0, -800.0],
            max: [800.0, 800.0],
        }
    }

    /// The traced far framing, reproduced by the case-9 law.
    fn traced_menu_tr() -> [f32; 3] {
        menu_framing(Some(traced_formation()), 0.0).tr
    }

    /// Build the case-9 formation box from retail seat rows.
    fn seats_box(
        party: &[legaia_engine_core::battle_seats::Seat],
        monsters: &[legaia_engine_core::battle_seats::Seat],
    ) -> FormationBox {
        let mut b: Option<FormationBox> = None;
        for s in party.iter().chain(monsters) {
            FormationBox::extend(&mut b, s.x as f32, s.z as f32);
        }
        b.expect("non-empty formation")
    }

    /// **The independent check on the case-9 law.** The traced far framing
    /// (`TR.z = 7680`) was measured on the solo-Vahn tutorial fight. Feeding
    /// that fight's *retail seat table* rows - party count 1, monster count 1
    /// (`FUN_800513F0`'s `0x800775C8` / `0x80077608`) - through the formation
    /// law reproduces `7680` exactly, with nothing fitted to the trace: the
    /// seats give a 1600-unit Z span, `1600 * 3 = 0x12C0`, and the prescale
    /// lands on `7680`. A law that merely happened to pass through the
    /// measured point would not also land on the seat geometry that produced
    /// it.
    #[test]
    fn traced_menu_depth_falls_out_of_the_retail_seat_tables() {
        use legaia_engine_core::battle_seats::{MONSTER_SEATS, PARTY_SEATS};
        let solo = seats_box(&PARTY_SEATS[0][..1], &MONSTER_SEATS[0][..1]);
        assert_eq!(solo.max[1] - solo.min[1], 1600.0, "Vahn -800 vs Tetsu +800");
        let p = menu_framing(Some(solo), 0.0);
        assert_eq!(p.tr[2], 7680.0, "the traced far-framing depth");
        assert_eq!(p.focus, [0.0, 0.0, 0.0], "the fight is centred on origin");
        // And that box IS what the traced formation stands in for.
        assert_eq!(p.tr, traced_menu_tr());
    }

    /// A real three-member party pulls the camera further back than the solo
    /// fight the trace captured - the framing difference a solo-Vahn trace
    /// structurally cannot observe.
    #[test]
    fn multi_member_formations_frame_wider_than_the_traced_solo_fight() {
        use legaia_engine_core::battle_seats::{MONSTER_SEATS, PARTY_SEATS};
        let solo = menu_framing(
            Some(seats_box(&PARTY_SEATS[0][..1], &MONSTER_SEATS[0][..1])),
            0.0,
        );
        let trio = menu_framing(
            Some(seats_box(&PARTY_SEATS[2], &MONSTER_SEATS[0][..1])),
            0.0,
        );
        // 3 party + 1 monster spans -825..800 in Z = 1625 > the solo 1600.
        assert!(
            trio.tr[2] > solo.tr[2],
            "trio {:?} vs solo {:?}",
            trio.tr,
            solo.tr
        );
        assert_eq!(trio.tr[2], prescale_tr_z(1625 * 3));
        // A four-monster row widens it in X instead, and X now wins.
        let crowd = menu_framing(Some(seats_box(&PARTY_SEATS[2], &MONSTER_SEATS[3])), 0.0);
        assert_eq!(
            crowd.tr[2],
            prescale_tr_z(1800 * 3),
            "X span 1800 dominates"
        );
        assert!(crowd.tr[2] > trio.tr[2]);
    }

    /// Disc-gated: the per-character heights the submenu close-up reads come
    /// off the real battle-action overlay, and each of the three playable
    /// members frames at its own height. Skips and passes without a disc.
    #[test]
    fn real_disc_heights_give_each_member_its_own_framing() {
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
            return;
        }
        let mut prot = None;
        for base in ["extracted", "../../extracted"] {
            let p = std::path::PathBuf::from(base).join("PROT.DAT");
            if p.is_file() {
                prot = Some(p);
                break;
            }
        }
        let Some(prot) = prot else {
            eprintln!("[skip] extracted/PROT.DAT missing");
            return;
        };
        let mut archive = legaia_prot::archive::Archive::open(&prot).expect("open PROT.DAT");
        let entry = archive
            .entries
            .get(legaia_asset::battle_camera_table::BATTLE_ACTION_OVERLAY_PROT_INDEX)
            .cloned()
            .expect("PROT 0898");
        let mut bytes = Vec::new();
        archive
            .read_entry(&entry, &mut bytes)
            .expect("read PROT 0898");
        let table = legaia_asset::battle_camera_table::parse(&bytes).expect("height table");

        // Vahn's entry is the one the solo trace pinned, so it anchors the
        // table to the measurement.
        assert_eq!(
            table.height_for_char_id(1).map(|h| h as f32),
            Some(SUBMENU_HEIGHT_FALLBACK),
            "Vahn's disc height is the traced fallback"
        );
        // The three battle-party members each frame at their own height.
        let poses: Vec<BattleCamPose> = (1..=3u8)
            .map(|id| {
                BattleCamActor {
                    facing: 0,
                    height: table.height_for_char_id(id).map(|h| h as f32),
                    world: [0.0, 0.0, -800.0],
                }
                .submenu_pose()
            })
            .collect();
        for (i, a) in poses.iter().enumerate() {
            for b in &poses[i + 1..] {
                assert_ne!(a.tr[1], b.tr[1], "distinct per-character heights");
                assert_eq!(a.tr[0], b.tr[0]);
                assert_eq!(a.tr[2], b.tr[2]);
            }
        }
    }
}
