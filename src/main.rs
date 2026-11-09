#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Collage Editor — arrange photos into a simple collage and export it.

mod canvas;
mod collage;
mod i18n;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use canvas::CollageCanvas;
use collage::{CollageImage, Corner, CANVAS_H, CANVAS_W};
use display_info::DisplayInfo;
use i18n::Language;
use iced::futures::channel::mpsc::unbounded;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{
    button, column, container, responsive, row, scrollable, text, tooltip, Row, Space,
};
use iced::{
    border, mouse, theme, window, Alignment, Border, Color, Element, Length, Point, Shadow, Size,
    Subscription, Task, Theme,
};
use iced_font_awesome::fa_icon_solid;

const BACKGROUND: Color = Color::from_rgb8(10, 14, 24);
const SURFACE: Color = Color::from_rgb8(18, 25, 41);
const SURFACE_ALT: Color = Color::from_rgb8(27, 37, 58);
const SURFACE_HOVER: Color = Color::from_rgb8(35, 48, 74);
const BORDER: Color = Color::from_rgb8(48, 63, 94);
const TEXT: Color = Color::from_rgb8(232, 238, 250);
const MUTED: Color = Color::from_rgb8(144, 157, 184);
const ACCENT: Color = Color::from_rgb8(96, 130, 255);
const ACCENT_HOVER: Color = Color::from_rgb8(122, 152, 255);
const ACCENT_PRESSED: Color = Color::from_rgb8(78, 110, 232);
const SUCCESS: Color = Color::from_rgb8(72, 209, 150);
const DANGER: Color = Color::from_rgb8(255, 112, 132);
const DANGER_DIM: Color = Color::from_rgb8(55, 26, 37);
const SUCCESS_DIM: Color = Color::from_rgb8(21, 55, 45);

/// A4 pages are laid out like a photographer's portfolio: around ten photos
/// per page in a tight masonry arrangement. Larger imports are split over
/// several pages automatically.
const PAGE_IMPORT_CAPACITY: usize = 10;

const FLAG_EN: &[u8] = include_bytes!("../assets/flags/en.png");
const FLAG_SV: &[u8] = include_bytes!("../assets/flags/sv.png");

static FLAG_EN_HANDLE: LazyLock<ImageHandle> = LazyLock::new(|| flag_handle(FLAG_EN));
static FLAG_SV_HANDLE: LazyLock<ImageHandle> = LazyLock::new(|| flag_handle(FLAG_SV));

/// Decoding photos is memory hungry, so previews are decoded on a small
/// dedicated pool instead of the global rayon pool (which can be one thread
/// per core). This keeps large drops responsive without spiking memory.
static LOAD_POOL: LazyLock<rayon::ThreadPool> = LazyLock::new(|| {
    let threads = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(2)
        .clamp(1, 3);

    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|index| format!("collage-load-{index}"))
        .build()
        .expect("the image loading pool must be built")
});

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("Collage Editor")
        .theme(app_theme())
        .window(window::Settings {
            size: initial_window_size(),
            min_size: Some(Size::new(640.0, 480.0)),
            position: window::Position::Centered,
            icon: window_icon(),
            ..window::Settings::default()
        })
        .subscription(subscription)
        .style(|_state, _theme| theme::Style {
            background_color: BACKGROUND,
            text_color: TEXT,
        })
        .antialiasing(true)
        .run()
}

fn boot() -> (App, Task<Message>) {
    let mut app = App::default();

    // Optional demo folder used while developing the canvas. Loading happens
    // through the normal background pipeline so the window appears instantly.
    let task = match std::env::var("COLLAGE_DEMO") {
        Ok(dir) => app.add_paths(vec![PathBuf::from(dir)]),
        Err(_) => Task::none(),
    };

    (app, task)
}

fn initial_window_size() -> Size {
    let fallback = Size::new(1100.0, 780.0);

    let Ok(displays) = DisplayInfo::all() else {
        return fallback;
    };

    let Some(display) = displays
        .iter()
        .find(|display| display.is_primary)
        .or_else(|| displays.first())
    else {
        return fallback;
    };

    let scale = if display.scale_factor > 0.0 {
        display.scale_factor
    } else {
        1.0
    };

    // A collage editor benefits from more vertical room than 60% gives. The
    // extra height is what makes the A4 pages readable.
    Size::new(
        (display.width as f32 / scale * 0.7).clamp(760.0, 1400.0),
        (display.height as f32 / scale * 0.78).clamp(560.0, 1000.0),
    )
}

fn window_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("../assets/icons/icon-256.png");
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = (image.width(), image.height());
    window::icon::from_rgba(image.into_raw(), width, height).ok()
}

