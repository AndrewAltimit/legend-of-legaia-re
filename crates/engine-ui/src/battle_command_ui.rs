//! Battle **command-chip cluster** - the framed chips a party member's
//! command menu puts up, shared by every host that draws it.
//!
//! Retail's battle command UI is not a list. It is a cluster of plate
//! chips seated around a D-pad glyph, and every rect, seat and palette
//! below is packet-pinned in
//! [`legaia_engine_vm::battle_chrome`](../../engine-vm) out of a mednafen
//! battle save state's libgpu ordering table - a RAM image *is* the
//! frame's display list, so the seats are read out of the queued packet
//! words rather than measured off a screenshot. See
//! `docs/subsystems/battle.md` § battle-screen chrome.
//!
//! ## What the pins say
//!
//! * A chip is one **blue plate run** (`battle_chrome::PLATE_BLUE`): an
//!   8x20 left cap, 16x20 body tiles filling the interior with the final
//!   tile **clipped** to the remainder, and an 8x20 right cap. Total
//!   width is `interior + 16`.
//! * **One shared interior width per cluster** - a chip is sized to the
//!   cluster, not to its own label - and labels are **left-aligned** at
//!   the interior's left edge, four rows down.
//! * The **per-actor diamond** is centred `(228, 70)` with `dx = 44`,
//!   `dy = 32` and interior `48`, which seats its four plates at
//!   `(196, 28)` / `(152, 60)` / `(240, 60)` / `(196, 92)`.
//! * The round-level **`Begin | Run` pair** is centred `(160, 92)` with
//!   `dx = 38` and interior `36` - identical for a solo party and a trio.
//! * A **D-pad glyph** sits at the cluster centre: texels `(0, 112)`,
//!   16x16 through sub-palette 7, drawn 15x15.
//! * An **unavailable command still draws its chip**, carrying a single
//!   `-` glyph in place of the label.
//!
//! One law derives all of it from disc data: each surface is a record of
//! the screen-element placement table at `0x80076C10`, and
//! `pen = (rec.x, rec.y - 2)`, `plate = (rec.x - 8, rec.y - 6)` sized
//! `(rec.w + 16, 20)`.
//!
//! ## The two clusters are three phases
//!
//! Retail runs the clusters in **different phases** and their pinned
//! rects overlap, so it can never show two at once. Which phase a frame
//! is in is [`ChipPhase`], and each one names the seats its chips take:
//!
//! | Retail `ctx[+0x06]` | [`ChipPhase`] | Cluster | Chips |
//! |---|---|---|---|
//! | `0x1E` | `RoundPrompt` | [`CLUSTER_TOP_LEVEL`] | `Begin` \| `Run` |
//! | `0x28` | `CommandRing` | [`CLUSTER_COMMAND`] | `Item` / `Attack` / magic / `Spirit` |
//! | `0x78` | `AttackMode` | [`CLUSTER_COMMAND`] | `Auto` \| `Command` |
//!
//! The ring's four arms are the placement table's records `8..=11` in up /
//! left / right / down order, and the attack-mode pair re-uses the same
//! diamond's left and right arms (records `85` / `84`). Every seat here
//! is pinned - there is no invented row.
//!
//! Retail's selection cue is not pinned, so the port supplies one: the
//! chip under the cursor keeps its full plate tint and a white label
//! while the rest dim.
//!
//! Geometry is mirrored from `battle_chrome` as literals because
//! `engine-ui` sits below `engine-vm` in the crate graph; `engine-shell`'s
//! HUD tests pin the two sets equal, which is the only thing that keeps
//! the copy honest.

use crate::{SpriteDraw, TextDraw};
use legaia_asset::title_pak;

// --------------------------------------------------------------- geometry

/// Width of a plate run's left / right cap.
pub const PLATE_CAP_W: i32 = 8;
/// Height of every plate on the battle screen.
pub const PLATE_H: i32 = 20;
/// Width of one body tile; the last tile of a run is clipped to fit.
pub const PLATE_BODY_W: i32 = 16;
/// Rows from a chip's plate top down to its label pen.
pub const CHIP_LABEL_DY: i32 = 4;
/// Screen size the 16x16 D-pad glyph is drawn at.
pub const DPAD_DRAW: u32 = 15;

