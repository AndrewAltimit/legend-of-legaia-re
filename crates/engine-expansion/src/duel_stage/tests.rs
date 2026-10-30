use super::*;
use legaia_engine_modes::cards::duel::{DuelCard, DuelConfig, DuelSession};
use legaia_engine_modes::cards::table::sector_to_mode_angle;
use legaia_engine_modes::map_frame::MapFrame;
use legaia_engine_modes::session::{AiTier, ModeSession, SeatKind};
use legaia_expansion_format::cards::Rarity;
use legaia_expansion_format::modes::{DuelSeatNpc, DuelSfx};

fn table(facing: u8) -> DuelTableFrame {
    DuelTableFrame {
        frame: MapFrame {
            map_id: "m".into(),
            spawn: String::new(),
            origin: [4000, 3000],
            unit: 1,
        },
        facing: sector_to_mode_angle(facing),
        lift: 0.0,
        height: 60.0,
        board: 132.0,
        seat_distance: 150.0,
        draw_table: true,
        surface: None,
        sfx: DuelSfx::default(),
        opponent: Some(DuelSeatNpc {
            npc: "rival".into(),
            think: Some(3),
            cheer: Some(4),
            slump: Some(5),
            ..Default::default()
        }),
        spectators: vec![DuelSeatNpc {
            npc: "fan".into(),
            cheer: Some(6),
            ..Default::default()
        }],
    }
}

fn session(seed: u64) -> DuelSession {
    let card = |i: u8| DuelCard {
        id: format!("c{i}"),
        sides: [1 + i % 9, 1 + (i * 3) % 9, 1 + (i * 5) % 9, 1 + (i * 7) % 9],
        element: None,
        rarity: Rarity::Common,
    };
    DuelSession::new(DuelConfig {
        seats: [SeatKind::Ai(AiTier::Regular), SeatKind::Ai(AiTier::Expert)],
        decks: [(0..5).map(card).collect(), (5..10).map(card).collect()],
        elemental_cells: true,
        chain: true,
        stakes: false,
        win_booster: None,
        ai_think_frames: 20,
        seed,
    })
}

/// Run a whole AI duel through the stage; every frame's stage output and
/// the session's state hash.
fn run(seed: u64, staged: bool) -> (Vec<DuelStageFrame>, Vec<u64>) {
    let t = table(6);
    let mut s = session(seed);
    let mut stage = DuelStage::default();
    let (mut frames, mut hashes) = (Vec::new(), Vec::new());
    for _ in 0..3000 {
        if s.is_finished() {
            break;
        }
        s.tick(&[0, 0]);
        hashes.push(s.state_hash());
        if staged {
            frames.push(stage.step(&t, &s.view(), 0));
        }
    }
    // The stage plays on after the session ends (the results stage).
    if staged {
        let last = s.view();
        while stage.holds_results() {
            frames.push(stage.step(&t, &last, 0));
        }
    }
    (frames, hashes)
}

#[test]
fn the_stage_never_feeds_the_session() {
    let (_, with) = run(7, true);
    let (_, without) = run(7, false);
    assert_eq!(with, without);
    assert!(with.len() > 100, "the duel ran: {} frames", with.len());
}

#[test]
fn the_timeline_is_deterministic() {
    let (a, _) = run(11, true);
    let (b, _) = run(11, true);
    assert_eq!(a, b);
    // And not vacuous: another seed lays out another game.
    let (c, _) = run(12, true);
    assert_ne!(a, c);
}

#[test]
fn cards_deal_from_the_pile_into_the_hands() {
    let t = table(6);
    let s = session(1);
    let mut stage = DuelStage::default();
    let f0 = stage.step(&t, &s.view(), 0);
    assert_eq!(f0.cards.len(), 10);
    // Frame 0: every card on the dealer's pile, on the table.
    for c in &f0.cards {
        assert!(
            (c.height - t.top()).abs() < 3.0,
            "on the table: {}",
            c.height
        );
    }
    let mut f = f0;
    for _ in 0..ESTABLISH_FRAMES {
        f = stage.step(&t, &s.view(), 0);
    }
    // Dealt: the near hand behind the board's near edge, the far hand
    // beyond the far edge, each card standing up.
    let edge = t.board_depth() * 0.5;
    let (across, _) = t.axes(0);
    let c = t.centre();
    for card in &f.cards {
        let along = (card.centre[0] - c[0]) * across[0] + (card.centre[1] - c[1]) * across[1];
        let near = card.deck == 0;
        assert!(
            if near { along < -edge } else { along > edge },
            "deck {} card {} at {along}",
            card.deck,
            card.card
        );
        assert!(card.up[1] > 0.0, "standing up toward its holder");
    }
}

#[test]
fn grid_cards_sit_on_their_cells_facing_the_reader() {
    let (frames, _) = run(3, true);
    let last = frames.last().unwrap();
    let on_grid: Vec<_> = last
        .cards
        .iter()
        .filter(|c| (c.height - t_top()).abs() < 2.0)
        .collect();
    assert_eq!(on_grid.len(), 9, "a full grid");
    for c in on_grid {
        let cell = last
            .cells
            .iter()
            .position(|p| (p[0] - c.centre[0]).abs() < 0.5 && (p[1] - c.centre[1]).abs() < 0.5);
        assert!(cell.is_some(), "on a cell centre: {:?}", c.centre);
        // Flat, top edge toward the far seat.
        assert!(c.up[1].abs() < 1e-3);
    }
}

