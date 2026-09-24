//! `FUN_80024CFC` - allocate an actor and seat an animation record on it.
//!
//! Read off the disassembly (`see ghidra/scripts/funcs/80024cfc.txt`), the
//! routine takes `(index, descriptor, arg)`:
//!
//! 1. clears the descriptor's `+0x14` halfword (`sh zero,0x14(a0)` in the
//!    `jal` delay slot) and allocates through `FUN_80020DE0(descriptor,
//!    arg)`; a null actor returns null with nothing else stored;
//! 2. resolves the record as `base + word[index + 1]` of the pack whose base
//!    the word `*0x8007B7C8` holds (`sra v0,v0,0xe` = `index * 4`, then
//!    `lw v0,0x4(v0)` - the offset table past the pack's count word);
//! 3. stores `+0x4C = record`, `+0x5C = 0`, `+0x56 = 0xB`, `+0x68 = 100`,
//!    `+0x6E = 0` and returns the actor.
//!
//! `+0x56` non-zero is what makes the actor tick run the clip selector
//! `FUN_800204F8` (`crate::move_buffer`); `0xB` is the kick value.

/// The `+0x56` value the spawn stores.
pub const SPAWN_CLIP_KICK: u16 = 0xB;

/// The `+0x68` frame counter the spawn stores.
pub const SPAWN_FRAME_COUNTER: u16 = 100;

/// The fields `FUN_80024CFC` writes on the actor it allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnInit {
    /// `+0x4C` - byte offset of the record inside the pack.
    pub record_offset: usize,
    /// `+0x5C` - anim cursor, zeroed.
    pub anim_cursor: u16,
    /// `+0x56` - clip-selector kick, [`SPAWN_CLIP_KICK`].
    pub clip_kick: u16,
    /// `+0x68` - frame counter, [`SPAWN_FRAME_COUNTER`].
    pub frame_counter: u16,
    /// `+0x6E` - zeroed.
    pub field_6e: u16,
}

/// Offset of record `index` in a pack laid out `[count][offset; count]`
/// (the `lw v0,0x4(base + index*4)` read). `None` when the offset word is
/// out of the pack's bytes. Retail bounds nothing: an index past `count`
/// reads whatever follows the table.
pub fn pack_record_offset(pack: &[u8], index: i16) -> Option<usize> {
    let slot = usize::try_from(i32::from(index) * 4 + 4).ok()?;
    let word = pack.get(slot..slot + 4)?;
    Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]) as usize)
}

/// The stores `FUN_80024CFC` makes once its allocation succeeded.
///
/// PORT: FUN_80024CFC NOT WIRED: the only retail caller is the field overlay's MAIN_INIT, and the engine's scene host seats its actors through its own install path, never through this pack-record resolve
///
/// Retail truncates `index` to 16 bits and sign-extends it (`sll 0x10` /
/// `sra 0xe`), which is why it is an `i16` here.
pub fn spawn_init(pack: &[u8], index: i16) -> Option<SpawnInit> {
    Some(SpawnInit {
        record_offset: pack_record_offset(pack, index)?,
        anim_cursor: 0,
        clip_kick: SPAWN_CLIP_KICK,
        frame_counter: SPAWN_FRAME_COUNTER,
        field_6e: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_offset_reads_the_word_past_the_count() {
        // count 3, offsets 0x10 / 0x24 / 0x40.
        let mut pack = vec![3, 0, 0, 0];
        for off in [0x10u32, 0x24, 0x40] {
            pack.extend(off.to_le_bytes());
        }
        assert_eq!(pack_record_offset(&pack, 0), Some(0x10));
        assert_eq!(pack_record_offset(&pack, 2), Some(0x40));
        assert_eq!(pack_record_offset(&pack, 3), None, "past the bytes");
        assert_eq!(
            pack_record_offset(&pack, -1),
            Some(3),
            "index -1 reads the count"
        );
        let s = spawn_init(&pack, 1).unwrap();
        assert_eq!(
            s,
            SpawnInit {
                record_offset: 0x24,
                anim_cursor: 0,
                clip_kick: 0xB,
                frame_counter: 100,
                field_6e: 0,
            }
        );
    }
}
