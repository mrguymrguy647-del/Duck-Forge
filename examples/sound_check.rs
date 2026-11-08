//! Sound check: plays music and a few looping sounds while keeping the
//! computer busy, so you can hear whether sound comes out smooth.
//!
//! Run it with:   cargo run --release --example sound_check
//! Controls:      Space plays a coin, Up and Down change the music's volume,
//!                M stops or starts the music, T stops or starts the tone,
//!                B changes how busy the game keeps the computer, Escape quits.
//!
//! Listen for a minute or two. The music should sound just like the file,
//! and the tone should be one clean, steady note: a click, crackle or skip
//! in either is a glitch. When you close the window, the console says how
//! many times the sound thread was late, like:
//!
//!     duckforge: sound: 0 late refills in 2 minutes (...)
//!
//! To check your own game's music, point SOUND_CHECK_MUSIC at its file:
//!
//!     SOUND_CHECK_MUSIC=path/to/music.wav cargo run --release --example sound_check
//!
//! On Windows, set it first: `set SOUND_CHECK_MUSIC=path\to\music.wav` in
//! Command Prompt, or `$env:SOUND_CHECK_MUSIC="path\to\music.wav"` in
//! PowerShell. To try a bigger sound buffer, set DUCKFORGE_SOUND_BLOCK=4096
//! the same way.

use std::time::{Duration, Instant};

use duckforge::prelude::*;

// ---- Tuning knobs ----

/// The music played if SOUND_CHECK_MUSIC isn't set.
const MUSIC: &str = "examples/duck_peaks/assets/music.wav";
const HUM: &str = "examples/duck_craft/assets/hum.wav";
const MUSIC_VOLUME: f32 = 0.5;
const HUM_VOLUME: f32 = 0.25;
const TONE_VOLUME: f32 = 0.12;
/// How much Up and Down change the music's volume.
const VOLUME_STEP: f32 = 0.1;
/// Milliseconds of busy work each drawn frame, like a game drawing on the
/// CPU. B moves to the next one. (A frame at 60 FPS is about 16.7.)
const BUSY_STEPS: [u32; 4] = [12, 25, 0, 6];

/// A steady 220 Hz note, half a second long, with no fade at either end:
/// 110 whole waves, so it loops with no seam. Any click in it is a glitch.
const TONE: Sound = Sound {
    name: "tone",
    wave: Wave::Sine,
    freq: 220.0,
    slide_to: 0.0,
    ms: 500.0,
    volume: 1.0,
    attack: 0.0,
    decay: 0.0,
};

const BACKGROUND: Color = Color::from_hex(0x1d2433);
const TITLE: Color = Color::from_hex(0xffd94a);
const TEXT: Color = Color::from_hex(0xd8dee9);
const DIM: Color = Color::from_hex(0x8a94a6);
const BAR: Color = Color::from_hex(0x5fb36a);

struct SoundCheck {
    music: Wav,
    hum: Wav,
    /// The music loop while it plays.
    music_loop: Option<LoopId>,
    music_volume: f32,
    tone_loop: Option<LoopId>,
    /// Which of [`BUSY_STEPS`] is in use.
    busy_step: usize,
    coins: u32,
}

impl SoundCheck {
    fn busy_ms(&self) -> u32 {
        BUSY_STEPS[self.busy_step]
    }
}

impl Game for SoundCheck {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.frame() == 1 {
            self.music_loop = Some(ctx.play_loop(&self.music, self.music_volume));
            ctx.play_loop(&self.hum, HUM_VOLUME);
            self.tone_loop = Some(ctx.play_loop(TONE, TONE_VOLUME));
        }
        if ctx.input.was_pressed(Key::Space) {
            ctx.play(Sound::coin());
            self.coins += 1;
        }