fn t_top() -> f32 {
    table(6).top()
}

#[test]
fn a_capture_turns_the_card_over_and_changes_its_colour_halfway() {
    // Whole games over a few seeds, so some move captures.
    let games: Vec<Vec<DuelStageFrame>> = (1..6).map(|seed| run(seed, true).0).collect();
    // Find a card whose shown owner changes; it must be mid-flip (its
    // face tipped on edge) around the change and lifted off the table.
    let mut seen = false;
    for w in games.iter().flat_map(|g| g.windows(2)) {
        for c in &w[1].cards {
            let Some(p) = w[0]
                .cards
                .iter()
                .find(|p| (p.deck, p.card) == (c.deck, c.card))
            else {
                continue;
            };
            if p.owner != c.owner {
                // The colour changes at the half turn, face down, with the
                // card lifted off its cell.
                assert!(c.height - t_top() > 8.0, "lifted mid-flip: {}", c.height);
                seen = true;
            }
        }
    }
    assert!(seen, "some capture flipped");
    // On its way round a flipping card stands on its long edge.
    let frames: Vec<&DuelStageFrame> = games.iter().flatten().collect();
    assert!(
        frames
            .iter()
            .flat_map(|f| &f.cards)
            .any(|c| c.right[1].abs() > 0.9 * c.right[0].hypot(c.right[2]) && c.up[1].abs() < 1e-3),
        "a card turned on edge"
    );
    // And each flip came with a burst and a flip cue.
    assert!(frames.iter().any(|f| {
        f.bursts
            .iter()
            .any(|b| matches!(b.kind, BurstKind::Capture | BurstKind::Chain))
    }));
}

#[test]
fn the_seats_and_the_npcs_stand_around_the_table() {
    let t = table(2);
    let s = session(2);
    let mut stage = DuelStage::default();
    let f = stage.step(&t, &s.view(), 0);
    let opp = f.npcs.iter().find(|n| n.npc == "rival").unwrap();
    let at = opp.pos.unwrap();
    let want = t.seat_pos(1);
    assert!((at[0] - want[0]).abs() < 1e-3 && (at[1] - want[1]).abs() < 1e-3);
    let fan = f.npcs.iter().find(|n| n.npc == "fan").unwrap();
    assert!(fan.pos.is_none(), "a spectator keeps its own tile");
    // The leader stands on the player's side of the board.
    let leader = stage.leader(&t);
    let (across, _) = t.axes(0);
    let c = t.centre();
    let along = (leader.pos[0] - c[0]) * across[0] + (leader.pos[1] - c[1]) * across[1];
    assert!(along < 0.0);
}

#[test]
fn the_far_seat_sees_the_table_turned_round() {
    let t = table(6);
    let s = session(4);
    let (mut a, mut b) = (DuelStage::default(), DuelStage::default());
    let fa = a.step(&t, &s.view(), 0);
    let fb = b.step(&t, &s.view(), 1);
    // Cell 0 is on the far side for both peers.
    let c = t.centre();
    let along = |p: [f32; 2], seat: usize| {
        let (ac, _) = t.axes(seat);
        (p[0] - c[0]) * ac[0] + (p[1] - c[1]) * ac[1]
    };
    assert!(along(fa.cells[0], 0) > 0.0);
    assert!(along(fb.cells[0], 1) > 0.0);
    // Each sees the other's seat as the opponent's.
    assert_ne!(fa.opponent_seat, fb.opponent_seat);
}

#[test]
fn the_reaction_shot_holds_the_results() {
    let (frames, _) = run(9, true);
    assert!(frames.iter().any(|f| f.outro));
    let last = frames.last().unwrap();
    assert!(last.outro);
    // The result banner was up during the outro.
    assert!(frames.iter().any(|f| {
        f.banner
            .as_deref()
            .is_some_and(|b| b == "You win!" || b == "You lose" || b == "Draw")
    }));
}

/// A watcher (a spectating peer) reads the winner by name: the end never
/// says "You win!" or "You lose" to someone with no seat.
#[test]
fn a_watcher_reads_the_winner_by_name() {
    let t = table(6);
    let mut s = session(9);
    let mut stage = DuelStage {
        watching: true,
        seat_names: ["Vahn".into(), "Rival".into()],
        ..DuelStage::default()
    };
    let mut banners = Vec::new();
    for _ in 0..3000 {
        if s.is_finished() {
            break;
        }
        s.tick(&[0, 0]);
        banners.extend(stage.step(&t, &s.view(), 0).banner);
    }
    let last = s.view();
    while stage.holds_results() {
        banners.extend(stage.step(&t, &last, 0).banner);
    }
    assert!(banners.iter().all(|b| !b.starts_with("You")), "{banners:?}");
    assert!(
        banners
            .iter()
            .any(|b| b == "Vahn wins!" || b == "Rival wins!" || b == "Draw"),
        "{banners:?}"
    );
}
