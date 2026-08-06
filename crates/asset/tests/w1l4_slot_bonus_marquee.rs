//! Disc-gated fixture for the casino slot machine's **bonus round and its
//! marquee** - the gate the five `legaia_asset::minigame_slot_scene` kernels
//! behind it sit on (`FUN_801CEC94`, `FUN_801CFFF0`, `FUN_801D069C`,
//! `FUN_801D0FA8`, `FUN_801D3230`).
//!
//! The bonus round is not a separate screen: it is the same machine with the
//! reels rotated onto their second strip, and the `0 x 0 x 0` -> `9 x 5 x 0`
//! strip across the top of the cabinet is the machine's own **dot-matrix
//! marquee**, not a caption drawn beside it. So the whole of what this exercises
//! is one chain - cut the 21 message bitmaps out of the art page
//! (`parse_messages`), decide what the matrix says this frame
//! (`compose_marquee_frame`), and blit it (`clear_dots` + `place_message`, via
//! `render_marquee`) - plus the reel cylinder the numerals are drawn on
//! (`reel_y` / `reel_z` / `reel_shade`).
//!
//! **The gate is seeded, not ground.** Nothing here rolls RNG waiting for a
//! jackpot: a bonus round's marquee state is six overlay globals
//! ([`MarqueeFrame`]), and this drives the exact tuples a round produces - the
//! round-pip strip between spins, the tally filling one claimed column at a
//! time, and the payout caption sliding in. The *machine-driven* half of the
//! same gate (a real bonus round played to its product payout, whose live state
//! feeds these same kernels) is
//! `crates/web-viewer/tests/w1l4_slot_bonus_marquee_ladder.rs`; this file is the
//! disc-data half and the one that can spawn the production CLI caller.
//!
//! The last rung spawns `CARGO_BIN_EXE_asset slot-scene`, which is the only
//! **non-test** caller these kernels have (`crates/asset/src/bin/asset/minigame.rs`).
//! A spawned `CARGO_BIN_EXE_*` inherits `LLVM_PROFILE_FILE`, so its profile is
//! merged into the run's - the same mechanism `w1a_fmv_ladder` uses to reach the
//! `mdec` subcommand.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` or the extracted PROT entries are
//! absent. No disc byte is asserted - only structure the disc must satisfy.

use std::path::{Path, PathBuf};

use legaia_asset::minigame_art;
use legaia_asset::minigame_slot_scene as slot;
use legaia_asset::slot_payout;

/// The slot overlay (PROT 0975) as the CLI reads it: the raw entry file. The
/// scene tables are addressed by **file** offset (`PAYLINE_TABLE_OFFSET` =
/// `0x4E68`), so no static-overlay lift is involved.
const OVERLAY_FILE: &str = "0975_other_game.BIN";
/// The five-TIM art pack the dot-matrix page is cut out of (PROT 1200).
const ART_FILE: &str = "1200_other4.BIN";

fn prot_entry(name: &str) -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted/PROT", "../../extracted/PROT"] {
        let p = Path::new(dir).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

struct Fixture {
    overlay_path: PathBuf,
    art_path: PathBuf,
    scene: slot::SlotScene,
}

fn fixture() -> Option<Fixture> {
    let overlay_path = prot_entry(OVERLAY_FILE)?;
    let art_path = prot_entry(ART_FILE)?;
    let overlay = std::fs::read(&overlay_path).ok()?;
    let art_raw = std::fs::read(&art_path).ok()?;
    let tims = minigame_art::parse_art_pack(&art_raw).expect("slot art pack (PROT 1200) parses");
    let (page, w, _h) = minigame_art::slot_page_indices(&tims, slot::DOT_PAGE)
        .expect("the dot-matrix art page (pack index 3)");
    // `parse_scene` runs `parse_messages` - FUN_801CEC94's StoreImage +
    // nibble-expansion pass, done straight off the decoded page.
    let scene = slot::parse_scene(&overlay, &page, w).expect("slot scene parses");
    Some(Fixture {
        overlay_path,
        art_path,
        scene,
    })
}

fn lit(buf: &[u8]) -> usize {
    buf.iter().filter(|&&d| d != 0).count()
}

