use super::*;
use std::cell::RefCell;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Clear {
        rect: Rect12,
        rgb: (u8, u8, u8),
    },
    Move {
        src: Rect12,
        dst: (u16, i16),
    },
    Sprite {
        rect: Rect12,
        pixel_data_ptr: u32,
        is_strip: bool,
    },
    StpOr {
        pixel_data_ptr: u32,
        count: i32,
    },
    PrimAlloc(u8),
    HBar {
        x: i16,
        y: i16,
        w: i16,
        color: u32,
    },
    CenteredText {
        label_ptr: u32,
        mode_a: u16,
        mode_b: u16,
        x: i16,
        y: i16,
    },
    CardInit(u32),
}

#[derive(Default)]
struct RecHost {
    events: RefCell<Vec<Event>>,
    alpha_gate: bool,
    text_block_height: i32,
    text_label_widths: std::collections::HashMap<u32, i32>,
}

impl RecHost {
    fn take(&self) -> Vec<Event> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

impl PrimHost for RecHost {
    fn queue_clear_rect(&mut self, rect: Rect12, r: u8, g: u8, b: u8) {
        self.events.borrow_mut().push(Event::Clear {
            rect,
            rgb: (r, g, b),
        });
    }
    fn queue_move_image(&mut self, src: Rect12, dst_x: u16, dst_y: i16) {
        self.events.borrow_mut().push(Event::Move {
            src,
            dst: (dst_x, dst_y),
        });
    }
    fn emit_sprite(&mut self, rect: Rect12, pixel_data_ptr: u32, is_strip_pass: bool) {
        self.events.borrow_mut().push(Event::Sprite {
            rect,
            pixel_data_ptr,
            is_strip: is_strip_pass,
        });
    }
    fn stp_or_pixels(&mut self, pixel_data_ptr: u32, count: i32) {
        self.events.borrow_mut().push(Event::StpOr {
            pixel_data_ptr,
            count,
        });
    }
    fn stp_or_gate_set(&self) -> bool {
        self.alpha_gate
    }
    fn prim_packet_alloc(&mut self, packet_type: u8) {
        self.events.borrow_mut().push(Event::PrimAlloc(packet_type));
    }
    fn queue_horizontal_bar(&mut self, x: i16, y: i16, w: i16, color: u32) {
        self.events
            .borrow_mut()
            .push(Event::HBar { x, y, w, color });
    }
    fn menu_text_block_height(&self) -> i32 {
        self.text_block_height
    }
    fn menu_text_label_width(&self, label_ptr: u32) -> i32 {
        self.text_label_widths.get(&label_ptr).copied().unwrap_or(0)
    }
    fn queue_centered_text(&mut self, label_ptr: u32, mode_a: u16, mode_b: u16, x: i16, y: i16) {
        self.events.borrow_mut().push(Event::CenteredText {
            label_ptr,
            mode_a,
            mode_b,
            x,
            y,
        });
    }
    fn init_card_state(&mut self, arg: u32) {
        self.events.borrow_mut().push(Event::CardInit(arg));
    }
}

#[test]
fn rect12_round_trips_le_bytes() {
    let r = Rect12::new(-3, 4, 320, 240);
    let bytes = r.to_le_bytes();
    let back = Rect12::from_le_bytes(&bytes).unwrap();
    assert_eq!(r, back);
}

#[test]
fn rect12_from_le_bytes_rejects_short_input() {
    assert!(Rect12::from_le_bytes(&[0u8; 7]).is_none());
}

#[test]
fn exec_clear_image_always_queues() {
    let mut host = RecHost::default();
    let r = Rect12::new(0, 0, 320, 4);
    exec_clear_image(&mut host, r, 0x10, 0x20, 0x30);
    assert_eq!(
        host.take(),
        vec![Event::Clear {
            rect: r,
            rgb: (0x10, 0x20, 0x30),
        }]
    );
}

#[test]
fn exec_move_image_skips_zero_extent() {
    let mut host = RecHost::default();
    let outcome = exec_move_image(&mut host, Rect12::new(0, 0, 0, 64), 100, 50);
    assert_eq!(outcome, MoveImageOutcome::SkippedZeroExtent);
    let outcome = exec_move_image(&mut host, Rect12::new(0, 0, 64, 0), 100, 50);
    assert_eq!(outcome, MoveImageOutcome::SkippedZeroExtent);
    assert!(host.take().is_empty(), "no queue on zero extent");
}

#[test]
fn exec_move_image_queues_when_both_dimensions_nonzero() {
    let mut host = RecHost::default();
    let src = Rect12::new(10, 20, 30, 40);
    let outcome = exec_move_image(&mut host, src, 100, 50);
    assert_eq!(outcome, MoveImageOutcome::Queued);
    assert_eq!(
        host.take(),
        vec![Event::Move {
            src,
            dst: (100, 50),
        }]
    );
}

/// Build a descriptor whose main record is the descriptor itself -
/// the `flags & 8 == 0` shape, where `0x80019A14` sets `s0 = s1`.
fn desc_flat(tag: u32, flags: u32, rect: Rect12, ptr: u32) -> SpriteDescriptor {
    SpriteDescriptor {
        tag,
        flags,
        rect,
        pixel_data_ptr: ptr,
        main_rect: rect,
        main_pixel_data_ptr: ptr,
    }
}

#[test]
fn tag_0x11_descriptor_emits_one_raw_sprite() {
    let mut host = RecHost::default();
    // Flags are read only past the tag-0x11 early-out at 0x80019940.
    let desc = desc_flat(0x11, 0xFFFF_FFFF, Rect12::new(64, 32, 128, 64), 0x8010_1234);
    exec_sprite_descriptor(&mut host, &desc);
    assert_eq!(
        host.take(),
        vec![Event::Sprite {
            rect: desc.rect,
            pixel_data_ptr: desc.pixel_data_ptr,
            is_strip: false,
        }]
    );
}

#[test]
fn complex_descriptor_without_strip_bit_emits_only_the_main_sprite() {
    let mut host = RecHost::default();
    let desc = desc_flat(0x42, 0x02, Rect12::new(0, 0, 16, 16), 0x8020_0000);
    exec_sprite_descriptor(&mut host, &desc);
    assert_eq!(
        host.take(),
        vec![Event::Sprite {
            rect: desc.rect,
            pixel_data_ptr: desc.pixel_data_ptr,
            is_strip: false,
        }]
    );
}

#[test]
fn strip_bit_runs_the_stp_or_pass_then_the_strip_then_the_main_sprite() {
    let mut host = RecHost {
        alpha_gate: true,
        ..Default::default()
    };
    let desc = desc_flat(0x42, 0x08 | 0x02, Rect12::new(5, 7, 4, 3), 0x8030_0000);
    exec_sprite_descriptor(&mut host, &desc);
    assert_eq!(
        host.take(),
        vec![
            Event::StpOr {
                pixel_data_ptr: 0x8030_0000,
                count: 12,
            },
            Event::Sprite {
                rect: Rect12::new(5, 7, 12, 1), // w * h, h = 1
                pixel_data_ptr: 0x8030_0000,
                is_strip: true,
            },
            Event::Sprite {
                rect: desc.rect,
                pixel_data_ptr: 0x8030_0000,
                is_strip: false,
            },
        ]
    );
}

/// `_DAT_8007B998` gates the STP-OR loop and nothing else: the `beq`
/// at `0x80019998` branches to `0x800199EC`, which is the emit.
#[test]
fn clear_gate_drops_the_stp_or_pass_but_still_emits_the_strip() {
    let mut host = RecHost::default(); // alpha_gate = false
    let desc = desc_flat(0x42, 0x08 | 0x02, Rect12::new(0, 0, 8, 4), 0x8040_0000);
    exec_sprite_descriptor(&mut host, &desc);
    assert_eq!(
        host.take(),
        vec![
            Event::Sprite {
                rect: Rect12::new(0, 0, 32, 1),
                pixel_data_ptr: 0x8040_0000,
                is_strip: true,
            },
            Event::Sprite {
                rect: desc.rect,
                pixel_data_ptr: 0x8040_0000,
                is_strip: false,
            },
        ],
        "the gate must not suppress the strip emit"
    );
}

/// A zero pixel count takes the `blez` at `0x800199A8` past the loop -
/// to the same emit.
#[test]
fn zero_pixel_count_drops_the_stp_or_pass_but_still_emits_the_strip() {
    let mut host = RecHost {
        alpha_gate: true,
        ..Default::default()
    };
    let desc = desc_flat(0x42, 0x08, Rect12::new(3, 4, 0, 9), 0x8045_0000);
    exec_sprite_descriptor(&mut host, &desc);
    let events = host.take();
    assert!(
        !events.iter().any(|e| matches!(e, Event::StpOr { .. })),
        "no OR pass over an empty array"
    );
    assert_eq!(events.len(), 2, "strip + main still emit");
}

#[test]
fn sprite_emit_variant_decodes_low_two_flag_bits() {
    assert_eq!(
        SpriteEmitVariant::from_flags(0),
        SpriteEmitVariant::QuarterWidth
    );
    assert_eq!(
        SpriteEmitVariant::from_flags(1),
        SpriteEmitVariant::HalfWidth
    );
    assert_eq!(SpriteEmitVariant::from_flags(2), SpriteEmitVariant::Raw);
    assert_eq!(
        SpriteEmitVariant::from_flags(3),
        SpriteEmitVariant::NoWidthStore
    );
    // Upper bits ignored.
    assert_eq!(
        SpriteEmitVariant::from_flags(0xFFFF_FFFC),
        SpriteEmitVariant::QuarterWidth
    );
}

/// All four `flags & 3` arms converge on the common tail at
/// `0x80019AAC`, which reloads the **raw** extents into `0x14(sp)` /
/// `0x16(sp)` right before `jal 0x800583C8`. So the arm changes
/// nothing - and `== 3` is not a skip, it just jumps there directly
/// (`0x80019A4C`).
#[test]
fn every_flags_and_3_arm_emits_the_same_raw_extents() {
    let rect = Rect12::new(0, 0, 16, 16);
    for low in 0..=3u32 {
        let mut host = RecHost::default();
        let desc = desc_flat(0x42, low, rect, 0x8050_0000);
        exec_sprite_descriptor(&mut host, &desc);
        assert_eq!(
            host.take(),
            vec![Event::Sprite {
                rect,
                pixel_data_ptr: 0x8050_0000,
                is_strip: false,
            }],
            "flags & 3 == {low} must emit the raw rect exactly once"
        );
    }
}

/// With `flags & 8` set, `0x800199F8..0x80019A10` re-bases the main
/// emit onto `desc + ((u32 @ +0x08 & !3) + 8)` - a different
/// sub-record. The strip still comes from the `+0x0C` rect.
#[test]
fn strip_bit_moves_the_main_emit_onto_the_resolved_sub_record() {
    let mut host = RecHost::default();
    let desc = SpriteDescriptor {
        tag: 0x42,
        flags: 0x08,
        rect: Rect12::new(1, 2, 3, 4),
        pixel_data_ptr: 0x8060_0000,
        main_rect: Rect12::new(9, 9, 32, 8),
        main_pixel_data_ptr: 0x8060_1000,
    };
    exec_sprite_descriptor(&mut host, &desc);
    assert_eq!(
        host.take(),
        vec![
            Event::Sprite {
                rect: Rect12::new(1, 2, 12, 1),
                pixel_data_ptr: 0x8060_0000,
                is_strip: true,
            },
            Event::Sprite {
                rect: Rect12::new(9, 9, 32, 8),
                pixel_data_ptr: 0x8060_1000,
                is_strip: false,
            },
        ]
    );
}

// ----- exec_centered_bar (FUN_801E36C4) ------------------------------

#[test]
fn exec_centered_bar_emits_packet_alloc_then_hbar() {
    let mut host = RecHost::default();
    exec_centered_bar(&mut host, 100, 50, 40, 0xAABBCCDD);
    let events = host.take();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], Event::PrimAlloc(0x44));
    // x_centered = x - w/2 - 2 = 100 - 20 - 2 = 78; y = 50 + 6 = 56.
    assert_eq!(
        events[1],
        Event::HBar {
            x: 78,
            y: 56,
            w: 40,
            color: 0xAABBCCDD,
        }
    );
}

