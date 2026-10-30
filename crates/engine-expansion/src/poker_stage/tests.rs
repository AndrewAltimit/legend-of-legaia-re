use super::*;
use legaia_engine_modes::cards::table::sector_to_mode_angle;
use legaia_engine_modes::map_frame::MapFrame;
use legaia_engine_modes::poker::{PokerConfig, PokerSession};
use legaia_engine_modes::session::{AiTier, ModeSession, SeatKind};
use legaia_expansion_format::modes::{DuelSeatNpc, DuelSfx};

fn table() -> PokerTableFrame {
    PokerTableFrame {
        frame: MapFrame {
            map_id: "m".into(),
            spawn: String::new(),
            origin: [3000, 3000],
            unit: 1,
        },
        facing: sector_to_mode_angle(6),
        lift: 0.0,
        height: 64.0,
        radius: 130.0,
        seats: 4,
        draw_table: true,
        surface: None,
        sfx: DuelSfx::default(),
        players: (1..4)
            .map(|i| DuelSeatNpc {
                npc: format!("p{i}"),
                name: format!("Player {i}"),
                think: Some(2),
                fold: Some(3),
                cheer: Some(4),
                ..Default::default()
            })
            .collect(),
        spectators: Vec::new(),
    }
}

fn session(seed: u64) -> PokerSession {
    PokerSession::new(PokerConfig {
        seats: vec![SeatKind::Ai(AiTier::Regular); 4],
        buy_in: 400,
        max_hands: Some(4),
        ai_think_frames: 6,
        showdown_frames: 90,
        seed,
        ..PokerConfig::default()
    })
}

/// Run AI hands through the stage seen from `local`.
fn run(seed: u64, local: usize, staged: bool) -> (Vec<PokerStageFrame>, Vec<u64>) {
    let t = table();
    let mut s = session(seed);
    let mut stage = PokerStage::default();
    let (mut frames, mut hashes) = (Vec::new(), Vec::new());
    for _ in 0..20_000 {
        if s.is_finished() {
            break;
        }
        s.tick(&[0; 4]);
        hashes.push(s.state_hash());
        if staged {
            frames.push(stage.step(&t, &s.view(), local));
        }
    }
    (frames, hashes)
}

#[test]
fn the_stage_never_feeds_the_session() {
    let (_, a) = run(3, 0, true);
    let (_, b) = run(3, 0, false);
    assert_eq!(a, b);
    assert!(a.len() > 200);
}

#[test]
fn the_timeline_is_deterministic() {
    let (a, _) = run(5, 0, true);
    let (b, _) = run(5, 0, true);
    assert_eq!(a, b);
    let (c, _) = run(6, 0, true);
    assert_ne!(a, c);
}

#[test]
fn other_seats_hole_cards_stay_hidden_until_a_showdown() {
    for local in [0, 2] {
        let (frames, _) = run(7, local, true);
        let mut shown = false;
        for f in &frames {
            for (seat, h) in f.visible.holes.iter().enumerate() {
                if seat != local && h.is_some() {
                    shown = true;
                }
            }
            // No hidden card draws a face: a hole card of another seat
            // shows the back's rectangle unless the page holds its face.
            for c in f.cards.iter().filter(|c| c.deck < 6 && c.deck != local) {
                if f.visible.holes.get(c.deck).is_none_or(Option::is_none) {
                    assert_eq!(c.face_uv, c.back_uv, "seat {} card {}", c.deck, c.card);
                }
            }
            // The viewer's own cards always have their faces.
            assert!(f.visible.holes.get(local).is_some());
        }
        assert!(shown, "some showdown showed a hand");
    }
}

#[test]
fn every_seat_has_a_chair_and_the_viewer_the_near_one() {
    let t = table();
    let s = session(1);
    let v = s.view();
    for local in 0..4 {
        let mut st = PokerStage::default();
        let f = st.step(&t, &v, local);
        // The NPCs stand in their seats' chairs, never in the viewer's.
        let near = t.chair_pos(0);
        for n in &f.npcs {
            let p = n.pos.unwrap();
            assert!(
                (p[0] - near[0]).hypot(p[1] - near[1]) > 10.0,
                "{local}: {n:?}"
            );
        }
        assert_eq!(f.npcs.len(), if local == 0 { 3 } else { 2 });
    }
}

#[test]
fn chips_fly_and_pots_are_won() {
    let (frames, _) = run(9, 0, true);
    assert!(
        frames.iter().any(|f| f.stacks.len() > 4),
        "stacks on the felt"
    );
    assert!(frames.iter().any(|f| !f.amounts.is_empty()), "bets float");
    assert!(
        frames.iter().any(|f| f
            .banner
            .as_deref()
            .is_some_and(|b| b.contains("wins") || b.contains("You win"))),
        "a pot was won"
    );
    assert!(frames.iter().any(|f| !f.cues.is_empty()));
}

#[test]
fn a_contested_showdown_holds_the_felt() {
    let (frames, _) = run(7, 0, true);
    // Some frame shows another seat's hand face up, and the next several
    // frames still do (the hold), so the sweep has time to play.
    let shown: Vec<bool> = frames
        .iter()
        .map(|f| {
            f.visible
                .holes
                .iter()
                .enumerate()
                .any(|(s, h)| s != 0 && h.is_some())
        })
        .collect();
    let longest = shown.split(|&b| !b).map(<[bool]>::len).max().unwrap_or(0);
    assert!(longest >= 100, "a showdown stays up {longest} frames");
}
