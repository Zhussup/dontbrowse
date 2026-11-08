use std::io::Cursor;

use image::imageops::FilterType;
use image::{ImageReader, Limits, RgbImage};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::util::color::rgb_or_256;

/// A block of 2 vertical pixels per terminal cell, rendered with ▀.
#[derive(Clone, Debug)]
pub struct HalfblockImage {
    pub width: u16,
    pub height: u16,
    pub cells: Vec<Vec<(Color, Color)>>,
}

impl HalfblockImage {
    pub fn lines(&self) -> Vec<Line<'static>> {
        self.cells
            .iter()
            .map(|row| {
                let spans: Vec<Span<'static>> = row
                    .iter()
                    .map(|(top, bottom)| Span::styled("▀", Style::new().fg(*top).bg(*bottom)))
                    .collect();
                Line::from(spans)
            })
            .collect()
    }
}

pub fn from_bytes_fill(
    bytes: &[u8],
    target_w: u16,
    target_h: u16,
    truecolor: bool,
) -> anyhow::Result<HalfblockImage> {
    let image = decode(bytes)?;
    if target_w == 0 || target_h == 0 {
        anyhow::bail!("zero-sized image target");
    }
    let resized = resize_cover(&image, target_w as u32, target_h as u32 * 2);
    Ok(cells_from(&resized, target_w, target_h, truecolor))
}

pub fn from_bytes_fit(
    bytes: &[u8],
    max_w: u16,
    max_h: u16,
    truecolor: bool,
) -> anyhow::Result<HalfblockImage> {
    let image = decode(bytes)?;
    if max_w == 0 || max_h == 0 {
        anyhow::bail!("zero-sized image target");
    }
    let width = image.width().max(1);
    let height = image.height().max(1);
    let max_px_h = max_h as u32 * 2;
    let scale = (max_w as f32 / width as f32).min(max_px_h as f32 / height as f32);
    let target_w = ((width as f32 * scale).round() as u32).clamp(1, max_w as u32);
    let target_px_h = ((height as f32 * scale).round() as u32).clamp(1, max_px_h);
    let resized = image::imageops::resize(&image, target_w, target_px_h, FilterType::Triangle);
    let rows = target_px_h.div_ceil(2) as u16;
    Ok(cells_from(&resized, target_w as u16, rows, truecolor))
}

fn decode(bytes: &[u8]) -> anyhow::Result<RgbImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(8_000);
    limits.max_image_height = Some(8_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?;
    Ok(image.to_rgb8())
}

fn resize_cover(image: &RgbImage, width: u32, height: u32) -> RgbImage {
    let iw = image.width().max(1) as f32;
    let ih = image.height().max(1) as f32;
    let scale = (width as f32 / iw).max(height as f32 / ih);
    let resized_w = ((iw * scale).round() as u32).max(width).max(1);
    let resized_h = ((ih * scale).round() as u32).max(height).max(1);
    let resized = image::imageops::resize(image, resized_w, resized_h, FilterType::Triangle);
    let x = resized_w.saturating_sub(width) / 2;
    let y = resized_h.saturating_sub(height) / 2;
    image::imageops::crop_imm(&resized, x, y, width, height).to_image()
}

fn cells_from(image: &RgbImage, width: u16, height: u16, truecolor: bool) -> HalfblockImage {
    let mut cells = Vec::with_capacity(height as usize);
    for row in 0..height as u32 {
        let mut line = Vec::with_capacity(width as usize);
        for x in 0..width as u32 {
            let top = image.get_pixel(x.min(image.width() - 1), (row * 2).min(image.height() - 1));
            let bottom_y = (row * 2 + 1).min(image.height() - 1);
            let bottom = image.get_pixel(x.min(image.width() - 1), bottom_y);
            line.push((
                rgb_or_256(top[0], top[1], top[2], truecolor),
                rgb_or_256(bottom[0], bottom[1], bottom[2], truecolor),
            ));
        }
        cells.push(line);
    }
    HalfblockImage {
        width,
        height,
        cells,
    }
}
