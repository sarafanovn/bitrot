use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_VERSION: AtomicU64 = AtomicU64::new(1);

fn next_version() -> u64 {
    NEXT_VERSION.fetch_add(1, Ordering::Relaxed)
}

/// Grayscale mask (0 = no effect, 1 = full effect) stored at work resolution.
///
/// Cloning is cheap: data is shared until the next edit. Every edit gets a new
/// globally unique version, so equality is a version comparison.
#[derive(Clone)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    data: Arc<Vec<f32>>,
    version: u64,
}

impl PartialEq for Mask {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Ellipse,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gradient {
    Linear,
    Radial,
}

impl Mask {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: Arc::new(vec![0.0; width * height]),
            version: next_version(),
        }
    }

    #[cfg(test)]
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|&v| v <= 0.0)
    }

    fn data_mut(&mut self) -> &mut Vec<f32> {
        self.version = next_version();
        Arc::make_mut(&mut self.data)
    }

    pub fn fill(&mut self, value: f32) {
        self.data_mut().fill(value);
    }

    pub fn invert(&mut self) {
        self.data_mut().iter_mut().for_each(|v| *v = 1.0 - *v);
    }

    /// One round brush dab. `hardness` is the fraction of the radius at full strength.
    pub fn dab(&mut self, cx: f32, cy: f32, radius: f32, hardness: f32, opacity: f32, erase: bool) {
        let radius = radius.max(0.5);
        let (w, h) = (self.width as i64, self.height as i64);
        let x0 = ((cx - radius).floor() as i64).max(0);
        let x1 = ((cx + radius).ceil() as i64).min(w - 1);
        let y0 = ((cy - radius).floor() as i64).max(0);
        let y1 = ((cy + radius).ceil() as i64).min(h - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }
        let hardness = hardness.clamp(0.0, 0.999);
        let data = self.data_mut();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let d = (dx * dx + dy * dy).sqrt() / radius;
                if d >= 1.0 {
                    continue;
                }
                let v = if d <= hardness {
                    1.0
                } else {
                    1.0 - smoothstep((d - hardness) / (1.0 - hardness))
                } * opacity;
                apply(&mut data[(y * w + x) as usize], v, erase);
            }
        }
    }

    /// Dabs along a segment so fast strokes stay continuous.
    pub fn stroke(&mut self, from: (f32, f32), to: (f32, f32), radius: f32, hardness: f32, opacity: f32, erase: bool) {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let len = (dx * dx + dy * dy).sqrt();
        let spacing = (radius * 0.25).max(0.5);
        let steps = (len / spacing).ceil().max(1.0) as usize;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            self.dab(from.0 + dx * t, from.1 + dy * t, radius, hardness, opacity, erase);
        }
    }

    /// Fills a rectangle or ellipse whose edge fades inward over `feather` pixels.
    pub fn shape(&mut self, shape: Shape, a: (f32, f32), b: (f32, f32), feather: f32, erase: bool) {
        let (minx, maxx) = (a.0.min(b.0), a.0.max(b.0));
        let (miny, maxy) = (a.1.min(b.1), a.1.max(b.1));
        if maxx - minx < 1.0 || maxy - miny < 1.0 {
            return;
        }
        let (w, h) = (self.width as i64, self.height as i64);
        let x0 = (minx.floor() as i64).max(0);
        let x1 = (maxx.ceil() as i64).min(w - 1);
        let y0 = (miny.floor() as i64).max(0);
        let y1 = (maxy.ceil() as i64).min(h - 1);
        let (cx, cy) = ((minx + maxx) * 0.5, (miny + maxy) * 0.5);
        let (rx, ry) = ((maxx - minx) * 0.5, (maxy - miny) * 0.5);
        let data = self.data_mut();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                // Approximate distance from the edge, positive inside.
                let inside = match shape {
                    Shape::Rect => (px - minx).min(maxx - px).min(py - miny).min(maxy - py),
                    Shape::Ellipse => {
                        let nx = (px - cx) / rx;
                        let ny = (py - cy) / ry;
                        let r = (nx * nx + ny * ny).sqrt();
                        let grad = ((nx / rx).powi(2) + (ny / ry).powi(2)).sqrt();
                        if r < 1e-6 || grad < 1e-9 {
                            rx.min(ry)
                        } else {
                            (1.0 - r) * r / grad
                        }
                    }
                };
                if inside <= 0.0 {
                    continue;
                }
                let v = if feather < 1.0 { 1.0 } else { smoothstep(inside / feather) };
                apply(&mut data[(y * w + x) as usize], v, erase);
            }
        }
    }

    /// Replaces the mask with a gradient: 0 at `start`, 1 at `end` and beyond.
    /// Radial gradients grow outward from `start` with radius |end - start|.
    pub fn gradient(&mut self, kind: Gradient, start: (f32, f32), end: (f32, f32)) {
        let (dx, dy) = (end.0 - start.0, end.1 - start.1);
        let len2 = dx * dx + dy * dy;
        if len2 < 1.0 {
            return;
        }
        let len = len2.sqrt();
        let w = self.width;
        let data = self.data_mut();
        for (i, m) in data.iter_mut().enumerate() {
            let px = (i % w) as f32 + 0.5 - start.0;
            let py = (i / w) as f32 + 0.5 - start.1;
            let t = match kind {
                Gradient::Linear => (px * dx + py * dy) / len2,
                Gradient::Radial => (px * px + py * py).sqrt() / len,
            };
            *m = smoothstep(t);
        }
    }

    /// Bilinear resample to another resolution (used for full-size export).
    pub fn resampled(&self, width: usize, height: usize) -> Vec<f32> {
        if width == self.width && height == self.height {
            return self.data.to_vec();
        }
        let sx = self.width as f32 / width as f32;
        let sy = self.height as f32 / height as f32;
        let (mw, mh) = (self.width, self.height);
        let src = &self.data;
        let mut out = vec![0.0; width * height];
        for y in 0..height {
            let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (mh - 1) as f32);
            let y0 = fy as usize;
            let y1 = (y0 + 1).min(mh - 1);
            let ty = fy - y0 as f32;
            for x in 0..width {
                let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (mw - 1) as f32);
                let x0 = fx as usize;
                let x1 = (x0 + 1).min(mw - 1);
                let tx = fx - x0 as f32;
                let top = src[y0 * mw + x0] * (1.0 - tx) + src[y0 * mw + x1] * tx;
                let bot = src[y1 * mw + x0] * (1.0 - tx) + src[y1 * mw + x1] * tx;
                out[y * width + x] = top * (1.0 - ty) + bot * ty;
            }
        }
        out
    }
}

fn apply(m: &mut f32, v: f32, erase: bool) {
    if erase {
        *m = m.min(1.0 - v);
    } else {
        *m = m.max(v);
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