fn flag_handle(bytes: &[u8]) -> ImageHandle {
    let image = image::load_from_memory(bytes)
        .expect("bundled flag images must be valid PNGs")
        .into_rgba8();
    let (width, height) = (image.width(), image.height());
    ImageHandle::from_rgba(width, height, image.into_raw())
}

#[derive(Default)]
struct Page {
    images: Vec<CollageImage>,
    selected: Option<usize>,
}

struct App {
    language: Language,
    pages: Vec<Page>,
    active_page: usize,
    dragging: Option<Drag>,
    resizing: Option<Resizing>,
    drop_target: Option<(usize, Point)>,
    snap: bool,
    hovered: bool,
    pending_loads: usize,
    load_errors: usize,
    last_load_error: Option<String>,
    exporting: bool,
    notice: Option<Notice>,
    last_output_dir: Option<PathBuf>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            language: Language::load(),
            pages: vec![Page::default()],
            active_page: 0,
            dragging: None,
            resizing: None,
            drop_target: None,
            snap: true,
            hovered: false,
            pending_loads: 0,
            load_errors: 0,
            last_load_error: None,
            exporting: false,
            notice: None,
            last_output_dir: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    page: usize,
    index: usize,
    offset_x: f32,
    offset_y: f32,
}

#[derive(Debug, Clone, Copy)]
struct Resizing {
    page: usize,
    index: usize,
    corner: Corner,
    fixed: Point,
    start_width: f32,
    start_height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeKind {
    Error,
    Success,
}

struct Notice {
    kind: NoticeKind,
    text: String,
}

impl Notice {
    fn error(text: String) -> Self {
        Self {
            kind: NoticeKind::Error,
            text,
        }
    }