/// Which arm of a cluster a chip takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipSeat {
    Up,
    Left,
    Right,
    Down,
}

/// A cluster of command chips around a D-pad glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChipCluster {
    /// Screen centre - also the D-pad glyph's centre.
    pub centre: (i32, i32),
    /// Centre-to-chip-centre distance along the horizontal arms.
    pub dx: i32,
    /// Centre-to-chip-centre distance along the vertical arms; `0` when
    /// the cluster has none.
    pub dy: i32,
    /// Interior width every chip in the cluster is built at.
    pub interior_w: i32,
}

/// The per-actor command diamond (`battle_chrome::CLUSTER_COMMAND`).
pub const CLUSTER_COMMAND: ChipCluster = ChipCluster {
    centre: (228, 70),
    dx: 44,
    dy: 32,
    interior_w: 48,
};

/// The round-open `Begin | Run` prompt (`battle_chrome::CLUSTER_TOP_LEVEL`).
///
/// Retail's own labels for the pair are static `SCUS_942.54` rodata the
/// placement records point at before the battle overlay is even loaded
/// (`legaia_asset::battle_ui_strings::{SCUS_BEGIN, SCUS_RUN}`), which is
/// the disc-side proof that this cluster is the round prompt and not a
/// second command row.
pub const CLUSTER_TOP_LEVEL: ChipCluster = ChipCluster {
    centre: (160, 92),
    dx: 38,
    dy: 0,
    interior_w: 36,
};

impl ChipCluster {
    /// On-screen width of one of this cluster's plates.
    pub const fn plate_width(&self) -> i32 {
        self.interior_w + 2 * PLATE_CAP_W
    }

    /// Top-left corner of the plate at `seat`.
    pub const fn plate_origin(&self, seat: ChipSeat) -> (i32, i32) {
        let half = self.plate_width() / 2;
        let (dx, dy) = match seat {
            ChipSeat::Up => (0, -self.dy),
            ChipSeat::Left => (-self.dx, 0),
            ChipSeat::Right => (self.dx, 0),
            ChipSeat::Down => (0, self.dy),
        };
        (self.centre.0 + dx - half, self.centre.1 + dy - PLATE_H / 2)
    }

    /// Label pen for the chip at `seat`: the interior's left edge, four
    /// rows down. Labels are left-aligned in the interior, not centred.
    pub const fn label_seat(&self, seat: ChipSeat) -> (i32, i32) {
        let (x, y) = self.plate_origin(seat);
        (x + PLATE_CAP_W, y + CHIP_LABEL_DY)
    }

    /// The D-pad glyph's drawn rect, centred on the cluster.
    pub const fn dpad_rect(&self) -> (i32, i32, u32, u32) {
        (self.centre.0 - 8, self.centre.1 - 8, DPAD_DRAW, DPAD_DRAW)
    }
}

// ---------------------------------------------------------------- seating

/// Which cluster + arm one chip takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandSeat {
    /// A seat on the packet-pinned per-actor diamond.
    Diamond(ChipSeat),
    /// A seat on the pinned round-prompt pair ([`CLUSTER_TOP_LEVEL`]).
    TopLevel(ChipSeat),
}

impl CommandSeat {
    /// The cluster this seat belongs to.
    pub const fn cluster(self) -> ChipCluster {
        match self {
            Self::Diamond(_) => CLUSTER_COMMAND,
            Self::TopLevel(_) => CLUSTER_TOP_LEVEL,
        }
    }

    /// The arm within the cluster.
    pub const fn seat(self) -> ChipSeat {
        match self {
            Self::Diamond(s) | Self::TopLevel(s) => s,
        }
    }

    /// Plate top-left of this seat.
    pub const fn plate_origin(self) -> (i32, i32) {
        self.cluster().plate_origin(self.seat())
    }

    /// Label pen of this seat.
    pub const fn label_seat(self) -> (i32, i32) {
        self.cluster().label_seat(self.seat())
    }
}

