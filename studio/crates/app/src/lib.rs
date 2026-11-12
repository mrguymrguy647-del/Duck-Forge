//! Duck Forge Studio's window: what's drawn, and turning clicks and keys
//! into the Studio's commands.
//!
//! Everything the Studio *does* is in `studio_core`. This crate only shows
//! it. It's a library as well as a program, so the tests can draw every
//! screen with no window and save a picture of it.

pub mod app;
pub mod build_mode;
pub mod feel_panel;
pub mod files_panel;
pub mod game_view;
pub mod home;
pub mod keys;
pub mod panels;
pub mod pictures;
pub mod scene_panel;
pub mod theme;
pub mod update_banner;

pub use app::StudioApp;
