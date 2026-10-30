//! The multiplayer screens: the room screen (players, the host's game
//! pick, ready and start, chat, emotes) and the small room panel drawn over
//! the field while players roam.
//!
//! Like the mode screens these are the port's own, in the same flat style
//! and from one composition for both play hosts. The host's netplay driver
//! fills a [`NetScreen`] / [`NetPanel`] with plain rows; nothing here knows
//! about sockets or sessions.

use legaia_render_kernels::screen_prim::ScreenPrim;

use super::{
    Canvas, LINE, STAGE_H, STAGE_W, clip, footer, header, ink, labels_to_draws, pen, seat_color,
    wrap,
};
use crate::TextDraw;

/// One room member as the screen lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetPlayerRow {
    pub name: String,
    /// `None` = spectating.
    pub seat: Option<u8>,
    pub ready: bool,
    pub host: bool,
    /// This machine's player.
    pub you: bool,
}

/// One menu row: a label and, for a row that cycles, its current value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetMenuRow {
    pub label: String,
    pub value: Option<String>,
    /// Greyed out (the host cannot start until everyone is ready).
    pub enabled: bool,
}

/// The room screen.
#[derive(Debug, Clone, Copy)]
pub struct NetScreen<'a> {
    /// "Room K7QX2M" (or "Connecting...").
    pub title: &'a str,
    /// Where others join: the host's LAN address, or the relay's.
    pub address: Option<&'a str>,
    pub players: &'a [NetPlayerRow],
    pub rows: &'a [NetMenuRow],
    pub cursor: usize,
    /// Recent chat and room notices, newest last.
    pub chat: &'a [String],
    /// A chat line being typed.
    pub draft: Option<&'a str>,
    /// The last error or notice (a desync, a refused join).
    pub notice: Option<&'a str>,
}

/// The roaming panel: a corner box naming the room and its players.
#[derive(Debug, Clone, Copy)]
pub struct NetPanel<'a> {
    pub title: &'a str,
    pub players: &'a [NetPlayerRow],
    /// The newest chat lines.
    pub chat: &'a [String],
    pub notice: Option<&'a str>,
}

/// The field prop hunt's HUD: a corner panel over the field.
#[derive(Debug, Clone, Copy)]
pub struct NetHuntHud<'a> {
    /// "Choose a prop", "Hide!", "Hunt", "Round over".
    pub phase: &'a str,
    /// Seconds left in the phase.
    pub seconds: u32,
    pub round: u8,
    /// "Hunter - health 80", "Hider - slot12", "Found".
    pub role: &'a str,
    /// The control line for this phase and role.
    pub hint: &'a str,
    /// Hiders still hidden / all hiders.
    pub hidden: (usize, usize),
    /// The newest event line.
    pub last: Option<&'a str>,
}

/// The hunt HUD's flat prims, in stage pixels.
pub fn net_hunt_prims(h: &NetHuntHud<'_>) -> Vec<ScreenPrim> {
    compose_hunt(h).prims
}

/// The hunt HUD's labels, in stage pixels.
pub fn net_hunt_texts(font: &legaia_font::Font, h: &NetHuntHud<'_>) -> Vec<TextDraw> {
    labels_to_draws(font, &compose_hunt(h).labels)
}

/// The hunt HUD's labels as strings.
pub fn net_hunt_strings(h: &NetHuntHud<'_>) -> Vec<String> {
    compose_hunt(h).labels.into_iter().map(|l| l.text).collect()
}

/// Top of the corner panels drawn over the field (the hunt HUD, the roaming
/// room panel): below the field party HUD's three plates across the top of
/// the stage, which a toast or a level-up raises over the same corner.
pub const NET_PANEL_TOP: i32 = 56;

fn compose_hunt(h: &NetHuntHud<'_>) -> Canvas {
    let mut c = Canvas::default();
    let (x, y, w) = (STAGE_W - 150, NET_PANEL_TOP, 146);
    let lines = 4 + i32::from(h.last.is_some());
    c.panel(x, y, w, lines * 12 + 6, [10, 14, 24, 200], ink::HIGHLIGHT);
    c.text(
        format!("Prop hunt - round {}", h.round + 1),
        x + 4,
        y + 1,
        pen::GOLD,
    );
    c.text(
        format!("{} {}s", h.phase, h.seconds),
        x + 4,
        y + 13,
        pen::WHITE,
    );
    c.text(h.role, x + 4, y + 25, pen::GREEN);
    c.text(
        format!("Hidden {}/{}", h.hidden.0, h.hidden.1),
        x + 4,
        y + 37,
        pen::DIM,
    );
    if let Some(l) = h.last {
        c.text(l, x + 4, y + 49, pen::WHITE);
    }
    footer(&mut c, h.hint);
    c
}

