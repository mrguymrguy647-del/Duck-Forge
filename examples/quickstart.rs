use duckforge::prelude::*;

/// Catch the falling star with the paddle.
struct Catch {
    paddle: Rect,
    star: Vec2,
    score: u32,
}

impl Game for Catch {
    fn update(&mut self, ctx: &mut Context) {
        let speed = ctx.input.axis(Key::Left, Key::Right) * 150.0;
        self.paddle.x = (self.paddle.x + speed * ctx.dt()).clamp(0.0, 288.0);
        self.star.y += 90.0 * ctx.dt();
        let caught = circle_vs_rect(self.star, 4.0, self.paddle);
        if caught {
            self.score += 1;
            ctx.play(Sound::coin());
        }
        if caught || self.star.y > 240.0 {
            self.star = vec2(ctx.rng.range(10.0, 310.0), 0.0);
        }
        ctx.report("score", self.score);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::BLACK);
        canvas.fill_rect(self.paddle, Color::WHITE);
        canvas.fill_circle(self.star, 4.0, Color::YELLOW);
        let score = format!("SCORE {}", self.score);
        canvas.draw_text(&score, vec2(4.0, 4.0), 1, Color::WHITE);
    }
}

fn new_game() -> Catch {
    let paddle = Rect::new(144.0, 220.0, 32.0, 6.0);
    let star = vec2(160.0, 0.0);
    Catch {
        paddle,
        star,
        score: 0,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(Config::default(), new_game())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holding_right_moves_the_paddle_right() {
        let played = PlayTest::new(Config::default(), new_game())
            .script(r#"{"events": [{"frame": 1, "press": "Right"}]}"#)
            .frames(60)
            .run();
        assert_eq!(played.game().paddle.x, 288.0); // all the way right
    }
}
