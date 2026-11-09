//! Collage images, patterns, snapping, and export helpers.

use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use iced::widget::image::Handle as ImageHandle;
use iced::Point;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Rgba, RgbaImage};

use crate::i18n::Language;

pub const CANVAS_W: f32 = 707.0;
pub const CANVAS_H: f32 = 1000.0;
pub const EXPORT_W: u32 = 2480;
pub const EXPORT_H: u32 = 3508;
pub const GRID_STEP: f32 = 0.05;

const PREVIEW_MAX: u32 = 768;
const JPEG_QUALITY: u8 = 92;
const MARGIN_X: f32 = 0.018;
const MARGIN_Y: f32 = 0.018;
const GAP: f32 = 0.006;
const MIN_IMAGE_SIZE: f32 = 0.03;
const MAX_ROW_IMAGES: usize = 4;

#[derive(Debug, Clone)]
pub struct CollageImage {
    pub path: PathBuf,
    pub handle: ImageHandle,
    pub aspect: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl CollageImage {
    pub fn left(&self) -> f32 {
        self.x - self.width / 2.0
    }

    pub fn top(&self) -> f32 {
        self.y - self.height / 2.0
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left()
            && x <= self.left() + self.width
            && y >= self.top()
            && y <= self.top() + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    /// The direction from the fixed (opposite) corner towards this one.
    pub fn direction(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (-1.0, -1.0),
            Self::TopRight => (1.0, -1.0),
            Self::BottomLeft => (-1.0, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }

    /// The corner that stays put while the opposite handle is dragged.
    pub fn fixed_corner(self, center: Point, width: f32, height: f32) -> Point {
        let (dx, dy) = self.direction();

        Point::new(center.x - dx * width / 2.0, center.y - dy * height / 2.0)
    }
}

/// Scales an image proportionally from a dragged corner, keeping the opposite
/// corner fixed and the image inside the page.
pub fn resize_from_corner(
    corner: Corner,
    pointer: Point,
    fixed: Point,
    start_width: f32,
    start_height: f32,
) -> (f32, f32, f32, f32) {
    let start_width = start_width.max(f32::EPSILON);
    let start_height = start_height.max(f32::EPSILON);
    let (dx, dy) = corner.direction();

    let scale_x = (pointer.x - fixed.x).abs() / start_width;
    let scale_y = (pointer.y - fixed.y).abs() / start_height;

    // Follow whichever axis the pointer moved furthest away from its starting
    // size. This makes both horizontal and diagonal drags feel natural.
    let scale = if (scale_x - 1.0).abs() > (scale_y - 1.0).abs() {
        scale_x
    } else {
        scale_y
    };

    let available_x = if dx > 0.0 { 1.0 - fixed.x } else { fixed.x };
    let available_y = if dy > 0.0 { 1.0 - fixed.y } else { fixed.y };
    let min_scale = (MIN_IMAGE_SIZE / start_width).max(MIN_IMAGE_SIZE / start_height);
    let max_scale = (available_x / start_width)
        .min(available_y / start_height)
        .max(min_scale);

    let scale = scale.clamp(min_scale, max_scale);
    let width = start_width * scale;
    let height = start_height * scale;
    let center = Point::new(fixed.x + dx * width / 2.0, fixed.y + dy * height / 2.0);

    (center.x, center.y, width, height)
}

#[derive(Debug, Default)]
pub struct CollectedImages {
    pub paths: Vec<PathBuf>,
    pub skipped: usize,
}

/// Expands files and folders into a sorted, de-duplicated list of images.
pub fn collect_images(paths: &[PathBuf]) -> CollectedImages {
    let mut collected = CollectedImages::default();
    let mut seen = HashSet::new();

    for path in paths {
        visit(path, 0, &mut collected, &mut seen);
    }

    collected
        .paths
        .sort_by_key(|path| path.file_name().map(|name| name.to_ascii_lowercase()));

    collected
}

fn visit(path: &Path, depth: usize, collected: &mut CollectedImages, seen: &mut HashSet<String>) {
    if depth > 16 {
        collected.skipped += 1;
        return;
    }

    if path.is_dir() {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    visit(&entry.path(), depth + 1, collected, seen);
                }
            }
            Err(_) => collected.skipped += 1,
        }
        return;
    }

