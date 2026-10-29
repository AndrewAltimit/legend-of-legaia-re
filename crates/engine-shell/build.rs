//! The Windows executables (`legaia-engine.exe`, and `legaia-launch.exe`,
//! shipped as `Legend of Legaia.exe`) carry the game icon
//! (`legaia_app_theme::win_icon`); other targets build nothing here.

fn main() {
    legaia_app_theme::win_icon::embed_in_bins(legaia_app_theme::icons::GAME_ICO);
}