    fn success(text: String) -> Self {
        Self {
            kind: NoticeKind::Success,
            text,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    AddPhotos,
    FilesDropped(Vec<PathBuf>),
    HoverChanged(bool),
    ImageLoaded(usize, PathBuf, Result<CollageImage, String>),
    SetLanguage(Language),
    ToggleSnap,
    Clear,
    AddPage,
    SortPage(usize),
    ActivatePage(usize),
    DragStart {
        page: usize,
        index: usize,
        pointer: Point,
    },
    DragMove {
        page: usize,
        pointer: Point,
    },
    DragHover {
        page: usize,
        pointer: Point,
    },
    ResizeStart {
        page: usize,
        index: usize,
        corner: Corner,
    },
    ResizeMove {
        page: usize,
        pointer: Point,
    },
    PointerUp,
    ExportPage(usize),
    ExportAllPages,
    ExportBoth,
    ExportHtml,
    ExportFinished(Result<PathBuf, String>),
    OpenOutput,
    DismissNotice,
}

impl App {
    fn active(&self) -> &Page {
        &self.pages[self.active_page]
    }

    fn active_mut(&mut self) -> &mut Page {
        if self.pages.is_empty() {
            self.pages.push(Page::default());
            self.active_page = 0;
        }

        let index = self.active_page.min(self.pages.len() - 1);
        &mut self.pages[index]
    }

    fn active_has_images(&self) -> bool {
        !self.active().images.is_empty()
    }

    fn page_source_directory(&self, page: usize) -> Option<&Path> {
        self.pages
            .get(page)?
            .images
            .first()
            .and_then(|image| image.path.parent())
    }

    fn any_source_directory(&self) -> Option<&Path> {
        self.pages
            .iter()
            .flat_map(|page| page.images.iter())
            .next()
            .and_then(|image| image.path.parent())
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        if self.pages.is_empty() {
            self.pages.push(Page::default());
            self.active_page = 0;
        }

        let page_index = self.active_page.min(self.pages.len() - 1);
        let collected = collage::collect_images(&paths);

        if collected.paths.is_empty() {
            self.notice = Some(Notice::error(
                self.language.no_supported_images().to_owned(),
            ));
            return Task::none();
        }

        let existing: HashSet<String> = self.pages[page_index]
            .images
            .iter()
            .map(|image| image.path.to_string_lossy().to_lowercase())
            .collect();

        let paths: Vec<PathBuf> = collected
            .paths
            .into_iter()
            .filter(|path| !existing.contains(&path.to_string_lossy().to_lowercase()))
            .collect();

        if paths.is_empty() {
            self.notice = Some(Notice::error(self.language.already_added().to_owned()));
            return Task::none();
        }

        if self.pending_loads == 0 {
            self.load_errors = 0;
            self.last_load_error = None;
        }

        let split_across_pages = self.pages[page_index].images.is_empty();
        let mut assignments = Vec::with_capacity(paths.len());

        if split_across_pages {
            let mut paths = paths.into_iter();

            for _ in 0..PAGE_IMPORT_CAPACITY {
                let Some(path) = paths.next() else {
                    break;
                };
                assignments.push((page_index, path));
            }

            loop {
                let mut chunk = Vec::new();

                for _ in 0..PAGE_IMPORT_CAPACITY {
                    let Some(path) = paths.next() else {
                        break;
                    };
                    chunk.push(path);
                }

                if chunk.is_empty() {
                    break;
                }

                let target = self.pages.len();
                self.pages.push(Page::default());

                for path in chunk {
                    assignments.push((target, path));
                }
            }
        } else {
            for path in paths {
                assignments.push((page_index, path));
            }
        }

        self.pending_loads += assignments.len();
        self.notice = None;

        let (sender, receiver) = unbounded();

        LOAD_POOL.spawn(move || {
            use rayon::prelude::*;

            assignments.par_iter().for_each(|(page, path)| {
                let result = collage::load_preview(path);
                let _ = sender.unbounded_send(Message::ImageLoaded(*page, path.clone(), result));
            });
        });

        Task::stream(receiver)
    }

    fn finish_export(&mut self, result: Result<PathBuf, String>) {
        self.exporting = false;

        match result {
            Ok(path) => {
                self.last_output_dir = if path.is_dir() {
                    Some(path.clone())
                } else {
                    path.parent().map(Path::to_path_buf)
                };
                self.notice = Some(Notice::success(
                    self.language.exported(&path.display().to_string()),
                ));
            }
            Err(error) => {
                self.notice = Some(Notice::error(self.language.export_failed(&error)));
            }
        }
    }
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::AddPhotos => {
            let picked = rfd::FileDialog::new()
                .set_title(app.language.add_photos_title())
                .add_filter(
                    app.language.images_filter(),
                    &[
                        "jpg", "jpeg", "png", "webp", "tif", "tiff", "bmp", "ico", "tga", "qoi",
                        "dds", "hdr", "ff", "pnm", "pbm", "pgm", "ppm", "pam", "gif",
                    ],
                )
                .pick_files();

            if let Some(paths) = picked {
                app.add_paths(paths)
            } else {
                Task::none()
            }
        }
        Message::FilesDropped(paths) => {
            app.hovered = false;
            app.add_paths(paths)
        }
        Message::HoverChanged(hovered) => {
            app.hovered = hovered;
            Task::none()
        }
        Message::ImageLoaded(page_index, path, result) => {
            app.pending_loads = app.pending_loads.saturating_sub(1);

            if let Some(page) = app.pages.get_mut(page_index) {
                match result {
                    Ok(image) => {
                        page.images.push(image);
                        collage::apply_masonry(&mut page.images);
                    }
                    Err(error) => {
                        app.load_errors += 1;
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        app.last_load_error = Some(app.language.load_failed(&name, &error));
                    }
                }
            }

            if app.pending_loads == 0 && app.load_errors > 0 {
                let text = if app.load_errors == 1 {
                    app.last_load_error
                        .take()
                        .unwrap_or_else(|| app.language.load_failed_count(1))
                } else {
                    app.language.load_failed_count(app.load_errors)
                };
                app.notice = Some(Notice::error(text));
            }

            Task::none()
        }
        Message::SetLanguage(language) => {
            if app.language != language {
                app.language = language;
                language.save();
            }
            Task::none()
        }
        Message::ToggleSnap => {
            app.snap = !app.snap;
            Task::none()
        }
        Message::Clear => {
            if !app.exporting {
                let page = app.active_mut();
                page.images.clear();
                page.selected = None;
                app.dragging = None;
                app.resizing = None;
                app.drop_target = None;
                app.notice = None;
            }
            Task::none()
        }
        Message::AddPage => {
            app.pages.push(Page::default());
            app.active_page = app.pages.len() - 1;
            app.dragging = None;
            app.resizing = None;
            app.drop_target = None;
            app.notice = None;
            Task::none()
        }
        Message::SortPage(page) => {
            if let Some(page_state) = app.pages.get_mut(page) {
                collage::apply_masonry(&mut page_state.images);
                page_state.selected = None;
                app.active_page = page;
                app.dragging = None;
                app.resizing = None;
                app.drop_target = None;
            }
            Task::none()
        }
        Message::ActivatePage(page) => {
            if page < app.pages.len() {
                app.active_page = page;
                app.dragging = None;
                app.resizing = None;
                app.drop_target = None;
            }
            Task::none()
        }
        Message::DragStart {
            page,
            index,
            pointer,
        } => {
            if let Some(page_state) = app.pages.get_mut(page) {
                if let Some(image) = page_state.images.get(index) {
                    let offset_x = pointer.x - image.x;
                    let offset_y = pointer.y - image.y;
                    let image = page_state.images.remove(index);
                    page_state.images.push(image);
                    let index = page_state.images.len() - 1;
                    page_state.selected = Some(index);
                    app.active_page = page;
                    app.dragging = Some(Drag {
                        page,
                        index,
                        offset_x,
                        offset_y,
                    });
                    app.resizing = None;
                }
            }
            Task::none()
        }
        Message::DragMove { page, pointer } => {
            if let Some(drag) = app.dragging {
                if drag.page == page {
                    if let Some(page_state) = app.pages.get_mut(page) {
                        let x = pointer.x - drag.offset_x;
                        let y = pointer.y - drag.offset_y;

                        if let Some(image) = page_state.images.get_mut(drag.index) {
                            let (x, y) = if app.snap {
                                collage::snap_to_grid(image, x, y)
                            } else {
                                collage::clamp_to_page(image, x, y)
                            };

                            image.x = x;
                            image.y = y;
                        }

                        app.drop_target = Some((page, pointer));
                    }
                }
            }
            Task::none()
        }
        Message::DragHover { page, pointer } => {
            if app.dragging.is_some() && page < app.pages.len() {
                app.drop_target = Some((page, pointer));
            }
            Task::none()
        }
        Message::ResizeStart {
            page,
            index,
            corner,
        } => {
            if let Some(page_state) = app.pages.get_mut(page) {
                if let Some(image) = page_state.images.get(index) {
                    let fixed = corner.fixed_corner(
                        Point::new(image.x, image.y),
                        image.width,
                        image.height,
                    );
                    let start_width = image.width;
                    let start_height = image.height;
                    page_state.selected = Some(index);
                    app.active_page = page;
                    app.resizing = Some(Resizing {
                        page,
                        index,
                        corner,
                        fixed,
                        start_width,
                        start_height,
                    });
                    app.dragging = None;
                    app.drop_target = None;
                }
            }
            Task::none()
        }
        Message::ResizeMove { page, pointer } => {
            if let Some(resizing) = app.resizing {
                if resizing.page == page {
                    if let Some(page_state) = app.pages.get_mut(page) {
                        if let Some(image) = page_state.images.get_mut(resizing.index) {
                            let (x, y, width, height) = collage::resize_from_corner(
                                resizing.corner,
                                pointer,
                                resizing.fixed,
                                resizing.start_width,
                                resizing.start_height,
                            );
                            image.x = x;
                            image.y = y;
                            image.width = width;
                            image.height = height;
                        }
                    }
                }
            }
            Task::none()
        }
        Message::PointerUp => {
            if let Some(drag) = app.dragging {
                if let Some((target_page, pointer)) = app.drop_target {
                    if target_page != drag.page {
                        let moved = app.pages.get_mut(drag.page).and_then(|page_state| {
                            page_state.selected = None;

                            if drag.index < page_state.images.len() {
                                Some(page_state.images.remove(drag.index))
                            } else {
                                None
                            }
                        });

                        if let Some(mut image) = moved {
                            image.x = (pointer.x - drag.offset_x)
                                .clamp(image.width / 2.0, 1.0 - image.width / 2.0);
                            image.y = (pointer.y - drag.offset_y)
                                .clamp(image.height / 2.0, 1.0 - image.height / 2.0);

                            if let Some(target_state) = app.pages.get_mut(target_page) {
                                let index = target_state.images.len();
                                target_state.images.push(image);

                                if let Some(image) = target_state.images.get_mut(index) {
                                    let (x, y) = if app.snap {
                                        collage::snap_to_grid(image, image.x, image.y)
                                    } else {
                                        collage::clamp_to_page(image, image.x, image.y)
                                    };
                                    image.x = x;
                                    image.y = y;
                                }

                                target_state.selected = Some(index);
                                app.active_page = target_page;
                            }
                        }
                    }
                }
            }

            app.dragging = None;
            app.resizing = None;
            app.drop_target = None;
            Task::none()
        }
        Message::ExportPage(page) => {
            let Some(page_state) = app.pages.get(page) else {
                return Task::none();
            };

            if page_state.images.is_empty() {
                app.notice = Some(Notice::error(app.language.no_page_images().to_owned()));
                return Task::none();
            }

            if app.exporting {
                return Task::none();
            }

            let mut dialog = rfd::FileDialog::new()
                .set_title(app.language.save_image_title())
                .set_file_name(format!("collage-page-{}.png", page + 1))
                .add_filter("PNG", &["png"])
                .add_filter("JPEG", &["jpg", "jpeg"]);

            if let Some(directory) = app.page_source_directory(page) {
                dialog = dialog.set_directory(directory);
            }

            let Some(mut path) = save_file_safely(dialog) else {
                return Task::none();
            };

            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase());

            if !matches!(extension.as_deref(), Some("png" | "jpg" | "jpeg")) {
                path.set_extension("png");
            }

            let images = page_state.images.clone();
            app.active_page = page;
            app.exporting = true;
            app.notice = None;

            Task::perform(
                async move { collage::export_image(&images, &path).map(|_| path) },
                Message::ExportFinished,
            )
        }
        Message::ExportAllPages => {
            if app.pages.iter().all(|page| page.images.is_empty()) {
                app.notice = Some(Notice::error(app.language.no_images().to_owned()));
                return Task::none();
            }

            if app.exporting {
                return Task::none();
            }

            let mut dialog = rfd::FileDialog::new().set_title(app.language.export_all_title());

            if let Some(directory) = app.any_source_directory() {
                dialog = dialog.set_directory(directory);
            }

            let Some(directory) = pick_folder_safely(dialog) else {
                return Task::none();
            };

            let pages: Vec<Vec<CollageImage>> =
                app.pages.iter().map(|page| page.images.clone()).collect();
            app.exporting = true;
            app.notice = None;

            Task::perform(
                async move { collage::export_all_pages(&pages, &directory) },
                Message::ExportFinished,
            )
        }
        Message::ExportBoth => {
            if app.pages.iter().all(|page| page.images.is_empty()) {
                app.notice = Some(Notice::error(app.language.no_images().to_owned()));
                return Task::none();
            }

            if app.exporting {
                return Task::none();
            }

            let mut dialog = rfd::FileDialog::new().set_title(app.language.export_both_title());

            if let Some(directory) = app.any_source_directory() {
                dialog = dialog.set_directory(directory);
            }

            let Some(directory) = pick_folder_safely(dialog) else {
                return Task::none();
            };

            let pages: Vec<Vec<CollageImage>> =
                app.pages.iter().map(|page| page.images.clone()).collect();
            let language = app.language;
            app.exporting = true;
            app.notice = None;

            Task::perform(
                async move { collage::export_presentation(&pages, &directory, language) },
                Message::ExportFinished,
            )
        }
        Message::ExportHtml => {
            if app.pages.iter().all(|page| page.images.is_empty()) {
                app.notice = Some(Notice::error(app.language.no_images().to_owned()));
                return Task::none();
            }

            if app.exporting {
                return Task::none();
            }

            let mut dialog = rfd::FileDialog::new().set_title(app.language.export_html_title());

            if let Some(directory) = app.any_source_directory() {
                dialog = dialog.set_directory(directory);
            }

            let Some(directory) = pick_folder_safely(dialog) else {
                return Task::none();
            };
            let path = directory.join("index.html");

            let pages: Vec<Vec<CollageImage>> =
                app.pages.iter().map(|page| page.images.clone()).collect();
            let language = app.language;
            app.exporting = true;
            app.notice = None;

            Task::perform(
                async move { collage::export_html(&pages, &path, language).map(|_| path) },
                Message::ExportFinished,
            )
        }
        Message::ExportFinished(result) => {
            app.finish_export(result);
            Task::none()
        }
        Message::OpenOutput => {
            if let Some(directory) = &app.last_output_dir {
                reveal_in_file_manager(directory);
            }
            Task::none()
        }
        Message::DismissNotice => {
            app.notice = None;
            Task::none()
        }
    }
}

fn subscription(_app: &App) -> Subscription<Message> {
    iced::event::listen_with(|event, _status, _window| match event {
        iced::Event::Window(window::Event::FileDropped(path)) => {
            Some(Message::FilesDropped(vec![path]))
        }
        iced::Event::Window(window::Event::FileHovered(_)) => Some(Message::HoverChanged(true)),
        iced::Event::Window(window::Event::FilesHoveredLeft) => Some(Message::HoverChanged(false)),
        // End a drag or resize even when the button is released outside.
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::PointerUp)
        }
        _ => None,
    })
}

