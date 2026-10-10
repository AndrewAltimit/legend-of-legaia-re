//! Save-UI sub-screen graph.
//!
//! PORT: FUN_801DC6B4 (outer dispatcher), FUN_801E4F40 (sub-screen pointer table)
//! REF: FUN_801DD35C (the card-driver pair's shared body; its op selector is
//!      what fixes which of `0x18` / `0x19` is which direction)
//! REF: FUN_801E37CC (the BIOS erase the save direction issues before writing)
//!
//! The retail save UI is not one screen but a graph of small step
//! machines. An outer dispatcher runs a fade-in / dispatch / fade-out
//! cycle, and the dispatch case indirects through a pointer table into
//! whichever sub-screen is current. Each sub-screen owns a step counter,
//! invokes an actor-VM display script on its first step, waits for that
//! script to go idle, then either advances its own step or writes a new
//! sub-screen id - which is how control moves through the graph.
//!
//! The id space is **not** save-specific: `0x801E4F40` is the menu overlay's
//! whole screen table (33 entries, `0x00..=0x20`), so the same dispatcher
//! runs the Items, Magic, Equip, shop and casino screens. What makes this
//! module the *save* graph is the subset it carries step machines for, plus
//! the entry-context decode that names which family a record opens.
//!
//! Two globals carry all of it: the sub-screen id and the step counter.
//! A sub-screen never returns a value; it *is* the transition, by writing
//! the id global. That makes the graph a plain state machine once lifted
//! out of the pointer-table indirection, which is what this module is.
//!
//! ## What this models, and what it does not
//!
//! This is the **control flow** - which screen follows which, on what
//! input, and where the outer fade sits around it. The screens' *content*
//! (panels, slot previews, the info panel) is [`crate::save_select`],
//! which models the same UI as player-facing phases rather than retail
//! ids. The two are complementary: a host drives the session for content
//! and can key retail-exact chrome off [`SaveSubScreen`].
//!
//! A sub-screen this module carries no step machine for is not therefore
//! unknown, and [`SaveSubScreen`] says which of the two it is. Of the
//! table's 33 ids, 14 are stepped here, `0x16` is a bare frame-flush
//! wrapper ([`SaveSubScreen::FrameFlushTick`]), and the remaining 18 are
//! [`SaveSubScreen::Routed`] - screens another `engine-core` module already
//! ports, named per id by [`SaveSubScreen::routed_port`] and handed to the
//! host as [`SubScreenEffect::Route`]. [`SaveSubScreen::Unpinned`] survives
//! only for an id past the end of the retail table. See
//! `docs/subsystems/save-screen.md` for the per-id table.
//!
//! WIRED: [`crate::save_screen::SaveScreenFlow`] constructs a
//! [`SaveScreenMachine`] on its first card-rack frame and ticks it around
//! [`crate::save_select::SaveSelectSession`], so both hosts run it - the flow
//! is the kernel they share. The two models are joined at the card op, not
//! stacked: the driver's `script_busy` / `card_done` waits are answered by
//! the flow's [`crate::save_select::CardIoMachine`] result, and the outer
//! fade's input threshold is what suppresses the pad while a screen comes up.
//!
//! Two parts of the graph stay unreached and say so on themselves: the
//! screens whose per-frame body belongs to another module (they park here and
//! the host drives them), and the `sub15_*` family below, which no step
//! machine calls at all - hosting the machine does not reach a free function,
//! and a reader should not read this heading onto them.

mod machine;
mod sub15_list;
mod subscreen_routes;

