//! A census the battle AI's scope rows rest on: Cort's records `0xB7..=0xBA`
//! (copies of `0xB4` with no physical swing candidate) are seated by no
//! formation row on the disc. The formation cell `DAT_8007BD0C` is written by
//! the encounter reader from these rows, by the arena init from PROT 0977's
//! own opponent table (which names none of them), and by `FUN_8005567C` from
//! the writer-less debug id `_DAT_8007B7FC`.

use legaia_disc_patch::disc::DiscPatcher;
use legaia_patcher::encounter::SceneEncounters;

#[test]
fn no_formation_seats_the_unused_cort_records() {
    let Some(p) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let Ok(bytes) = std::fs::read(p) else {
        return;
    };
    let patcher = DiscPatcher::open(bytes).expect("open");
    let (mut rows, mut cort) = (0u32, 0u32);
    for idx in 0..patcher.entry_count() {
        let Some(scene) = patcher
            .read_entry(idx)
            .ok()
            .and_then(|e| SceneEncounters::locate(&e, idx))
        else {
            continue;
        };
        for f in 0..scene.formation_count() {
            let ids = scene.formation_ids(f);
            rows += 1;
            cort += u32::from(ids.contains(&0xB4));
            assert!(
                !ids.iter().any(|&i| (0xB7..=0xBA).contains(&i)),
                "entry {idx} formation {f} seats {ids:x?}"
            );
        }
    }
    assert!(rows > 500 && cort >= 2, "{rows} rows, {cort} Cort rows");
}
