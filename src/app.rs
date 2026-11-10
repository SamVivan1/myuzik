//! Application routing state.
//!
//! The running UI lives in [`crate::ui::shell::Shell`]; this module only holds
//! the route enum that the shell and the tests share.

/// A top-level destination in the main window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Setup,
    Home,
    Search,
    Albums,
    Artists,
    Playlists,
    Favorites,
    Album(String),
    Artist(String),
    Playlist(String),
    NowPlaying,
    Settings,
}