pub use machine::*;
pub use sub15_list::*;
pub use subscreen_routes::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod sub15_tests {
    use super::*;

    fn record() -> Vec<u8> {
        vec![0u8; 0x414]
    }

    /// Steps pair up: the settle step and its running twin read the same
    /// list. Getting this wrong shows as a screen that changes what it
    /// lists one frame after it opens.
    #[test]
    fn each_list_has_a_settle_step_and_a_running_twin() {
        for (settle, running, want) in [
            (2u8, 5u8, Sub15ListSource::Abilities),
            (3, 6, Sub15ListSource::Magic),
            (4, 7, Sub15ListSource::Third),
        ] {
            assert_eq!(sub15_list_source(settle), want);
            assert_eq!(sub15_list_source(running), want);
            assert_eq!(
                sub15_frame(settle, 1),
                Sub15Frame::Settle { next_step: running },
                "settle step {settle} advances by three, not one",
            );
        }
        for step in [0u8, 1, 8, 9, 0xFF] {
            assert_eq!(sub15_list_source(step), Sub15ListSource::None);
        }
    }

    /// The ability list counts *set bits*, over exactly `0x40` of them -
    /// the accessory-passive index space. A bit past 64 is outside the
    /// field the walk reads.
    #[test]
    fn ability_rows_are_the_popcount_of_the_64_slot_bitfield() {
        let mut r = record();
        // bits 0, 31, 32 and 63 - one at each word boundary.
        r[SUB15_ABILITY_BITS] = 0x01;
        r[SUB15_ABILITY_BITS + 3] = 0x80;
        r[SUB15_ABILITY_BITS + 4] = 0x01;
        r[SUB15_ABILITY_BITS + 7] = 0x80;
        assert_eq!(sub15_list_len(5, &r, 0), 4);

        // Bit 64 lives in the third word, past the walk's `< 0x40` bound.
        r[SUB15_ABILITY_BITS + 8] = 0x01;
        assert_eq!(sub15_list_len(5, &r, 0), 4);
    }

    /// The Ra-Seru gate is read before the count, so spells with no Seru
    /// equipped is an *empty* list, not a populated one.
    #[test]
    fn magic_rows_are_gated_on_the_raseru_slot_not_on_the_spell_count() {
        let mut r = record();
        r[SUB15_SPELL_COUNT] = 7;
        assert_eq!(sub15_list_len(6, &r, 2), 0, "no Ra-Seru -> no rows");
        r[SUB15_EQUIP + 2] = 0x40;
        assert_eq!(sub15_list_len(6, &r, 2), 7);
        // A different slot index reads a different equip byte.
        assert_eq!(sub15_list_len(6, &r, 3), 0);
    }

    #[test]
    fn third_list_rows_come_from_one_byte() {
        let mut r = record();
        r[SUB15_THIRD_COUNT] = 0x0C;
        assert_eq!(sub15_list_len(7, &r, 0), 0x0C);
        assert_eq!(sub15_list_len(4, &r, 0), 0x0C);
    }

    /// The reject test runs ahead of the settle test, so an empty list is
    /// refused on the frame the screen opens rather than the one after.
    #[test]
    fn an_empty_list_is_rejected_on_the_settle_frame() {
        assert_eq!(sub15_frame(2, 0), Sub15Frame::Reject);
        assert_eq!(sub15_frame(5, 0), Sub15Frame::Reject);
        assert_eq!(sub15_frame(5, 3), Sub15Frame::Browse { rows: 3 });
    }

    /// The swap moves all three parallel arrays together; permuting only
    /// the id list would decouple a spell from its companion byte.
    #[test]
    fn the_row_swap_moves_all_three_arrays() {
        let mut r = record();
        r[SUB15_SPELL_LIST] = 0x81;
        r[SUB15_SPELL_LIST + 1] = 0x82;
        r[SUB15_SPELL_AUX] = 0x11;
        r[SUB15_SPELL_AUX + 1] = 0x22;
        r[SUB15_ROW_WORDS..SUB15_ROW_WORDS + 4].copy_from_slice(&1u32.to_le_bytes());
        r[SUB15_ROW_WORDS + 4..SUB15_ROW_WORDS + 8].copy_from_slice(&2u32.to_le_bytes());

        sub15_swap_rows(&mut r, 0, 1);
        assert_eq!((r[SUB15_SPELL_LIST], r[SUB15_SPELL_LIST + 1]), (0x82, 0x81));
        assert_eq!((r[SUB15_SPELL_AUX], r[SUB15_SPELL_AUX + 1]), (0x22, 0x11));
        assert_eq!(
            u32::from_le_bytes(r[SUB15_ROW_WORDS..SUB15_ROW_WORDS + 4].try_into().unwrap()),
            2
        );

        // Same row twice is a no-op, and an out-of-record row is skipped
        // rather than panicking.
        let before = r.clone();
        sub15_swap_rows(&mut r, 1, 1);
        sub15_swap_rows(&mut r, 0, 0x400);
        assert_eq!(r, before);
    }
}
