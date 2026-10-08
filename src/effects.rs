//! Digital decay effects. All pixel sizes in parameters are in full-resolution
//! pixels; `Ctx::scale` converts them to the resolution being rendered.

use rayon::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transition {
    /// Effect is rendered at full strength and blended with the original by the mask.
    Feather,
    /// Mask drives the effect itself: probability and strength grow with the mask value.
    Growth,
}

// Secondary parameters are fixed to keep the UI small.
const DISINTEGRATE_SPREAD: f32 = 40.0;
const DISINTEGRATE_FADE: f32 = 0.6;
const GLITCH_SLICE: f32 = 18.0;
const GLITCH_PROBABILITY: f32 = 0.5;
const GLITCH_BLOCK: f32 = 32.0;

#[derive(Clone, PartialEq, Debug)]
pub struct Disintegrate {
    pub size: f32,
    pub amount: f32,
    pub distance: f32,
    pub direction: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Pixelate {
    pub size: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct PixelSort {
    pub vertical: bool,
    /// Only pixels brighter than this are sorted.
    pub threshold: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Glitch {
    pub shift: f32,
    pub rgb_split: f32,
    pub blocks: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Effect {
    Disintegrate(Disintegrate),
    Pixelate(Pixelate),
    PixelSort(PixelSort),
    Glitch(Glitch),
}

impl Effect {
    pub const ALL_NAMES: [&'static str; 4] = ["Disintegrate", "Pixelate", "Pixel Sort", "Glitch"];

    pub fn by_index(i: usize) -> Self {
        match i {
            0 => Effect::Disintegrate(Disintegrate { size: 6.0, amount: 0.8, distance: 120.0, direction: 0.0 }),
            1 => Effect::Pixelate(Pixelate { size: 24.0 }),
            2 => Effect::PixelSort(PixelSort { vertical: false, threshold: 0.25 }),
            _ => Effect::Glitch(Glitch { shift: 60.0, rgb_split: 6.0, blocks: 0.15 }),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Effect::Disintegrate(_) => Self::ALL_NAMES[0],
            Effect::Pixelate(_) => Self::ALL_NAMES[1],
            Effect::PixelSort(_) => Self::ALL_NAMES[2],
            Effect::Glitch(_) => Self::ALL_NAMES[3],
        }
    }
}

pub struct Ctx<'a> {
    pub width: usize,
    pub height: usize,
    pub mask: &'a [f32],
    pub growth: bool,
    pub scale: f32,
    pub seed: u64,
}

impl Ctx<'_> {
    /// Mask value that drives the effect: the real mask in growth mode, otherwise 1.
    #[inline]
    fn m(&self, x: usize, y: usize) -> f32 {
        if self.growth { self.mask[y * self.width + x] } else { 1.0 }
    }

    fn px(&self, v: f32) -> f32 {
        (v * self.scale).max(1.0)
    }
}

pub fn apply(effect: &Effect, src: &[u8], ctx: &Ctx) -> Vec<u8> {
    match effect {
        Effect::Disintegrate(p) => disintegrate(src, p, ctx),
        Effect::Pixelate(p) => pixelate(src, p, ctx),
        Effect::PixelSort(p) => pixel_sort(src, p, ctx),
        Effect::Glitch(p) => glitch(src, p, ctx),
    }
}

// ---------- randomness ----------

#[inline]
fn hash(seed: u64, a: u64, b: u64, c: u64) -> u64 {
    let mut z = seed
        ^ a.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ c.wrapping_mul(0x1656_67B1_9E37_79F9);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[inline]
fn rand01(seed: u64, a: u64, b: u64, c: u64) -> f32 {
    (hash(seed, a, b, c) >> 40) as f32 / (1u64 << 24) as f32
}

#[inline]
fn get(src: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
    let i = (y * w + x) * 4;
    [src[i], src[i + 1], src[i + 2], src[i + 3]]
}

// ---------- pixelate ----------

fn pixelate(src: &[u8], p: &Pixelate, ctx: &Ctx) -> Vec<u8> {
    let (w, h) = (ctx.width, ctx.height);
    let max_block = ctx.px(p.size);
    let mut out = vec![0u8; src.len()];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let s = (1.0 + (max_block - 1.0) * ctx.m(x, y)).round().max(1.0) as usize;
            let px = if s <= 1 {
                get(src, w, x, y)
            } else {
                // Average four samples inside the block for a cleaner colour.
                let bx = x / s * s;
                let by = y / s * s;
                let mut acc = [0u32; 4];
                for (ox, oy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                    let sx = (bx + s * ox / 4).min(w - 1);
                    let sy = (by + s * oy / 4).min(h - 1);
                    let c = get(src, w, sx, sy);
                    for k in 0..4 {
                        acc[k] += c[k] as u32;
                    }
                }
                acc.map(|v| (v / 4) as u8)
            };
            row[x * 4..x * 4 + 4].copy_from_slice(&px);
        }
    });
    out
}

// ---------- disintegrate ----------

fn disintegrate(src: &[u8], p: &Disintegrate, ctx: &Ctx) -> Vec<u8> {
    let (w, h) = (ctx.width, ctx.height);
    let s = ctx.px(p.size).round() as usize;
    let dist = p.distance * ctx.scale;
    let base = p.direction.to_radians();
    let spread = DISINTEGRATE_SPREAD.to_radians();
    let seed = ctx.seed;

    struct Piece {
        order: u64,
        x: usize,
        y: usize,
        dx: f32,
        dy: f32,
        alpha: f32,
    }

    let mut pieces = Vec::new();
    for by in 0..h.div_ceil(s) {
        for bx in 0..w.div_ceil(s) {
            let (x, y) = (bx * s, by * s);
            let m = ctx.m((x + s / 2).min(w - 1), (y + s / 2).min(h - 1));
            let (bx, by) = (bx as u64, by as u64);
            if m <= 0.0 || rand01(seed, bx, by, 0) >= p.amount * m {
                continue;
            }
            let travel = 0.25 + 0.75 * rand01(seed, bx, by, 1);
            let d = dist * travel * m.sqrt();
            let a = base + (rand01(seed, bx, by, 2) - 0.5) * spread;
            pieces.push(Piece {
                order: hash(seed, bx, by, 3),
                x,
                y,
                dx: a.cos() * d,
                dy: -a.sin() * d,
                alpha: 1.0 - DISINTEGRATE_FADE * travel,
            });
        }
    }

    // Pieces leave transparent holes behind.
    let mut out = src.to_vec();
    for piece in &pieces {
        for y in piece.y..(piece.y + s).min(h) {
            for x in piece.x..(piece.x + s).min(w) {
                let i = (y * w + x) * 4;
                out[i..i + 4].fill(0);
            }
        }
    }

    pieces.sort_unstable_by_key(|p| p.order);
    for piece in &pieces {
        let ox = piece.dx.round() as i64;
        let oy = piece.dy.round() as i64;
        for y in piece.y..(piece.y + s).min(h) {
            let ty = y as i64 + oy;
            if ty < 0 || ty >= h as i64 {
                continue;
            }
            for x in piece.x..(piece.x + s).min(w) {
                let tx = x as i64 + ox;
                if tx < 0 || tx >= w as i64 {
                    continue;
                }
                let c = get(src, w, x, y);
                let i = (ty as usize * w + tx as usize) * 4;
                over(&mut out[i..i + 4], c, piece.alpha);
            }
        }
    }
    out
}

/// Straight-alpha "source over" compositing.
fn over(dst: &mut [u8], src: [u8; 4], extra_alpha: f32) {
    let sa = src[3] as f32 / 255.0 * extra_alpha.clamp(0.0, 1.0);
    let da = dst[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return;
    }
    for k in 0..3 {
        let v = (src[k] as f32 * sa + dst[k] as f32 * da * (1.0 - sa)) / oa;
        dst[k] = v.round() as u8;
    }
    dst[3] = (oa * 255.0).round() as u8;
}

// ---------- pixel sort ----------

fn luminance(c: [u8; 4]) -> f32 {
    (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0
}

fn transpose<T: Copy + Default>(src: &[T], w: usize, h: usize, channels: usize) -> Vec<T> {
    let mut out = vec![T::default(); src.len()];
    for y in 0..h {
        for x in 0..w {
            let s = (y * w + x) * channels;
            let d = (x * h + y) * channels;
            out[d..d + channels].copy_from_slice(&src[s..s + channels]);
        }
    }
    out
}

fn pixel_sort(src: &[u8], p: &PixelSort, ctx: &Ctx) -> Vec<u8> {
    if !p.vertical {
        return sort_rows(src.to_vec(), ctx.mask.to_vec(), ctx.width, p, ctx);
    }
    let (w, h) = (ctx.width, ctx.height);
    let t_img = transpose(src, w, h, 4);
    let t_mask = transpose(ctx.mask, w, h, 1);
    let sorted = sort_rows(t_img, t_mask, h, p, ctx);
    transpose(&sorted, h, w, 4)
}

fn sort_rows(mut img: Vec<u8>, mask: Vec<f32>, w: usize, p: &PixelSort, ctx: &Ctx) -> Vec<u8> {
    img.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        // In growth mode each line gets its own threshold, so the sorted streaks
        // end at different places across a soft mask edge.
        let threshold = 0.02 + 0.96 * rand01(ctx.seed, y as u64, 7, 0);
        let mask_row = &mask[y * w..(y + 1) * w];
        let pixels: Vec<[u8; 4]> = row.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        let keys: Vec<f32> = pixels.iter().map(|&c| luminance(c)).collect();
        let eligible = |i: usize| {
            keys[i] >= p.threshold && (!ctx.growth || mask_row[i] > threshold)
        };
        let mut x = 0;
        while x < w {
            if !eligible(x) {
                x += 1;
                continue;
            }
            let start = x;
            while x < w && eligible(x) {
                x += 1;
            }
            let mut span: Vec<(f32, [u8; 4])> = (start..x).map(|i| (keys[i], pixels[i])).collect();
            span.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (k, (_, c)) in span.into_iter().enumerate() {
                let i = (start + k) * 4;
                row[i..i + 4].copy_from_slice(&c);
            }
        }
    });
    img
}