/// Seat of each entry of `battle_input::BattleCommand::MENU`, in that
/// order (`Item`, `Attack`, magic, `Spirit`) - the diamond's up, left,
/// right and down arms, which is the order the placement table's records
/// `8..=11` sit in.
pub const MENU_SEATS: [CommandSeat; 4] = [
    CommandSeat::Diamond(ChipSeat::Up),
    CommandSeat::Diamond(ChipSeat::Left),
    CommandSeat::Diamond(ChipSeat::Right),
    CommandSeat::Diamond(ChipSeat::Down),
];

/// Seat of each entry of `battle_input::RoundChoice::PROMPT` (`Begin`,
/// `Run`) - the round prompt's pinned left / right pair.
pub const ROUND_PROMPT_SEATS: [CommandSeat; 2] = [
    CommandSeat::TopLevel(ChipSeat::Left),
    CommandSeat::TopLevel(ChipSeat::Right),
];

/// Seat of each entry of `battle_input::AttackMode::PROMPT` (`Auto`,
/// `Command`) - the diamond's own left / right arms, which is where the
/// placement records `85` / `84` put them.
pub const ATTACK_MODE_SEATS: [CommandSeat; 2] = [
    CommandSeat::Diamond(ChipSeat::Left),
    CommandSeat::Diamond(ChipSeat::Right),
];

/// Which selection surface a frame is drawing - the port's mirror of
/// retail's flow byte `ctx[+0x06]` for the three states that put chips up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChipPhase {
    /// Retail `0x1E` - the round-open `Begin | Run` prompt.
    RoundPrompt,
    /// Retail `0x28` - the four-arm command ring.
    #[default]
    CommandRing,
    /// Retail `0x78` - the `Auto | Command` attack-mode prompt.
    AttackMode,
}

impl ChipPhase {
    /// The seats this phase's chips take, in chip order.
    pub const fn seats(self) -> &'static [CommandSeat] {
        match self {
            Self::RoundPrompt => &ROUND_PROMPT_SEATS,
            Self::CommandRing => &MENU_SEATS,
            Self::AttackMode => &ATTACK_MODE_SEATS,
        }
    }

    /// The cluster whose centre carries this phase's face-button glyph.
    /// Retail draws it every frame of all three states - at `(152, 84)` for
    /// the round prompt (`801d102c`) and `(220, 62)` for the ring and the
    /// attack-mode prompt (`801d1188` / `801d16e8`).
    pub const fn cluster(self) -> ChipCluster {
        match self {
            Self::RoundPrompt => CLUSTER_TOP_LEVEL,
            Self::CommandRing | Self::AttackMode => CLUSTER_COMMAND,
        }
    }
}

// ------------------------------------------------------------ atlas rects

/// Where the chip pieces sit in a host's sprite atlas.
///
/// The plate 3-slice is the battle chrome's own blue row; the D-pad glyph
/// is the same `(0, 112)` cell the arts-input screen draws
/// ([`title_pak::OVERLAY_SYSTEM_UI_ARTS_DPAD`]), which is why both
/// clusters can share one baked atlas cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandChipAtlas {
    pub plate_cap_l: (u32, u32, u32, u32),
    pub plate_body: (u32, u32, u32, u32),
    pub plate_cap_r: (u32, u32, u32, u32),
    pub dpad: (u32, u32, u32, u32),
}

impl CommandChipAtlas {
    /// The pieces at their natural system-UI sheet coordinates, which is
    /// also where `engine-core`'s baked atlas seats every one of them.
    pub const SHEET: Self = Self {
        plate_cap_l: title_pak::OVERLAY_SYSTEM_UI_BATTLE_PLATE_CAP_L,
        plate_body: title_pak::OVERLAY_SYSTEM_UI_BATTLE_PLATE_BODY,
        plate_cap_r: title_pak::OVERLAY_SYSTEM_UI_BATTLE_PLATE_CAP_R,
        dpad: title_pak::OVERLAY_SYSTEM_UI_ARTS_DPAD,
    };

    /// The pieces as a [`crate::BattleChromeRects`] carries them, keeping
    /// the shared D-pad cell.
    pub fn from_battle_chrome(r: &crate::BattleChromeRects) -> Self {
        Self {
            plate_cap_l: r.plate_cap_l,
            plate_body: r.plate_body,
            plate_cap_r: r.plate_cap_r,
            dpad: title_pak::OVERLAY_SYSTEM_UI_ARTS_DPAD,
        }
    }
}

// ------------------------------------------------------------------ frame