/// Quick reactions ("* Noa cheers") floating over a networked game: a
/// stack of small bubbles down the right edge, newest at the bottom, each
/// fading as `age` (frames since it arrived, `0..REACTION_FRAMES`) runs out.
pub fn net_reaction_prims(reactions: &[(String, u32)]) -> Vec<ScreenPrim> {
    compose_reactions(reactions).prims
}

/// The reaction bubbles' labels, in stage pixels.
pub fn net_reaction_texts(font: &legaia_font::Font, reactions: &[(String, u32)]) -> Vec<TextDraw> {
    labels_to_draws(font, &compose_reactions(reactions).labels)
}

/// The reaction bubbles' labels as strings.
pub fn net_reaction_strings(reactions: &[(String, u32)]) -> Vec<String> {
    compose_reactions(reactions)
        .labels
        .into_iter()
        .map(|l| l.text)
        .collect()
}

/// Frames a quick reaction floats.
pub const REACTION_FRAMES: u32 = 150;

fn compose_reactions(reactions: &[(String, u32)]) -> Canvas {
    let mut c = Canvas::default();
    let shown = &reactions[reactions.len().saturating_sub(4)..];
    let base = STAGE_H - 40 - shown.len() as i32 * 16;
    for (i, (text, age)) in shown.iter().enumerate() {
        // Each bubble rises a few pixels as it ages.
        let rise = (*age as i32).min(REACTION_FRAMES as i32) / 30;
        let y = base + i as i32 * 16 - rise;
        let w = 132;
        let x = STAGE_W - w - 4;
        let alpha = if *age + 30 > REACTION_FRAMES {
            140
        } else {
            210
        };
        c.panel(x, y, w, 15, [40, 26, 74, alpha], [228, 76, 160, 255]);
        c.text(text.clone(), x + 4, y, pen::GOLD);
    }
    c
}

/// The room screen's flat prims, in stage pixels.
pub fn net_screen_prims(s: &NetScreen<'_>) -> Vec<ScreenPrim> {
    compose_screen(s).prims
}

/// The room screen's labels, in stage pixels.
pub fn net_screen_texts(font: &legaia_font::Font, s: &NetScreen<'_>) -> Vec<TextDraw> {
    labels_to_draws(font, &compose_screen(s).labels)
}

/// The room screen's labels as strings, in draw order.
pub fn net_screen_strings(s: &NetScreen<'_>) -> Vec<String> {
    compose_screen(s)
        .labels
        .into_iter()
        .map(|l| l.text)
        .collect()
}

/// The roaming panel's flat prims, in stage pixels.
pub fn net_panel_prims(p: &NetPanel<'_>) -> Vec<ScreenPrim> {
    compose_panel(p).prims
}

/// The roaming panel's labels, in stage pixels.
pub fn net_panel_texts(font: &legaia_font::Font, p: &NetPanel<'_>) -> Vec<TextDraw> {
    labels_to_draws(font, &compose_panel(p).labels)
}

/// The roaming panel's labels as strings.
pub fn net_panel_strings(p: &NetPanel<'_>) -> Vec<String> {
    compose_panel(p)
        .labels
        .into_iter()
        .map(|l| l.text)
        .collect()
}

fn player_label(p: &NetPlayerRow) -> String {
    let mut s = p.name.clone();
    if p.you {
        s.push_str(" (you)");
    }
    s
}

fn player_state(p: &NetPlayerRow) -> &'static str {
    match (p.host, p.seat, p.ready) {
        (true, _, _) => "Host",
        (_, None, _) => "Watching",
        (_, _, true) => "Ready",
        _ => "Not ready",
    }
}

