//! Which retail build a disc is, and how its text is encoded.
//!
//! The export's disc-coordinate sections (scene dialog, raw carriers, monster
//! names) read the same structures on every build, but two things differ:
//!
//! - the **executable**: the name tables, system strings and overlay UI pools
//!   are keyed by `SCUS_942.54` virtual addresses, which only the USA build
//!   has (the PAL and Japanese executables are other builds with other
//!   layouts - see `docs/tooling/pal-localizations.md`);
//! - the **text codec**: the Latin builds frame a line `0x1F <glyphs> 0x00`
//!   (the PAL builds add accented glyphs above `0x7E`), the Japanese build
//!   uses count-led Shift-JIS lines ([`super::sjis`]).

use crate::disc::DiscPatcher;

/// How dialog text is framed and encoded on a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCodec {
    /// `0x1F <glyphs> 0x00`; `allow_high` accepts the PAL accented glyphs
    /// above `0x7E` as letters (see [`super::segments::qualifies_ext`]).
    Latin { allow_high: bool },
    /// Count-led Shift-JIS lines ([`super::sjis`]).
    ShiftJis,
}

/// A disc's build, as its `SYSTEM.CNF` boot line names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscBuild {
    /// Boot executable name (`SCUS_942.54`, `SCES_019.47`, `SCPS_100.59`, ...).
    pub exe: String,
    /// Dialog text codec.
    pub codec: TextCodec,
}

impl DiscBuild {
    /// `true` for the USA build - the one whose executable the `scus:` /
    /// `ui:` key spaces address.
    pub fn is_usa(&self) -> bool {
        self.exe == "SCUS_942.54"
    }

    /// Language code of the disc's own text.
    pub fn language(&self) -> &'static str {
        match self.codec {
            TextCodec::ShiftJis => "ja",
            TextCodec::Latin { .. } => super::lift::source_build_for_exe(&self.exe)
                .map(|b| b.lang)
                .unwrap_or("en"),
        }
    }

    /// Human label for the pack header (`game:`).
    pub fn label(&self) -> String {
        match self.exe.as_str() {
            "SCUS_942.54" => "Legend of Legaia (USA) SCUS-94254".to_string(),
            "SCPS_100.59" => "Legaia Densetsu (Japan) SCPS-10059".to_string(),
            exe => match super::lift::source_build_for_exe(exe) {
                Some(b) => format!("Legend of Legaia - {} - {exe}", b.label),
                None => format!("Legend of Legaia - {exe}"),
            },
        }
    }
}

/// `true` for a Japanese boot executable (retail `SCPS`, `SLPS`, and the
/// `PAPX` / `SCPM` demo prefixes).
fn is_japanese_exe(exe: &str) -> bool {
    ["SCPS", "SLPS", "SCPM", "PAPX"]
        .iter()
        .any(|p| exe.starts_with(p))
}

/// Detect the disc's build. A disc whose `SYSTEM.CNF` is unreadable but which
/// carries `SCUS_942.54` is the USA build.
pub fn detect(patcher: &DiscPatcher) -> DiscBuild {
    let exe = super::lift::boot_exe_name(patcher)
        .ok()
        .or_else(|| {
            patcher
                .read_named_file("SCUS_942.54")
                .map(|_| "SCUS_942.54".to_string())
        })
        .unwrap_or_default();
    let codec = if is_japanese_exe(&exe) {
        TextCodec::ShiftJis
    } else {
        TextCodec::Latin {
            allow_high: exe != "SCUS_942.54",
        }
    };
    DiscBuild { exe, codec }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_prefixes() {
        assert!(is_japanese_exe("SCPS_100.59"));
        assert!(!is_japanese_exe("SCUS_942.54"));
        assert!(!is_japanese_exe("SCES_019.47"));
    }
}