        // Up and Down: the music glides to its new volume.
        let change =
            ctx.input.was_pressed(Key::Up) as i32 - ctx.input.was_pressed(Key::Down) as i32;
        if change != 0 {
            let volume = self.music_volume + change as f32 * VOLUME_STEP;
            self.music_volume = (volume * 10.0).round().clamp(0.0, 10.0) / 10.0;
            if let Some(music) = self.music_loop {
                ctx.set_loop_volume(music, self.music_volume);
            }
        }
        if ctx.input.was_pressed(Key::M) {
            match self.music_loop.take() {
                Some(music) => ctx.stop_loop(music),
                None => self.music_loop = Some(ctx.play_loop(&self.music, self.music_volume)),
            }
        }
        if ctx.input.was_pressed(Key::T) {
            match self.tone_loop.take() {
                Some(tone) => ctx.stop_loop(tone),
                None => self.tone_loop = Some(ctx.play_loop(TONE, TONE_VOLUME)),
            }
        }
        if ctx.input.was_pressed(Key::B) {
            self.busy_step = (self.busy_step + 1) % BUSY_STEPS.len();
        }
        ctx.report("music_volume", self.music_volume);
        ctx.report("busy_ms", self.busy_ms() as f32);
    }

    fn draw(&self, canvas: &mut Canvas) {
        // Keep the computer busy, the way a game drawing lots on the CPU
        // does. (Headless runs only draw when a picture is saved, so this
        // doesn't slow down tests.)
        let busy = Duration::from_millis(u64::from(self.busy_ms()));
        let started = Instant::now();
        while started.elapsed() < busy {
            std::hint::spin_loop();
        }

        canvas.clear(BACKGROUND);
        canvas.draw_text("SOUND CHECK", vec2(8.0, 8.0), 2, TITLE);
        let on_off = |on: bool| if on { "ON" } else { "OFF" };
        let lines = [
            format!("MUSIC: {}", on_off(self.music_loop.is_some())),
            format!("TONE:  {}", on_off(self.tone_loop.is_some())),
            format!("BUSY:  {} MS A FRAME", self.busy_ms()),
            format!("COINS: {}", self.coins),
        ];
        for (row, line) in lines.iter().enumerate() {
            canvas.draw_text(line, vec2(8.0, 32.0 + 12.0 * row as f32), 1, TEXT);
        }
        // The music's volume as a bar.
        canvas.draw_text("VOLUME", vec2(8.0, 86.0), 1, TEXT);
        canvas.fill_rect(Rect::new(64.0, 86.0, 100.0, 7.0), DIM);
        canvas.fill_rect(Rect::new(64.0, 86.0, 100.0 * self.music_volume, 7.0), BAR);

        let help = [
            "SPACE COIN   UP/DOWN MUSIC VOLUME",
            "M MUSIC   T TONE   B BUSY   ESC QUIT",
            "",
            "THE TONE SHOULD BE ONE CLEAN NOTE.",
            "A CLICK OR CRACKLE IS A GLITCH.",
            "",
            "WHEN YOU CLOSE THE WINDOW, THE CONSOLE",
            "SAYS HOW MANY REFILLS WERE LATE.",
        ];
        for (row, line) in help.iter().enumerate() {
            canvas.draw_text(line, vec2(8.0, 108.0 + 12.0 * row as f32), 1, DIM);
        }
    }
}

fn new_game() -> Result<SoundCheck, duckforge::LoadError> {
    let music = std::env::var("SOUND_CHECK_MUSIC").unwrap_or_else(|_| String::from(MUSIC));
    Ok(SoundCheck {
        music: Wav::load(music)?,
        hum: Wav::load(HUM)?,
        music_loop: None,
        music_volume: MUSIC_VOLUME,
        tone_loop: None,
        busy_step: 0,
        coins: 0,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        title: String::from("Sound check"),
        ..Config::default()
    };
    duckforge::run(config, new_game()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn played(script: &str, frames: u64) -> Simulation<SoundCheck> {
        PlayTest::new(Config::default(), new_game().unwrap())
            .script(script)
            .frames(frames)
            .run()
    }

    #[test]
    fn the_music_hum_and_tone_loop_from_the_start() {
        let sim = played(r#"{"events": []}"#, 120);
        let names: Vec<&str> = sim.loops().iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["music", "hum", "tone"]);
        assert!(sim.loops().iter().all(|l| l.is_playing()));
    }

    #[test]
    fn up_and_down_change_the_music_volume() {
        let sim = played(
            r#"{"events": [{"frame": 10, "press": "Up"}, {"frame": 11, "release": "Up"},
                           {"frame": 20, "press": "Up"}, {"frame": 21, "release": "Up"},
                           {"frame": 30, "press": "Down"}, {"frame": 31, "release": "Down"}]}"#,
            40,
        );
        let music = &sim.loops()[0];
        let volumes: Vec<f32> = music.volumes.iter().map(|&(_, v)| v).collect();
        assert_eq!(volumes, [0.5, 0.6, 0.7, 0.6]);
    }

    #[test]
    fn m_stops_and_starts_the_music_and_space_plays_a_coin() {
        let sim = played(
            r#"{"events": [{"frame": 10, "press": "M"}, {"frame": 11, "release": "M"},
                           {"frame": 20, "press": "M"}, {"frame": 21, "release": "M"},
                           {"frame": 30, "press": "Space"}, {"frame": 31, "release": "Space"}]}"#,
            40,
        );
        let music: Vec<_> = sim.loops().iter().filter(|l| l.name == "music").collect();
        assert_eq!(music.len(), 2, "stopped, then started again");
        assert_eq!(music[0].stop, Some(10));
        assert!(music[1].is_playing());
        assert_eq!(sim.sounds().len(), 1);
        assert_eq!(sim.game().coins, 1);
    }
}
