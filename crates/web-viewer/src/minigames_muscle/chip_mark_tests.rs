use super::*;
use legaia_engine_core::muscle_dome::{self as md, MuscleCard};

fn session(special: u32) -> MuscleDomeSession {
    let card = MuscleCard {
        command_id: 0x0C,
        cost: 0x1E,
    };
    let mut s = MuscleDomeSession::new([card; 4], [card; 4], [120, 120], [400, 400], 1);
    s.set_special_word(special);
    s
}

/// The forbidden Item chip's X comes placed by `FUN_801DBC30`'s port:
/// a 64x16 blit at `(anchor.x - 8, anchor.y - 4)` off the `etim` page's
/// red X, CLUT `0x7704` = sub-palette 4.
#[test]
fn a_forbidden_chip_carries_the_cross_out_quad() {
    let rows = LegaiaMinigames::muscle_chip_json(&session(md::SPECIAL_ITEM_FORBIDDEN));
    let item = rows.iter().find(|r| r["chip"] == "item").unwrap();
    assert_eq!(item["mark"], "forbidden");
    let q = &item["mark_quad"];
    assert_eq!((q["x"].as_i64(), q["y"].as_i64()), (Some(196), Some(30)));
    assert_eq!((q["dw"].as_i64(), q["dh"].as_i64()), (Some(64), Some(16)));
    assert_eq!((q["u"].as_i64(), q["v"].as_i64()), (Some(0), Some(96)));
    assert_eq!(q["pal"].as_i64(), Some(4));
    let spirit = rows.iter().find(|r| r["chip"] == "spirit").unwrap();
    assert!(spirit["mark_quad"].is_null());
}