/// The message bank the bonus round reads out of: eleven numerals, a
/// multiplication sign, two round pips and the word "coin", all cut out of the
/// disc's own art page.
///
/// The bank's *roles* are pinned by the ids `FUN_801CFFF0` indexes them with, so
/// the falsifiable claim is that the disc actually carries eleven distinct
/// numeral bitmaps - a bonus reel can land on **10**, and retail gives it a glyph
/// of its own rather than two digit cells.
#[test]
fn the_bonus_rounds_message_bank_cuts_out_of_the_disc() {
    let Some(f) = fixture() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing (disc-gated)");
        return;
    };
    let msgs = &f.scene.messages;
    assert_eq!(msgs.len(), slot::MESSAGE_COUNT, "21-record bank");

    // Every record is the matrix's full height - the bank is a strip of
    // 13-row bitmaps, which is what lets one blit fill the whole matrix.
    for (i, m) in msgs.iter().enumerate() {
        assert_eq!(
            m.h as usize,
            slot::DOT_ROWS,
            "message {i} is not {} rows tall",
            slot::DOT_ROWS
        );
        assert_eq!(
            m.bitmap.len(),
            m.w as usize * m.h as usize,
            "message {i} bitmap is not w*h texels"
        );
    }

    // The eleven numerals "0".."10" the tally and the caption print.
    let numerals: Vec<&slot::MarqueeMessage> = (0..=slot::MSG_NUMBER_MAX)
        .map(|n| &msgs[slot::MSG_NUMBER_BASE + n])
        .collect();
    for (n, m) in numerals.iter().enumerate() {
        assert!(
            lit(&m.bitmap) > 0,
            "numeral \"{n}\" (record {}) is blank on the disc",
            slot::MSG_NUMBER_BASE + n
        );
    }
    // "10" is a glyph of its own, not a re-use of "1" or "0" - which is the
    // whole reason the bank has eleven numerals rather than ten. It is *not*
    // a wider cell, though: the disc draws "10" inside the same cell as every
    // other numeral, so distinctness is the claim and width is not.
    let ten = &numerals[10];
    assert_ne!(
        ten.bitmap, numerals[1].bitmap,
        "the \"10\" glyph must not be the \"1\" glyph"
    );
    assert_ne!(
        ten.bitmap, numerals[0].bitmap,
        "the \"10\" glyph must not be the \"0\" glyph"
    );
    // All eleven are distinct bitmaps - a bank with a repeat would print two
    // different bonus factors as the same numeral.
    for a in 0..=slot::MSG_NUMBER_MAX {
        for b in (a + 1)..=slot::MSG_NUMBER_MAX {
            assert_ne!(
                numerals[a].bitmap, numerals[b].bitmap,
                "numerals \"{a}\" and \"{b}\" are the same bitmap"
            );
        }
    }

    // The tally's separator and the two round pips, which must differ from each
    // other - a filled pip and a hollow one are the round counter's whole state.
    assert!(lit(&msgs[slot::MSG_TIMES].bitmap) > 0, "the 'x' glyph");
    let (on, off) = (
        &msgs[slot::MSG_ROUND_PIP_ON],
        &msgs[slot::MSG_ROUND_PIP_OFF],
    );
    assert!(
        lit(&on.bitmap) > 0 && lit(&off.bitmap) > 0,
        "both pips draw"
    );
    assert!(
        lit(&on.bitmap) > lit(&off.bitmap),
        "the filled pip must light more dots than the hollow one ({} vs {})",
        lit(&on.bitmap),
        lit(&off.bitmap)
    );
    assert!(lit(&msgs[slot::MSG_COINS].bitmap) > 0, "the \"coin\" word");
}

