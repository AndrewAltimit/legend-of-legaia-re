//! Disc-gated: five scenes the selector opens on their story cutscene, one
//! per way a beat is started, each driven through the hosts' arm
//! (`SceneHost::arm_picker_story_entry`) from the free-roam staging, followed
//! to its end with Cross paging the text.
//!
//! | Picker name | Scene | Started by | Record |
//! |---|---|---|---|
//! | Buma | `bubu1` | the `map03` thaw hop's arrival tile `(0x46, 0x0F)` | `P2[1]` |
//! | Buma (frozen) | `bubu2` | the `map03` entrance tile `(0x45, 0x13)`, C2 `0x608` + `0x3D3` raised | `P2[3]` |
//! | Bio Castle (E) | `jouine` | the entry script's `0x44`, behind `jouind`'s hand-off flag `0x512` | `P2[5]` |
//! | Noa's dream (Conkram) | `urudre2` | the entry script's `44 37`, behind `uru` `P2[33]`'s `0x3F7` | `P2[9]` |
//! | Ra-Seru scene (2) | `juui2` | Terra's trigger row two tiles up the ramp from the `noaru` warp | `P2[16]` |
//!
//! Each beat must engage the player on the first ticks, run its own record,
//! and hand control back in the scene - except Noa's dream, whose record ends
//! in its `0x3F` to `map01`, as retail's does. A second pick clears the flags
//! the first one raised.
//!
//! **The party.** Each beat runs with the party retail has there: Vahn, Noa
//! and Gala, Vahn leading. No record on the way into these scenes changes the
//! party - the disc's party ops (`0x3C` / `0x3D` / `4C 0x`) sit elsewhere -
//! and the last one before them leaves the three together: `uru` `P2[30]` /
//! `P2[32]` / `P2[34]` (`3D 00 3D 01 3D 02 3C 00 3C 01 3C 02`, the presence
//! flags `0x10..0x12` raised with them) after each Uru Mais dream, `nilboa`
//! `P2[28]` after the Nivora split. Retail memory-card saves agree: the party
//! word at `0x80084594` reads three members, Vahn leading (`0x80084597 = 0`),
//! in the card-loaded `noaru`, `rugi`, `jouind`, `deene` and `conc2` states.
//! Terra's "Noa, wait a minute" in `juui2` speaks to a member of that party;
//! the field shows only the leader. Noa's Dreams (`dream`) is the row whose
//! own entry script hands Noa the lead (`4C 01` in `P1[0]`), and the
//! selector keeps it.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset - CI runs without disc data.

use std::path::PathBuf;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::{BeatTrigger, SceneHost, picker_entry_beat};
use legaia_engine_system::input::PadButton;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// `(scene, how it starts, record, where the beat ends, the leader)`;
/// `None` = control comes back in the scene. Leader ids: `0` Vahn, `1` Noa.
const CASES: &[(&str, BeatTrigger, u8, Option<&str>, u8)] = &[
    ("bubu1", BeatTrigger::ArrivalTile, 1, None, 0),
    ("bubu2", BeatTrigger::ArrivalTile, 3, None, 0),
    ("jouine", BeatTrigger::EntryScript, 5, None, 0),
    ("urudre2", BeatTrigger::EntryScript, 9, Some("map01"), 0),
    ("juui2", BeatTrigger::NearArrival, 16, None, 0),
    ("dream", BeatTrigger::ArrivalTile, 0, Some("bylon"), 1),
];

/// The party every case runs with: Vahn, Noa and Gala.
const PARTY: [u8; 3] = [0, 1, 2];

/// The partition-2 record whose body the running context executes.
fn running_record(h: &SceneHost) -> Option<u8> {
    let sc = h.scene.as_ref()?;
    let man = sc.field_man_payload(&h.index).ok()??;
    let mf = legaia_scene_formats::man_section::parse(&man).ok()?;
    let w = &h.world;
    let body = w
        .cutscene
        .timeline
        .as_ref()
        .filter(|t| !t.is_done())
        .or_else(|| w.field_vm.helper_contexts.iter().find(|c| c.holds_player()))?
        .bytecode
        .clone();
    (0..=255u8).find(|&i| {
        legaia_engine_field::man_field_scripts::partition_record_span(&mf, &man, 2, i as usize)
            .is_some_and(|(start, _, len)| man.get(start..start + len) == Some(&body[..]))
    })
}

