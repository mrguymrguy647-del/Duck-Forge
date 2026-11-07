//! Saving and loading: the island as you left it.
//!
//! The windowed game saves to `duck_craft_save.dat` in the folder you run it
//! from: when you quit with Escape, every 30 seconds, and when you press
//! **F5**. Next time it carries on from there. To grow a new island,
//! delete the file. Tests, replays and fuzz runs never read or write it.
//!
//! The file: a short header, then every block (packed as runs of the same
//! block, so it's small: about 30 KB), what you carry, where you are, the
//! time of day, and the ducklings.

use super::*;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// The file's first bytes. Version 2 keeps stacks in slots; version 1 (a
/// count of each block, gold and coal) still loads.
const MAGIC: &[u8; 10] = b"DUCKCRAFT2";
const MAGIC_V1: &[u8; 10] = b"DUCKCRAFT1";
pub const SAVE_FILE: &str = "duck_craft_save.dat";
/// Seconds between saves while you play.
pub const AUTOSAVE_SECONDS: f32 = 30.0;

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.0.len() < n {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the save file ends too soon",
            ));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn vec3(&mut self) -> io::Result<Vec3> {
        Ok(vec3(self.f32()?, self.f32()?, self.f32()?))
    }
}

fn bad(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}

impl DuckCraft {
    /// The game as bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer(MAGIC.to_vec());
        // Blocks, as runs: (how many, which).
        let blocks = self.world.grid.blocks();
        let mut i = 0;
        while i < blocks.len() {
            let block = blocks[i];
            let mut run = 1;
            while i + run < blocks.len() && blocks[i + run] == block && run < u32::MAX as usize {
                run += 1;
            }
            w.u32(run as u32);
            w.u8(block);
            i += run;
        }
        for slot in &self.inventory.slots {
            match slot {
                Some(stack) => {
                    w.u8(stack.item.id());
                    w.u32(stack.count);
                }
                None => {
                    w.u8(0);
                    w.u32(0);
                }
            }
        }
        for v in [self.dug, self.placed, self.crafted, self.imprints] {
            w.u32(v);
        }
        w.u32(self.vitals.health);
        w.u32(self.vitals.food);
        w.f32(self.vitals.air);
        let mut plants: Vec<(&[i32; 3], &Plant)> = self.world.plants.iter().collect();
        plants.sort();
        w.u32(plants.len() as u32);
        for (&[x, y, z], &plant) in plants {
            w.u8(x as u8);
            w.u8(y as u8);
            w.u8(z as u8);
            w.u8(plant.id());
        }
        w.vec3(self.position);
        w.f32(self.yaw);
        w.f32(self.pitch);
        w.f32(self.time);
        w.u8(self.slot as u8);
        w.u8(self.behind_view as u8);
        w.u8(self.ducklings.len() as u8);
        for duckling in &self.ducklings {
            w.vec3(duckling.position);
            w.f32(duckling.yaw);
            w.u32(duckling.follows.unwrap_or(0));
        }
        w.0
    }

    /// A game from bytes `to_bytes` made.
    pub fn from_bytes(art: Rc<Art>, bytes: &[u8]) -> io::Result<DuckCraft> {
        let mut r = Reader(bytes);
        let version = match r.take(MAGIC.len())? {
            m if m == MAGIC => 2,
            m if m == MAGIC_V1 => 1,
            _ => return Err(bad("not a Duck Craft save")),
        };
        // The island's blocks, over a freshly grown one (for its spawn).
        let mut world = World::generate();
        let total = SIZE * TALL * SIZE;
        let mut cells = Vec::with_capacity(total);
        while cells.len() < total {
            let run = r.u32()? as usize;
            let block = r.u8()?;
            if block as usize >= BLOCK_KINDS || cells.len() + run > total {
                return Err(bad("the save's blocks don't fit the island"));
            }
            cells.extend(std::iter::repeat_n(block, run));
        }
        world.lamps.clear();
        for y in 0..TALL as i32 {
            for z in 0..SIZE as i32 {
                for x in 0..SIZE as i32 {
                    let block = cells[(y as usize * SIZE + z as usize) * SIZE + x as usize];
                    world.grid.set(x, y, z, block);
                    if block == Block::Lamp as BlockId {
                        world.lamps.push([x, y, z]);
                    }
                }
            }
        }
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                world.update_sky(x, z);
            }
        }
        let mut game = DuckCraft::new(art, world);
        game.inventory = Inventory::empty();
        if version == 1 {
            // A count of each kind of block, then gold and coal.
            for block in Block::ALL {
                let count = r.u32()?;
                if block != Block::Air {
                    game.inventory.add(Item::Block(block), count);
                }
            }
            let gold = r.u32()?;
            game.inventory.add(Item::Gold, gold);
            let coal = r.u32()?;
            game.inventory.add(Item::Coal, coal);
        } else {
            for slot in game.inventory.slots.iter_mut() {
                let (id, count) = (r.u8()?, r.u32()?);
                *slot = Item::from_id(id)
                    .filter(|_| count > 0)
                    .map(|item| Stack::new(item, count.min(items::STACK)));
            }
        }
        game.dug = r.u32()?;
        game.placed = r.u32()?;
        game.crafted = r.u32()?;
        game.imprints = r.u32()?;
        if version >= 2 {
            game.vitals.health = r.u32()?.clamp(1, health::MAX_HEALTH);
            game.vitals.food = r.u32()?.min(health::MAX_FOOD);
            game.vitals.air = r.f32()?.clamp(0.0, health::MAX_AIR);
            let count = r.u32()?;
            game.world.plants.clear();
            for _ in 0..count {
                let (x, y, z, id) = (r.u8()? as i32, r.u8()? as i32, r.u8()? as i32, r.u8()?);
                if let Some(plant) = Plant::from_id(id) {
                    game.world.plants.insert([x, y, z], plant);
                }
            }
        } else {
            // An old save has no plants: keep the freshly grown ones that
            // still have room (air, on grass).
            let world = &game.world;
            let gone: Vec<[i32; 3]> = world
                .plants
                .keys()
                .filter(|&&[x, y, z]| {
                    world.get(x, y, z) != Block::Air || !plants::grows_on(world.get(x, y - 1, z))
                })
                .copied()
                .collect();
            for [x, y, z] in gone {
                game.world.remove_plant(x, y, z);
            }
        }
        game.world.plants_changed = true;
        game.position = r.vec3()?;
        game.yaw = r.f32()?;
        game.pitch = r.f32()?;
        game.time = r.f32()?;
        game.slot = (r.u8()? as usize).min(HOTBAR_SLOTS - 1);
        game.behind_view = r.u8()? != 0;
        let count = r.u8()? as usize;
        game.ducklings.clear();
        for _ in 0..count {
            let position = r.vec3()?;
            let yaw = r.f32()?;
            let follows = r.u32()?;
            let mut duckling = ducklings::Duckling::new(position, yaw);
            duckling.follows = (follows > 0).then_some(follows);
            game.ducklings.push(duckling);
        }
        // If the place you stood is now inside a block (an odd file),
        // start at the spawn instead.
        if game.body_hits(game.position) {
            game.position = game.world.spawn;
        }
        game.update_chunks();
        game.show_help = false;
        game.last_save = game.time;
        Ok(game)
    }

    /// Saves to `save_path`, if the game has one. Writes a new file and
    /// then swaps it in, so a crash mid-save keeps the last good one.
    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = self.save_path.clone() else {
            return Ok(());
        };
        let fresh = path.with_extension("dat.new");
        std::fs::File::create(&fresh)?.write_all(&self.to_bytes())?;
        std::fs::rename(&fresh, &path)?;
        self.last_save = self.time;
        Ok(())
    }

    /// Autosave, and F5.
    pub fn maybe_save(&mut self, ctx: &mut Context) {
        if self.save_path.is_none() {
            if ctx.input.was_pressed(Key::F5) {
                self.say("Saving is off in test and replay runs");
            }
            return;
        }
        let asked = ctx.input.was_pressed(Key::F5);
        if asked || self.time - self.last_save >= AUTOSAVE_SECONDS {
            match self.save() {
                Ok(()) if asked => self.say("Saved"),
                Ok(()) => {}
                Err(error) => self.say(format!("Couldn't save: {error}")),
            }
        }
    }
}

/// The game to play: the saved island if there is one, else a new one.
/// `None` for the path: a new island, never saved (headless runs).
pub fn load_or_new(path: Option<PathBuf>) -> Result<DuckCraft, LoadError> {
    let art = Rc::new(Art::load()?);
    let mut game = match path.as_deref().filter(|p| p.exists()) {
        Some(p) => match read(p).and_then(|bytes| DuckCraft::from_bytes(art.clone(), &bytes)) {
            Ok(game) => {
                eprintln!("duck_craft: carried on from {}", p.display());
                game
            }
            Err(error) => {
                eprintln!(
                    "duck_craft: couldn't load {} ({error}); a new island",
                    p.display()
                );
                DuckCraft::new(art, World::generate())
            }
        },
        None => DuckCraft::new(art, World::generate()),
    };
    game.save_path = path;
    Ok(game)
}

fn read(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}