fn compose_screen(s: &NetScreen<'_>) -> Canvas {
    let mut c = Canvas::default();
    c.rect(0, 0, STAGE_W, STAGE_H, ink::BACKDROP);
    header(&mut c, s.title, "Multiplayer");
    let mut y = 20;
    if let Some(a) = s.address {
        c.text(format!("Join at {a}"), 8, y, pen::DIM);
        y += LINE;
    }

    // Players down the left.
    let (px, pw) = (6, 150);
    let ph = s.players.len().max(1) as i32 * LINE + 6;
    c.panel(px, y + 2, pw, ph, ink::PANEL, ink::PANEL_EDGE);
    for (i, p) in s.players.iter().enumerate() {
        let ry = y + 4 + i as i32 * LINE;
        if let Some(seat) = p.seat {
            c.dot(px + 7, ry + 7, 2, seat_color(usize::from(seat)));
        }
        let ink = if p.you { pen::WHITE } else { pen::DIM };
        c.text(player_label(p), px + 13, ry, ink);
        let state_ink = if p.ready || p.host {
            pen::GREEN
        } else {
            pen::DIM
        };
        c.text_right(player_state(p), px + pw - 4, ry, state_ink);
    }

    // The menu down the right.
    let (mx, mw) = (162, 152);
    let mh = s.rows.len().max(1) as i32 * 16 + 6;
    c.panel(mx, y + 2, mw, mh, ink::PANEL, ink::PANEL_EDGE);
    for (i, r) in s.rows.iter().enumerate() {
        let ry = y + 5 + i as i32 * 16;
        let on = i == s.cursor;
        if on {
            c.rect(mx + 2, ry - 1, mw - 4, 16, ink::SELECT);
            c.rect(mx + 2, ry - 1, 3, 16, ink::HIGHLIGHT);
        }
        let ink = match (r.enabled, on) {
            (false, _) => pen::DIM,
            (true, true) => pen::WHITE,
            (true, false) => [0.78, 0.80, 0.84, 1.0],
        };
        c.text(r.label.clone(), mx + 9, ry, ink);
        if let Some(v) = &r.value {
            c.text_right(
                v.clone(),
                mx + mw - 6,
                ry,
                if on { pen::GOLD } else { pen::DIM },
            );
        }
    }

    // Chat along the bottom.
    let rows = 5usize;
    let cy = STAGE_H - 17 - (rows as i32 + 1) * LINE - 6;
    c.panel(
        6,
        cy,
        STAGE_W - 12,
        (rows as i32 + 1) * LINE + 4,
        ink::PANEL,
        ink::PANEL_EDGE,
    );
    for (i, l) in wrapped_tail(s.chat, SCREEN_CHAT_CHARS, rows)
        .into_iter()
        .enumerate()
    {
        c.text(l, 10, cy + 1 + i as i32 * LINE, pen::WHITE);
    }
    let draft = match s.draft {
        Some(d) => format!("> {d}_"),
        None => String::new(),
    };
    c.text(draft, 10, cy + 1 + rows as i32 * LINE, pen::GOLD);
    if let Some(n) = s.notice {
        c.text_centre(n, STAGE_W / 2, cy - LINE - 2, pen::RED);
    }
    footer(
        &mut c,
        if s.draft.is_some() {
            "Enter: send   Esc: cancel"
        } else {
            "Up/Down: row  Left/Right: change  Cross: go  Circle: close"
        },
    );
    c
}

/// Characters a roaming-panel line holds (the panel is 128 px, its
/// labels inset 4; a player row's dot takes 9 more).
pub const PANEL_CHARS: usize = 19;
/// Wrapped chat lines the roaming panel shows, newest last.
pub const PANEL_CHAT_LINES: usize = 5;
/// Characters a room-screen chat line holds.
const SCREEN_CHAT_CHARS: usize = 48;

/// `chat` wrapped to `width`, keeping the newest messages whole that fit in
/// `rows` lines (the newest one's last `rows` lines when it alone runs
/// longer), so no message shows from its middle.
fn wrapped_tail(chat: &[String], width: usize, rows: usize) -> Vec<String> {
    let mut kept: Vec<Vec<String>> = Vec::new();
    let mut used = 0;
    for msg in chat.iter().rev() {
        let lines = wrap(msg, width);
        if used + lines.len() > rows {
            if kept.is_empty() {
                kept.push(lines[lines.len() - rows..].to_vec());
            }
            break;
        }
        used += lines.len();
        kept.push(lines);
    }
    kept.into_iter().rev().flatten().collect()
}

