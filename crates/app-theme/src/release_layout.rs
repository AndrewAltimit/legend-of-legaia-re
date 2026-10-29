//! Where the release archive puts the programs (`docs/tooling/releases.md`),
//! as the two programs that start the game look for it: the `legaia-launch`
//! stub (`Legend of Legaia` / `Legend of Legaia.exe`) and Legaia Explorer's
//! Play button.
//!
//! The archive's top folder holds only what a player opens - the game and
//! Legaia Explorer (as `.app` bundles on macOS) - and every command-line
//! program, `legaia-engine` among them, sits in [`TOOLS_DIR`] below it. A
//! `cargo build` puts every binary side by side in `target/<profile>/`
//! instead; [`find_engine`] accepts both.

use std::path::{Path, PathBuf};
use std::string::String;

/// The release archive's folder of command-line programs.
pub const TOOLS_DIR: &str = "command-line-tools";

/// The macOS app bundle the game ships as.
pub const GAME_APP: &str = "Legend of Legaia.app";

/// The engine binary's file name on this platform.
pub fn engine_file_name() -> String {
    std::format!("legaia-engine{}", std::env::consts::EXE_SUFFIX)
}

/// The `legaia-engine` that belongs with the program at `exe`. Tried in
/// each of the four folders above `exe` (its own folder first; a program
/// inside a macOS `.app` sits three folders below the archive folder):
/// the engine in that folder, in its [`TOOLS_DIR`], and inside a
/// [`GAME_APP`] there.
pub fn find_engine(exe: &Path) -> Option<PathBuf> {
    let name = engine_file_name();
    exe.ancestors()
        .skip(1)
        .take(4)
        .flat_map(|d| {
            [
                d.join(&name),
                d.join(TOOLS_DIR).join(&name),
                d.join(GAME_APP).join("Contents").join("MacOS").join(&name),
            ]
        })
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    /// A scratch folder under the system temp dir, removed on drop.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(std::format!(
                "legaia-release-layout-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_the_engine_in_the_tools_folder() {
        let s = Scratch::new("tools");
        let root = &s.0;
        assert_eq!(find_engine(&root.join("Legend of Legaia")), None);
        let engine = root.join(TOOLS_DIR).join(engine_file_name());
        touch(&engine);
        assert_eq!(
            find_engine(&root.join("Legend of Legaia")),
            Some(engine.clone())
        );
        assert_eq!(
            find_engine(&root.join("Legaia Explorer")),
            Some(engine.clone())
        );
        // Legaia Explorer.app/Contents/MacOS/legaia-explorer
        let bundled = root
            .join("Legaia Explorer.app")
            .join("Contents")
            .join("MacOS")
            .join("legaia-explorer");
        assert_eq!(find_engine(&bundled), Some(engine));
    }

    #[test]
    fn a_build_folder_engine_beside_the_program_comes_first() {
        let s = Scratch::new("beside");
        let root = &s.0;
        let beside = root.join(engine_file_name());
        touch(&beside);
        touch(&root.join(TOOLS_DIR).join(engine_file_name()));
        assert_eq!(find_engine(&root.join("legaia-launch")), Some(beside));
    }

    #[test]
    fn finds_the_engine_inside_the_game_app() {
        let s = Scratch::new("app");
        let root = &s.0;
        let engine = root
            .join(GAME_APP)
            .join("Contents")
            .join("MacOS")
            .join(engine_file_name());
        touch(&engine);
        let explorer = root
            .join("Legaia Explorer.app")
            .join("Contents")
            .join("MacOS")
            .join("legaia-explorer");
        assert_eq!(find_engine(&explorer), Some(engine));
    }
}