// ---------- glitch ----------

fn glitch(src: &[u8], p: &Glitch, ctx: &Ctx) -> Vec<u8> {
    let (w, h) = (ctx.width, ctx.height);
    let slice = ctx.px(GLITCH_SLICE);
    let shift = p.shift * ctx.scale;
    let split = p.rgb_split * ctx.scale;
    let cb = ctx.px(GLITCH_BLOCK).round() as usize;
    let seed = ctx.seed;
    let wrap = |x: f32| (x.round() as i64).rem_euclid(w as i64) as usize;

    let mut out = vec![0u8; src.len()];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let si = (y as f32 / slice) as u64;
        let slice_r = rand01(seed, si, 0, 11);
        let slice_shift = (rand01(seed, si, 1, 11) * 2.0 - 1.0) * shift;
        for x in 0..w {
            let m = ctx.m(x, y);
            if m <= 0.0 {
                row[x * 4..x * 4 + 4].copy_from_slice(&get(src, w, x, y));
                continue;
            }
            let dx = if slice_r < GLITCH_PROBABILITY * m { slice_shift } else { 0.0 };
            let sx = x as f32 - dx;
            let sp = split * m;

            let (bx, by) = ((x / cb) as u64, (y / cb) as u64);
            let corrupted = rand01(seed, bx, by, 12) < p.blocks * m;
            let variant = hash(seed, bx, by, 13) % 4;
            let mut sy = y;
            let mut sxx = sx;
            if corrupted && variant == 0 {
                // Copy the block from somewhere nearby.
                let ox = (rand01(seed, bx, by, 14) - 0.5) * cb as f32 * 8.0;
                let oy = (rand01(seed, bx, by, 15) - 0.5) * cb as f32 * 8.0;
                sxx += ox;
                sy = ((y as f32 + oy).round() as i64).clamp(0, h as i64 - 1) as usize;
            }

            let r = get(src, w, wrap(sxx + sp), sy)[0];
            let g_px = get(src, w, wrap(sxx), sy);
            let b = get(src, w, wrap(sxx - sp), sy)[2];
            let mut c = [r, g_px[1], b, g_px[3]];
            if corrupted {
                match variant {
                    1 => c = [c[1], c[2], c[0], c[3]],
                    2 => {
                        for k in 0..3 {
                            c[k] = (c[k] & 0xC0) | 0x20;
                        }
                    }
                    3 => {
                        for k in 0..3 {
                            c[k] = 255 - c[k];
                        }
                    }
                    _ => {}
                }
            }
            row[x * 4..x * 4 + 4].copy_from_slice(&c);
        }
    });
    out
}
