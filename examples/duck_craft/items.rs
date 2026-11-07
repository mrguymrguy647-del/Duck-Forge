//! Things you carry, and the bag you carry them in.
//!
//! An [`Item`] is a block or something else (gold, coal, food). Items sit
//! in [`Stack`]s of up to 64 in the [`Inventory`]: 36 slots, the first 9 the
//! hotbar along the bottom of the screen, the other 27 the backpack you see
//! in the inventory screen (**E**).

use super::*;

/// Something you can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Block(Block),
    /// From gold ore.
    Gold,
    /// From coal ore.
    Coal,
    /// Sometimes from leaves. Food.
    Apple,
    /// Sometimes from tall grass. A little food; three make bread.
    Seeds,
    /// Food: the best there is.
    Bread,
    /// Knapped stone on a stick: hits harder than wings. One to a slot.
    Blade,
}

/// How many fit in one slot, for most things (see `Item::max_stack`).
pub const STACK: u32 = 64;
/// All the slots: the hotbar (0 to 8), then the backpack (9 to 35).
pub const SLOTS: usize = 36;
pub const HOTBAR_SLOTS: usize = 9;

impl Item {
    pub fn name(self) -> &'static str {
        match self {
            Item::Block(block) => block.name(),
            Item::Gold => "Gold",
            Item::Coal => "Coal",
            Item::Apple => "Apple",
            Item::Seeds => "Seeds",
            Item::Bread => "Bread",
            Item::Blade => "Stone blade",
        }
    }

    /// How many fit in one slot.
    pub fn max_stack(self) -> u32 {
        match self {
            Item::Blade => 1,
            _ => STACK,
        }
    }

    /// Its picture in the atlas.
    pub fn tile(self) -> &'static str {
        match self {
            Item::Block(block) => block.tiles().map_or("dirt", |tiles| tiles[1]),
            Item::Gold => "gold_nugget",
            Item::Coal => "coal_lump",
            Item::Apple => "apple",
            Item::Seeds => "seeds",
            Item::Bread => "bread",
            Item::Blade => "stone_blade",
        }
    }

    /// The block it puts down, if it's a block.
    pub fn block(self) -> Option<Block> {
        match self {
            Item::Block(block) => Some(block),
            _ => None,
        }
    }

    /// How much hunger eating it fills (out of 20), if it's food.
    pub fn food(self) -> Option<u32> {
        match self {
            Item::Apple => Some(4),
            Item::Seeds => Some(1),
            Item::Bread => Some(6),
            _ => None,
        }
    }

    /// Everything there is to carry, for the creative panel.
    pub fn all() -> Vec<Item> {
        let blocks = Block::ALL
            .into_iter()
            .filter(|&b| b != Block::Air)
            .map(Item::Block);
        blocks
            .chain([
                Item::Gold,
                Item::Coal,
                Item::Apple,
                Item::Seeds,
                Item::Bread,
                Item::Blade,
            ])
            .collect()
    }

    /// A number for the save file (0 is "no item").
    pub fn id(self) -> u8 {
        match self {
            Item::Block(block) => block as u8, // 1 to 15 (Air is 0, never carried)
            Item::Gold => 100,
            Item::Coal => 101,
            Item::Apple => 102,
            Item::Seeds => 103,
            Item::Bread => 104,
            Item::Blade => 105,
        }
    }

    pub fn from_id(id: u8) -> Option<Item> {
        Some(match id {
            100 => Item::Gold,
            101 => Item::Coal,
            102 => Item::Apple,
            103 => Item::Seeds,
            104 => Item::Bread,
            105 => Item::Blade,
            n if (1..BLOCK_KINDS as u8).contains(&n) => Item::Block(Block::ALL[n as usize]),
            _ => return None,
        })
    }
}

/// Some of one item, in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    pub item: Item,
    pub count: u32,
}

impl Stack {
    pub fn new(item: Item, count: u32) -> Stack {
        Stack { item, count }
    }
}

/// The 36 slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub slots: [Option<Stack>; SLOTS],
}

impl Inventory {
    pub fn empty() -> Inventory {
        Inventory {
            slots: [None; SLOTS],
        }
    }

    /// How many of `item` there are, in every slot.
    pub fn count(&self, item: Item) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|stack| stack.item == item)
            .map(|stack| stack.count)
            .sum()
    }

    /// Puts `count` of `item` in: first on stacks of it that have room,
    /// then in empty slots, the hotbar before the backpack. Returns how
    /// many didn't fit.
    pub fn add(&mut self, item: Item, mut count: u32) -> u32 {
        for slot in self.slots.iter_mut().flatten() {
            if count == 0 {
                break;
            }
            if slot.item == item && slot.count < item.max_stack() {
                let moved = count.min(item.max_stack() - slot.count);
                slot.count += moved;
                count -= moved;
            }
        }
        for slot in self.slots.iter_mut() {
            if count == 0 {
                break;
            }
            if slot.is_none() {
                let moved = count.min(item.max_stack());
                *slot = Some(Stack::new(item, moved));
                count -= moved;
            }
        }
        count
    }

    /// Takes up to `count` of `item` out, from the backpack's last slots
    /// first (so the hotbar keeps its things longest). Returns how many it
    /// took.
    pub fn take(&mut self, item: Item, count: u32) -> u32 {
        let mut taken = 0;
        for slot in self.slots.iter_mut().rev() {
            if taken == count {
                break;
            }
            if let Some(stack) = slot {
                if stack.item == item {
                    let moved = (count - taken).min(stack.count);
                    stack.count -= moved;
                    taken += moved;
                    if stack.count == 0 {
                        *slot = None;
                    }
                }
            }
        }
        taken
    }

    /// Takes one from slot `i`; returns what it was.
    pub fn take_one_from(&mut self, i: usize) -> Option<Item> {
        let stack = self.slots[i].as_mut()?;
        let item = stack.item;
        stack.count -= 1;
        if stack.count == 0 {
            self.slots[i] = None;
        }
        Some(item)
    }

    /// Makes the count of `item` exactly `count` (for tests).
    #[cfg(test)]
    pub fn set(&mut self, item: Item, count: u32) {
        self.take(item, u32::MAX);
        self.add(item, count);
    }

    /// The slot holding `item` in the hotbar, if one does.
    pub fn hotbar_slot_of(&self, item: Item) -> Option<usize> {
        (0..HOTBAR_SLOTS).find(|&i| self.slots[i].is_some_and(|s| s.item == item))
    }
}

/// What you start with in creative: a hotbar of building blocks (they
/// never run out).
pub fn creative_kit() -> Inventory {
    let mut inventory = Inventory::empty();
    for (i, block) in [
        Block::Grass,
        Block::Cobblestone,
        Block::Planks,
        Block::Log,
        Block::Glass,
        Block::Brick,
        Block::Lamp,
        Block::Leaves,
        Block::Sand,
    ]
    .into_iter()
    .enumerate()
    {
        inventory.slots[i] = Some(Stack::new(Item::Block(block), STACK));
    }
    inventory
}

/// What you start with: building blocks, lamps for the night, and bread.
pub fn starting_kit() -> Inventory {
    let mut inventory = Inventory::empty();
    for (i, (item, count)) in [
        (Item::Block(Block::Planks), 32),
        (Item::Block(Block::Glass), 16),
        (Item::Block(Block::Brick), 32),
        (Item::Block(Block::Lamp), 8),
        (Item::Bread, 3),
    ]
    .into_iter()
    .enumerate()
    {
        inventory.slots[i] = Some(Stack::new(item, count));
    }
    inventory
}
