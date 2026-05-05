//! Region detection from a PSX disc's `SYSTEM.CNF`.
//!
//! `SYSTEM.CNF` always contains a `BOOT=` line pointing at the executable, e.g.:
//!
//! ```text
//! BOOT= cdrom:\SCUS_942.54;1
//! ```
//!
//! The executable's name prefix encodes the publisher region:
//!
//! | Prefix | Region | Example |
//! |--------|--------|---------|
//! | SCUS / SLUS / SCPS-* (NA-pack) | North America | `SCUS_942.54` |
//! | SCES / SLES                     | Europe        | `SCES_017.52` |
//! | SLPS / SLPM / SCPS / SIPS       | Japan         | `SLPS_015.00` |
//! | other                           | Unknown       | — |
//!
//! Note: SCPS overlaps NA and JP catalogs (Sony released some "Special"
//! titles under SCPS in both regions). We treat SCPS as Japan since SCUS
//! covers NA. Override via the `Region::ForceRegion` helper if needed.

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    NorthAmerica,
    Europe,
    Japan,
    Unknown,
}

impl Region {
    pub fn name(&self) -> &'static str {
        match self {
            Region::NorthAmerica => "North America",
            Region::Europe => "Europe",
            Region::Japan => "Japan",
            Region::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetectedRegion {
    /// The exact executable name (e.g. "SCUS_942.54").
    pub executable: String,
    /// Inferred region from the executable prefix.
    pub region: Region,
    /// 4-letter prefix lifted from the executable name.
    pub prefix: String,
}

/// Parse a `SYSTEM.CNF` byte buffer and return the detected region.
pub fn parse(buf: &[u8]) -> Result<DetectedRegion> {
    let text =
        std::str::from_utf8(buf).context("SYSTEM.CNF is not valid UTF-8 (PSX should be ASCII)")?;
    for line in text.lines() {
        let line = line.trim();
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("boot=") {
            let val = rest.trim();
            // Recover the original-cased value (lower may have lost case).
            let original = line.split_once('=').map(|x| x.1.trim()).unwrap_or(val);
            return parse_boot_value(original);
        }
    }
    bail!("SYSTEM.CNF has no BOOT= line")
}

fn parse_boot_value(boot: &str) -> Result<DetectedRegion> {
    // Strip optional "cdrom:\" prefix and ";N" suffix.
    let after_cdrom = boot
        .split_once(':')
        .map(|x| x.1.trim_start_matches('\\').trim_start_matches('/'))
        .unwrap_or(boot);
    let executable = after_cdrom
        .split(';')
        .next()
        .unwrap_or(after_cdrom)
        .trim()
        .to_string();

    if executable.len() < 4 {
        bail!("BOOT= value '{}' too short to derive a region prefix", boot);
    }
    let prefix: String = executable.chars().take(4).collect();
    let region = region_from_prefix(&prefix);
    Ok(DetectedRegion {
        executable,
        region,
        prefix,
    })
}

fn region_from_prefix(prefix: &str) -> Region {
    let p = prefix.to_ascii_uppercase();
    match p.as_str() {
        "SCUS" | "SLUS" => Region::NorthAmerica,
        "SCES" | "SLES" => Region::Europe,
        "SLPS" | "SLPM" | "SCPS" | "SIPS" | "SLKA" | "SLKH" => Region::Japan,
        _ => Region::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_na_legaia() {
        let buf = b"BOOT= cdrom:\\SCUS_942.54;1\nTCB= 4\nEVENT= 10\nSTACK= 801FFFFC\n";
        let d = parse(buf).unwrap();
        assert_eq!(d.executable, "SCUS_942.54");
        assert_eq!(d.prefix, "SCUS");
        assert_eq!(d.region, Region::NorthAmerica);
    }

    #[test]
    fn parses_eu_legaia_hypothetical() {
        let buf = b"BOOT=cdrom:\\SCES_017.52;1\n";
        let d = parse(buf).unwrap();
        assert_eq!(d.executable, "SCES_017.52");
        assert_eq!(d.region, Region::Europe);
    }

    #[test]
    fn parses_jp_legaia_hypothetical() {
        let buf = b"BOOT=cdrom:\\SLPS_015.00;1\n";
        let d = parse(buf).unwrap();
        assert_eq!(d.executable, "SLPS_015.00");
        assert_eq!(d.region, Region::Japan);
    }

    #[test]
    fn handles_missing_boot_line() {
        let buf = b"TCB=4\nEVENT=10\n";
        assert!(parse(buf).is_err());
    }

    #[test]
    fn handles_unknown_prefix() {
        let buf = b"BOOT= cdrom:\\XXXX_999.99;1\n";
        let d = parse(buf).unwrap();
        assert_eq!(d.region, Region::Unknown);
    }

    #[test]
    fn case_insensitive_boot_keyword() {
        let buf = b"boot= cdrom:\\SCUS_942.54;1\n";
        let d = parse(buf).unwrap();
        assert_eq!(d.region, Region::NorthAmerica);
    }
}