fn compose_panel(p: &NetPanel<'_>) -> Canvas {
    let mut c = Canvas::default();
    let chat = wrapped_tail(p.chat, PANEL_CHARS, PANEL_CHAT_LINES);
    let notice: Vec<String> = p
        .notice
        .map(|n| wrap(n, PANEL_CHARS).into_iter().take(2).collect())
        .unwrap_or_default();
    let lines = p.players.len() + chat.len() + notice.len() + 1;
    let (w, x, y) = (128, STAGE_W - 132, NET_PANEL_TOP);
    let h = lines as i32 * 12 + 6;
    c.panel(x, y, w, h, [10, 14, 24, 200], ink::PANEL_EDGE);
    c.text(clip(p.title, PANEL_CHARS), x + 4, y + 1, pen::GOLD);
    let mut ry = y + 1 + 12;
    for pl in p.players {
        if let Some(seat) = pl.seat {
            c.dot(x + 7, ry + 7, 2, seat_color(usize::from(seat)));
        }
        c.text(
            clip(&player_label(pl), PANEL_CHARS - 2),
            x + 13,
            ry,
            if pl.you { pen::WHITE } else { pen::DIM },
        );
        ry += 12;
    }
    for l in chat {
        c.text(l, x + 4, ry, pen::WHITE);
        ry += 12;
    }
    for n in notice {
        c.text(n, x + 4, ry, pen::RED);
        ry += 12;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<NetPlayerRow> {
        vec![
            NetPlayerRow {
                name: "Vahn".into(),
                seat: Some(0),
                ready: false,
                host: true,
                you: true,
            },
            NetPlayerRow {
                name: "Noa".into(),
                seat: Some(1),
                ready: true,
                host: false,
                you: false,
            },
        ]
    }

    #[test]
    fn the_room_screen_lists_players_rows_and_chat() {
        let players = rows();
        let menu = vec![NetMenuRow {
            label: "Game".into(),
            value: Some("Practice Table".into()),
            enabled: true,
        }];
        let chat = vec!["Noa: hi".to_string()];
        let s = NetScreen {
            title: "Room K7QX2M",
            address: Some("192.168.1.4:7462"),
            players: &players,
            rows: &menu,
            cursor: 0,
            chat: &chat,
            draft: Some("hel"),
            notice: Some("Desync at frame 120"),
        };
        let strings = net_screen_strings(&s);
        for needle in [
            "Room K7QX2M",
            "Join at 192.168.1.4:7462",
            "Vahn (you)",
            "Host",
            "Ready",
            "Practice Table",
            "Noa: hi",
            "> hel_",
            "Desync at frame 120",
        ] {
            assert!(
                strings.iter().any(|l| l == needle),
                "`{needle}` missing: {strings:?}"
            );
        }
        assert!(net_screen_prims(&s).len() > 5);
    }

    #[test]
    fn the_hunt_hud_reads_the_phase_and_role() {
        let h = NetHuntHud {
            phase: "Hunt",
            seconds: 42,
            round: 0,
            role: "Hunter - health 80",
            hint: "Cross: tag a prop",
            hidden: (1, 2),
            last: Some("Seat 2: decoy!"),
        };
        let s = net_hunt_strings(&h);
        for needle in [
            "Prop hunt - round 1",
            "Hunt 42s",
            "Hunter - health 80",
            "Hidden 1/2",
        ] {
            assert!(s.iter().any(|l| l == needle), "{needle}: {s:?}");
        }
        assert!(!net_hunt_prims(&h).is_empty());
    }

    #[test]
    fn the_roaming_panel_names_the_room() {
        let players = rows();
        let p = NetPanel {
            title: "Room K7QX2M",
            players: &players,
            chat: &[],
            notice: None,
        };
        let s = net_panel_strings(&p);
        assert_eq!(s[0], "Room K7QX2M");
        assert!(s.contains(&"Noa".to_string()));
        assert!(!net_panel_prims(&p).is_empty());
    }

    #[test]
    fn long_round_labels_wrap_inside_the_roaming_panel() {
        let players = rows();
        let chat = vec![
            "Round 4/4: Grid Duel - Practice Duel (Vahn vs Noa)".to_string(),
            "Round 4: Practice Duel - Vahn won".to_string(),
        ];
        let p = NetPanel {
            title: "Room K7QX2M",
            players: &players,
            chat: &chat,
            notice: Some("A very long notice that a pack could raise"),
        };
        let s = net_panel_strings(&p);
        assert!(s.iter().all(|l| l.chars().count() <= PANEL_CHARS), "{s:?}");
        assert!(s.iter().any(|l| l.contains("Grid")), "{s:?}");
        // Whole messages, newest last; the panel grows to hold them.
        assert!(s.iter().any(|l| l.starts_with("Round 4/4")), "{s:?}");
        assert!(s.iter().any(|l| l.ends_with("Vahn won")), "{s:?}");
        assert_eq!(super::super::wrap("Supercalifragilistic", 8).len(), 3);
    }
}
