//! Tall grass and flowers: little plants on the grass.
//!
//! A plant isn't a block: it's two crossed pictures standing in an air cell
//! on top of grass. You walk through it; one click breaks it (tall grass
//! sometimes gives seeds); a block placed there, or digging the grass
//! under it, gets rid of it. They're all drawn as one mesh, built again
//! when one changes.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Plant {
    TallGrass,
    YellowFlower,
    RedFlower,
    WhiteFlower,
}

pub const SEED_CHANCE: f32 = 0.3; // of seeds, from tall grass

impl Plant {
    pub const ALL: [Plant; 4] = [
        Plant::TallGrass,
        Plant::YellowFlower,
        Plant::RedFlower,
        Plant::WhiteFlower,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Plant::TallGrass => "Tall grass",
            Plant::YellowFlower => "Yellow flower",
            Plant::RedFlower => "Red flower",
            Plant::WhiteFlower => "White flower",
        }
    }

    pub fn tile(self) -> &'static str {
        match self {
            Plant::TallGrass => "tall_grass",
            Plant::YellowFlower => "flower_yellow",
            Plant::RedFlower => "flower_red",
            Plant::WhiteFlower => "flower_white",
        }
    }

    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn from_id(id: u8) -> Option<Plant> {
        Plant::ALL.get(id as usize).copied()
    }

    /// How tall it stands (the picture's box), in blocks.
    pub fn height(self) -> f32 {
        match self {
            Plant::TallGrass => 0.9,
            _ => 0.8,
        }
    }
}

/// Whether a plant can grow on this block.
pub fn grows_on(block: Block) -> bool {
    matches!(block, Block::Grass | Block::Dirt)
}

impl World {
    /// Scatters plants over the grass: tall grass about, flowers in patches.
    pub fn grow_plants(&mut self) {
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                let mut top = TALL as i32 - 1;
                while top > 0 && self.get(x, top, z) == Block::Air {
                    top -= 1;
                }
                if self.get(x, top, z) != Block::Grass {
                    continue;
                }
                let r = random01(x, 7, z, SEED + 30);
                let patch = noise2(x as f32 / 6.0, z as f32 / 6.0, SEED + 31);
                let plant = if patch > 0.66 && r < 0.4 {
                    let which = random01(x, 8, z, SEED + 32);
                    let flower = (noise2(x as f32 / 9.0, z as f32 / 9.0, SEED + 33) * 3.0) as usize;
                    Some(if which < 0.25 {
                        Plant::TallGrass
                    } else {
                        [Plant::YellowFlower, Plant::RedFlower, Plant::WhiteFlower][flower.min(2)]
                    })
                } else if r < 0.22 {
                    Some(Plant::TallGrass)
                } else {
                    None
                };
                if let Some(plant) = plant {
                    self.plants.insert([x, top + 1, z], plant);
                }
            }
        }
        self.plants_changed = true;
    }

    /// The plant in cell (x, y, z), if there is one.
    pub fn plant(&self, x: i32, y: i32, z: i32) -> Option<Plant> {
        self.plants.get(&[x, y, z]).copied()
    }

    /// Takes the plant out of a cell (if there's one).
    pub fn remove_plant(&mut self, x: i32, y: i32, z: i32) -> Option<Plant> {
        let plant = self.plants.remove(&[x, y, z]);
        if plant.is_some() {
            self.plants_changed = true;
        }
        plant
    }

    /// After block (x, y, z) changed: a plant can't share a cell with a
    /// block, and needs grass or dirt under it.
    pub fn plants_after_set(&mut self, x: i32, y: i32, z: i32, block: Block) {
        if block != Block::Air {
            self.remove_plant(x, y, z);
        }
        if !grows_on(block) {
            self.remove_plant(x, y + 1, z);
        }
    }
}

impl Art {
    /// Every plant as two crossed pictures (both sides), shaded by the
    /// light where it stands, in one mesh textured with the atlas.
    pub fn plant_mesh(&self, world: &World) -> Mesh {
        let mut mesh = Mesh::new().with_texture(self.palette.texture().clone());
        let mut cells: Vec<(&[i32; 3], &Plant)> = world.plants.iter().collect();
        cells.sort(); // the same mesh for the same plants, every time
        for (&[x, y, z], &plant) in cells {
            let uv = self.plant_uvs[plant as usize];
            let shade = (255.0 * light(&world.grid, &world.sky, x, y, z)) as u8;
            let color = Color::rgb(shade, shade, shade);
            let (low, high) = (0.15, 0.85);
            let top = y as f32 + plant.height();
            let base = vec3(x as f32, y as f32, z as f32);
            for (a, b) in [((low, low), (high, high)), ((low, high), (high, low))] {
                let corners = [
                    base + vec3(a.0, 0.0, a.1),
                    vec3(base.x + a.0, top, base.z + a.1),
                    vec3(base.x + b.0, top, base.z + b.1),
                    base + vec3(b.0, 0.0, b.1),
                ];
                let points = [
                    vec2(uv.x, uv.y + uv.h),
                    vec2(uv.x, uv.y),
                    vec2(uv.x + uv.w, uv.y),
                    vec2(uv.x + uv.w, uv.y + uv.h),
                ];
                let i: Vec<usize> = corners.iter().map(|&c| mesh.add_vertex(c)).collect();
                mesh.add_quad_uv(i[0], i[1], i[2], i[3], points, color);
                mesh.add_quad_uv(
                    i[3],
                    i[2],
                    i[1],
                    i[0],
                    [points[3], points[2], points[1], points[0]],
                    color,
                );
            }
        }
        mesh
    }
}