    if !path.is_file() {
        collected.skipped += 1;
        return;
    }

    if ImageFormat::from_path(path).is_err() {
        collected.skipped += 1;
        return;
    }

    let key = path.to_string_lossy().to_lowercase();
    if seen.insert(key) {
        collected.paths.push(path.to_path_buf());
    } else {
        collected.skipped += 1;
    }
}

pub fn load_preview(path: &Path) -> Result<CollageImage, String> {
    let image = load_oriented(path)?;
    let preview = image.thumbnail(PREVIEW_MAX, PREVIEW_MAX);
    let (width, height) = (preview.width().max(1), preview.height().max(1));
    let rgba = preview.to_rgba8();
    let handle = ImageHandle::from_rgba(width, height, rgba.into_raw());
    let aspect = width as f32 / height as f32;

    let width = 0.3;
    let height = width * CANVAS_W / (aspect * CANVAS_H);

    Ok(CollageImage {
        path: path.to_path_buf(),
        handle,
        aspect,
        x: 0.5,
        y: 0.5,
        width,
        height,
    })
}

fn load_oriented(path: &Path) -> Result<DynamicImage, String> {
    let reader = ImageReader::open(path)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?;

    let mut decoder = reader.into_decoder().map_err(|error| error.to_string())?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(|error| error.to_string())?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn fit_box(aspect: f32, max_width: f32, max_height: f32) -> (f32, f32) {
    let max_width = max_width.max(0.01);
    let max_height = max_height.max(0.01);
    let mut width = max_width;
    let mut height = width * (CANVAS_W / CANVAS_H) / aspect.max(f32::EPSILON);

    if height > max_height {
        height = max_height;
        width = height * aspect * (CANVAS_H / CANVAS_W);
    }

    (width.max(0.01), height.max(0.01))
}

/// Picks the row breaks that make a justified gallery fill the page. Images
/// keep their own aspect ratio and each row is scaled to exactly fit the page
/// width, producing the tight portfolio masonry look.
fn best_rows(aspects: &[f32], usable_w: f32, usable_h: f32) -> Vec<(usize, usize)> {
    let count = aspects.len();

    if count <= 16 {
        let mut best_score = f32::INFINITY;
        let mut best_rows = Vec::new();

        for mask in 0..(1usize << (count - 1)) {
            let mut rows = Vec::new();
            let mut start = 0;

            for index in 0..count {
                let is_break = index == count - 1 || (mask & (1 << index)) != 0;
                if is_break {
                    rows.push((start, index + 1));
                    start = index + 1;
                }
            }

            let (total_height, heights) = rows_height(&rows, aspects, usable_w);

            if rows.iter().any(|(start, end)| end - start > MAX_ROW_IMAGES) {
                continue;
            }

            if count >= 3 && rows.iter().any(|(start, end)| end - start == 1) {
                continue;
            }

            let mean_height = heights.iter().sum::<f32>() / heights.len().max(1) as f32;
            let variance = heights
                .iter()
                .map(|height| (height - mean_height).powi(2))
                .sum::<f32>()
                / heights.len().max(1) as f32;
            let mean_count = rows
                .iter()
                .map(|(start, end)| (end - start) as f32)
                .sum::<f32>()
                / rows.len().max(1) as f32;
            let count_variance = rows
                .iter()
                .map(|(start, end)| {
                    let difference = (end - start) as f32 - mean_count;
                    difference.powi(2)
                })
                .sum::<f32>()
                / rows.len().max(1) as f32;

            let score = if total_height <= usable_h {
                usable_h - total_height
            } else {
                (total_height - usable_h) * 1000.0
            } + variance * 8.0
                + count_variance * 1.5
                + rows.len() as f32 * 1.0e-4;

            if score < best_score {
                best_score = score;
                best_rows = rows;
            }
        }

        return best_rows;
    }

    // A conservative fallback for very large pages: use rows of four.
    let mut rows = Vec::new();
    let mut start = 0;

    while start < count {
        let end = (start + 4).min(count);
        rows.push((start, end));
        start = end;
    }

    rows
}

fn rows_height(rows: &[(usize, usize)], aspects: &[f32], usable_w: f32) -> (f32, Vec<f32>) {
    let ratio = CANVAS_H / CANVAS_W;
    let mut heights = Vec::with_capacity(rows.len());
    let mut total = 0.0;

    for (index, (start, end)) in rows.iter().enumerate() {
        let len = end - start;
        let gap = GAP * (len.saturating_sub(1)) as f32;
        let sum: f32 = aspects[*start..*end].iter().sum();
        let height = (usable_w - gap).max(0.0) / (ratio * sum.max(f32::EPSILON));
        heights.push(height);
        total += height + if index > 0 { GAP } else { 0.0 };
    }

    (total, heights)
}

/// Arranges images in a justified masonry collage: mixed row heights, tight
/// gaps, and every row filling the width of the A4 page.
pub fn apply_masonry(images: &mut [CollageImage]) {
    let count = images.len();

    if count == 0 {
        return;
    }

    let usable_w = 1.0 - 2.0 * MARGIN_X;
    let usable_h = 1.0 - 2.0 * MARGIN_Y;
    let ratio = CANVAS_H / CANVAS_W;

    if count == 1 {
        let (width, height) = fit_box(images[0].aspect, usable_w, usable_h);
        images[0].x = 0.5;
        images[0].y = 0.5;
        images[0].width = width;
        images[0].height = height;
        return;
    }

    let aspects: Vec<f32> = images.iter().map(|image| image.aspect).collect();
    let rows = best_rows(&aspects, usable_w, usable_h);
    let (total_height, heights) = rows_height(&rows, &aspects, usable_w);
    let mut y = MARGIN_Y + (usable_h - total_height).max(0.0) / 2.0;

    for ((start, end), height) in rows.iter().zip(heights.iter()) {
        let mut x = MARGIN_X;

        for index in *start..*end {
            let width = height * ratio * aspects[index];
            let image = &mut images[index];
            image.width = width;
            image.height = *height;
            image.x = x + width / 2.0;
            image.y = y + height / 2.0;
            x += width + GAP;
        }

        y += height + GAP;
    }
}

pub fn hit_test(images: &[CollageImage], x: f32, y: f32) -> Option<usize> {
    images.iter().rposition(|image| image.contains(x, y))
}

/// Move an image freely, only clamping it so it stays fully inside the page.
/// With snapping disabled this gives complete, unmodified pointer movement.
pub fn clamp_to_page(image: &CollageImage, x: f32, y: f32) -> (f32, f32) {
    let half_w = image.width / 2.0;
    let half_h = image.height / 2.0;

    (
        x.clamp(half_w, (1.0 - half_w).max(half_w)),
        y.clamp(half_h, (1.0 - half_h).max(half_h)),
    )
}

/// Snap the image's top-left corner to the nearest grid line and keep the
/// image on the page. Only the grid is used; other images, page edges, and the
/// page centre never pull the image around.
pub fn snap_to_grid(image: &CollageImage, x: f32, y: f32) -> (f32, f32) {
    let left = snap_value(x - image.width / 2.0).clamp(0.0, (1.0 - image.width).max(0.0));
    let top = snap_value(y - image.height / 2.0).clamp(0.0, (1.0 - image.height).max(0.0));

    (left + image.width / 2.0, top + image.height / 2.0)
}

fn snap_value(value: f32) -> f32 {
    (value / GRID_STEP).round() * GRID_STEP
}

pub fn export_image(images: &[CollageImage], path: &Path) -> Result<(), String> {
    let mut canvas = RgbaImage::from_pixel(EXPORT_W, EXPORT_H, Rgba([255, 255, 255, 255]));

    for image in images {
        let source = load_oriented(&image.path)?.to_rgba8();
        let target_w = (image.width * EXPORT_W as f32).round().max(1.0) as u32;
        let target_h = (image.height * EXPORT_H as f32).round().max(1.0) as u32;
        let (fit_w, fit_h) = fit_box(image.aspect, target_w as f32, target_h as f32);
        let fit_w = fit_w.round().max(1.0) as u32;
        let fit_h = fit_h.round().max(1.0) as u32;
        let resized = image::imageops::resize(&source, fit_w, fit_h, FilterType::Lanczos3);

        let left = (image.left() * EXPORT_W as f32).round() as i64 + (target_w - fit_w) as i64 / 2;
        let top = (image.top() * EXPORT_H as f32).round() as i64 + (target_h - fit_h) as i64 / 2;

        image::imageops::overlay(&mut canvas, &resized, left, top);
    }

    let format = ImageFormat::from_path(path).unwrap_or(ImageFormat::Png);

    if format == ImageFormat::Jpeg {
        let rgb = DynamicImage::ImageRgba8(canvas).to_rgb8();
        let mut file = fs::File::create(path).map_err(|error| error.to_string())?;
        JpegEncoder::new_with_quality(&mut file, JPEG_QUALITY)
            .encode_image(&rgb)
            .map_err(|error| error.to_string())
    } else {
        DynamicImage::ImageRgba8(canvas)
            .save_with_format(path, format)
            .map_err(|error| error.to_string())
    }
}

pub fn export_all_pages(pages: &[Vec<CollageImage>], directory: &Path) -> Result<PathBuf, String> {
    export_pages_into(pages, directory)?;
    Ok(directory.to_path_buf())
}

pub fn export_presentation(
    pages: &[Vec<CollageImage>],
    directory: &Path,
    language: Language,
) -> Result<PathBuf, String> {
    let folder = directory.join("presentation");
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;

    export_pages_into(pages, &folder)?;
    export_html(pages, &folder.join("index.html"), language)?;

    Ok(folder)
}

fn export_pages_into(pages: &[Vec<CollageImage>], directory: &Path) -> Result<(), String> {
    let mut page_number = 0;

    for images in pages {
        if images.is_empty() {
            continue;
        }

        page_number += 1;
        let path = directory.join(format!("collage-page-{page_number}.png"));
        export_image(images, &path)?;
    }

    Ok(())
}

pub fn export_html(
    pages: &[Vec<CollageImage>],
    path: &Path,
    language: Language,
) -> Result<(), String> {
    let mut pages_markup = String::new();
    let total_images: usize = pages.iter().map(|images| images.len()).sum();
    let mut photo_number = 0;

    for images in pages.iter().filter(|images| !images.is_empty()) {
        let mut elements = String::new();

        for (index, image) in images.iter().enumerate() {
            photo_number += 1;

            let source = load_oriented(&image.path)?.thumbnail(1800, 1800);
            let (mime, bytes) = encode_for_web(&source)?;
            let encoded = base64_encode(&bytes);
            let left = image.left() * 100.0;
            let top = image.top() * 100.0;
            let width = image.width * 100.0;
            let height = image.height * 100.0;
            let alt = language.photo_alt(photo_number);

            elements.push_str(&format!(
                r#"<div class="tile" style="left:{left:.4}%;top:{top:.4}%;width:{width:.4}%;height:{height:.4}%;z-index:{index};"><img src="data:{mime};base64,{encoded}" alt="{alt}" loading="lazy" decoding="async" /></div>"#,
            ));
        }

        pages_markup.push_str(&format!(
            r#"<section class="page"><div class="collage">{elements}</div></section>"#,
        ));
    }

    let html = HTML_TEMPLATE
        .replace("__LANG__", language.code())
        .replace("__TITLE__", language.html_title())
        .replace("__PRESENTATION__", language.html_presentation())
        .replace("__COUNT__", &language.photo_count(total_images))
        .replace("__CLOSE__", language.close())
        .replace("__PREVIOUS__", language.previous())
        .replace("__NEXT__", language.next())
        .replace("__PAGES__", &pages_markup);

    fs::write(path, html).map_err(|error| error.to_string())
}

const HTML_TEMPLATE: &str = r##"<!doctype html>
<html lang="__LANG__">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>__TITLE__</title>
<style>
  * { box-sizing: border-box; }
  html, body { margin: 0; min-height: 100%; }
  body {
    min-height: 100vh;
    background: #f5f4f1;
    color: #292524;
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    -webkit-font-smoothing: antialiased;
    text-rendering: optimizeLegibility;
  }
  .site-header {
    position: sticky;
    top: 0;
    z-index: 60;
    background: #fff;
    border-bottom: 1px solid #e7e5e4;
  }
  .site-header-inner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    width: 100%;
    max-width: 1400px;
    margin: 0 auto;
    padding: 22px 28px;
  }
  .brand {
    margin: 0;
    font-size: 12px;
    font-weight: 300;
    letter-spacing: 0.3em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  .brand strong { font-weight: 600; }
  .site-nav {
    display: flex;
    align-items: center;
    gap: 24px;
    font-size: 10px;
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: #78716c;
  }
  .site-nav .count { color: #292524; font-weight: 500; }
  .pages {
    display: flex;
    align-items: flex-start;
    gap: 26px;
    overflow-x: auto;
    padding: 32px 28px 46px;
    scroll-snap-type: x proximity;
    scrollbar-width: thin;
    scrollbar-color: #d6d3d1 transparent;
  }
  .page {
    flex: 0 0 auto;
    width: min(78vw, 620px);
    scroll-snap-align: start;
  }
  .collage {
    position: relative;
    width: 100%;
    aspect-ratio: 210 / 297;
    overflow: hidden;
    background: #fff;
    border-radius: 3px;
    box-shadow: 0 18px 55px rgba(41, 37, 36, 0.14);
  }
  .tile {
    position: absolute;
    overflow: hidden;
    background: #fff;
  }
  .tile img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
    object-position: center;
    cursor: pointer;
    user-select: none;
    -webkit-user-drag: none;
    transition: transform 0.55s cubic-bezier(0.33, 1, 0.68, 1), filter 0.55s ease;
  }
  .tile::after {
    content: "";
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.035);
    opacity: 0;
    transition: opacity 0.4s ease;
    pointer-events: none;
  }
  .tile:hover img { transform: scale(1.02); }
  .tile:hover::after { opacity: 1; }
  .tile:focus-within img { transform: scale(1.02); }
  .tile img:focus-visible {
    outline: 2px solid #292524;
    outline-offset: -2px;
  }
  @media (max-width: 760px) {
    .site-header-inner { padding: 18px; }
    .brand { font-size: 10px; }
    .site-nav { font-size: 9px; }
    .pages { padding: 22px 18px 34px; gap: 18px; }
    .page { width: 86vw; }
  }
  .lightbox {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: none;
    align-items: center;
    justify-content: center;
    background: rgba(12, 10, 9, 0.95);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    overscroll-behavior: contain;
  }
  .lightbox.open { display: flex; }
  .lb-image {
    max-width: min(92vw, 1600px);
    max-height: 82vh;
    object-fit: contain;
    border-radius: 2px;
    box-shadow: 0 34px 110px rgba(0, 0, 0, 0.7);
    user-select: none;
    -webkit-user-drag: none;
  }
  .lb-close,
  .lb-prev,
  .lb-next {
    position: absolute;
    z-index: 110;
    border: 0;
    background: none;
    color: rgba(255, 255, 255, 0.45);
    cursor: pointer;
    transition: color 0.2s ease, transform 0.2s ease;
  }
  .lb-close {
    top: 14px;
    right: 20px;
    padding: 8px;
    font-size: 34px;
    line-height: 1;
  }
  .lb-prev,
  .lb-next {
    top: 50%;
    padding: 18px;
    font-size: 58px;
    line-height: 1;
    transform: translateY(-50%);
  }
  .lb-prev { left: 14px; }
  .lb-next { right: 14px; }
  .lb-close:hover,
  .lb-prev:hover,
  .lb-next:hover { color: #fff; }
  .lb-prev:hover { transform: translateY(-50%) translateX(-2px); }
  .lb-next:hover { transform: translateY(-50%) translateX(2px); }
  .lb-caption {
    position: absolute;
    bottom: 50px;
    left: 50%;
    margin: 0;
    transform: translateX(-50%);
    font-size: 11px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: rgba(255, 255, 255, 0.62);
    pointer-events: none;
    text-align: center;
  }
  .lb-counter {
    position: absolute;
    bottom: 24px;
    left: 50%;
    margin: 0;
    transform: translateX(-50%);
    font-size: 11px;
    letter-spacing: 0.2em;
    color: rgba(255, 255, 255, 0.4);
  }
</style>
</head>
<body>
<header class="site-header">
  <div class="site-header-inner">
    <h1 class="brand">__TITLE__ <strong>__PRESENTATION__</strong></h1>
    <nav class="site-nav" aria-label="__TITLE__">
      <span class="count">__COUNT__</span>
    </nav>
  </div>
</header>
<main class="pages">
__PAGES__
</main>
<div class="lightbox" id="lightbox" role="dialog" aria-modal="true" aria-hidden="true">
  <button class="lb-close" type="button" aria-label="__CLOSE__">×</button>
  <button class="lb-prev" type="button" aria-label="__PREVIOUS__">‹</button>
  <img class="lb-image" alt="" draggable="false" />
  <button class="lb-next" type="button" aria-label="__NEXT__">›</button>
  <p class="lb-caption"></p>
  <p class="lb-counter"></p>
</div>
<script>
(() => {
  const items = Array.from(document.querySelectorAll('.collage img'));
  const lightbox = document.getElementById('lightbox');
  const preview = lightbox.querySelector('.lb-image');
  const caption = lightbox.querySelector('.lb-caption');
  const counter = lightbox.querySelector('.lb-counter');
  let current = 0;
  let touchStartX = null;

  const show = (index) => {
    current = (index + items.length) % items.length;
    const source = items[current];
    preview.src = source.src;
    preview.alt = source.alt;
    caption.textContent = source.alt;
    counter.textContent = (current + 1) + ' / ' + items.length;
  };

  const open = (index) => {
    show(index);
    lightbox.classList.add('open');
    lightbox.setAttribute('aria-hidden', 'false');
    document.body.style.overflow = 'hidden';
    lightbox.querySelector('.lb-close').focus();
  };

  const close = () => {
    lightbox.classList.remove('open');
    lightbox.setAttribute('aria-hidden', 'true');
    document.body.style.overflow = '';
  };

  items.forEach((item, index) => {
    item.tabIndex = 0;
    item.addEventListener('click', () => open(index));
    item.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        open(index);
      }
    });
  });

  lightbox.addEventListener('click', (event) => {
    if (event.target === lightbox) close();
  });
  lightbox.querySelector('.lb-close').addEventListener('click', close);
  lightbox.querySelector('.lb-prev').addEventListener('click', (event) => {
    event.stopPropagation();
    show(current - 1);
  });
  lightbox.querySelector('.lb-next').addEventListener('click', (event) => {
    event.stopPropagation();
    show(current + 1);
  });

  window.addEventListener('keydown', (event) => {
    if (!lightbox.classList.contains('open')) return;
    if (event.key === 'Escape') close();
    if (event.key === 'ArrowLeft') show(current - 1);
    if (event.key === 'ArrowRight') show(current + 1);
  });

  lightbox.addEventListener('touchstart', (event) => {
    touchStartX = event.touches[0].clientX;
  }, { passive: true });

  lightbox.addEventListener('touchend', (event) => {
    if (touchStartX === null) return;
    const delta = event.changedTouches[0].clientX - touchStartX;
    if (Math.abs(delta) > 50) {
      if (delta < 0) show(current + 1);
      else show(current - 1);
    }
    touchStartX = null;
  });

  const hashMatch = window.location.hash.match(/photo=(\d+)/);
  if (hashMatch) {
    const index = Math.max(0, Number(hashMatch[1]) - 1);
    if (index < items.length) open(index);
  }
})();
</script>
</body>
</html>
"##;

