//! The version every shipped program shows and reports: the launcher's
//! header, each program's `--version`, Explorer's About box and every bug
//! report's version line.
//!
//! A release build bakes in its tag: `scripts/ci/release-build.sh` exports
//! [`RELEASE_VERSION_ENV`] before it builds, and [`VERSION`] reads it at
//! compile time. Any other build (a local `cargo build`) falls back to the
//! workspace's crate version, which is not a release number - before this
//! crate a command-line tool's `--version` read `0.1.0` whatever release it
//! shipped in.
//!
//! The crate has no dependencies, so a format crate's binary can take it
//! without pulling anything else into its build.

/// The environment variable a release build sets to the release's own
/// version (`scripts/ci/release-build.sh`, from the tag), read at compile
/// time by [`VERSION`].
pub const RELEASE_VERSION_ENV: &str = "LEGAIA_RELEASE_VERSION";

/// The release this build ships in, or the workspace crate version when the
/// build was not made by the release script.
pub const VERSION: &str = match option_env!("LEGAIA_RELEASE_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => env!("CARGO_PKG_VERSION"),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_never_empty() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn the_env_name_is_the_one_option_env_reads() {
        // `option_env!` takes a literal, so the name is spelt twice; this
        // keeps the two spellings one.
        let src = include_str!("lib.rs");
        let needle = ["option_env!(\"", RELEASE_VERSION_ENV, "\")"].concat();
        assert!(src.contains(&needle));
    }
}