/// The **claimed-column tally**: `0 x 0 x 0` when a round opens, one column
/// filling in per reel stop. Driven straight off the six globals
/// `FUN_801CFFF0` reads (`MarqueeFrame`), seeded with the values a bonus round
/// produces rather than rolled for.
///
/// The falsifiable part is not "something drew" - a composer that returned the
/// same strip every frame would pass that. It is that the *rasterised dot
/// buffer* changes as columns are claimed, and that the numeral each column
/// prints is `claimed - 0xF`, the same factor the payout multiplies.
#[test]
fn the_bonus_tally_fills_in_column_by_column_on_the_dot_matrix() {
    let Some(f) = fixture() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing (disc-gated)");
        return;
    };
    let msgs = &f.scene.messages;

    // A round paying 9 x 5 x 3 = 135. Retail's claimed latch is
    // `payline value + 1` over a bonus strip carrying `0x10..=0x19`, and the
    // numeral a value draws is `value - 0xF` - so numeral `n` sits on strip
    // value `0xF + n` and latches a claim of `0x10 + n`. (Off by one either
    // way and the tally prints a different factor than the payout multiplies,
    // which is exactly the disagreement the shared latch exists to prevent.)
    let claim = |n: i32| 0x10 + n;
    let numbers = [9, 5, 3];

    let frame_at = |claimed: [i32; 3]| slot::MarqueeFrame {
        feature_mode: slot::FEATURE_MODE_BONUS_ROUND,
        // State 3 = stopping: the tally strip's own state.
        reel_state: 3,
        bonus_rounds: 2,
        claimed,
        ..Default::default()
    };

    let mut previous: Option<Vec<u8>> = None;
    for taken in 0..=3usize {
        let mut claimed = [0i32; 3];
        for (r, c) in claimed.iter_mut().enumerate().take(taken) {
            *c = claim(numbers[r]);
        }
        let placed = slot::compose_marquee_frame(&frame_at(claimed));

        // Three numerals + two multiplication signs, every frame of the round.
        assert_eq!(
            placed.len(),
            slot::TALLY_NUMBER_COLS.len() + slot::TALLY_TIMES_COLS.len(),
            "the tally strip is 3 numerals and 2 signs ({taken} claimed)"
        );
        for (reel, &col) in slot::TALLY_NUMBER_COLS.iter().enumerate() {
            let msg = placed
                .iter()
                .find(|p| p.col == col as i32)
                .unwrap_or_else(|| panic!("nothing at tally column {col}"))
                .msg;
            let want = if reel < taken {
                slot::MSG_NUMBER_BASE + numbers[reel] as usize
            } else {
                // An unclaimed column prints the "0" glyph, not nothing.
                slot::MSG_NUMBER_BASE
            };
            assert_eq!(
                msg, want,
                "reel {reel} column with {taken} claimed: the tally prints \
                 `claimed - 0xF`, i.e. the same factor the payout multiplies"
            );
        }
        for &col in slot::TALLY_TIMES_COLS.iter() {
            assert!(
                placed
                    .iter()
                    .any(|p| p.col == col as i32 && p.msg == slot::MSG_TIMES),
                "a multiplication sign belongs at column {col}"
            );
        }

        // Rasterise: `render_marquee` = `clear_dots` + one `place_message` per
        // placement. The buffer must actually change as a column is claimed.
        let buf = slot::render_marquee(&placed, msgs);
        assert_eq!(buf.len(), slot::DOT_COLS * slot::DOT_STRIDE);
        assert!(
            lit(&buf) > 0,
            "the rasterised tally strip is empty with {taken} claimed"
        );
        if let Some(prev) = previous.as_ref() {
            assert_ne!(
                prev,
                &buf,
                "claiming column {} left the dot matrix unchanged",
                taken - 1
            );
        }
        previous = Some(buf);
    }

    // Between spins of the round the same strip is the round counter instead:
    // states 1-2 draw three pips, `bonus_rounds` of them filled.
    for rounds in 0..=3i32 {
        let placed = slot::compose_marquee_frame(&slot::MarqueeFrame {
            feature_mode: slot::FEATURE_MODE_BONUS_ROUND,
            reel_state: 1,
            bonus_rounds: rounds,
            ..Default::default()
        });
        assert_eq!(placed.len(), slot::ROUND_PIP_COLS.len());
        let filled = placed
            .iter()
            .filter(|p| p.msg == slot::MSG_ROUND_PIP_ON)
            .count();
        assert_eq!(
            filled, rounds as usize,
            "{rounds} rounds owed should light {rounds} pips"
        );
        assert!(lit(&slot::render_marquee(&placed, msgs)) > 0);
    }
}

