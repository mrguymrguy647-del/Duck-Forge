//! The inventory screen (**E**): your duck, a 2 x 2 crafting grid, the
//! backpack and the hotbar, worked with the mouse.
//!
//! - **left click** a slot: pick up its stack, or put down (or swap) the
//!   one you hold; on the same item, as much as fits goes on
//! - **right click**: pick up half, or put down one
//! - **Shift + click**: send the stack across, between the hotbar and the
//!   backpack, or from the crafting grid into the bag
//! - put things in the grid that match a recipe and the result shows on
//!   the right; click it to make it (Shift: as many as you can)
//! - the **book** button lists the recipes; click one to fill the grid
//!   from your bag
//! - **E** or **Escape** closes it; what's in the grid and in your hand
//!   goes back in the bag
//!
//! The window lets go of the mouse pointer while it's open.

use super::*;

/// Some of these, one in each grid slot (in any order), make that.
pub struct Recipe {
    pub needs: &'static [Item],
    pub makes: (Item, u32),
}

const fn block(b: Block) -> Item {
    Item::Block(b)
}

pub const RECIPES: [Recipe; 7] = [
    Recipe {
        needs: &[block(Block::Log)],
        makes: (block(Block::Planks), 4),
    },
    Recipe {
        needs: &[Item::Seeds, Item::Seeds, Item::Seeds],
        makes: (Item::Bread, 1),
    },
    Recipe {
        needs: &[block(Block::Sand), block(Block::Sand), Item::Coal],
        makes: (block(Block::Glass), 2),
    },
    Recipe {
        needs: &[block(Block::Dirt), block(Block::Dirt), Item::Coal],
        makes: (block(Block::Brick), 2),
    },
    Recipe {
        needs: &[
            block(Block::Cobblestone),
            block(Block::Cobblestone),
            Item::Coal,
        ],
        makes: (block(Block::Stone), 2),
    },
    Recipe {
        needs: &[Item::Gold, block(Block::Glass)],
        makes: (block(Block::Lamp), 2),
    },
    Recipe {
        needs: &[
            block(Block::Leaves),
            block(Block::Leaves),
            block(Block::Leaves),
            block(Block::Leaves),
        ],
        makes: (block(Block::Dirt), 1),
    },
];

/// The recipe the grid's contents make, if any: the same items, one a
/// slot, in any order.
pub fn matching_recipe(grid: &[Option<Stack>; 4]) -> Option<&'static Recipe> {
    let mut have: Vec<Item> = grid.iter().flatten().map(|s| s.item).collect();
    if have.is_empty() {
        return None;
    }
    have.sort_by_key(|item| item.id());
    RECIPES.iter().find(|recipe| {
        let mut needs = recipe.needs.to_vec();
        needs.sort_by_key(|item| item.id());
        needs == have
    })
}

/// The inventory screen while it's open.
#[derive(Debug, Clone, Default)]
pub struct Screen {
    /// The crafting grid: top left, top right, bottom left, bottom right.
    pub grid: [Option<Stack>; 4],
    /// The stack on the mouse pointer.
    pub held: Option<Stack>,
    pub book_open: bool,
}

/// A place on the screen that holds a stack, or does something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    /// One of the 36 inventory slots.
    Slot(usize),
    Grid(usize),
    Result,
    Book,
    /// A recipe in the book.
    Recipe(usize),
}

// The layout, in canvas pixels. The panel is 176 x 166, in the middle of
// the screen (moved right when the recipe book is open beside it).
const PANEL_W: f32 = 176.0;
const PANEL_H: f32 = 166.0;
const BOOK_W: f32 = 112.0;
const SLOT: f32 = 18.0;
const ROW_H: f32 = 20.0;

pub fn panel(book_open: bool) -> Rect {
    let width = if book_open {
        PANEL_W + BOOK_W + 4.0
    } else {
        PANEL_W
    };
    let left = ((WIDTH - width) / 2.0).floor() + if book_open { BOOK_W + 4.0 } else { 0.0 };
    Rect::new(left, ((HEIGHT - PANEL_H) / 2.0).floor(), PANEL_W, PANEL_H)
}

fn book_panel(book_open: bool) -> Rect {
    let p = panel(book_open);
    Rect::new(p.x - BOOK_W - 4.0, p.y, BOOK_W, PANEL_H)
}

