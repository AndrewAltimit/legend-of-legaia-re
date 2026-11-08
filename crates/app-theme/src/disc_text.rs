//! The wording for a disc image that stops before the disc does, shared by
//! the launcher and Explorer.
//!
//! An interrupted copy or download leaves a `.bin` that opens fine: its first
//! sectors are whole, so the only thing that notices is a read past the end,
//! and that read's own message ("failed to fill whole buffer") tells a player
//! nothing. Each program walks the image's file table and asks here.

use std::error::Error;
use std::format;
use std::io;
use std::string::String;

/// Bytes per sector of the raw dump the programs read.
const RAW_SECTOR: u64 = 2352;
/// Bytes of a file's data per sector, as the ISO9660 file table counts them.
const DATA_SECTOR: u64 = 2048;

/// The image ends inside the disc's own file list, so how much is missing
/// cannot be said.
pub const CUT_SHORT: &str = "This disc image is incomplete: the file ends too early.";

/// What to do about an incomplete image.
pub const INCOMPLETE_FIX: &str = "It was probably cut short while copying or downloading. \
     Copy or dump it again, then choose the whole .bin.";

/// What to do about an image whose own files are damaged or missing.
pub const DAMAGED_FIX: &str = "The image is damaged or only part of the disc: dump it again.";

/// Whether `e` is a read that ran off the end of the file.
pub fn is_short_read(e: &(dyn Error + 'static)) -> bool {
    e.downcast_ref::<io::Error>()
        .is_some_and(|io| io.kind() == io::ErrorKind::UnexpectedEof)
}

/// The sector count the files of a disc reach to, from each file's
/// `(start sector, size in bytes)` in the ISO9660 table.
pub fn sectors_needed(files: impl IntoIterator<Item = (u32, u32)>) -> u64 {
    files
        .into_iter()
        .map(|(lba, size)| u64::from(lba) + u64::from(size).div_ceil(DATA_SECTOR))
        .max()
        .unwrap_or(0)
}

/// The sentence for an image of `file_len` bytes whose files reach to
/// `needed` sectors, or `None` when it holds them all.
pub fn incomplete_image(file_len: u64, needed: u64) -> Option<String> {
    let have = file_len / RAW_SECTOR;
    if have >= needed {
        return None;
    }
    // Rounded down, so that a nearly whole image never reads as 100%.
    let percent = have * 100 / needed;
    Some(format!(
        "This disc image is incomplete: the file holds only {percent}% of the disc."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;

    #[test]
    fn a_file_reaches_to_its_last_partial_sector() {
        assert_eq!(sectors_needed([(10, 2048)]), 11);
        assert_eq!(sectors_needed([(10, 2049)]), 12);
        assert_eq!(sectors_needed([(10, 0)]), 10);
        assert_eq!(sectors_needed([(100, 2048), (10, 2048)]), 101);
        assert_eq!(sectors_needed([]), 0);
    }

    #[test]
    fn a_whole_image_is_not_incomplete() {
        assert_eq!(incomplete_image(100 * RAW_SECTOR, 100), None);
        assert_eq!(incomplete_image(101 * RAW_SECTOR, 100), None);
        assert_eq!(incomplete_image(0, 0), None);
    }

    #[test]
    fn a_short_image_says_how_much_it_holds() {
        let text = incomplete_image(43 * RAW_SECTOR, 100).unwrap();
        assert!(text.contains("only 43%"), "{text}");
        // One sector short of a big disc is still not "100%".
        let text = incomplete_image(99_999 * RAW_SECTOR, 100_000).unwrap();
        assert!(text.contains("only 99%"), "{text}");
    }

    #[test]
    fn only_an_end_of_file_counts_as_a_short_read() {
        let eof: Box<dyn Error> = Box::new(io::Error::from(io::ErrorKind::UnexpectedEof));
        assert!(is_short_read(eof.as_ref()));
        let denied: Box<dyn Error> = Box::new(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(!is_short_read(denied.as_ref()));
    }
}