/// The payout caption a bonus round ends on - and the **unsigned** destination
/// clip that hides it while it slides in.
///
/// The caption is composed at `row = min(frame - 0xD, 0)`, i.e. it starts 13 rows
/// *above* the matrix. `place_message` clips its destination unsigned
/// (`sltiu`), so those rows fall away; a port that clipped with a bare
/// `row < DOT_ROWS` would land every one of them and the caption would appear
/// fully formed on its first frame. That is what the dot counts below pin.
#[test]
fn the_rounds_payout_caption_slides_in_under_the_unsigned_clip() {
    let Some(f) = fixture() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing (disc-gated)");
        return;
    };
    let msgs = &f.scene.messages;

    // 9 x 5 x 3 = 135 coins - a real three-digit bonus product, which also pins
    // the leading-zero suppression (the thousands place must not draw).
    let payout = 135;
    let mut counts = Vec::new();
    for frame in 1..=(slot::PAYOUT_SLIDE_ROWS + 4) {
        let placed = slot::compose_marquee_frame(&slot::MarqueeFrame {
            payout,
            payout_frame: frame,
            feature_mode: slot::FEATURE_MODE_BONUS_ROUND,
            reel_state: 4,
            ..Default::default()
        });
        // The caption owns the whole strip: no tally is composed under it.
        assert!(
            placed.iter().all(|p| p.msg != slot::MSG_TIMES),
            "the caption must displace the tally, not overlay it"
        );
        assert_eq!(
            placed[0].row,
            (frame - slot::PAYOUT_SLIDE_ROWS).min(0),
            "caption row at frame {frame}"
        );
        counts.push(lit(&slot::render_marquee(&placed, msgs)));
    }

    assert!(
        counts[0] < *counts.last().unwrap(),
        "the caption must arrive gradually: first frame lit {} of the settled {}",
        counts[0],
        counts.last().unwrap()
    );
    assert!(
        counts.windows(2).all(|w| w[1] >= w[0]),
        "the caption's lit-dot count must not go backwards: {counts:?}"
    );
    // Once it has landed it holds - the composer clamps the row at 0.
    let settled = &counts[slot::PAYOUT_SLIDE_ROWS as usize..];
    assert!(
        settled.windows(2).all(|w| w[0] == w[1]),
        "the landed caption must hold still: {settled:?}"
    );

    // Leading-zero suppression tests the WHOLE figure at each place, so 135
    // draws three digits and 405 keeps its interior zero.
    let digits_of = |n: i32| {
        slot::compose_marquee_frame(&slot::MarqueeFrame {
            payout: n,
            payout_frame: 0x40,
            ..Default::default()
        })
    };
    let d135 = digits_of(135);
    assert_eq!(d135.len(), 4, "three digits + the word \"coin\"");
    assert!(
        !d135
            .iter()
            .any(|p| p.col == slot::PAYOUT_DIGIT_COLS[0] as i32),
        "135 must not draw a thousands digit"
    );
    let d405 = digits_of(405);
    let tens = d405
        .iter()
        .find(|p| p.col == slot::PAYOUT_DIGIT_COLS[2] as i32)
        .expect("405 keeps its interior zero");
    assert_eq!(
        tens.msg,
        slot::MSG_NUMBER_BASE,
        "the interior zero is drawn"
    );
    // The bonus round's ceiling, 10 x 10 x 10, is the four-digit case.
    assert_eq!(
        digits_of(slot_payout::BONUS_PAYOUT_MAX as i32).len(),
        5,
        "1000 draws all four places plus \"coin\""
    );
}