/// Where each spot is (its 18 x 18 box, or the button's).
pub fn spot_rect(spot: Spot, book_open: bool) -> Rect {
    let p = panel(book_open);
    let at = |x: f32, y: f32| Rect::new(p.x + x, p.y + y, SLOT, SLOT);
    match spot {
        Spot::Slot(i) if i < HOTBAR_SLOTS => at(7.0 + i as f32 * SLOT, 141.0),
        Spot::Slot(i) => {
            let i = i - HOTBAR_SLOTS;
            at(7.0 + (i % 9) as f32 * SLOT, 83.0 + (i / 9) as f32 * SLOT)
        }
        Spot::Grid(i) => at(87.0 + (i % 2) as f32 * SLOT, 17.0 + (i / 2) as f32 * SLOT),
        Spot::Result => at(143.0, 26.0),
        Spot::Book => Rect::new(p.x + 87.0, p.y + 58.0, 20.0, 18.0),
        Spot::Recipe(i) => {
            let b = book_panel(book_open);
            Rect::new(
                b.x + 4.0,
                b.y + 16.0 + i as f32 * ROW_H,
                b.w - 8.0,
                ROW_H - 2.0,
            )
        }
    }
}

/// The duck's picture box.
fn preview_rect(book_open: bool) -> Rect {
    let p = panel(book_open);
    Rect::new(p.x + 7.0, p.y + 7.0, 72.0, 70.0)
}

/// Every spot, for finding the one under the pointer.
fn spots(book_open: bool) -> impl Iterator<Item = Spot> {
    let book = if book_open { RECIPES.len() } else { 0 };
    (0..SLOTS)
        .map(Spot::Slot)
        .chain((0..4).map(Spot::Grid))
        .chain([Spot::Result, Spot::Book])
        .chain((0..book).map(Spot::Recipe))
}

pub fn spot_at(point: Vec2, book_open: bool) -> Option<Spot> {
    spots(book_open).find(|&spot| spot_rect(spot, book_open).contains(point))
}

impl DuckCraft {
    /// Opens and closes the screen (E), and works it while it's open.
    pub fn inventory_screen(&mut self, ctx: &mut Context) {
        let input = &ctx.input;
        if self.screen.is_none() {
            if input.was_pressed(Key::E) && !self.vitals.fainted {
                self.screen = Some(Screen::default());
                self.digging = None;
                self.show_help = false;
                ctx.lock_pointer(false);
                ctx.play(click(700.0));
            }
            return;
        }
        if input.was_pressed(Key::E) || input.was_pressed(Key::Escape) {
            self.close_inventory();
            ctx.lock_pointer(true);
            ctx.play(click(500.0));
            return;
        }
        let shift = input.is_down(Key::LeftShift);
        let left = input.was_mouse_pressed(MouseButton::Left);
        let right = input.was_mouse_pressed(MouseButton::Right);
        // Number keys still pick the hotbar slot, as outside.
        for (i, key) in [
            Key::Digit1,
            Key::Digit2,
            Key::Digit3,
            Key::Digit4,
            Key::Digit5,
            Key::Digit6,
            Key::Digit7,
            Key::Digit8,
            Key::Digit9,
        ]
        .into_iter()
        .enumerate()
        {
            if input.was_pressed(key) {
                self.slot = i;
            }
        }
        if !(left || right) {
            return;
        }
        let Some(point) = input.mouse_position() else {
            return;
        };
        let book_open = self.screen.as_ref().is_some_and(|s| s.book_open);
        let Some(spot) = spot_at(point, book_open) else {
            return;
        };
        let sound = self.click_spot(spot, left, shift);
        if let Some(freq) = sound {
            ctx.play(click(freq));
        }
    }

