//! Collage images, patterns, snapping, and export helpers.

use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use iced::widget::image::Handle as ImageHandle;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Rgba, RgbaImage};

use crate::i18n::Language;

pub const CANVAS_W: f32 = 1600.0;
pub const CANVAS_H: f32 = 1000.0;
pub const EXPORT_W: u32 = 3000;
pub const EXPORT_H: u32 = 1875;
pub const GRID_STEP: f32 = 0.05;

const PREVIEW_MAX: u32 = 768;
const JPEG_QUALITY: u8 = 92;
const MARGIN_X: f32 = 0.035;
const MARGIN_Y: f32 = 0.035;
const GAP: f32 = 0.014;
const SNAP_THRESHOLD: f32 = 0.012;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pattern {
    #[default]
    Grid,
    Mosaic,
    Strip,
    Hero,
    Stack,
}

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

pub fn apply_pattern(images: &mut [CollageImage], pattern: Pattern) {
    if images.is_empty() {
        return;
    }

    let cells = pattern_cells(pattern, images.len());

    for (image, cell) in images.iter_mut().zip(cells) {
        let (width, height) = fit_box(image.aspect, cell.2 - GAP, cell.3 - GAP);
        image.width = width;
        image.height = height;
        image.x = cell.0 + cell.2 / 2.0;
        image.y = cell.1 + cell.3 / 2.0;
    }
}

fn fit_box(aspect: f32, max_width: f32, max_height: f32) -> (f32, f32) {
    let max_width = max_width.max(0.01);
    let max_height = max_height.max(0.01);

    let mut width = max_width;
    let mut height = width * CANVAS_W / (aspect * CANVAS_H);

    if height > max_height {
        height = max_height;
        width = height * aspect * CANVAS_H / CANVAS_W;
    }

    (width.max(0.01), height.max(0.01))
}

type Cell = (f32, f32, f32, f32);

fn pattern_cells(pattern: Pattern, count: usize) -> Vec<Cell> {
    let usable_w = 1.0 - 2.0 * MARGIN_X;
    let usable_h = 1.0 - 2.0 * MARGIN_Y;

    if count == 1 {
        return vec![(MARGIN_X, MARGIN_Y, usable_w, usable_h)];
    }

    match pattern {
        Pattern::Grid => {
            let cols = (count as f32).sqrt().ceil() as usize;
            let rows = count.div_ceil(cols);
            let cell_w = usable_w / cols as f32;
            let cell_h = usable_h / rows as f32;
            let mut cells = Vec::with_capacity(count);

            for index in 0..count {
                let row = index / cols;
                let col = index % cols;
                let remaining = count - row * cols;
                let in_row = remaining.min(cols);
                let offset = (cols - in_row) as f32 * cell_w / 2.0;
                cells.push((
                    MARGIN_X + offset + col as f32 * cell_w,
                    MARGIN_Y + row as f32 * cell_h,
                    cell_w,
                    cell_h,
                ));
            }

            cells
        }
        Pattern::Mosaic => {
            let left_w = usable_w * 0.57;
            let right_w = usable_w - left_w;
            let remaining = count - 1;
            let cols = if remaining <= 2 { 1 } else { 2 };
            let rows = remaining.div_ceil(cols);
            let cell_w = right_w / cols as f32;
            let cell_h = usable_h / rows as f32;
            let mut cells = vec![(MARGIN_X, MARGIN_Y, left_w, usable_h)];

            for index in 0..remaining {
                let row = index / cols;
                let col = index % cols;
                let in_row = (remaining - row * cols).min(cols);
                let offset = (cols - in_row) as f32 * cell_w / 2.0;
                cells.push((
                    MARGIN_X + left_w + offset + col as f32 * cell_w,
                    MARGIN_Y + row as f32 * cell_h,
                    cell_w,
                    cell_h,
                ));
            }

            cells
        }
        Pattern::Strip => {
            let cell_w = usable_w / count as f32;
            (0..count)
                .map(|index| (MARGIN_X + index as f32 * cell_w, MARGIN_Y, cell_w, usable_h))
                .collect()
        }
        Pattern::Hero => {
            let hero_h = usable_h * 0.68;
            let bottom_h = usable_h - hero_h;
            let remaining = count - 1;
            let cell_w = usable_w / remaining as f32;
            let mut cells = vec![(MARGIN_X, MARGIN_Y, usable_w, hero_h)];

            for index in 0..remaining {
                cells.push((
                    MARGIN_X + index as f32 * cell_w,
                    MARGIN_Y + hero_h,
                    cell_w,
                    bottom_h,
                ));
            }

            cells
        }
        Pattern::Stack => {
            let cell_w = usable_w * 0.78;
            let cell_h = usable_h / count as f32;
            let x = MARGIN_X + (usable_w - cell_w) / 2.0;
            (0..count)
                .map(|index| (x, MARGIN_Y + index as f32 * cell_h, cell_w, cell_h))
                .collect()
        }
    }
}

pub fn hit_test(images: &[CollageImage], x: f32, y: f32) -> Option<usize> {
    images.iter().rposition(|image| image.contains(x, y))
}

pub fn snap_center(
    images: &[CollageImage],
    index: usize,
    x: f32,
    y: f32,
    grid: bool,
) -> (f32, f32) {
    let Some(image) = images.get(index) else {
        return (x, y);
    };

    let half_w = image.width / 2.0;
    let half_h = image.height / 2.0;
    let x = snap_axis(images, index, x, half_w, true, grid);
    let y = snap_axis(images, index, y, half_h, false, grid);

    (x.clamp(half_w, 1.0 - half_w), y.clamp(half_h, 1.0 - half_h))
}