/// The *scrolling* blit, `FUN_801D069C` - the marquee's other copy loop, and the
/// one whose clip is on the **source**.
///
/// The two are not variants of one routine: the scroll offsets and clips the
/// source (signed), the placement offsets and clips the destination (unsigned).
/// Scrolling a message off its own left edge must empty the buffer; placing one
/// at a negative column must not.
#[test]
fn the_marquees_two_blits_clip_opposite_ends() {
    let Some(f) = fixture() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing (disc-gated)");
        return;
    };
    let msg = &f.scene.messages[slot::MSG_COINS];

    // Every frame starts from a cleared buffer (the negative-id clear command).
    let blank = slot::clear_dots();
    assert_eq!(blank.len(), slot::DOT_COLS * slot::DOT_STRIDE);
    assert_eq!(lit(&blank), 0, "the clear command leaves no lit dot");

    // Scroll: source-clipped. At offset 0 the message is on screen; scrolled a
    // full width past its own right edge nothing of it remains, and scrolled a
    // whole matrix in from the left it has not arrived yet.
    let on = slot::compose_marquee(msg, 0, 0);
    assert!(lit(&on) > 0, "the \"coin\" word draws at source offset 0");
    let past = slot::compose_marquee(msg, msg.w as i32, 0);
    assert_eq!(lit(&past), 0, "scrolled past its own width, nothing draws");
    let before = slot::compose_marquee(msg, -(slot::DOT_COLS as i32), 0);
    assert_eq!(lit(&before), 0, "scrolled a full matrix left, nothing yet");

    // The contrast that decides what the marquee can express. Both blits take
    // a negative offset; only one of them *loses* dots to it.
    //
    //   scroll  (-k): the source window moves, so the whole message is still
    //                 in the buffer - just k columns further right.
    //   placement(-k): the destination is clipped unsigned, so the message's
    //                 first k columns fall off the left edge and are gone.
    const SHIFT: i32 = 4;
    let scrolled = slot::compose_marquee(msg, -SHIFT, 0);
    assert_eq!(
        lit(&scrolled),
        lit(&on),
        "the scroll blit moves the message, it does not cut it"
    );
    let mut buf = slot::clear_dots();
    slot::place_message(&mut buf, msg, -SHIFT, 0);
    assert!(
        lit(&buf) > 0 && lit(&buf) < lit(&on),
        "the placement blit cuts the overhang off (lit {} vs {} placed at 0)",
        lit(&buf),
        lit(&on)
    );
    let mut off = slot::clear_dots();
    slot::place_message(&mut off, msg, slot::DOT_COLS as i32, 0);
    assert_eq!(lit(&off), 0, "placed past the last column, nothing lands");
    let mut above = slot::clear_dots();
    slot::place_message(&mut above, msg, 0, -(slot::DOT_ROWS as i32));
    assert_eq!(lit(&above), 0, "placed a full matrix above, nothing lands");
}

