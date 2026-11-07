//! Crafting: turn what you dig into things to build with.
//!
//! Press **E** for the menu. **W/S** (or **A/D**), the arrow keys, the mouse
//! wheel or **1 to 5** pick a recipe; **Space**, **Enter** or a click makes it once, with **Shift**
//! as many times as you can; **E** or **Escape** closes it. The duck stands
//! still while it's open.

use super::*;

/// Something you carry: a block, or the gold and coal dug from ore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Block(Block),
    Gold,
    Coal,
}

impl Item {
    pub fn name(self) -> &'static str {
        match self {
            Item::Block(block) => block.name(),
            Item::Gold => "Gold",
            Item::Coal => "Coal",
        }
    }

    /// The picture for it in the menu: ore for gold and coal.
    pub fn icon_block(self) -> Block {
        match self {
            Item::Block(block) => block,
            Item::Gold => Block::GoldOre,
            Item::Coal => Block::CoalOre,
        }
    }
}

/// Some of these makes that many of that.
pub struct Recipe {
    pub needs: &'static [(Item, u32)],
    pub makes: (Block, u32),
    /// What it is, in the menu.
    pub note: &'static str,
}

pub const RECIPES: [Recipe; 5] = [
    Recipe {
        needs: &[(Item::Block(Block::Log), 1)],
        makes: (Block::Planks, 4),
        note: "saw a log",
    },
    Recipe {
        needs: &[(Item::Block(Block::Sand), 2), (Item::Coal, 1)],
        makes: (Block::Glass, 2),
        note: "melt sand",
    },
    Recipe {
        needs: &[(Item::Block(Block::Dirt), 2), (Item::Coal, 1)],
        makes: (Block::Brick, 2),
        note: "bake mud",
    },
    Recipe {
        needs: &[(Item::Gold, 1), (Item::Block(Block::Glass), 1)],
        makes: (Block::Lamp, 2),
        note: "a gold lantern",
    },
    Recipe {
        needs: &[(Item::Block(Block::Leaves), 4)],
        makes: (Block::Dirt, 1),
        note: "compost",
    },
];

impl DuckCraft {
    /// How many of `item` you carry.
    pub fn count(&self, item: Item) -> u32 {
        match item {
            Item::Block(block) => self.carried[block as usize],
            Item::Gold => self.gold,
            Item::Coal => self.coal,
        }
    }

    fn take(&mut self, item: Item, amount: u32) {
        match item {
            Item::Block(block) => self.carried[block as usize] -= amount,
            Item::Gold => self.gold -= amount,
            Item::Coal => self.coal -= amount,
        }
    }

    /// How many times you could make `recipe` now.
    pub fn can_make(&self, recipe: &Recipe) -> u32 {
        recipe
            .needs
            .iter()
            .map(|&(item, amount)| self.count(item) / amount)
            .min()
            .unwrap_or(0)
    }

    /// Makes `recipe` up to `times` times; returns how many it made.
    pub fn craft(&mut self, recipe: &Recipe, times: u32) -> u32 {
        let times = times.min(self.can_make(recipe));
        for &(item, amount) in recipe.needs {
            self.take(item, amount * times);
        }
        let (block, amount) = recipe.makes;
        self.carried[block as usize] += amount * times;
        self.crafted += amount * times;
        times
    }

    /// Opens and closes the menu (E), and works it while it's open.
    pub fn crafting(&mut self, ctx: &mut Context) {
        let input = &ctx.input;
        let Some(mut row) = self.menu else {
            if input.was_pressed(Key::E) {
                self.menu = Some(0);
                self.digging = None;
                self.show_help = false;
                ctx.play(click(700.0));
            }
            return;
        };
        if input.was_pressed(Key::E) || input.was_pressed(Key::Escape) {
            self.menu = None;
            ctx.play(click(500.0));
            return;
        }
        let rows = RECIPES.len();
        let pressed = |keys: &[Key]| keys.iter().any(|&k| input.was_pressed(k));
        let up = pressed(&[Key::W, Key::A, Key::Up, Key::Left]) || input.scroll() > 0.0;
        let down = pressed(&[Key::S, Key::D, Key::Down, Key::Right]) || input.scroll() < 0.0;
        let digits = [
            Key::Digit1,
            Key::Digit2,
            Key::Digit3,
            Key::Digit4,
            Key::Digit5,
        ];
        let jump_to = digits.iter().take(rows).position(|&k| input.was_pressed(k));
        if up {
            row = (row + rows - 1) % rows;
        }
        if down {
            row = (row + 1) % rows;
        }
        if let Some(chosen) = jump_to {
            row = chosen;
        }
        let make = input.was_pressed(Key::Space)
            || input.was_pressed(Key::Enter)
            || input.was_mouse_pressed(MouseButton::Left);
        let all = input.is_down(Key::LeftShift);
        self.menu = Some(row);
        if up || down || jump_to.is_some() {
            ctx.play(click(900.0));
        }
        if make {
            let recipe = &RECIPES[row];
            let made = self.craft(recipe, if all { u32::MAX } else { 1 });
            let (block, amount) = recipe.makes;
            if made > 0 {
                self.say(format!("Made {} {}", amount * made, block.name()));
                ctx.play(Sound {
                    name: "craft",
                    wave: Wave::Square,
                    freq: 440.0,
                    slide_to: 880.0,
                    ms: 110.0,
                    volume: 0.18,
                    decay: 80.0,
                    ..Default::default()
                });
            } else {
                let missing = recipe
                    .needs
                    .iter()
                    .find(|&&(item, amount)| self.count(item) < amount)
                    .map_or("", |&(item, _)| item.name());
                self.say(format!("Not enough {missing}"));
                ctx.play(click(180.0));
            }
        }
    }

