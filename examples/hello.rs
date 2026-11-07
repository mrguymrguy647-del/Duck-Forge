//! The smallest useful duckforge program: a square you can move around.
//!
//! Run it with:   cargo run --example hello
//! Controls:      arrow keys or WASD to move, click to teleport, Escape to quit.

use duckforge::prelude::*;

const SIZE: f32 = 16.0;
const SPEED: f32 = 120.0; // pixels per second

/// All of our game's state lives in this struct.
struct Hello {
    position: Vec2,
}

impl Game for Hello {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }

        // Which way do the held keys point? Each axis is -1.0, 0.0 or 1.0.
        let x = ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D);
        let y = ctx.input.axis(Key::Up, Key::Down) + ctx.input.axis(Key::W, Key::S);
        // Normalizing stops diagonal movement from being faster than straight movement.
        let direction = vec2(x, y).normalized();

        // distance = speed x time
        self.position += direction * SPEED * ctx.dt();

        // Clicking teleports the square so its center is under the mouse.
        // (`mouse_position` is `None` until the mouse has been over the window.)
        if let Some(mouse) = ctx.input.mouse_position() {
            if ctx.input.was_mouse_pressed(MouseButton::Left) {
                self.position = mouse - vec2(SIZE / 2.0, SIZE / 2.0);
            }
        }

        // Keep the square on the screen.
        self.position.x = self.position.x.clamp(0.0, ctx.width() - SIZE);
        self.position.y = self.position.y.clamp(0.0, ctx.height() - SIZE);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::rgb(25, 25, 45));
        canvas.draw_text("HELLO, RUST!", vec2(8.0, 8.0), 2, Color::WHITE);
        canvas.draw_text(
            "MOVE WITH THE ARROW KEYS, OR CLICK",
            vec2(8.0, 24.0),
            1,
            Color::GRAY,
        );

        let square = Rect::new(self.position.x, self.position.y, SIZE, SIZE);
        canvas.fill_rect(square, Color::ORANGE);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        title: String::from("Hello, duckforge"),
        ..Config::default()
    };
    let game = Hello {
        position: vec2(152.0, 112.0),
    };
    duckforge::run(config, game)
}