    /// What a click on `spot` does (`left`, else right; `shift` held).
    /// Returns the pitch of a click to play, if anything happened.
    pub fn click_spot(&mut self, spot: Spot, left: bool, shift: bool) -> Option<f32> {
        let mut screen = self.screen.take()?;
        let sound = match spot {
            Spot::Book => {
                screen.book_open = !screen.book_open;
                Some(800.0)
            }
            Spot::Recipe(i) => {
                self.fill_grid(&mut screen, &RECIPES[i]);
                Some(900.0)
            }
            Spot::Result => self.take_result(&mut screen, shift),
            Spot::Slot(i) if shift && left => {
                // Across: hotbar to backpack, backpack to hotbar.
                if let Some(stack) = self.inventory.slots[i].take() {
                    let range = if i < HOTBAR_SLOTS {
                        HOTBAR_SLOTS..SLOTS
                    } else {
                        0..HOTBAR_SLOTS
                    };
                    let left_over = add_into(&mut self.inventory.slots[range], stack);
                    self.inventory.slots[i] = left_over;
                    Some(650.0)
                } else {
                    None
                }
            }
            Spot::Grid(i) if shift && left => {
                if let Some(stack) = screen.grid[i].take() {
                    let left_over = self.inventory.add(stack.item, stack.count);
                    if left_over > 0 {
                        screen.grid[i] = Some(Stack::new(stack.item, left_over));
                    }
                    Some(650.0)
                } else {
                    None
                }
            }
            Spot::Slot(i) => click_slot(&mut self.inventory.slots[i], &mut screen.held, left),
            Spot::Grid(i) => click_slot(&mut screen.grid[i], &mut screen.held, left),
        };
        self.screen = Some(screen);
        sound
    }

    /// Takes what the grid makes: onto the held stack, or (Shift) as many
    /// as can be made, into the bag.
    fn take_result(&mut self, screen: &mut Screen, shift: bool) -> Option<f32> {
        let recipe = matching_recipe(&screen.grid)?;
        let (item, amount) = recipe.makes;
        let mut made = 0;
        loop {
            if matching_recipe(&screen.grid).is_none_or(|r| !std::ptr::eq(r, recipe)) {
                break;
            }
            if shift {
                if !room_for(&self.inventory, item, amount) {
                    break;
                }
                self.inventory.add(item, amount);
            } else {
                match &mut screen.held {
                    None => screen.held = Some(Stack::new(item, amount)),
                    Some(held) if held.item == item && held.count + amount <= items::STACK => {
                        held.count += amount;
                    }
                    Some(_) => break,
                }
            }
            for slot in screen.grid.iter_mut() {
                if let Some(stack) = slot {
                    stack.count -= 1;
                    if stack.count == 0 {
                        *slot = None;
                    }
                }
            }
            made += 1;
            self.crafted += amount;
            if !shift {
                break;
            }
        }
        if made > 0 {
            self.say(format!("Made {} {}", made * amount, item.name()));
            Some(1100.0)
        } else {
            None
        }
    }

    /// The recipe book: empties the grid into the bag, then puts in one of
    /// each thing the recipe needs, if the bag has them.
    fn fill_grid(&mut self, screen: &mut Screen, recipe: &Recipe) {
        for slot in screen.grid.iter_mut() {
            if let Some(stack) = slot.take() {
                self.inventory.add(stack.item, stack.count);
            }
        }
        let mut missing = None;
        for &item in recipe.needs {
            let need = recipe.needs.iter().filter(|&&i| i == item).count() as u32;
            if self.inventory.count(item) < need {
                missing = Some(item);
            }
        }
        if let Some(item) = missing {
            self.say(format!("Not enough {}", item.name()));
            return;
        }
        for (i, &item) in recipe.needs.iter().enumerate() {
            self.inventory.take(item, 1);
            screen.grid[i] = Some(Stack::new(item, 1));
        }
    }

    /// Closes the screen, putting the grid and the held stack back in the
    /// bag.
    pub fn close_inventory(&mut self) {
        let Some(screen) = self.screen.take() else {
            return;
        };
        let mut lost = 0;
        for stack in screen.grid.into_iter().chain([screen.held]).flatten() {
            lost += self.inventory.add(stack.item, stack.count);
        }
        if lost > 0 {
            self.say(format!("Your bag was full: {lost} things were lost"));
        }
    }

