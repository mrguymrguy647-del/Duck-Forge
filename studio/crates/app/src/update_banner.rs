//! The "a new version is out" banner on Home.
//!
//! It shows only when the check found a newer Studio (see
//! `studio_core::update`). "What's new" opens the release's page; "Update"
//! downloads its installer and starts it, and the Studio closes so the
//! installer can put the new version in. Nothing is downloaded until
//! Update is pressed. "Not now" hides it until the Studio starts again.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use studio_core::update::{self, Curl, Download, NewVersion, UpdateCheck};

use crate::theme;

/// Where the new-version check is.
#[derive(Default)]
pub enum UpdateState {
    /// Not checking (turned off in Settings, or a test).
    #[default]
    Off,
    Checking(UpdateCheck),
    /// Up to date, or no internet: nothing to show.
    Nothing,
    Available(NewVersion),
    Downloading(NewVersion, Download),
    /// The installer was downloaded and started: the Studio closes.
    Installing(PathBuf),
    /// The download (after Update was pressed) didn't work, and why.
    Failed(NewVersion, String),
    /// "Not now".
    Dismissed,
}

/// What a person did with the banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerChoice {
    WhatsNew(String),
    Update,
    NotNow,
}

impl UpdateState {
    /// Starts checking, with the real internet.
    pub fn check() -> UpdateState {
        UpdateState::Checking(UpdateCheck::start(Curl, env!("CARGO_PKG_VERSION")))
    }

    /// Takes in the check's or the download's answer, if it came. Returns
    /// true when the new installer has been started and the Studio should
    /// close.
    pub fn poll(&mut self) -> bool {
        match self {
            UpdateState::Checking(check) => {
                if let Some(answer) = check.poll() {
                    *self = match answer {
                        Some(new) => UpdateState::Available(new),
                        None => UpdateState::Nothing,
                    };
                }
                false
            }
            UpdateState::Downloading(new, download) => {
                let Some(done) = download.poll() else {
                    return false;
                };
                let new = new.clone();
                match done.and_then(|file| update::start_installer(&file).map(|()| file)) {
                    Ok(file) => {
                        *self = UpdateState::Installing(file);
                        true
                    }
                    Err(why) => {
                        *self = UpdateState::Failed(new, why);
                        false
                    }
                }
            }
            _ => false,
        }
    }

    /// Update was pressed: download the installer, or, if the release has
    /// none, open its page.
    pub fn update(&mut self, ctx: &egui::Context) {
        let new = match self {
            UpdateState::Available(new) | UpdateState::Failed(new, _) => new.clone(),
            _ => return,
        };
        if new.installer.is_none() {
            ctx.open_url(egui::OpenUrl::new_tab(&new.whats_new));
            return;
        }
        let download = Download::start(Curl, &new, &update::download_folder());
        *self = UpdateState::Downloading(new, download);
    }

    /// Whether the banner shows.
    pub fn shows(&self) -> bool {
        matches!(
            self,
            UpdateState::Available(_)
                | UpdateState::Downloading(..)
                | UpdateState::Installing(_)
                | UpdateState::Failed(..)
        )
    }
}

/// The banner, if there's something to show.
pub fn banner(ui: &mut egui::Ui, state: &UpdateState) -> Option<BannerChoice> {
    let mut choice = None;
    let new = match state {
        UpdateState::Available(new)
        | UpdateState::Downloading(new, _)
        | UpdateState::Failed(new, _) => new,
        UpdateState::Installing(_) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Starting the installer. The Studio closes so it can be updated...");
            });
            return None;
        }
        _ => return None,
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("Duck Forge Studio {} is out.", new.version))
                .strong()
                .color(theme::DUCK_YELLOW),
        );
        ui.label(RichText::new(format!("You have {}.", env!("CARGO_PKG_VERSION"))).weak());
        if ui.link("What's new").clicked() {
            choice = Some(BannerChoice::WhatsNew(new.whats_new.clone()));
        }
        match state {
            UpdateState::Downloading(..) => {
                ui.spinner();
                ui.label("Downloading the new version...");
            }
            _ => {
                let label = if matches!(state, UpdateState::Failed(..)) {
                    "Try again"
                } else {
                    "Update"
                };
                if ui
                    .add(theme::primary_button(label))
                    .on_hover_text(
                        "Downloads the new installer and starts it. Your games and settings \
                         stay as they are.",
                    )
                    .clicked()
                {
                    choice = Some(BannerChoice::Update);
                }
                if ui.button("Not now").clicked() {
                    choice = Some(BannerChoice::NotNow);
                }
            }
        }
    });
    if let UpdateState::Failed(_, why) = state {
        ui.label(
            RichText::new(format!(
                "The download didn't work ({why}). Try again, or get the installer from the \
                 What's new page."
            ))
            .small()
            .color(theme::LOOK),
        );
    }
    choice
}
