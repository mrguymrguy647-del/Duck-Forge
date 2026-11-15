//! Expectations: what each moment of a film (or a game) must show,
//! written down before it's rendered, and judged by the review.
//!
//! ```
//! # use duckforge::prelude::*;
//! # fn example(ctx: &mut Context) {
//! // In init: the plan.
//! ctx.expect(Expect::at(51.6).thing("host").on_screen().fills_height(0.4).lit_above(0.25));
//! ctx.expect(Expect::at(37.8).caption_readable());
//! ctx.expect(Expect::on("impact").sound("thump").within_frames(2));
//! ctx.expect(Expect::on("enemy attacks").thing("enemy").on_screen().lit_above(0.2).at_least_px(80.0));
//! # }
//! ```

/// When an expectation is checked.
#[derive(Debug, Clone, PartialEq)]
pub enum When {
    /// At this many seconds into the run (the frame nearest it).
    At(f32),
    /// On every frame the game marks this event (`ctx.event`).
    On(String),
}

/// What must be true then.
#[derive(Debug, Clone, PartialEq)]
pub enum What {
    /// A named thing (`canvas.label_model` or `canvas.label`).
    Thing {
        name: String,
        /// On the screen at all.
        on_screen: bool,
        /// At least this share of the picture's height (0.4: 40%).
        min_height_share: Option<f32>,
        /// At least this many pixels tall.
        min_px: Option<f32>,
        /// Its pixels at least this bright on average (0 black to 1 white).
        min_light: Option<f32>,
    },
    /// Every text on screen is readable on every matching profile: enough
    /// contrast, big enough, and clear of what each app covers.
    CaptionReadable,
    /// A sound by this name starts within this many frames.
    Sound { name: String, within_frames: u64 },
}

/// One thing a moment must show. See the [module docs](self).
#[derive(Debug, Clone, PartialEq)]
pub struct Expect {
    pub when: When,
    pub what: What,
}

impl Expect {
    /// Checked at `seconds` into the run.
    pub fn at(seconds: f32) -> ExpectWhen {
        ExpectWhen(When::At(seconds))
    }

    /// Checked every time the game marks `event` with `ctx.event(event)`.
    pub fn on(event: &str) -> ExpectWhen {
        ExpectWhen(When::On(event.to_string()))
    }

    /// Shorthand: `sound` starts within `frames` of every `event`.
    pub fn sound_near(sound: &str, event: &str, frames: u64) -> Expect {
        Expect::on(event).sound(sound).within_frames(frames)
    }

    fn thing_mut(
        mut self,
        change: impl FnOnce(&mut Option<f32>, &mut Option<f32>, &mut Option<f32>, &mut bool),
    ) -> Expect {
        if let What::Thing {
            min_height_share,
            min_px,
            min_light,
            on_screen,
            ..
        } = &mut self.what
        {
            change(min_height_share, min_px, min_light, on_screen);
        }
        self
    }

    /// The thing must be on the screen.
    pub fn on_screen(self) -> Expect {
        self.thing_mut(|_, _, _, on| *on = true)
    }

    /// The thing must be at least `share` of the picture's height (0.4: 40%).
    pub fn fills_height(self, share: f32) -> Expect {
        self.thing_mut(|h, _, _, on| {
            *h = Some(share);
            *on = true;
        })
    }

    /// The thing must be at least `pixels` tall.
    pub fn at_least_px(self, pixels: f32) -> Expect {
        self.thing_mut(|_, px, _, on| {
            *px = Some(pixels);
            *on = true;
        })
    }

    /// The thing's pixels must be at least this bright on average (0 to 1).
    pub fn lit_above(self, light: f32) -> Expect {
        self.thing_mut(|_, _, l, on| {
            *l = Some(light);
            *on = true;
        })
    }

    /// A sound must start within `frames` of the moment.
    pub fn within_frames(mut self, frames: u64) -> Expect {
        if let What::Sound { within_frames, .. } = &mut self.what {
            *within_frames = frames;
        }
        self
    }

    /// The expectation in words, for the review.
    pub fn words(&self) -> String {
        let when = match &self.when {
            When::At(s) => format!("at {s:.2} s"),
            When::On(e) => format!("when \"{e}\""),
        };
        let what = match &self.what {
            What::Thing {
                name,
                on_screen,
                min_height_share,
                min_px,
                min_light,
            } => {
                let mut parts = Vec::new();
                if *on_screen {
                    parts.push(String::from("on screen"));
                }
                if let Some(h) = min_height_share {
                    parts.push(format!("at least {:.0}% of the height", h * 100.0));
                }
                if let Some(px) = min_px {
                    parts.push(format!("at least {px:.0} px tall"));
                }
                if let Some(l) = min_light {
                    parts.push(format!("lit above {l:.2}"));
                }
                format!("\"{name}\" {}", parts.join(", "))
            }
            What::CaptionReadable => {
                String::from("the text on screen is readable on every matching profile")
            }
            What::Sound {
                name,
                within_frames,
            } => format!("\"{name}\" starts within {within_frames} frames"),
        };
        format!("{when}, {what}")
    }
}

/// [`Expect::at`] or [`Expect::on`], waiting to be told what.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpectWhen(When);

impl ExpectWhen {
    /// A named thing (then `.on_screen()`, `.fills_height(0.4)`,
    /// `.at_least_px(80.0)`, `.lit_above(0.3)`).
    pub fn thing(self, name: &str) -> Expect {
        Expect {
            when: self.0,
            what: What::Thing {
                name: name.to_string(),
                on_screen: true,
                min_height_share: None,
                min_px: None,
                min_light: None,
            },
        }
    }

    /// Every text on screen is readable on every matching profile.
    pub fn caption_readable(self) -> Expect {
        Expect {
            when: self.0,
            what: What::CaptionReadable,
        }
    }

    /// A sound by this name (then `.within_frames(2)`; 2 unless you say).
    pub fn sound(self, name: &str) -> Expect {
        Expect {
            when: self.0,
            what: What::Sound {
                name: name.to_string(),
                within_frames: 2,
            },
        }
    }
}

/// Something the game marks as it plays: an event, a place change, a
/// cut, or a stretch whose motion is meant (`ctx.intend`).
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    Event(String),
    Place(String),
    Cut,
    /// What's intended, and for how many seconds.
    Intend(String, f32),
}
