use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use image::RgbaImage;
use rayon::prelude::*;

use crate::effects::{self, Ctx, Effect, Transition};
use crate::mask::Mask;

#[derive(Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub enabled: bool,
    pub transition: Transition,
    pub effect: Effect,
    pub mask: Mask,
    pub seed: u64,
}

/// Applies the enabled layers bottom-to-top. `scale` = target width / full-resolution width.
pub fn render(source: &RgbaImage, layers: &[Layer], scale: f32) -> RgbaImage {
    let (w, h) = (source.width() as usize, source.height() as usize);
    let mut cur = source.as_raw().clone();
    for layer in layers {
        if !layer.enabled || layer.mask.is_empty() {
            continue;
        }
        let mask = layer.mask.resampled(w, h);
        let growth = layer.transition == Transition::Growth;
        let ctx = Ctx { width: w, height: h, mask: &mask, growth, scale, seed: layer.seed };
        let fx = effects::apply(&layer.effect, &cur, &ctx);
        cur.par_chunks_mut(4).zip(fx.par_chunks(4)).enumerate().for_each(|(i, (dst, src))| {
            // Growth already shaped the effect with the mask (and may push pixels
            // outside it), so only feather blends by mask.
            let t = if growth { 1.0 } else { mask[i] };
            for k in 0..4 {
                dst[k] = (dst[k] as f32 + (src[k] as f32 - dst[k] as f32) * t).round() as u8;
            }
        });
    }
    RgbaImage::from_raw(w as u32, h as u32, cur).expect("buffer size matches")
}

pub struct Job {
    pub id: u64,
    pub source: Arc<RgbaImage>,
    pub layers: Vec<Layer>,
    pub scale: f32,
}

pub struct Done {
    pub id: u64,
    pub image: RgbaImage,
}

/// Background preview renderer that always skips to the newest queued job.
pub struct Worker {
    jobs: Sender<Job>,
    pub results: Receiver<Done>,
}

impl Worker {
    pub fn new(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = channel::<Job>();
        let (done_tx, results) = channel();
        std::thread::spawn(move || {
            while let Ok(mut job) = job_rx.recv() {
                while let Ok(newer) = job_rx.try_recv() {
                    job = newer;
                }
                let image = render(&job.source, &job.layers, job.scale);
                if done_tx.send(Done { id: job.id, image }).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { jobs, results }
    }

    pub fn submit(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_image(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let ring = (((fx - 0.5).hypot(fy - 0.5) * 30.0).sin() * 0.5 + 0.5) * 255.0;
            image::Rgba([(fx * 255.0) as u8, ring as u8, (fy * 255.0) as u8, 255])
        })
    }

    /// Mask fades from 0 on the left third to 1 on the right third.
    fn gradient_mask(w: usize, h: usize) -> Mask {
        let mut mask = Mask::new(w, h);
        mask.shape(crate::mask::Shape::Rect, (w as f32 / 3.0, -200.0), (w as f32 * 2.0, h as f32 + 200.0), w as f32 / 3.0, false);
        mask
    }

    #[test]
    fn every_effect_respects_unmasked_area() {
        let (w, h) = (300, 200);
        let src = test_image(w, h);
        let out_dir = std::env::var("PICS_OUT").ok();
        for i in 0..4 {
            for transition in [Transition::Feather, Transition::Growth] {
                let effect = Effect::by_index(i);
                let layer = Layer {
                    name: String::new(),
                    enabled: true,
                    transition,
                    effect: effect.clone(),
                    mask: gradient_mask(w as usize, h as usize),
                    seed: 42,
                };
                let out = render(&src, &[layer], 0.5);
                assert_eq!(out.dimensions(), src.dimensions());
                // Disintegration deliberately throws pieces outside the mask.
                if !matches!(effect, Effect::Disintegrate(_)) {
                    for y in 0..h {
                        for x in 0..w / 3 {
                            assert_eq!(out.get_pixel(x, y), src.get_pixel(x, y), "{} {transition:?} at {x},{y}", effect.name());
                        }
                    }
                }
                assert_ne!(out, src, "{} {transition:?} changed nothing", effect.name());
                if let Some(dir) = &out_dir {
                    out.save(format!("{dir}/{i}_{transition:?}.png")).unwrap();
                }
            }
        }
    }

    #[test]
    fn linear_gradient_goes_from_clean_to_full() {
        let mut mask = Mask::new(100, 1);
        mask.gradient(crate::mask::Gradient::Linear, (20.0, 0.0), (80.0, 0.0));
        let d = mask.data();
        assert_eq!(d[10], 0.0);
        assert_eq!(d[90], 1.0);
        assert!(d[30] < d[50] && d[50] < d[70]);
    }

    #[test]
    fn export_mask_is_upsampled() {
        let mut mask = Mask::new(10, 10);
        mask.fill(1.0);
        let big = mask.resampled(40, 30);
        assert_eq!(big.len(), 1200);
        assert!(big.iter().all(|&v| (v - 1.0).abs() < 1e-6));
    }
}