fn encode_for_web(image: &DynamicImage) -> Result<(&'static str, Vec<u8>), String> {
    if image.color().has_alpha() {
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        Ok(("image/png", bytes))
    } else {
        let rgb = image.to_rgb8();
        let mut bytes = Vec::new();
        {
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 88);
            encoder
                .encode_image(&rgb)
                .map_err(|error| error.to_string())?;
        }
        Ok(("image/jpeg", bytes))
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);

        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 0b11) << 4) | (b >> 4)) as usize] as char);

        if chunk.len() > 1 {
            output.push(TABLE[(((b & 0b1111) << 2) | (c >> 6)) as usize] as char);
        } else {
            output.push('=');
        }

        if chunk.len() > 2 {
            output.push(TABLE[(c & 0b111111) as usize] as char);
        } else {
            output.push('=');
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    fn fake_image(aspect: f32) -> CollageImage {
        CollageImage {
            path: PathBuf::from("test.png"),
            handle: ImageHandle::from_rgba(1, 1, vec![255, 255, 255, 255]),
            aspect,
            x: 0.5,
            y: 0.5,
            width: 0.3,
            height: 0.3 * CANVAS_W / (aspect * CANVAS_H),
        }
    }

    #[test]
    fn masonry_places_every_image_without_overlap() {
        for count in 1..=12 {
            let mut images: Vec<_> = (0..count)
                .map(|index| fake_image(if index % 2 == 0 { 1.5 } else { 0.75 }))
                .collect();
            apply_masonry(&mut images);
            assert_eq!(images.len(), count);

            for image in &images {
                assert!(image.width > 0.0 && image.height > 0.0);
                assert!(image.left() >= -1.0e-4);
                assert!(image.top() >= -1.0e-4);
                assert!(image.left() + image.width <= 1.0 + 1.0e-4);
                assert!(image.top() + image.height <= 1.0 + 1.0e-4);
            }

            for i in 0..count {
                for j in i + 1..count {
                    let a = &images[i];
                    let b = &images[j];
                    let overlap_x = (a.width + b.width) / 2.0 - (a.x - b.x).abs();
                    let overlap_y = (a.height + b.height) / 2.0 - (a.y - b.y).abs();
                    assert!(
                        overlap_x <= 1.0e-3 || overlap_y <= 1.0e-3,
                        "tiles {i} and {j} overlap for {count} images"
                    );
                }
            }
        }
    }

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn free_movement_is_only_clamped_to_the_page() {
        let image = fake_image(1.0);

        let (x, y) = clamp_to_page(&image, 0.42, 0.37);
        assert!((x - 0.42).abs() < f32::EPSILON);
        assert!((y - 0.37).abs() < f32::EPSILON);

        // Even near the page centre and edges nothing else pulls the image.
        let (x, y) = clamp_to_page(&image, 0.501, 0.499);
        assert!((x - 0.501).abs() < f32::EPSILON);
        assert!((y - 0.499).abs() < f32::EPSILON);

        let (x, y) = clamp_to_page(&image, 0.99, 0.99);
        assert!(x <= 1.0 - image.width / 2.0 + f32::EPSILON);
        assert!(y <= 1.0 - image.height / 2.0 + f32::EPSILON);
    }

    #[test]
    fn snapping_rounds_the_top_left_to_the_grid_only() {
        let image = fake_image(1.0);

        let (x, y) = snap_to_grid(&image, 0.43, 0.62);
        let left = x - image.width / 2.0;
        let top = y - image.height / 2.0;

        assert!((left - 0.30).abs() < 1.0e-4);
        assert!((top - 0.50).abs() < 1.0e-4);
        assert!(left >= 0.0 && top >= 0.0);

        let (x, y) = snap_to_grid(&image, 0.99, 0.99);
        assert!(x <= 1.0 - image.width / 2.0 + f32::EPSILON);
        assert!(y <= 1.0 - image.height / 2.0 + f32::EPSILON);
    }

    #[test]
    fn resizing_from_a_corner_keeps_aspect_and_stays_on_page() {
        let fixed = Point::new(0.2, 0.3);
        let (x, y, width, height) =
            resize_from_corner(Corner::BottomRight, Point::new(0.6, 0.8), fixed, 0.2, 0.3);

        assert!((width / height - 0.2 / 0.3).abs() < 1.0e-4);
        assert!((x - width / 2.0 - fixed.x).abs() < 1.0e-4);
        assert!((y - height / 2.0 - fixed.y).abs() < 1.0e-4);
        assert!(x + width / 2.0 <= 1.0 + 1.0e-4);
        assert!(y + height / 2.0 <= 1.0 + 1.0e-4);
        assert!(width >= MIN_IMAGE_SIZE - 1.0e-4);
        assert!(height >= MIN_IMAGE_SIZE - 1.0e-4);
    }

    #[test]
    fn exports_a_high_resolution_image() {
        let directory = tempfile::tempdir().unwrap();
        let source = RgbaImage::from_pixel(64, 48, Rgba([200, 40, 60, 255]));
        let source_path = directory.path().join("source.png");
        source.save(&source_path).unwrap();

        let mut images = vec![image_from_path(&source_path, 4.0 / 3.0)];
        apply_masonry(&mut images);

        let png_path = directory.path().join("collage.png");
        export_image(&images, &png_path).unwrap();
        let exported = image::open(&png_path).unwrap();
        assert_eq!(exported.dimensions(), (EXPORT_W, EXPORT_H));

        let jpeg_path = directory.path().join("collage.jpg");
        export_image(&images, &jpeg_path).unwrap();
        let exported = image::open(&jpeg_path).unwrap();
        assert_eq!(exported.dimensions(), (EXPORT_W, EXPORT_H));
    }

    #[test]
    fn exports_all_pages_and_a_presentation_folder() {
        let directory = tempfile::tempdir().unwrap();
        let source = RgbaImage::from_pixel(64, 48, Rgba([60, 120, 200, 255]));
        let source_path = directory.path().join("source.png");
        source.save(&source_path).unwrap();

        let mut images = vec![image_from_path(&source_path, 4.0 / 3.0)];
        apply_masonry(&mut images);
        let pages = vec![images];

        let all_dir = directory.path().join("all-pages");
        fs::create_dir_all(&all_dir).unwrap();
        export_all_pages(&pages, &all_dir).unwrap();
        assert!(all_dir.join("collage-page-1.png").exists());

        let presentation_dir = directory.path().join("export");
        let folder = export_presentation(&pages, &presentation_dir, Language::English).unwrap();
        assert_eq!(
            folder.file_name().and_then(|name| name.to_str()),
            Some("presentation")
        );
        assert!(folder.join("collage-page-1.png").exists());
        assert!(folder.join("index.html").exists());
    }

    #[test]
    fn exports_a_self_contained_html_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = RgbaImage::from_pixel(64, 48, Rgba([40, 180, 120, 255]));
        let source_path = directory.path().join("source.png");
        source.save(&source_path).unwrap();

        let mut images = vec![image_from_path(&source_path, 4.0 / 3.0)];
        apply_masonry(&mut images);
        let pages = vec![images];

        let html_path = directory.path().join("collage.html");
        export_html(&pages, &html_path, Language::English).unwrap();

        let html = fs::read_to_string(&html_path).unwrap();
        assert!(html.contains(r#"<html lang="en">"#));
        assert!(html.contains("<title>Collage</title>"));
        assert!(html.contains("data:image/"));
        assert!(html.contains(r#"alt="Photo 1""#));
        assert!(html.contains(r#"class="tile""#));
        assert!(html.contains(r#"<h1 class="brand">Collage <strong>Presentation</strong></h1>"#));
        assert!(!html.contains("page-title"));
        assert!(html.contains("transform: scale(1.02)"));
        assert!(html.contains(r#"<div class="collage">"#));
        assert!(html.contains(r#"class="lightbox""#));
        assert!(html.contains("touchstart"));
    }

    fn image_from_path(path: &Path, aspect: f32) -> CollageImage {
        CollageImage {
            path: path.to_path_buf(),
            handle: ImageHandle::from_rgba(1, 1, vec![255, 255, 255, 255]),
            aspect,
            x: 0.5,
            y: 0.5,
            width: 0.3,
            height: 0.3 * CANVAS_W / (aspect * CANVAS_H),
        }
    }
}