    pub fn draw_inventory(&self, canvas: &mut Canvas, camera: &Camera3D, mouse: Option<Vec2>) {
        let Some(screen) = &self.screen else {
            return;
        };
        let book = screen.book_open;
        canvas.fill_rect(
            Rect::new(0.0, 0.0, WIDTH, HEIGHT),
            Color::rgba(10, 10, 16, 140),
        );
        let p = panel(book);
        raised(canvas, p);
        canvas.label("inventory", p);

        // The duck, on dark, looking at the pointer.
        let preview = preview_rect(book);
        canvas.fill_rect(preview, Color::rgb(28, 28, 34));
        sunken(canvas, preview);

        // Crafting.
        canvas.draw_text(
            "Crafting",
            vec2(p.x + 87.0, p.y + 6.0),
            1,
            Color::rgb(60, 60, 66),
        );
        for i in 0..4 {
            self.draw_slot(canvas, spot_rect(Spot::Grid(i), book), screen.grid[i]);
        }
        let arrow = vec2(p.x + 126.0, p.y + 32.0);
        canvas.fill_rect(
            Rect::new(arrow.x, arrow.y + 2.0, 9.0, 3.0),
            Color::rgb(90, 90, 96),
        );
        canvas.fill_triangle(
            arrow + vec2(9.0, -2.0),
            arrow + vec2(14.0, 3.5),
            arrow + vec2(9.0, 9.0),
            Color::rgb(90, 90, 96),
        );
        let result = matching_recipe(&screen.grid).map(|r| Stack::new(r.makes.0, r.makes.1));
        let result_rect = spot_rect(Spot::Result, book);
        let big = Rect::new(result_rect.x - 4.0, result_rect.y - 4.0, 26.0, 26.0);
        canvas.fill_rect(big, Color::rgb(139, 139, 139));
        sunken(canvas, big);
        self.draw_stack(
            canvas,
            result,
            vec2(result_rect.x + 1.0, result_rect.y + 1.0),
        );

        // The recipe book button: a little green book.
        let b = spot_rect(Spot::Book, book);
        raised(canvas, b);
        let cover = Rect::new(b.x + 5.0, b.y + 3.0, 10.0, 12.0);
        canvas.fill_rect(cover, Color::from_hex(0x3c8a3c));
        canvas.fill_rect(
            Rect::new(cover.x + 1.0, cover.y + 1.0, 1.0, 10.0),
            Color::from_hex(0x245a24),
        );
        canvas.fill_rect(
            Rect::new(cover.x + 4.0, cover.y + 4.0, 4.0, 1.0),
            Color::from_hex(0xd8f0c0),
        );
        canvas.fill_rect(
            Rect::new(cover.x + 4.0, cover.y + 6.0, 4.0, 1.0),
            Color::from_hex(0xd8f0c0),
        );

        // The bag and the hotbar.
        for i in 0..SLOTS {
            self.draw_slot(
                canvas,
                spot_rect(Spot::Slot(i), book),
                self.inventory.slots[i],
            );
        }
        let hand = spot_rect(Spot::Slot(self.slot), book);
        canvas.draw_rect(
            Rect::new(hand.x - 1.0, hand.y - 1.0, hand.w + 2.0, hand.h + 2.0),
            Color::WHITE,
        );

        if book {
            self.draw_book(canvas);
        }

        // The pointer: what's under it lights up; the held stack follows it.
        if let Some(point) = mouse {
            if let Some(spot) = spot_at(point, book) {
                if matches!(spot, Spot::Slot(_) | Spot::Grid(_) | Spot::Result) {
                    let r = spot_rect(spot, book);
                    canvas.fill_rect(
                        Rect::new(r.x + 1.0, r.y + 1.0, 16.0, 16.0),
                        Color::rgba(255, 255, 255, 80),
                    );
                }
                let under = match spot {
                    Spot::Slot(i) => self.inventory.slots[i],
                    Spot::Grid(i) => screen.grid[i],
                    Spot::Result => result,
                    _ => None,
                };
                if let (Some(stack), None) = (under, screen.held) {
                    tooltip(canvas, point, stack.item.name());
                }
            }
        }
        self.draw_duck_preview(canvas, camera, mouse);
        if let (Some(point), Some(held)) = (mouse, screen.held) {
            self.draw_stack(canvas, Some(held), point - vec2(8.0, 8.0));
        }
        if let (Some(p), true) = (mouse, self.draw_pointer) {
            // A little arrow, outlined.
            canvas.fill_triangle(
                p + vec2(-1.0, -1.0),
                p + vec2(-1.0, 12.0),
                p + vec2(9.0, 8.0),
                Color::BLACK,
            );
            canvas.fill_triangle(p, p + vec2(0.0, 10.0), p + vec2(7.0, 7.0), Color::WHITE);
        }
    }

    fn draw_slot(&self, canvas: &mut Canvas, rect: Rect, stack: Option<Stack>) {
        canvas.fill_rect(rect, Color::rgb(139, 139, 139));
        sunken(canvas, rect);
        self.draw_stack(canvas, stack, vec2(rect.x + 1.0, rect.y + 1.0));
    }