#[test]
fn exec_centered_bar_gates_off_screen_y() {
    let mut host = RecHost::default();
    // y == 0xF1 should gate (retail: `slti v0,s1,0xf1; beq v0,zero,...`).
    exec_centered_bar(&mut host, 100, 0xF1, 40, 0);
    // y > 0xF1 also gates.
    exec_centered_bar(&mut host, 100, 0xFF, 40, 0);
    assert!(host.take().is_empty(), "off-screen y skips both calls");
    // y = 0xF0 (just inside) does emit.
    exec_centered_bar(&mut host, 100, 0xF0, 40, 0);
    assert_eq!(host.take().len(), 2);
}

#[test]
fn exec_centered_bar_signed_divide_matches_retail_idiom() {
    // The retail `srl+addu+sra` idiom is signed-divide-toward-zero.
    // For w = -5: -5/2 == -2 (toward zero), so x_centered = x - (-2) - 2 = x.
    let mut host = RecHost::default();
    exec_centered_bar(&mut host, 50, 10, -5, 0);
    let events = host.take();
    match &events[1] {
        Event::HBar { x, w, .. } => {
            assert_eq!(*w, -5);
            assert_eq!(*x, 50, "w=-5 → half_w=-2, x_centered = 50 - (-2) - 2 = 50");
        }
        other => panic!("expected HBar, got {other:?}"),
    }
}