fn save_file_safely(dialog: rfd::FileDialog) -> Option<PathBuf> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dialog.save_file()))
        .ok()
        .flatten()
}

fn pick_folder_safely(dialog: rfd::FileDialog) -> Option<PathBuf> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dialog.pick_folder()))
        .ok()
        .flatten()
}

fn reveal_in_file_manager(path: &Path) {
    #[cfg(target_os = "windows")]
    let _ = Command::new("explorer").arg(path).spawn();

    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(path).spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(path).spawn();
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn view(app: &App) -> Element<'_, Message> {
    let mut content = column![header(app), toolbar(app)]
        .spacing(9)
        .width(Length::Fill)
        .height(Length::Fill);

    if let Some(notice) = &app.notice {
        content = content.push(notice_view(app.language, notice));
    }

    content = content.push(pages_view(app));
    content = content.push(footer(app));

    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(BACKGROUND.into()),
            ..container::Style::default()
        })
        .into()
}

fn header(app: &App) -> Element<'_, Message> {
    let language = app.language;

    row![
        column![
            text(language.title()).size(30).color(TEXT),
            text(language.subtitle()).size(13).color(MUTED),
        ]
        .spacing(3),
        Space::new().width(Length::Fill),
        flag_button(
            Language::English,
            language == Language::English,
            &FLAG_EN_HANDLE,
            language.english(),
        ),
        flag_button(
            Language::Swedish,
            language == Language::Swedish,
            &FLAG_SV_HANDLE,
            language.swedish(),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn toolbar(app: &App) -> Element<'_, Message> {
    let language = app.language;

    row![
        button(
            row![
                icon("plus", 13.0, TEXT),
                text(language.add_photos()).size(12),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .on_press(Message::AddPhotos)
        .padding([8, 12])
        .style(secondary_button),
        Space::new().width(Length::Fill),
        toggle_icon_button(
            "magnet",
            language.snap(),
            app.snap,
            (!app.exporting).then_some(Message::ToggleSnap),
        ),
        icon_button(
            "trash",
            language.clear(),
            (app.active_has_images() && !app.exporting).then_some(Message::Clear),
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn pages_view(app: &App) -> Element<'_, Message> {
    responsive(move |size| {
        let plus_width = (size.width * 0.16).clamp(88.0, 150.0);
        let available_width = (size.width - plus_width - 52.0).max(120.0);
        let available_height = (size.height - 60.0).max(150.0);

        let page_width = (available_height * (CANVAS_W / CANVAS_H)).min(available_width);
        let page_height = page_width * (CANVAS_H / CANVAS_W);
        let page_size = Size::new(page_width, page_height);

        let mut pages = Row::new()
            .spacing(18)
            .align_y(Alignment::End)
            .padding([0, 2]);

        for (index, page) in app.pages.iter().enumerate() {
            pages = pages.push(page_view(app, index, page, page_size));
        }

        pages = pages.push(add_page_card(app.language, page_height + 8.0, plus_width));

        scrollable(pages)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new(),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(pages_scrollable_style)
            .into()
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn page_view<'a>(app: &'a App, index: usize, page: &'a Page, size: Size) -> Element<'a, Message> {
    let active = index == app.active_page;
    let drag_source = app.dragging.map(|drag| drag.page);
    let is_drop_target = app.dragging.is_some()
        && app
            .drop_target
            .is_some_and(|(drop_page, _)| drop_page == index);
    let canvas = iced::widget::canvas(CollageCanvas {
        page: index,
        images: &page.images,
        selected: if active { page.selected } else { None },
        drag_source,
        resizing: app
            .resizing
            .filter(|resizing| resizing.page == index)
            .map(|resizing| resizing.corner),
        grid: app.snap,
        empty_hint: app.language.empty_page(),
    })
    .width(Length::Fixed(size.width))
    .height(Length::Fixed(size.height));

    let has_images = !page.images.is_empty();
    let label = row![
        text(app.language.page_label(index + 1))
            .size(11)
            .color(if active { ACCENT } else { MUTED }),
        Space::new().width(Length::Fill),
        page_text_button(
            "table-cells-large",
            app.language.sort_page(),
            (has_images && !app.exporting).then_some(Message::SortPage(index)),
        ),
        page_text_button(
            "download",
            app.language.export_image(),
            (has_images && !app.exporting).then_some(Message::ExportPage(index)),
        ),
        text(app.language.photo_count(page.images.len()))
            .size(10)
            .color(MUTED),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fixed(size.width));

    let page_column = column![label, canvas]
        .spacing(6)
        .width(Length::Fixed(size.width));

    container(page_column)
        .padding(8)
        .style(page_frame_style(active, is_drop_target))
        .into()
}

fn add_page_card(language: Language, height: f32, width: f32) -> Element<'static, Message> {
    button(
        container(
            column![
                icon("plus", 22.0, MUTED),
                text(language.add_page()).size(11).color(MUTED),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center),
    )
    .on_press(Message::AddPage)
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .padding(0)
    .style(add_page_button)
    .into()
}

fn notice_view(language: Language, notice: &Notice) -> Element<'_, Message> {
    let (icon_name, color) = match notice.kind {
        NoticeKind::Error => ("circle-exclamation", DANGER),
        NoticeKind::Success => ("circle-check", SUCCESS),
    };

    container(
        row![
            icon(icon_name, 13.0, color),
            text(&notice.text).size(12).color(TEXT).width(Length::Fill),
            icon_button("xmark", language.dismiss(), Some(Message::DismissNotice)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill),
    )
    .padding([7, 12])
    .width(Length::Fill)
    .style(notice_style(notice.kind))
    .into()
}

fn footer(app: &App) -> Element<'_, Message> {
    let language = app.language;
    let page = app.active();

    let status: Option<(&'static str, Color, String)> = if app.exporting {
        Some(("gears", ACCENT, language.exporting().to_owned()))
    } else if app.pending_loads > 0 {
        Some(("circle-notch", ACCENT, language.loading(app.pending_loads)))
    } else if page.images.is_empty() {
        None
    } else {
        Some(("images", MUTED, language.photo_count(page.images.len())))
    };

    let mut status_row: Row<'_, Message> = Row::new()
        .spacing(7)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    if let Some((icon_name, color, label)) = status {
        status_row = status_row.push(icon(icon_name, 12.0, color));
        status_row = status_row.push(text(label).size(11).color(MUTED));
        status_row = status_row.push(text("·").size(11).color(BORDER));
    }

    status_row = status_row.push(
        text(language.page_indicator(app.active_page + 1, app.pages.len()))
            .size(11)
            .color(MUTED),
    );
    status_row = status_row.push(Space::new().width(Length::Fill));

    if app.last_output_dir.is_some() && !app.exporting {
        status_row = status_row.push(icon_button(
            "folder-open",
            language.open_folder(),
            Some(Message::OpenOutput),
        ));
    }

    let can_export_all = app.pages.iter().any(|page| !page.images.is_empty()) && !app.exporting;

    status_row = status_row.push(export_all_pages_button(language, can_export_all));
    status_row = status_row.push(export_html_button(language, can_export_all));
    status_row = status_row.push(export_both_button(language, can_export_all));

    status_row.into()
}

fn export_all_pages_button(language: Language, enabled: bool) -> Element<'static, Message> {
    button(
        row![
            icon("download", 12.0, if enabled { Color::WHITE } else { MUTED }),
            text(language.export_all_pages()).size(12),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press_maybe(enabled.then_some(Message::ExportAllPages))
    .padding([7, 11])
    .style(primary_button)
    .into()
}

fn export_html_button(language: Language, enabled: bool) -> Element<'static, Message> {
    button(
        row![
            icon("file-code", 12.0, if enabled { TEXT } else { MUTED }),
            text(language.export_html()).size(12),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press_maybe(enabled.then_some(Message::ExportHtml))
    .padding([7, 11])
    .style(secondary_button)
    .into()
}

fn export_both_button(language: Language, enabled: bool) -> Element<'static, Message> {
    button(
        row![
            icon("box-archive", 12.0, if enabled { TEXT } else { MUTED }),
            text(language.export_both()).size(12),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press_maybe(enabled.then_some(Message::ExportBoth))
    .padding([7, 11])
    .style(secondary_button)
    .into()
}

fn page_text_button(
    name: &'static str,
    label: &'static str,
    message: Option<Message>,
) -> Element<'static, Message> {
    button(
        row![
            icon(name, 11.0, if message.is_some() { TEXT } else { MUTED }),
            text(label).size(10),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .on_press_maybe(message)
    .padding([4, 7])
    .style(secondary_button)
    .into()
}

fn icon(name: &str, size: f32, color: Color) -> Element<'static, Message> {
    fa_icon_solid::<Theme>(name).size(size).color(color).into()
}

fn icon_button(
    name: &'static str,
    hint: &'static str,
    message: Option<Message>,
) -> Element<'static, Message> {
    tooltip(
        button(icon(
            name,
            13.0,
            if message.is_some() { TEXT } else { MUTED },
        ))
        .on_press_maybe(message)
        .padding([7, 9])
        .style(secondary_button),
        tooltip_label(hint),
        tooltip::Position::Top,
    )
    .gap(6)
    .into()
}

fn toggle_icon_button(
    name: &'static str,
    hint: &'static str,
    active: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
    let color = if active {
        Color::WHITE
    } else if message.is_some() {
        TEXT
    } else {
        MUTED
    };

    tooltip(
        button(icon(name, 13.0, color))
            .on_press_maybe(message)
            .padding([7, 9])
            .style(toggle_button_style(active)),
        tooltip_label(hint),
        tooltip::Position::Top,
    )
    .gap(6)
    .into()
}

fn tooltip_label(hint: &'static str) -> Element<'static, Message> {
    container(text(hint).size(11).color(TEXT))
        .padding([5, 8])
        .style(tooltip_style)
        .into()
}

fn flag_icon(handle: &'static LazyLock<ImageHandle>) -> Element<'static, Message> {
    iced::widget::image((**handle).clone())
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(16.0))
        .into()
}

fn flag_button(
    language: Language,
    selected: bool,
    handle: &'static LazyLock<ImageHandle>,
    hint: &'static str,
) -> Element<'static, Message> {
    tooltip(
        button(flag_icon(handle))
            .on_press(Message::SetLanguage(language))
            .padding(4)
            .style(flag_button_style(selected)),
        tooltip_label(hint),
        tooltip::Position::Bottom,
    )
    .gap(6)
    .into()
}

// ---------------------------------------------------------------------------
// Styling
// ---------------------------------------------------------------------------

fn app_theme() -> Theme {
    Theme::custom(
        "Collage Editor",
        theme::Palette {
            background: BACKGROUND,
            text: TEXT,
            primary: ACCENT,
            success: SUCCESS,
            warning: Color::from_rgb8(255, 191, 92),
            danger: DANGER,
        },
    )
}

fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Active => ACCENT,
        button::Status::Hovered => ACCENT_HOVER,
        button::Status::Pressed => ACCENT_PRESSED,
        button::Status::Disabled => SURFACE_ALT,
    };

    button::Style {
        background: Some(background.into()),
        text_color: if matches!(status, button::Status::Disabled) {
            MUTED
        } else {
            Color::WHITE
        },
        border: border::rounded(9),
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.25),
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        ..button::Style::default()
    }
}

fn secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Disabled => (SURFACE_ALT, MUTED),
        button::Status::Hovered | button::Status::Pressed => (SURFACE_HOVER, TEXT),
        _ => (SURFACE_ALT, TEXT),
    };

    button::Style {
        background: Some(background.into()),
        text_color,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 9.0.into(),
        },
        ..button::Style::default()
    }
}

fn add_page_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => SURFACE_HOVER,
        _ => SURFACE,
    };
    let border_color = match status {
        button::Status::Hovered | button::Status::Pressed => ACCENT,
        _ => BORDER,
    };

    button::Style {
        background: Some(background.into()),
        text_color: MUTED,
        border: Border {
            color: border_color,
            width: 1.5,
            radius: 12.0.into(),
        },
        ..button::Style::default()
    }
}

fn toggle_button_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = if selected {
            ACCENT
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => SURFACE_HOVER,
                _ => SURFACE_ALT,
            }
        };

        button::Style {
            background: Some(background.into()),
            text_color: if selected { Color::WHITE } else { TEXT },
            border: Border {
                color: if selected { ACCENT } else { BORDER },
                width: 1.0,
                radius: 9.0.into(),
            },
            ..button::Style::default()
        }
    }
}