    fn draw_book(&self, canvas: &mut Canvas) {
        let b = book_panel(true);
        raised(canvas, b);
        canvas.draw_text(
            "Recipes",
            vec2(b.x + 6.0, b.y + 5.0),
            1,
            Color::rgb(60, 60, 66),
        );
        for (i, recipe) in RECIPES.iter().enumerate() {
            let row = spot_rect(Spot::Recipe(i), true);
            let ready = recipe.needs.iter().all(|&item| {
                self.inventory.count(item)
                    >= recipe.needs.iter().filter(|&&n| n == item).count() as u32
            });
            canvas.fill_rect(
                row,
                if ready {
                    Color::rgb(150, 160, 150)
                } else {
                    Color::rgb(120, 112, 112)
                },
            );
            sunken(canvas, row);
            // The ingredients, then the result.
            let mut x = row.x + 2.0;
            let mut kinds: Vec<(Item, u32)> = Vec::new();
            for &item in recipe.needs {
                match kinds.iter_mut().find(|(k, _)| *k == item) {
                    Some((_, n)) => *n += 1,
                    None => kinds.push((item, 1)),
                }
            }
            for (item, n) in kinds {
                self.draw_stack(canvas, Some(Stack::new(item, n)), vec2(x, row.y + 1.0));
                x += 18.0;
            }
            canvas.draw_text(
                ">",
                vec2(row.x + row.w - 27.0, row.y + 6.0),
                1,
                Color::rgb(50, 50, 56),
            );
            let (item, n) = recipe.makes;
            self.draw_stack(
                canvas,
                Some(Stack::new(item, n)),
                vec2(row.x + row.w - 19.0, row.y + 1.0),
            );
        }
    }

    /// The duck in its box, turning to look at the pointer.
    fn draw_duck_preview(&self, canvas: &mut Canvas, camera: &Camera3D, mouse: Option<Vec2>) {
        let book = self.screen.as_ref().is_some_and(|s| s.book_open);
        let r = preview_rect(book);
        // A screen point, at `depth` ahead in view space (x right, y up).
        let depth = 5.0;
        let per_pixel = 2.0 * depth * (camera.fov / 2.0).tan() / HEIGHT;
        let to_view = |sx: f32, sy: f32| {
            vec3(
                (sx - WIDTH / 2.0) * per_pixel,
                (HEIGHT / 2.0 - sy) * per_pixel,
                depth,
            )
        };
        let feet = to_view(r.x + r.w / 2.0, r.y + r.h - 6.0);
        let eyes = vec2(r.x + r.w / 2.0, r.y + 18.0);
        let look = mouse.map_or(Vec2::ZERO, |m| m - eyes);
        let pose = Pose {
            // Facing out of the screen, turned a little towards the pointer.
            yaw: std::f32::consts::PI - (look.x / 60.0).clamp(-1.0, 1.0) * 0.7,
            pitch: (look.y / 60.0).clamp(-1.0, 1.0) * 0.5,
            swing: 0.0,
            lift: 0.0,
        };
        canvas.set_lighting(Lighting {
            sun_direction: vec3(0.4, 0.8, -0.6).normalized(),
            sun_color: Color::WHITE,
            sun_strength: 0.7,
            ambient: 0.55,
        });
        for (mesh, place) in self.art.duck.parts(feet, pose) {
            canvas.draw_mesh_in_view(mesh, &place, camera);
        }
    }
}

/// Puts `stack` into the first slots of `slots` it fits (same item with
/// room first, then empty). Returns what didn't fit.
fn add_into(slots: &mut [Option<Stack>], mut stack: Stack) -> Option<Stack> {
    for slot in slots.iter_mut().flatten() {
        if slot.item == stack.item && slot.count < items::STACK {
            let moved = stack.count.min(items::STACK - slot.count);
            slot.count += moved;
            stack.count -= moved;
        }
    }
    for slot in slots.iter_mut() {
        if stack.count == 0 {
            break;
        }
        if slot.is_none() {
            *slot = Some(stack);
            stack.count = 0;
        }
    }
    (stack.count > 0).then_some(stack)
}