    /// The menu, in the middle of the screen.
    pub fn draw_crafting(&self, canvas: &mut Canvas) {
        let Some(selected) = self.menu else {
            return;
        };
        let row_height = 22.0;
        let panel = Rect::new(
            40.0,
            22.0,
            WIDTH - 80.0,
            30.0 + row_height * RECIPES.len() as f32,
        );
        canvas.fill_rect(panel, Color::rgba(20, 18, 28, 220));
        canvas.draw_rect(panel, Color::from_hex(0xffd45a));
        canvas.draw_text_centered("CRAFTING", panel.y + 6.0, 1, Color::from_hex(0xffd45a));
        canvas.label("crafting", panel);
        for (i, recipe) in RECIPES.iter().enumerate() {
            let y = panel.y + 20.0 + i as f32 * row_height;
            let row = Rect::new(panel.x + 4.0, y, panel.w - 8.0, row_height - 2.0);
            let ready = self.can_make(recipe) > 0;
            if i == selected {
                canvas.fill_rect(row, Color::rgba(255, 212, 90, 60));
                canvas.draw_rect(row, Color::from_hex(0xffd45a));
            }
            let text_color = if ready {
                Color::WHITE
            } else {
                Color::rgb(120, 120, 130)
            };
            let opacity = if ready { 1.0 } else { 0.35 };
            let mut x = row.x + 4.0;
            let item =
                |canvas: &mut Canvas, x: &mut f32, icon: Block, amount: u32, have: Option<u32>| {
                    if let Some(picture) = &self.art.icons[icon as usize] {
                        canvas.blit_sprite(
                            picture,
                            vec2(*x, y + 2.0),
                            Blit {
                                opacity,
                                ..Default::default()
                            },
                        );
                    }
                    let label = format!("{amount}");
                    let enough = have.is_none_or(|have| have >= amount);
                    let color = if enough {
                        text_color
                    } else {
                        Color::from_hex(0xff7a6a)
                    };
                    canvas.draw_text(&label, vec2(*x + 18.0, y + 6.0), 1, color);
                    *x += 22.0 + Canvas::text_width(&label, 1);
                };
            for (n, &(need, amount)) in recipe.needs.iter().enumerate() {
                if n > 0 {
                    canvas.draw_text("+", vec2(x, y + 6.0), 1, text_color);
                    x += 9.0;
                }
                item(
                    canvas,
                    &mut x,
                    need.icon_block(),
                    amount,
                    Some(self.count(need)),
                );
            }
            canvas.draw_text(">", vec2(x + 1.0, y + 6.0), 1, text_color);
            x += 10.0;
            let (block, amount) = recipe.makes;
            item(canvas, &mut x, block, amount, None);
            let note_x = row.x + row.w - 4.0 - Canvas::text_width(recipe.note, 1);
            if note_x > x {
                canvas.draw_text(
                    recipe.note,
                    vec2(note_x, y + 6.0),
                    1,
                    Color::rgb(170, 170, 185),
                );
            }
        }
        let hint = "W/S or 1-5 pick  Space make  Shift: all  E close";
        canvas.draw_text_centered(hint, panel.y + panel.h + 3.0, 1, Color::WHITE);
    }
}

fn click(freq: f32) -> Sound {
    Sound {
        name: "click",
        wave: Wave::Triangle,
        freq,
        ms: 35.0,
        volume: 0.12,
        ..Default::default()
    }
}
