//! The cutscene class: an STR movie on screen. Retail's frame comes out of the
//! state's VRAM like any other; the engine's is the same movie frame through the
//! port's own decode path (`psx_mdec`), with no world and no window. A movie
//! has no state to seed, so the state scores the image channel alone.

use super::*;
use psx_mdec::str_sector::StrFrameAssembler;

/// `DAT_801CAD90`, the libcd streaming layer's current `frame_number`
/// ([cutscene](../../../../docs/subsystems/cutscene.md)).
pub const MOVIE_FRAME_NUMBER: u32 = 0x801C_AD90;
/// `_DAT_8007BA78`, the `fmv_id` op `4C E2` stores.
pub const MOVIE_FMV_ID: u32 = 0x8007_BA78;

/// The FMV dispatch table, resident in the movie overlay.
const FMV_TABLE: u32 = 0x801D_0A6C;

/// How far behind the stream's frame word the displayed frame may sit: the word
/// moves when a frame's chunks arrive, ahead of its decode, its upload and the
/// buffer flip. The candidates scored are that many frames back, and the best
/// names the lag in the detail.
const MOVIE_FRAME_LAG: u32 = 3;

/// The `(fmv id, stream frame number)` a movie-class capture stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailMovie {
    pub fmv: i16,
    pub frame_number: u32,
}

impl RetailMovie {
    pub(super) fn from_ram(ram: &[u8]) -> Self {
        Self {
            fmv: game_anchors::u16_at(ram, MOVIE_FMV_ID) as i16,
            frame_number: game_anchors::u32_at(ram, MOVIE_FRAME_NUMBER),
        }
    }

    /// The dispatch slot's `+0x04` colour flag (table `0x801D0A6C`, stride
    /// `0x20`): non-zero scans the frame out as 24-bit.
    pub(super) fn is_24bit(&self, ram: &[u8]) -> bool {
        let slot = FMV_TABLE.wrapping_add(u32::from(self.fmv as u16) * 0x20);
        game_anchors::u32_at(ram, slot + 4) != 0
    }
}

/// `(frame number, RGBA, width, height)`.
type DecodedFrame = (u32, Vec<u8>, usize, usize);

/// Decode the movie's frames whose number lies in `first..=last`, as
/// [`DecodedFrame`]s, off the extracted Form-1 file.
fn decode_frames(path: &std::path::Path, first: u32, last: u32) -> Result<Vec<DecodedFrame>> {
    const USER_DATA: usize = psx_iso::raw::USER_DATA_SIZE;
    let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut asm = StrFrameAssembler::new();
    let mut out = Vec::new();
    for sector in data.as_chunks::<USER_DATA>().0 {
        let Ok(Some((hdr, bitstream))) = asm.push_sector(sector) else {
            continue;
        };
        if hdr.frame_number < first {
            continue;
        }
        if hdr.frame_number > last {
            break;
        }
        let bits = psx_mdec::str_av::StrFrameBits {
            width: u32::from(hdr.width),
            height: u32::from(hdr.height),
            frame_number: hdr.frame_number,
            bitstream,
        };
        if let Some(rgba) = bits.decode_rgba() {
            out.push((
                hdr.frame_number,
                rgba,
                hdr.width as usize,
                hdr.height as usize,
            ));
        }
    }
    Ok(out)
}

/// Score a movie-class state.
pub(super) fn run_movie(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    retail: &RetailObs,
    report: &mut StateReport,
) {
    let Some(movie) = retail.movie else {
        report.unseeded = "movie state with no readable frame word".into();
        return;
    };
    let Some(name) = legaia_engine_system::movie::cutscene::fmv_index_to_str_filename(movie.fmv)
    else {
        report.unseeded = format!("fmv id {} names no movie on the disc", movie.fmv);
        return;
    };
    let Some(rf) = retail.frame.as_ref() else {
        report.unseeded = "no retail frame in the state".into();
        return;
    };
    let path = opts.extracted.join(name);
    let first = movie.frame_number.saturating_sub(MOVIE_FRAME_LAG);
    let frames = match decode_frames(&path, first, movie.frame_number) {
        Ok(f) => f,
        Err(e) => {
            report.unseeded = format!("{e:#}");
            return;
        }
    };
    let mut best: Option<(ImageScore, Frame, u32)> = None;
    for (number, rgba, w, h) in frames {
        let frame = Frame::from_movie(&rgba, w, h);
        let score = crate::retail_compare_image::score(rf, &frame);
        if best.as_ref().is_none_or(|b| score.within > b.0.within) {
            best = Some((score, frame, number));
        }
    }
    let Some((score, frame, number)) = best else {
        report.unseeded = format!(
            "{name} decodes no frame in {first}..={}",
            movie.frame_number
        );
        return;
    };
    if let Some(dir) = opts.out_dir {
        let _ = crate::retail_compare_image::write_side_by_side(
            &dir.join(format!("{}.png", entry.label)),
            rf,
            &frame,
        );
    }
    let mut ch = BTreeMap::new();
    ch.insert("image".to_string(), round3(score.within));
    report.detail.insert(
        "image".into(),
        format!(
            "{name} frame {number} (stream word {}): mae={:.1} within={:.3}",
            movie.frame_number, score.mae, score.within
        ),
    );
    report.image = Some(score);
    report.score = state_score(&ch);
    report.channels = ch;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_movie_words_read_off_ram() {
        let mut ram = vec![0u8; 0x20_0000];
        ram[(MOVIE_FMV_ID & 0x1F_FFFF) as usize] = 6;
        ram[(MOVIE_FRAME_NUMBER & 0x1F_FFFF) as usize] = 42;
        assert_eq!(
            RetailMovie::from_ram(&ram),
            RetailMovie {
                fmv: 6,
                frame_number: 42
            }
        );
    }
}