/// Whether `count` more of `item` would fit in the bag.
fn room_for(inventory: &Inventory, item: Item, count: u32) -> bool {
    let room: u32 = inventory
        .slots
        .iter()
        .map(|slot| match slot {
            None => items::STACK,
            Some(stack) if stack.item == item => items::STACK - stack.count,
            Some(_) => 0,
        })
        .sum();
    room >= count
}

/// A click on one slot with (or without) a stack on the pointer.
fn click_slot(slot: &mut Option<Stack>, held: &mut Option<Stack>, left: bool) -> Option<f32> {
    match (slot.as_mut(), held.as_mut(), left) {
        (None, None, _) => None,
        // Pick up all, or half.
        (Some(stack), None, true) => {
            *held = Some(*stack);
            *slot = None;
            Some(900.0)
        }
        (Some(stack), None, false) => {
            let half = stack.count.div_ceil(2);
            *held = Some(Stack::new(stack.item, half));
            stack.count -= half;
            if stack.count == 0 {
                *slot = None;
            }
            Some(900.0)
        }
        // Put down all, or one.
        (None, Some(h), true) => {
            *slot = Some(*h);
            *held = None;
            Some(700.0)
        }
        (None, Some(h), false) => {
            *slot = Some(Stack::new(h.item, 1));
            h.count -= 1;
            if h.count == 0 {
                *held = None;
            }
            Some(700.0)
        }
        // The same thing: as much as fits (or one).
        (Some(stack), Some(h), _) if stack.item == h.item => {
            let room = items::STACK - stack.count;
            let moved = if left { h.count.min(room) } else { 1.min(room) };
            if moved == 0 {
                return None;
            }
            stack.count += moved;
            h.count -= moved;
            if h.count == 0 {
                *held = None;
            }
            Some(700.0)
        }
        // Different things: swap (left click).
        (Some(stack), Some(h), true) => {
            std::mem::swap(stack, h);
            Some(800.0)
        }
        (Some(_), Some(_), false) => None,
    }
}

/// A raised grey panel or button, lit from the top left.
fn raised(canvas: &mut Canvas, r: Rect) {
    canvas.fill_rect(r, Color::rgb(198, 198, 198));
    canvas.fill_rect(Rect::new(r.x, r.y, r.w - 1.0, 2.0), Color::WHITE);
    canvas.fill_rect(Rect::new(r.x, r.y, 2.0, r.h - 1.0), Color::WHITE);
    canvas.fill_rect(
        Rect::new(r.x + 1.0, r.y + r.h - 2.0, r.w - 1.0, 2.0),
        Color::rgb(85, 85, 85),
    );
    canvas.fill_rect(
        Rect::new(r.x + r.w - 2.0, r.y + 1.0, 2.0, r.h - 1.0),
        Color::rgb(85, 85, 85),
    );
    canvas.draw_rect(r, Color::rgb(20, 20, 20));
}

/// A slot's sunken edges: dark top left, light bottom right.
fn sunken(canvas: &mut Canvas, r: Rect) {
    canvas.fill_rect(Rect::new(r.x, r.y, r.w, 1.0), Color::rgb(55, 55, 55));
    canvas.fill_rect(Rect::new(r.x, r.y, 1.0, r.h), Color::rgb(55, 55, 55));
    canvas.fill_rect(Rect::new(r.x, r.y + r.h - 1.0, r.w, 1.0), Color::WHITE);
    canvas.fill_rect(Rect::new(r.x + r.w - 1.0, r.y, 1.0, r.h), Color::WHITE);
}

/// A thing's name in a dark box by the pointer.
fn tooltip(canvas: &mut Canvas, point: Vec2, text: &str) {
    let w = Canvas::text_width(text, 1);
    let x = (point.x + 8.0).min(WIDTH - w - 6.0);
    let y = (point.y - 14.0).max(2.0);
    canvas.fill_rect(
        Rect::new(x - 3.0, y - 3.0, w + 6.0, 13.0),
        Color::rgba(16, 4, 22, 235),
    );
    canvas.draw_rect(
        Rect::new(x - 3.0, y - 3.0, w + 6.0, 13.0),
        Color::rgb(60, 20, 120),
    );
    canvas.draw_text(text, vec2(x, y), 1, Color::WHITE);
}

pub fn click(freq: f32) -> Sound {
    Sound {
        name: "click",
        wave: Wave::Triangle,
        freq,
        ms: 35.0,
        volume: 0.12,
        ..Default::default()
    }
}