fn snap_axis(
    images: &[CollageImage],
    index: usize,
    value: f32,
    half: f32,
    horizontal: bool,
    grid: bool,
) -> f32 {
    let edges = [value - half, value, value + half];

    let mut targets = vec![0.0, 0.5, 1.0];

    if grid {
        let steps = (1.0 / GRID_STEP).round() as usize;
        for step in 0..=steps {
            targets.push(step as f32 * GRID_STEP);
        }
    }

    for (other_index, other) in images.iter().enumerate() {
        if other_index == index {
            continue;
        }

        let (start, size) = if horizontal {
            (other.left(), other.width)
        } else {
            (other.top(), other.height)
        };

        targets.push(start);
        targets.push(start + size / 2.0);
        targets.push(start + size);
    }

    let mut best_delta = 0.0;
    let mut best_distance = f32::INFINITY;

    for edge in edges {
        for target in &targets {
            let delta = target - edge;
            let distance = delta.abs();

            if distance < SNAP_THRESHOLD && distance < best_distance {
                best_distance = distance;
                best_delta = delta;
            }
        }
    }

    value + best_delta
}

pub fn export_image(images: &[CollageImage], path: &Path) -> Result<(), String> {
    let mut canvas = RgbaImage::from_pixel(EXPORT_W, EXPORT_H, Rgba([255, 255, 255, 255]));

    for image in images {
        let source = load_oriented(&image.path)?.to_rgba8();
        let target_w = (image.width * EXPORT_W as f32).round().max(1.0) as u32;
        let target_h = (image.height * EXPORT_H as f32).round().max(1.0) as u32;
        let resized = image::imageops::resize(&source, target_w, target_h, FilterType::Lanczos3);

        let left = (image.left() * EXPORT_W as f32).round() as i64;
        let top = (image.top() * EXPORT_H as f32).round() as i64;

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

pub fn export_html(images: &[CollageImage], path: &Path, language: Language) -> Result<(), String> {
    let mut elements = String::new();

    for (index, image) in images.iter().enumerate() {
        let source = load_oriented(&image.path)?.thumbnail(1800, 1800);
        let (mime, bytes) = encode_for_web(&source)?;
        let encoded = base64_encode(&bytes);
        let left = image.left() * 100.0;
        let top = image.top() * 100.0;
        let width = image.width * 100.0;
        let height = image.height * 100.0;
        let alt = language.photo_alt(index + 1);

        elements.push_str(&format!(
            r#"<img src="data:{mime};base64,{encoded}" alt="{alt}" style="left:{left:.4}%;top:{top:.4}%;width:{width:.4}%;height:{height:.4}%;z-index:{index};" />"#,
        ));
    }

    let html = format!(
        r#"<!doctype html>
<html lang="{}">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>{}</title>
<style>
  * {{ box-sizing: border-box; }}
  html, body {{ margin: 0; min-height: 100%; }}
  body {{
    display: grid;
    place-items: center;
    min-height: 100vh;
    padding: 24px;
    background: #0a0e18;
  }}
  .collage {{
    position: relative;
    width: min(100%, 1600px);
    aspect-ratio: 16 / 10;
    overflow: hidden;
    border-radius: 14px;
    background: #fff;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.45);
  }}
  .collage img {{
    position: absolute;
    display: block;
    object-fit: contain;
    user-select: none;
    -webkit-user-drag: none;
  }}
</style>
</head>
<body>
<div class="collage">
{elements}
</div>
</body>
</html>
"#,
        language.code(),
        language.html_title(),
        elements = elements
    );

    fs::write(path, html).map_err(|error| error.to_string())
}

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
    fn every_pattern_places_every_image() {
        for pattern in [
            Pattern::Grid,
            Pattern::Mosaic,
            Pattern::Strip,
            Pattern::Hero,
            Pattern::Stack,
        ] {
            for count in 1..=12 {
                let mut images: Vec<_> = (0..count).map(|_| fake_image(1.5)).collect();
                apply_pattern(&mut images, pattern);
                assert_eq!(images.len(), count);
                assert!(images
                    .iter()
                    .all(|image| image.width > 0.0 && image.height > 0.0));
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
    fn snapping_keeps_images_inside_the_canvas() {
        let mut images = vec![fake_image(1.0), fake_image(1.0)];
        images[0].x = 0.99;
        images[0].y = 0.99;
        let (x, y) = snap_center(&images, 0, 0.99, 0.99, true);
        assert!(x <= 1.0 - images[0].width / 2.0 + f32::EPSILON);
        assert!(y <= 1.0 - images[0].height / 2.0 + f32::EPSILON);
    }

    #[test]
    fn exports_a_high_resolution_image() {
        let directory = tempfile::tempdir().unwrap();
        let source = RgbaImage::from_pixel(64, 48, Rgba([200, 40, 60, 255]));
        let source_path = directory.path().join("source.png");
        source.save(&source_path).unwrap();

        let mut images = vec![image_from_path(&source_path, 4.0 / 3.0)];
        apply_pattern(&mut images, Pattern::Grid);

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
    fn exports_a_self_contained_html_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = RgbaImage::from_pixel(64, 48, Rgba([40, 180, 120, 255]));
        let source_path = directory.path().join("source.png");
        source.save(&source_path).unwrap();

        let mut images = vec![image_from_path(&source_path, 4.0 / 3.0)];
        apply_pattern(&mut images, Pattern::Grid);

        let html_path = directory.path().join("collage.html");
        export_html(&images, &html_path, Language::English).unwrap();

        let html = fs::read_to_string(&html_path).unwrap();
        assert!(html.contains(r#"<html lang="en">"#));
        assert!(html.contains("<title>Collage</title>"));
        assert!(html.contains("data:image/"));
        assert!(html.contains(r#"alt="Photo 1""#));
        assert!(html.contains(r#"<div class="collage">"#));
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
