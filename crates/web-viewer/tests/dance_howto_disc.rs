//! Disc-gated: the minigames page's dance **how-to** mode runs the engine's
//! Disco King tutorial actor beside a one-dancer run, as the play hosts do,
//! and draws its prompt through the shared tutorial builder.
//!
//! Structural facts only - no Sony bytes asserted. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::minigames::LegaiaMinigames;

#[test]
fn the_howto_mode_runs_the_tutorial_actor() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(disc).expect("read disc");
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(bytes).expect("load disc");
    eprintln!("[ran] dance how-to");
    assert!(mg.dance_start_mode(2, false), "the how-to run starts");
    assert!(
        mg.dance_tutorial_active(),
        "the tutorial actor is installed"
    );
    assert_eq!(
        mg.dance_run_kinds().len(),
        1,
        "the how-to floor is one dancer"
    );
    let state: serde_json::Value = serde_json::from_str(&mg.dance_state_json()).unwrap();
    eprintln!("how-to state: {state}");
    // The opening prompt draws (Yes / No thanks and the cursor).
    mg.dance_tutorial_step(0);
    let px = mg.dance_tutorial_rgba(320, 240);
    assert_eq!(px.len(), 320 * 240 * 4);
    let lit = px.as_chunks::<4>().0.iter().filter(|p| p[3] != 0).count();
    assert!(lit > 0, "the prompt draws");
    // Confirming the prompt fires a cue and moves the script on.
    let cue = mg.dance_tutorial_step(1);
    eprintln!("confirm cue {cue:#x}");
    assert_ne!(cue, 0, "the confirm fires its cue");
    // A qualifier run installs no tutorial.
    assert!(mg.dance_start_mode(0, false));
    assert!(!mg.dance_tutorial_active());
    assert_eq!(
        mg.dance_run_kinds().len(),
        3,
        "the qualifier floor is three"
    );
}