/// One chip of the command menu.
#[derive(Clone, Copy, Debug)]
pub struct CommandChipView<'a> {
    /// The command's menu label.
    pub label: &'a str,
    /// `false` draws the chip with a single `-` in place of the label -
    /// retail keeps the plate and drops the word.
    pub enabled: bool,
}

/// Everything the cluster draws from. Hosts project their live command
/// session into this; `chips` is in the order the phase's own chip list
/// carries and is seated by [`ChipPhase::seats`].
#[derive(Clone, Copy, Debug)]
pub struct BattleCommandMenuFrame<'a> {
    pub chips: &'a [CommandChipView<'a>],
    /// Index of the chip under the cursor, if any.
    pub cursor: Option<usize>,
    /// Which selection surface these chips belong to.
    pub phase: ChipPhase,
}

/// Plate tint of the chip under the cursor - retail's selection cue is
/// not pinned, so the port draws the selected plate at full brightness.
pub const CHIP_TINT_SELECTED: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Plate tint of an unselected chip.
pub const CHIP_TINT_IDLE: [f32; 4] = [0.72, 0.74, 0.82, 1.0];
/// Plate tint of a chip whose command cannot be chosen.
pub const CHIP_TINT_DISABLED: [f32; 4] = [0.52, 0.52, 0.58, 1.0];
/// Label ink of the chip under the cursor.
pub const LABEL_SELECTED: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Label ink of an unselected chip.
pub const LABEL_IDLE: [f32; 4] = [0.70, 0.75, 0.86, 1.0];
/// Label ink of the `-` an unavailable command draws.
pub const LABEL_DISABLED: [f32; 4] = [0.55, 0.55, 0.58, 1.0];

fn tints(selected: bool, enabled: bool) -> ([f32; 4], [f32; 4]) {
    match (enabled, selected) {
        (false, _) => (CHIP_TINT_DISABLED, LABEL_DISABLED),
        (true, true) => (CHIP_TINT_SELECTED, LABEL_SELECTED),
        (true, false) => (CHIP_TINT_IDLE, LABEL_IDLE),
    }
}

// --------------------------------------------------------------- builders

/// Compose one plate run: a left cap at `x`, body tiles filling
/// `interior_w` with the **last tile clipped** to the remainder, and a
/// closing right cap. The clipped final tile is the retail behaviour, not
/// a rounding of it.
pub fn plate_run(
    atlas: &CommandChipAtlas,
    x: i32,
    y: i32,
    interior_w: i32,
    color: [f32; 4],
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1);
    let mut out = Vec::new();
    let mut blit = |src: (u32, u32, u32, u32), bx: i32, w: i32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + bx * scale as i32,
                stage_origin.1 + y * scale as i32,
                w as u32 * scale,
                PLATE_H as u32 * scale,
            ),
            src: (src.0, src.1, w as u32, PLATE_H as u32),
            color,
        });
    };
    blit(atlas.plate_cap_l, x, PLATE_CAP_W);
    let interior_x = x + PLATE_CAP_W;
    let mut done = 0;
    while done < interior_w {
        let w = PLATE_BODY_W.min(interior_w - done);
        blit(atlas.plate_body, interior_x + done, w);
        done += w;
    }
    blit(atlas.plate_cap_r, interior_x + interior_w, PLATE_CAP_W);
    out
}

/// The command cluster's sprite half: one plate run per chip, plus the
/// D-pad glyph at the centre of each cluster that has a chip on it.
///
/// `stage_origin` / `stage_scale` follow the same convention as the rest
/// of this crate's chrome builders: stage pixels on the canonical 320x240
/// stage, multiplied out to the surface.
pub fn battle_command_chip_sprites(
    atlas: &CommandChipAtlas,
    frame: &BattleCommandMenuFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1);
    let mut out: Vec<SpriteDraw> = Vec::new();
    let seats = frame.phase.seats();
    let mut any = false;
    for (i, chip) in frame.chips.iter().enumerate() {
        let Some(seat) = seats.get(i).copied() else {
            continue;
        };
        any = true;
        let (plate_tint, _) = tints(frame.cursor == Some(i), chip.enabled);
        let (px, py) = seat.plate_origin();
        out.extend(plate_run(
            atlas,
            px,
            py,
            seat.cluster().interior_w,
            plate_tint,
            stage_origin,
            stage_scale,
        ));
    }
    // The face-button glyph sits at the centre of whichever cluster this
    // phase is drawing - retail draws exactly one, every frame of all
    // three selection states.
    if any {
        let (dx, dy, dw, dh) = frame.phase.cluster().dpad_rect();
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + dx * scale as i32,
                stage_origin.1 + dy * scale as i32,
                dw * scale,
                dh * scale,
            ),
            src: atlas.dpad,
            color: CHIP_TINT_SELECTED,
        });
    }
    out
}

