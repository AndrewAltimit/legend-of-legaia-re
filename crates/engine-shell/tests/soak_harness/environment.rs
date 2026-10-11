//! Where the soak reads its game from and writes its output: the env knobs,
//! the disc / extracted source, session boot and the report directory.

use super::*;

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

pub(super) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

pub(super) fn env_u64(key: &str) -> Option<u64> {
    let v = std::env::var(key).ok()?;
    let v = v.trim();
    if let Some(hex) = v.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        v.parse().ok()
    }
}

pub(super) fn env_flag(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Where the session reads its assets from.
#[derive(Clone, Debug)]
pub(super) enum Source {
    Extracted(PathBuf),
    Disc(PathBuf),
}

pub(super) fn source() -> Option<Source> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from);
    let Some(disc) = disc.filter(|p| p.exists()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or missing (disc-gated convention)");
        return None;
    };
    let mut candidates = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        candidates.push(PathBuf::from(d));
    }
    candidates.push(repo_root().join("extracted"));
    for d in candidates {
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(Source::Extracted(d));
        }
    }
    Some(Source::Disc(disc))
}

pub(super) fn open_session(src: &Source) -> BootSession {
    let cfg = BootConfig {
        scene: legaia_engine_session::boot::DEFAULT_BOOT_SCENE.to_string(),
        enable_audio: false,
    };
    match src {
        Source::Extracted(d) => BootSession::open(d, &cfg).expect("open BootSession (extracted)"),
        Source::Disc(p) => BootSession::open_disc(p, &cfg).expect("open BootSession (disc)"),
    }
}

pub(super) fn out_dir(tag: &str) -> PathBuf {
    let base = std::env::var_os("LEGAIA_SOAK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target").join("soak"));
    base.join(tag)
}