// ----- exec_centered_text (FUN_801E3EE0) -----------------------------

#[test]
fn exec_centered_text_returns_half_height_and_emits_at_centered_position() {
    let mut host = RecHost {
        text_block_height: 11, // odd value to verify rounding
        ..Default::default()
    };
    host.text_label_widths.insert(0x8010_0000, 60);
    let half = exec_centered_text(&mut host, 0x8010_0000, 160, 100);
    // (11 + 1) / 2 = 6.
    assert_eq!(half, 6);
    // text_x = 160 - 60/2 = 130; text_y = 100 + 7 = 107.
    let events = host.take();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0],
        Event::CenteredText {
            label_ptr: 0x8010_0000,
            mode_a: 0,
            mode_b: 0,
            x: 130,
            y: 107,
        }
    );
}

#[test]
fn exec_centered_text_gated_off_screen_returns_zero_with_no_emit() {
    let mut host = RecHost {
        text_block_height: 14,
        ..Default::default()
    };
    host.text_label_widths.insert(0xDEAD_BEEF, 100);
    let r = exec_centered_text(&mut host, 0xDEAD_BEEF, 160, 0xF1);
    assert_eq!(r, 0, "off-screen y returns 0");
    assert!(host.take().is_empty(), "no emit on gate");
}

#[test]
fn exec_centered_text_handles_zero_width_label() {
    let mut host = RecHost {
        text_block_height: 8,
        ..Default::default()
    };
    // No widths registered → default 0; text_x = 160 - 0 = 160.
    let r = exec_centered_text(&mut host, 0xFEED_FACE, 160, 50);
    assert_eq!(r, (8 + 1) / 2);
    let events = host.take();
    match &events[0] {
        Event::CenteredText { x, y, .. } => {
            assert_eq!(*x, 160);
            assert_eq!(*y, 57);
        }
        other => panic!("expected CenteredText, got {other:?}"),
    }
}

// ----- exec_card_init (FUN_801E373C) ---------------------------------

#[test]
fn exec_card_init_dispatches_through_trait() {
    let mut host = RecHost::default();
    exec_card_init(&mut host, 0x42);
    assert_eq!(host.take(), vec![Event::CardInit(0x42)]);
}

#[test]
fn y_gate_constant_matches_retail_literal() {
    // Retail: `slti v0,s1,0xf1` in both FUN_801E36C4 and FUN_801E3EE0.
    assert_eq!(TITLE_PRIM_Y_GATE, 0xF1);
}