/// The command cluster's text half: each chip's label left-aligned in its
/// interior, or a single `-` when the command cannot be chosen.
pub fn battle_command_chip_text(
    font: &legaia_font::Font,
    frame: &BattleCommandMenuFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out: Vec<TextDraw> = Vec::new();
    let seats = frame.phase.seats();
    for (i, chip) in frame.chips.iter().enumerate() {
        let Some(seat) = seats.get(i).copied() else {
            continue;
        };
        let (_, ink) = tints(frame.cursor == Some(i), chip.enabled);
        let text = if chip.enabled { chip.label } else { "-" };
        let layout = font.layout_ascii(text);
        let (lx, ly) = seat.label_seat();
        let pen = (stage_origin.0 + lx * scale, stage_origin.1 + ly * scale);
        for g in &layout.glyphs {
            out.push(TextDraw {
                dst: (
                    pen.0 + g.dst_x * scale,
                    pen.1 + g.dst_y * scale,
                    g.width * scale as u32,
                    g.height * scale as u32,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color: ink,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chips(n: usize) -> Vec<CommandChipView<'static>> {
        const LABELS: [&str; 4] = ["Item", "Attack", "Meta", "Spirit"];
        (0..n)
            .map(|i| CommandChipView {
                label: LABELS[i % LABELS.len()],
                enabled: true,
            })
            .collect()
    }

    fn ring<'a>(chips: &'a [CommandChipView<'a>], cursor: usize) -> BattleCommandMenuFrame<'a> {
        BattleCommandMenuFrame {
            chips,
            cursor: Some(cursor),
            phase: ChipPhase::CommandRing,
        }
    }

    /// Every seat of the per-actor diamond, against the packet-pinned
    /// values `legaia_engine_vm::battle_chrome` carries.
    #[test]
    fn command_diamond_reproduces_the_submenu_capture() {
        let c = CLUSTER_COMMAND;
        assert_eq!(c.plate_origin(ChipSeat::Up), (196, 28));
        assert_eq!(c.plate_origin(ChipSeat::Left), (152, 60));
        assert_eq!(c.plate_origin(ChipSeat::Right), (240, 60));
        assert_eq!(c.plate_origin(ChipSeat::Down), (196, 92));
        assert_eq!(c.label_seat(ChipSeat::Up), (204, 32));
        assert_eq!(c.label_seat(ChipSeat::Left), (160, 64));
        assert_eq!(c.label_seat(ChipSeat::Down), (204, 96));
        assert_eq!(c.dpad_rect(), (220, 62, 15, 15));
        assert_eq!(c.plate_width(), 64);
    }

    #[test]
    fn top_level_pair_reproduces_the_begin_run_capture() {
        let c = CLUSTER_TOP_LEVEL;
        assert_eq!(c.plate_origin(ChipSeat::Left), (96, 82));
        assert_eq!(c.plate_origin(ChipSeat::Right), (172, 82));
        assert_eq!(c.label_seat(ChipSeat::Left), (104, 86));
        assert_eq!(c.label_seat(ChipSeat::Right), (180, 86));
        assert_eq!(c.dpad_rect(), (152, 84, 15, 15));
    }

    /// The 3-slice: full body tiles then a clipped remainder, caps at the
    /// ends. Both captured runs.
    #[test]
    fn plate_runs_clip_their_final_body_tile() {
        let seats = |run: &[SpriteDraw]| -> Vec<(i32, u32, u32)> {
            run.iter().map(|d| (d.dst.0, d.src.0, d.dst.2)).collect()
        };
        let a = plate_run(&CommandChipAtlas::SHEET, 196, 28, 48, [1.0; 4], (0, 0), 1);
        assert_eq!(
            seats(&a),
            vec![
                (196, 208, 8),
                (204, 192, 16),
                (220, 192, 16),
                (236, 192, 16),
                (252, 216, 8),
            ]
        );
        let b = plate_run(&CommandChipAtlas::SHEET, 96, 82, 36, [1.0; 4], (0, 0), 1);
        assert_eq!(
            seats(&b),
            vec![
                (96, 208, 8),
                (104, 192, 16),
                (120, 192, 16),
                (136, 192, 4),
                (140, 216, 8),
            ]
        );
    }

    /// Retail's two clusters overlap, which is what says they are two
    /// **phases** rather than two rows of one menu: no frame can draw both.
    /// That overlap is the whole reason the port stopped inventing a third
    /// row for the commands the diamond has no arm for.
    #[test]
    fn the_two_pinned_clusters_overlap_which_is_why_they_are_phases() {
        let overlaps = |a: (i32, i32), aw: i32, b: (i32, i32), bw: i32| {
            a.0 < b.0 + bw && b.0 < a.0 + aw && a.1 < b.1 + PLATE_H && b.1 < a.1 + PLATE_H
        };
        let down = CLUSTER_COMMAND.plate_origin(ChipSeat::Down);
        let dw = CLUSTER_COMMAND.plate_width();
        assert!(overlaps(
            down,
            dw,
            CLUSTER_TOP_LEVEL.plate_origin(ChipSeat::Right),
            CLUSTER_TOP_LEVEL.plate_width(),
        ));
        // Every seat any phase can draw stays inside the 320x240 stage.
        for phase in [
            ChipPhase::RoundPrompt,
            ChipPhase::CommandRing,
            ChipPhase::AttackMode,
        ] {
            for seat in phase.seats() {
                let (x, y) = seat.plate_origin();
                let w = seat.cluster().plate_width();
                assert!(x >= 0 && x + w <= 320, "{seat:?} left the stage");
                assert!(y >= 0 && y + PLATE_H <= 240, "{seat:?} left the stage");
            }
        }
    }

    /// Each phase seats its chips on pinned arms only, one chip per seat.
    #[test]
    fn every_phase_seats_its_chips_on_pinned_arms() {
        for phase in [
            ChipPhase::RoundPrompt,
            ChipPhase::CommandRing,
            ChipPhase::AttackMode,
        ] {
            let seats = phase.seats();
            let mut seen: Vec<(i32, i32)> = seats.iter().map(|s| s.plate_origin()).collect();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), seats.len(), "{phase:?} seats a chip twice");
        }
        // The ring is the diamond's four arms in up / left / right / down
        // order - the placement table's records 8..=11.
        assert_eq!(MENU_SEATS[0], CommandSeat::Diamond(ChipSeat::Up)); // Item
        assert_eq!(MENU_SEATS[1], CommandSeat::Diamond(ChipSeat::Left)); // Attack
        assert_eq!(MENU_SEATS[2], CommandSeat::Diamond(ChipSeat::Right)); // magic
        assert_eq!(MENU_SEATS[3], CommandSeat::Diamond(ChipSeat::Down)); // Spirit
        // The round prompt is the pinned pair, `Begin` left and `Run` right.
        assert_eq!(ROUND_PROMPT_SEATS[0].plate_origin(), (96, 82));
        assert_eq!(ROUND_PROMPT_SEATS[1].plate_origin(), (172, 82));
        // The attack-mode pair re-uses the ring's own left / right arms.
        assert_eq!(ATTACK_MODE_SEATS[0], MENU_SEATS[1]);
        assert_eq!(ATTACK_MODE_SEATS[1], MENU_SEATS[2]);
    }

    /// Each phase draws its own face-button glyph, at its own cluster
    /// centre - `(152, 84)` for the round prompt, `(220, 62)` for the two
    /// that sit on the diamond.
    #[test]
    fn one_plate_run_per_chip_plus_one_glyph_per_phase() {
        let expect = [
            (ChipPhase::RoundPrompt, 2usize, (152, 84, 15, 15)),
            (ChipPhase::CommandRing, 4, (220, 62, 15, 15)),
            (ChipPhase::AttackMode, 2, (220, 62, 15, 15)),
        ];
        for (phase, n, glyph) in expect {
            let all = chips(n);
            let sprites = battle_command_chip_sprites(
                &CommandChipAtlas::SHEET,
                &BattleCommandMenuFrame {
                    chips: &all,
                    cursor: Some(0),
                    phase,
                },
                (0, 0),
                1,
            );
            // Every chip is a 5-piece plate run (cap + 3 body + cap for the
            // 48-wide diamond interior, cap + 2 body + a clipped tile + cap
            // for the 36-wide prompt), plus the single glyph.
            assert_eq!(sprites.len(), n * 5 + 1, "{phase:?}");
            let g = sprites.last().unwrap();
            assert_eq!(g.src, title_pak::OVERLAY_SYSTEM_UI_ARTS_DPAD);
            assert_eq!(g.dst, glyph, "{phase:?} glyph drifted");
        }
    }

    /// A frame with no chips draws nothing at all - not even a lone glyph.
    #[test]
    fn an_empty_frame_draws_nothing() {
        let sprites = battle_command_chip_sprites(
            &CommandChipAtlas::SHEET,
            &BattleCommandMenuFrame {
                chips: &[],
                cursor: None,
                phase: ChipPhase::CommandRing,
            },
            (0, 0),
            1,
        );
        assert!(sprites.is_empty());
    }

    /// The pinned law: an unavailable command keeps its chip and draws a
    /// single `-` where the word would go. Retail's own `-` is the fifth
    /// slot of the Ra-Seru label run, so this is the disc's rule.
    #[test]
    fn an_unavailable_command_keeps_its_chip_and_draws_a_dash() {
        let font = legaia_font::Font::placeholder();
        let mut all = chips(4);
        all[3].enabled = false;
        let frame = ring(&all, 0);
        // The plate is still there.
        let sprites = battle_command_chip_sprites(&CommandChipAtlas::SHEET, &frame, (0, 0), 1);
        assert_eq!(sprites.len(), 4 * 5 + 1);
        let down_plate = CLUSTER_COMMAND.plate_origin(ChipSeat::Down);
        assert!(sprites.iter().any(|s| s.dst.1 == down_plate.1));
        // ... and its label is one glyph wide.
        let dash = font.layout_ascii("-").glyphs.len();
        let word = font.layout_ascii("Spirit").glyphs.len();
        let with = battle_command_chip_text(&font, &frame, (0, 0), 1).len();
        all[3].enabled = true;
        let without = battle_command_chip_text(&font, &ring(&all, 0), (0, 0), 1).len();
        assert_eq!(without - with, word - dash);
    }

    #[test]
    fn stage_scale_multiplies_every_seat() {
        let all = chips(4);
        let frame = ring(&all, 2);
        let one = battle_command_chip_sprites(&CommandChipAtlas::SHEET, &frame, (0, 0), 1);
        let two = battle_command_chip_sprites(&CommandChipAtlas::SHEET, &frame, (10, 20), 2);
        assert_eq!(one.len(), two.len());
        for (a, b) in one.iter().zip(two.iter()) {
            assert_eq!(b.dst.0, 10 + a.dst.0 * 2);
            assert_eq!(b.dst.1, 20 + a.dst.1 * 2);
            assert_eq!(b.dst.2, a.dst.2 * 2);
            assert_eq!(b.dst.3, a.dst.3 * 2);
            assert_eq!(b.src, a.src);
        }
    }

    /// Only the cursor's chip is drawn at full brightness.
    #[test]
    fn the_cursor_chip_is_the_only_bright_one() {
        let all = chips(4);
        let sprites =
            battle_command_chip_sprites(&CommandChipAtlas::SHEET, &ring(&all, 0), (0, 0), 1);
        let bright: Vec<&SpriteDraw> = sprites
            .iter()
            .filter(|s| s.color == CHIP_TINT_SELECTED)
            .collect();
        // The Item chip's five pieces, plus the face-button glyph.
        assert_eq!(bright.len(), 6);
        let item = CLUSTER_COMMAND.plate_origin(ChipSeat::Up);
        assert!(
            bright
                .iter()
                .any(|s| s.dst.0 == item.0 && s.dst.1 == item.1)
        );
    }
}