fn flag_button_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = if selected {
            SURFACE_HOVER
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => SURFACE_HOVER,
                _ => SURFACE,
            }
        };

        button::Style {
            background: Some(background.into()),
            text_color: TEXT,
            border: Border {
                color: if selected { ACCENT } else { BORDER },
                width: if selected { 2.0 } else { 1.0 },
                radius: 7.0.into(),
            },
            ..button::Style::default()
        }
    }
}

fn page_frame_style(active: bool, drop_target: bool) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: if active || drop_target {
            Some(SURFACE.into())
        } else {
            Some(Color::TRANSPARENT.into())
        },
        border: Border {
            color: if drop_target {
                SUCCESS
            } else if active {
                ACCENT
            } else {
                Color::TRANSPARENT
            },
            width: 2.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

fn pages_scrollable_style(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    style.container = container::Style::default();
    style.horizontal_rail.background = Some(SURFACE.into());
    style.horizontal_rail.scroller.background = ACCENT.into();
    style.horizontal_rail.scroller.border = border::rounded(4);
    style.vertical_rail.background = None;
    style.gap = None;
    style
}

fn notice_style(kind: NoticeKind) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(
            (if kind == NoticeKind::Error {
                DANGER_DIM
            } else {
                SUCCESS_DIM
            })
            .into(),
        ),
        border: Border {
            color: if kind == NoticeKind::Error {
                Color::from_rgb8(122, 52, 72)
            } else {
                Color::from_rgb8(36, 100, 80)
            },
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    }
}

fn tooltip_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_HOVER.into()),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}
