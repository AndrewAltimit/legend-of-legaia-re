//! Not disc-gated: runs `examples/bring_your_own_game.rs` end to end, so the
//! worked example for other PS1 projects stays true in CI. Every input it
//! touches (TIM, SEQ, ISO image, emulator states) is built in code.

#[allow(dead_code)]
#[path = "../examples/bring_your_own_game.rs"]
mod example;

#[test]
fn the_generic_crates_run_on_a_made_up_game() {
    let out = std::env::temp_dir().join(format!("legaia-byog-test-{}", std::process::id()));
    let r = example::run(&out).expect("every step of the example holds");
    assert!(r.png.exists(), "PNG written");
    assert!(r.disc.exists(), "disc image written");
    assert_eq!(r.seq_events, 5);
    assert_eq!(r.iso_files, ["DATA/LEVEL1.BIN", "MYGAME.EXE"]);
    assert_eq!(r.ram_diff_regions, 2);
    std::fs::remove_dir_all(&out).ok();
}