fn scene_name(h: &SceneHost) -> String {
    h.scene.as_ref().map(|s| s.name.clone()).unwrap_or_default()
}

#[test]
fn the_selector_opens_each_scene_on_its_cutscene() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut base = SceneHost::open_disc(&disc).expect("open disc host");
    let index = base.index.clone();
    let cat = base.arrival_catalog();
    // The hosts' picker party (`World::seed_picker_party`), as both hosts
    // arm it.
    let mut defaults = base.new_game_defaults.clone();
    if let Some(d) = defaults.as_mut() {
        d.picker_party = true;
    }
    for &(scene, trigger, record, ends_in, lead) in CASES {
        let beat = picker_entry_beat(scene).unwrap_or_else(|| panic!("[{scene}] has no row"));
        assert_eq!(beat.trigger, trigger, "[{scene}] trigger");
        assert_eq!(beat.record, record, "[{scene}] record");

        let mut h = SceneHost::new(index.clone());
        h.set_arrival_catalog(cat.clone());
        h.new_game_defaults = defaults.clone();
        h.world.stage_picker_entry(scene, false);
        assert!(h.arm_picker_story_entry(scene), "[{scene}] arm refused");
        h.enter_field_scene(scene, 0).expect("enter");

        let mut started = None;
        let mut ran = None;
        let mut outcome = None;
        for t in 0..9000u32 {
            let pad = if started.is_some() && t % 2 == 0 {
                PadButton::Cross.mask()
            } else {
                0
            };
            h.world.set_pad(pad);
            h.tick().expect("tick");
            assert_ne!(h.world.mode, SceneMode::Battle, "[{scene}] the beat fights");
            let now = scene_name(&h);
            if !now.eq_ignore_ascii_case(scene) {
                outcome = Some((Some(now), t - started.unwrap_or(t)));
                break;
            }
            let active = h.world.script_context_engages_player();
            match started {
                None if active => {
                    started = Some(t);
                    ran = running_record(&h);
                    assert_eq!(h.world.field_leader(), lead, "[{scene}] the beat's leader");
                    assert_eq!(
                        h.world.present_party_list(),
                        PARTY,
                        "[{scene}] the beat's party"
                    );
                }
                None => assert!(t < 4, "[{scene}] nothing started on the first ticks"),
                Some(t0) if !active => {
                    outcome = Some((None, t - t0));
                    break;
                }
                _ => {}
            }
        }
        let (left_for, ticks) = outcome.unwrap_or_else(|| panic!("[{scene}] never ended"));
        println!("[{scene}] P2[{ran:?}] ran {ticks} ticks, ends in {left_for:?}");
        assert_eq!(ran, Some(record), "[{scene}] the record that ran");
        assert!(
            ticks >= 300,
            "[{scene}] a cutscene, not a latch ({ticks} ticks)"
        );
        assert_eq!(
            left_for.as_deref().map(str::to_ascii_lowercase).as_deref(),
            ends_in,
            "[{scene}] where the beat hands control back"
        );
        if ends_in.is_none() {
            // Control is back: the player can walk.
            assert!(
                !h.world.script_context_engages_player(),
                "[{scene}] still held"
            );
        }
    }
}

/// The flags a pick raised for its beat go with the next pick's staging; a
/// flag the session already held is not touched.
#[test]
fn a_later_pick_clears_the_beat_flags() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut h = SceneHost::open_disc(&disc).expect("open disc host");
    let beat = picker_entry_beat("bubu2").expect("bubu2 row");
    assert!(beat.flags.contains(&0x608) && beat.flags.contains(&0x3D3));
    // A flag the session already holds stays the session's.
    h.world.system_flag_set(0x02F);
    h.world.stage_picker_entry("bubu2", false);
    assert!(h.arm_picker_story_entry("bubu2"));
    for &f in beat.flags {
        assert!(h.world.system_flag_test(f), "{f:#x} raised");
    }
    h.world.stage_picker_entry("garmel", false);
    assert!(
        h.world.system_flag_test(0x02F),
        "the session's own flag stays"
    );
    for &f in beat.flags.iter().filter(|&&f| f != 0x02F) {
        assert!(
            !h.world.system_flag_test(f),
            "{f:#x} cleared by the next pick"
        );
    }
}
