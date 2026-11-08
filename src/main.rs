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
use collage::{CollageImage, Pattern};
use display_info::DisplayInfo;
use i18n::Language;
use iced::futures::channel::mpsc::unbounded;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{button, column, container, mouse_area, row, text, tooltip, Row, Space};
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
    let fallback = Size::new(980.0, 680.0);

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

    // Aim for roughly 60% of the screen, but never force a window that is
    // larger than the user's display (which also keeps software rendering
    // cheap on high-DPI laptops).
    Size::new(
        (display.width as f32 / scale * 0.6).clamp(700.0, 1280.0),
        (display.height as f32 / scale * 0.6).clamp(500.0, 860.0),
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

struct App {
    language: Language,
    images: Vec<CollageImage>,
    pattern: Pattern,
    selected: Option<usize>,
    dragging: Option<Drag>,
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
            images: Vec::new(),
            pattern: Pattern::default(),
            selected: None,
            dragging: None,
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
    index: usize,
    offset_x: f32,
    offset_y: f32,
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
    ImageLoaded(PathBuf, Result<CollageImage, String>),
    Pattern(Pattern),
    SetLanguage(Language),
    ToggleSnap,
    Clear,
    DragStart { index: usize, pointer: Point },
    DragMove(Point),
    DragEnd,
    ExportImage,
    ExportHtml,
    ExportFinished(Result<PathBuf, String>),
    OpenOutput,
    DismissNotice,
}

impl App {
    fn add_paths(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        let collected = collage::collect_images(&paths);

        if collected.paths.is_empty() {
            self.notice = Some(Notice::error(
                self.language.no_supported_images().to_owned(),
            ));
            return Task::none();
        }

        let existing: HashSet<String> = self
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

        self.pending_loads += paths.len();
        self.notice = None;

        let (sender, receiver) = unbounded();

        LOAD_POOL.spawn(move || {
            use rayon::prelude::*;

            paths.par_iter().for_each(|path| {
                let result = collage::load_preview(path);
                let _ = sender.unbounded_send(Message::ImageLoaded(path.clone(), result));
            });
        });

        Task::stream(receiver)
    }

    fn apply_pattern(&mut self) {
        collage::apply_pattern(&mut self.images, self.pattern);
    }

    fn finish_export(&mut self, result: Result<PathBuf, String>) {
        self.exporting = false;

        match result {
            Ok(path) => {
                self.last_output_dir = path.parent().map(Path::to_path_buf);
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
        Message::ImageLoaded(path, result) => {
            app.pending_loads = app.pending_loads.saturating_sub(1);

            match result {
                Ok(image) => {
                    app.images.push(image);
                    app.apply_pattern();
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
        Message::Pattern(pattern) => {
            app.pattern = pattern;
            app.apply_pattern();
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
                app.images.clear();
                app.selected = None;
                app.dragging = None;
                app.notice = None;
                app.last_output_dir = None;
            }
            Task::none()
        }
        Message::DragStart { index, pointer } => {
            if let Some(image) = app.images.get(index) {
                let offset_x = pointer.x - image.x;
                let offset_y = pointer.y - image.y;
                let image = app.images.remove(index);
                app.images.push(image);
                let index = app.images.len() - 1;
                app.dragging = Some(Drag {
                    index,
                    offset_x,
                    offset_y,
                });
                app.selected = Some(index);
            }
            Task::none()
        }
        Message::DragMove(pointer) => {
            if let Some(drag) = app.dragging {
                let x = pointer.x - drag.offset_x;
                let y = pointer.y - drag.offset_y;
                let (x, y) = collage::snap_center(&app.images, drag.index, x, y, app.snap);

                if let Some(image) = app.images.get_mut(drag.index) {
                    image.x = x;
                    image.y = y;
                }
            }
            Task::none()
        }
        Message::DragEnd => {
            app.dragging = None;
            Task::none()
        }
        Message::ExportImage => {
            if app.images.is_empty() {
                app.notice = Some(Notice::error(app.language.no_images().to_owned()));
                return Task::none();
            }

            if app.exporting {
                return Task::none();
            }

            let Some(mut path) = rfd::FileDialog::new()
                .set_title(app.language.save_image_title())
                .set_file_name("collage.png")
                .add_filter("PNG", &["png"])
                .add_filter("JPEG", &["jpg", "jpeg"])
                .save_file()
            else {
                return Task::none();
            };

            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase());

            if !matches!(extension.as_deref(), Some("png" | "jpg" | "jpeg")) {
                path.set_extension("png");
            }

            let images = app.images.clone();
            app.exporting = true;
            app.notice = None;

            Task::perform(
                async move { collage::export_image(&images, &path).map(|_| path) },
                Message::ExportFinished,
            )
        }
        Message::ExportHtml => {
            if app.images.is_empty() {
                app.notice = Some(Notice::error(app.language.no_images().to_owned()));
                return Task::none();
            }

            if app.exporting {
                return Task::none();
            }

            let Some(mut path) = rfd::FileDialog::new()
                .set_title(app.language.save_html_title())
                .set_file_name("collage.html")
                .add_filter("HTML", &["html"])
                .save_file()
            else {
                return Task::none();
            };

            if path.extension().is_none() {
                path.set_extension("html");
            }

            let images = app.images.clone();
            let language = app.language;
            app.exporting = true;
            app.notice = None;

            Task::perform(
                async move { collage::export_html(&images, &path, language).map(|_| path) },
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
        // End a drag even when the button is released outside the canvas.
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::DragEnd)
        }
        _ => None,
    })
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
        .spacing(10)
        .width(Length::Fill)
        .height(Length::Fill);

    if !app.images.is_empty() {
        content = content.push(pattern_bar(app));
    }

    if let Some(notice) = &app.notice {
        content = content.push(notice_view(app.language, notice));
    }

    content = content.push(if app.images.is_empty() {
        empty_canvas(app)
    } else {
        canvas_view(app)
    });

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
    let can_export = !app.images.is_empty() && !app.exporting;

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
            (!app.images.is_empty() && !app.exporting).then_some(Message::Clear),
        ),
        primary_icon_button(
            "download",
            language.export_image(),
            can_export.then_some(Message::ExportImage),
        ),
        icon_button(
            "file-code",
            language.export_html(),
            can_export.then_some(Message::ExportHtml),
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn pattern_bar(app: &App) -> Element<'_, Message> {
    let language = app.language;

    row![
        pattern_button(language, app.pattern, Pattern::Grid, "table-cells-large"),
        pattern_button(language, app.pattern, Pattern::Mosaic, "object-group"),
        pattern_button(language, app.pattern, Pattern::Strip, "table-columns"),
        pattern_button(language, app.pattern, Pattern::Hero, "images"),
        pattern_button(language, app.pattern, Pattern::Stack, "table-list"),
        Space::new().width(Length::Fill),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn pattern_button(
    language: Language,
    current: Pattern,
    pattern: Pattern,
    icon_name: &'static str,
) -> Element<'static, Message> {
    let selected = current == pattern;
    let label = match pattern {
        Pattern::Grid => language.pattern_grid(),
        Pattern::Mosaic => language.pattern_mosaic(),
        Pattern::Strip => language.pattern_strip(),
        Pattern::Hero => language.pattern_hero(),
        Pattern::Stack => language.pattern_stack(),
    };

    tooltip(
        button(icon(
            icon_name,
            13.0,
            if selected { Color::WHITE } else { MUTED },
        ))
        .on_press(Message::Pattern(pattern))
        .padding([7, 9])
        .style(pattern_button_style(selected)),
        tooltip_label(label),
        tooltip::Position::Top,
    )
    .gap(6)
    .into()
}

fn empty_canvas(app: &App) -> Element<'_, Message> {
    let language = app.language;
    let border_color = if app.hovered { ACCENT } else { BORDER };

    let content = column![
        icon("images", 46.0, if app.hovered { ACCENT } else { MUTED }),
        text(language.drop_here()).size(22).color(TEXT),
        text(language.drop_hint()).size(13).color(MUTED),
        button(
            row![
                icon("plus", 13.0, TEXT),
                text(language.add_photos()).size(12),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .on_press(Message::AddPhotos)
        .padding([8, 14])
        .style(secondary_button),
    ]
    .spacing(10)
    .align_x(Alignment::Center);

    let zone = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme| container::Style {
            background: Some(SURFACE.into()),
            border: Border {
                color: border_color,
                width: 2.0,
                radius: 16.0.into(),
            },
            ..container::Style::default()
        });

    mouse_area(zone).on_press(Message::AddPhotos).into()
}

fn canvas_view(app: &App) -> Element<'_, Message> {
    container(
        iced::widget::canvas(CollageCanvas {
            images: &app.images,
            selected: app.selected,
            dragging: app.dragging.is_some(),
            grid: app.snap,
        })
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
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

    let status: Option<(&'static str, Color, String)> = if app.exporting {
        Some(("gears", ACCENT, language.exporting().to_owned()))
    } else if app.pending_loads > 0 {
        Some(("circle-notch", ACCENT, language.loading(app.pending_loads)))
    } else if app.images.is_empty() {
        None
    } else {
        Some(("images", MUTED, language.photo_count(app.images.len())))
    };

    let mut status_row: Row<'_, Message> = Row::new()
        .spacing(7)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    if let Some((icon_name, color, label)) = status {
        status_row = status_row.push(icon(icon_name, 12.0, color));
        status_row = status_row.push(text(label).size(11).color(MUTED));
    }

    status_row = status_row.push(Space::new().width(Length::Fill));

    if app.last_output_dir.is_some() && !app.exporting {
        status_row = status_row.push(icon_button(
            "folder-open",
            language.open_folder(),
            Some(Message::OpenOutput),
        ));
    }

    status_row.into()
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

fn primary_icon_button(
    name: &'static str,
    hint: &'static str,
    message: Option<Message>,
) -> Element<'static, Message> {
    let color = if message.is_some() {
        Color::WHITE
    } else {
        MUTED
    };

    tooltip(
        button(icon(name, 13.0, color))
            .on_press_maybe(message)
            .padding([7, 9])
            .style(primary_button),
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
        container(text(hint).size(11).color(TEXT))
            .padding([5, 8])
            .style(tooltip_style),
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
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => SURFACE_HOVER,
        _ => SURFACE_ALT,
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 9.0.into(),
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

fn pattern_button_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = if selected {
            SURFACE_HOVER
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => SURFACE_ALT,
                _ => SURFACE,
            }
        };

        button::Style {
            background: Some(background.into()),
            text_color: TEXT,
            border: Border {
                color: if selected { ACCENT } else { BORDER },
                width: if selected { 1.5 } else { 1.0 },
                radius: 8.0.into(),
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