/// The reel **cylinder** the bonus numerals ride on (`FUN_801D0FA8`).
///
/// The trap this pins is the one the module doc names: the payline face is not
/// the first face emitted. The first face is at angle `0x380` and the step is
/// `0x100`, so the face whose span crosses the shade peak (`z = -0x200`) is the
/// *fifth* - a renderer that indexes the strip straight off the face index draws
/// a payline whose three symbols are not the three that paid.
///
/// **Sample the face's mid-angle, not its leading edge.** A face spans `a` to
/// `a + 0x100` and the peak sits at `0x800`, which is the *interior* of face 4:
/// its two edge vertices are symmetric about the peak and therefore tie, so an
/// edge-sampled profile reports a two-way tie and names whichever of 4 / 5 the
/// scan settles on.
#[test]
fn the_reel_cylinders_shade_peaks_on_the_payline_face_not_the_first() {
    let Some(_f) = fixture() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing (disc-gated)");
        return;
    };

    let face_angle = |face: usize| slot::REEL_ANGLE_BASE + face as i32 * slot::REEL_ANGLE_STEP;
    // Per-face centre: the row that face carries is the one on the payline.
    let mid: Vec<(i32, i32)> = (0..slot::REEL_FACES)
        .map(|f| {
            let z = slot::reel_z(face_angle(f) + slot::REEL_ANGLE_STEP / 2);
            (z, slot::reel_shade(z))
        })
        .collect();
    let shades: Vec<i32> = mid.iter().map(|&(_, s)| s).collect();

    let peak = shades
        .iter()
        .enumerate()
        .max_by_key(|&(_, &s)| s)
        .map(|(i, _)| i)
        .unwrap();
    assert_eq!(
        peak, 4,
        "the shade peak is the fifth face, not the first: {shades:?}"
    );
    assert_eq!(
        shades.iter().filter(|&&s| s == shades[peak]).count(),
        1,
        "the payline face must be the unique peak: {shades:?}"
    );
    assert_eq!(
        shades[peak],
        slot::REEL_SHADE_MAX,
        "the payline face takes the full {:#x} brighten",
        slot::REEL_SHADE_MAX
    );
    // The peak is where the cylinder crosses the payline depth.
    assert_eq!(
        mid[peak].0,
        -slot::REEL_SHADE_Z_BIAS,
        "face {peak} should sit at z = -{:#x}",
        slot::REEL_SHADE_Z_BIAS
    );
    // The fan falls away to black either side - that fade is what caps the reel
    // window top and bottom, and what hides the near half of the cylinder (there
    // is no backface cull; the near half is simply shaded out).
    assert_eq!(shades[0], 0, "the first face is shaded out: {shades:?}");
    assert_eq!(
        shades[slot::REEL_FACES - 1],
        0,
        "the last face is shaded out: {shades:?}"
    );

    // The cylinder is an ellipse, not a circle: radius 585 in y, 512 in z.
    let ys: Vec<i32> = (0..slot::ANGLE_FULL)
        .step_by(0x10)
        .map(slot::reel_y)
        .collect();
    let y_max = ys.iter().copied().map(i32::abs).max().unwrap();
    let z_max = (0..slot::ANGLE_FULL)
        .step_by(0x10)
        .map(|a| slot::reel_z(a).abs())
        .max()
        .unwrap();
    assert_eq!(y_max, slot::REEL_Y_RADIUS, "y radius");
    assert_eq!(z_max, 0x1000 >> slot::REEL_Z_SHIFT, "z radius");
    assert_ne!(
        y_max, z_max,
        "the reel is a cylinder with an elliptic section"
    );
}

/// The production caller: `asset slot-scene` walks the reel cylinder, the
/// message bank, the per-frame composer and both blits over a real overlay from
/// `fn main`. Spawning it is what puts these kernels' **non-test** caller under
/// the same coverage run (`LLVM_PROFILE_FILE` is inherited).
#[test]
fn the_slot_scene_cli_walks_the_marquee_over_a_real_overlay() {
    let Some(f) = fixture() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing (disc-gated)");
        return;
    };
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_asset"))
        .arg("slot-scene")
        .arg(&f.overlay_path)
        .arg("--art")
        .arg(&f.art_path)
        .arg("--frames")
        .arg("5")
        .output()
        .expect("spawn the asset CLI");
    assert!(
        out.status.success(),
        "asset slot-scene failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("dot-matrix message bank"),
        "the CLI did not reach the message bank:\n{text}"
    );
    assert!(
        text.contains("per-frame composition"),
        "the CLI did not reach the composer:\n{text}"
    );
    // Non-vacuity: at least one composed frame must actually light dots. A
    // composer that returned nothing would still print every heading above.
    let dots: Vec<u32> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("rasterised strip: "))
        .filter_map(|l| l.split_whitespace().next())
        .filter_map(|n| n.parse().ok())
        .collect();
    assert!(
        !dots.is_empty() && dots.iter().any(|&n| n > 0),
        "the CLI rasterised no lit dot on any frame: {dots:?}\n{text}"
    );
    // The reel table must have a shade profile, not a constant: the depth cue
    // is the thing that curls the reel.
    let shades: Vec<i32> = text
        .lines()
        .skip_while(|l| !l.contains("face   angle"))
        .skip(1)
        .take(slot::REEL_FACES)
        .filter_map(|l| l.split_whitespace().last())
        .filter_map(|n| n.parse().ok())
        .collect();
    assert_eq!(shades.len(), slot::REEL_FACES, "reel table rows:\n{text}");
    assert!(
        shades.contains(&0) && shades.iter().any(|&s| s > 0),
        "the CLI's reel table has no depth cue: {shades:?}"
    );
}
