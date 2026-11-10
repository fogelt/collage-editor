//! Interactive collage canvas.

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use iced::widget::canvas::{
    self, Frame, Geometry, Image, Path, Program, Stroke, Text as CanvasText,
};
use iced::widget::image::{FilterMethod, Handle as ImageHandle};
use iced::{alignment, mouse, Alignment, Color, Point, Rectangle, Size};

use crate::collage::{self, CollageImage, Corner, CANVAS_H, CANVAS_W};
use crate::Message;

const HANDLE_SIZE: f32 = 9.0;
const HANDLE_HIT_RADIUS: f32 = 10.0;

/// The custom pointer is drawn by the canvas itself, so it looks the same on
/// every machine and can be bigger and softer than the system hand.
const CURSOR_SIZE: f32 = 32.0;
/// Hotspots are fractions of the cursor images, placed on the fingertip that
/// should sit exactly under the pointer in each pose. The PNGs are Kenney's
/// CC0 `hand_thin_open` / `hand_thin_closed` cursors.
const GRAB_HOTSPOT: (f32, f32) = (0.469, 0.078);
const GRABBING_HOTSPOT: (f32, f32) = (0.469, 0.188);

static GRAB_CURSOR: LazyLock<ImageHandle> =
    LazyLock::new(|| cursor_handle(include_bytes!("../assets/cursors/grab.png")));

static GRABBING_CURSOR: LazyLock<ImageHandle> =
    LazyLock::new(|| cursor_handle(include_bytes!("../assets/cursors/grabbing.png")));

fn cursor_handle(bytes: &[u8]) -> ImageHandle {
    let image = image::load_from_memory(bytes)
        .expect("the embedded cursor image must be a valid PNG")
        .to_rgba8();
    let (width, height) = image.dimensions();

    ImageHandle::from_rgba(width, height, image.into_raw())
}

pub struct CollageCanvas<'a> {
    pub page: usize,
    pub images: &'a [CollageImage],
    pub selected: Option<usize>,
    /// Whether any photo is currently being dragged. The dragged photo does
    /// not belong to any page while it is in flight.
    pub dragging: bool,
    /// The floating photo, drawn on top of this page while the pointer is
    /// over it. It is inserted into a page only when the drag ends.
    pub floating: Option<&'a CollageImage>,
    pub resizing: Option<Corner>,
    pub empty_hint: &'a str,
}

/// Static page geometry is cached per canvas. Pointer movement only redraws
/// the small dynamic overlay (selection handles and custom cursor), so
/// dragging one photo no longer repaints every page on every mouse event.
#[derive(Default)]
pub struct CanvasState {
    cache: canvas::Cache,
    fingerprint: Cell<u64>,
}

impl Program<Message> for CollageCanvas<'_> {
    type State = CanvasState;

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let board = board_rect(bounds);

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let cursor = cursor.position_in(bounds)?;

                if let Some(corner) = self.handle_at(cursor, board) {
                    if let Some(index) = self.selected {
                        return Some(canvas::Action::publish(Message::ResizeStart {
                            page: self.page,
                            index,
                            corner,
                        }));
                    }
                }

                let point = normalize(cursor, board);

                match collage::hit_test(self.images, point.x, point.y) {
                    Some(index) => Some(canvas::Action::publish(Message::DragStart {
                        page: self.page,
                        index,
                        pointer: point,
                    })),
                    None => Some(canvas::Action::publish(Message::ActivatePage(self.page))),
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if self.resizing.is_some() {
                    let origin = Point::new(bounds.x, bounds.y);
                    let cursor = cursor
                        .position_in(bounds)
                        .or_else(|| cursor.position_from(origin))?;
                    let point = normalize(cursor, board);

                    Some(canvas::Action::publish(Message::ResizeMove {
                        page: self.page,
                        pointer: point,
                    }))
                } else if self.dragging {
                    // Only claim the drop target when the pointer is actually
                    // inside this page; otherwise every page would report a
                    // position and steal the target from the hovered one.
                    let cursor = cursor.position_in(bounds)?;
                    let point = normalize(cursor, board);

                    Some(canvas::Action::publish(Message::DragOver {
                        page: self.page,
                        pointer: point,
                    }))
                } else {
                    // Redraw on every move so the custom hand follows the
                    // pointer and disappears as soon as it leaves a photo.
                    cursor
                        .position_in(bounds)
                        .map(|_| canvas::Action::request_redraw())
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorLeft) => {
                Some(canvas::Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let board = board_rect(bounds);
        let fingerprint = self.fingerprint(bounds);

        if state.fingerprint.get() != fingerprint {
            state.cache.clear();
            state.fingerprint.set(fingerprint);
        }

        let base = state.cache.draw(renderer, bounds.size(), |frame| {
            self.draw_layout(frame, board);
        });

        // Selection handles and the custom hand follow the pointer, so they
        // live in a separate layer that is cheap to redraw on every move.
        let mut overlay = Frame::new(renderer, bounds.size());
        let mut has_overlay = false;

        // The floating photo is drawn in the cheap overlay layer instead of
        // being inserted into a page. That keeps every page's cached layout
        // untouched while the pointer travels across page boundaries.
        if let Some(image) = self.floating {
            has_overlay = true;
            let tile = image_rect(image, board);
            let draw_rect = contain_rect(tile, image.aspect);
            overlay.draw_image(draw_rect, Image::new(image.handle.clone()));
        }

        if let Some(index) = self.selected {
            if let Some(image) = self.images.get(index) {
                has_overlay = true;
                let rect = image_rect(image, board);
                let accent = Color::from_rgb8(96, 130, 255);

                overlay.stroke_rectangle(
                    Point::new(rect.x, rect.y),
                    rect.size(),
                    Stroke::default().with_color(accent).with_width(2.0),
                );

                let handles = Path::new(|path| {
                    for corner in Corner::ALL {
                        let center = handle_position(corner, rect);
                        path.rectangle(
                            Point::new(center.x - HANDLE_SIZE / 2.0, center.y - HANDLE_SIZE / 2.0),
                            Size::new(HANDLE_SIZE, HANDLE_SIZE),
                        );
                    }
                });

                overlay.fill(&handles, Color::WHITE);
                overlay.stroke(
                    &handles,
                    Stroke::default().with_color(accent).with_width(1.5),
                );
            }
        }

        // Draw the custom hand last so it stays above every photo. The native
        // cursor is hidden whenever this is visible.
        if self.resizing.is_none() {
            if let Some(position) = cursor.position_in(bounds) {
                let grabbing = self.dragging;
                let over_image = if grabbing {
                    true
                } else {
                    let point = normalize(position, board);
                    self.handle_at(position, board).is_none()
                        && collage::hit_test(self.images, point.x, point.y).is_some()
                };

                if over_image {
                    has_overlay = true;
                    let handle = if grabbing {
                        (*GRABBING_CURSOR).clone()
                    } else {
                        (*GRAB_CURSOR).clone()
                    };
                    let hotspot = if grabbing {
                        GRABBING_HOTSPOT
                    } else {
                        GRAB_HOTSPOT
                    };
                    let origin = Point::new(
                        position.x - CURSOR_SIZE * hotspot.0,
                        position.y - CURSOR_SIZE * hotspot.1,
                    );
                    let mut image = Image::new(handle);
                    image.filter_method = FilterMethod::Linear;
                    overlay.draw_image(
                        Rectangle::new(origin, Size::new(CURSOR_SIZE, CURSOR_SIZE)),
                        image,
                    );
                }
            }
        }

        if has_overlay {
            vec![base, overlay.into_geometry()]
        } else {
            vec![base]
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(corner) = self.resizing {
            return resize_cursor(corner);
        }

        let Some(position) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };

        let board = board_rect(bounds);

        if self.dragging {
            return mouse::Interaction::Hidden;
        }

        if let Some(corner) = self.handle_at(position, board) {
            return resize_cursor(corner);
        }

        let point = normalize(position, board);

        if collage::hit_test(self.images, point.x, point.y).is_some() {
            mouse::Interaction::Hidden
        } else {
            mouse::Interaction::default()
        }
    }
}

impl CollageCanvas<'_> {
    fn draw_layout(&self, frame: &mut Frame<iced::Renderer>, board: Rectangle) {
        // Soft board shadow.
        frame.fill_rectangle(
            Point::new(board.x + 6.0, board.y + 8.0),
            board.size(),
            Color::from_rgba8(0, 0, 0, 0.35),
        );

        let board_path =
            Path::rounded_rectangle(Point::new(board.x, board.y), board.size(), 12.0.into());
        frame.fill(&board_path, Color::WHITE);

        if self.images.is_empty() && !self.empty_hint.is_empty() {
            frame.fill_text(CanvasText {
                content: self.empty_hint.to_owned(),
                position: board.center(),
                color: Color::from_rgb8(132, 143, 166),
                size: 14.0.into(),
                align_x: Alignment::Center.into(),
                align_y: alignment::Vertical::Center,
                ..CanvasText::default()
            });
        }

        for image in self.images {
            let tile = image_rect(image, board);
            let draw_rect = contain_rect(tile, image.aspect);
            let mut canvas_image = Image::new(image.handle.clone());
            canvas_image.filter_method = FilterMethod::Linear;

            frame.draw_image(draw_rect, canvas_image);
        }
    }

    fn fingerprint(&self, bounds: Rectangle) -> u64 {
        let mut hasher = DefaultHasher::new();

        bounds.width.to_bits().hash(&mut hasher);
        bounds.height.to_bits().hash(&mut hasher);
        self.empty_hint.hash(&mut hasher);
        self.images.len().hash(&mut hasher);

        for image in self.images {
            image.path.hash(&mut hasher);
            image.aspect.to_bits().hash(&mut hasher);
            image.x.to_bits().hash(&mut hasher);
            image.y.to_bits().hash(&mut hasher);
            image.width.to_bits().hash(&mut hasher);
            image.height.to_bits().hash(&mut hasher);
        }

        hasher.finish()
    }

    fn handle_at(&self, point: Point, board: Rectangle) -> Option<Corner> {
        let image = self.images.get(self.selected?)?;
        let rect = image_rect(image, board);

        Corner::ALL.into_iter().find(|corner| {
            let handle = handle_position(*corner, rect);
            (point.x - handle.x).abs() <= HANDLE_HIT_RADIUS
                && (point.y - handle.y).abs() <= HANDLE_HIT_RADIUS
        })
    }
}

fn handle_position(corner: Corner, rect: Rectangle) -> Point {
    let x = match corner {
        Corner::TopLeft | Corner::BottomLeft => rect.x,
        Corner::TopRight | Corner::BottomRight => rect.x + rect.width,
    };
    let y = match corner {
        Corner::TopLeft | Corner::TopRight => rect.y,
        Corner::BottomLeft | Corner::BottomRight => rect.y + rect.height,
    };

    Point::new(x, y)
}

fn resize_cursor(corner: Corner) -> mouse::Interaction {
    match corner {
        Corner::TopLeft | Corner::BottomRight => mouse::Interaction::ResizingDiagonallyDown,
        Corner::TopRight | Corner::BottomLeft => mouse::Interaction::ResizingDiagonallyUp,
    }
}

pub fn board_rect(bounds: Rectangle) -> Rectangle {
    let margin = 10.0;
    let available_w = (bounds.width - margin * 2.0).max(1.0);
    let available_h = (bounds.height - margin * 2.0).max(1.0);
    let aspect = CANVAS_W / CANVAS_H;

    let (width, height) = if available_w / available_h > aspect {
        (available_h * aspect, available_h)
    } else {
        (available_w, available_w / aspect)
    };

    // Canvas geometry is local to the widget; iced applies the widget's
    // position itself when it draws the frame.
    Rectangle {
        x: (bounds.width - width) / 2.0,
        y: (bounds.height - height) / 2.0,
        width,
        height,
    }
}

fn normalize(point: Point, board: Rectangle) -> Point {
    Point::new(
        (point.x - board.x) / board.width,
        (point.y - board.y) / board.height,
    )
}

fn image_rect(image: &CollageImage, board: Rectangle) -> Rectangle {
    Rectangle {
        x: board.x + image.left() * board.width,
        y: board.y + image.top() * board.height,
        width: image.width * board.width,
        height: image.height * board.height,
    }
}

/// The rectangle an image must occupy to fit inside its tile without
/// distortion.
fn contain_rect(tile: Rectangle, image_aspect: f32) -> Rectangle {
    let image_aspect = image_aspect.max(f32::EPSILON);
    let tile_aspect = tile.width / tile.height.max(f32::EPSILON);

    if image_aspect > tile_aspect {
        let width = tile.width;
        let height = width / image_aspect;

        Rectangle {
            x: tile.x,
            y: tile.y + (tile.height - height) / 2.0,
            width,
            height,
        }
    } else {
        let height = tile.height;
        let width = height * image_aspect;

        Rectangle {
            x: tile.x + (tile.width - width) / 2.0,
            y: tile.y,
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_uses_widget_local_coordinates() {
        let bounds = Rectangle::new(Point::new(120.0, 80.0), Size::new(900.0, 500.0));
        let board = board_rect(bounds);

        // iced translates the canvas frame by the widget position, so the
        // board must start inside the local [0, width] x [0, height] box.
        assert!(board.x >= 0.0);
        assert!(board.y >= 0.0);
        assert!(board.x + board.width <= bounds.width);
        assert!(board.y + board.height <= bounds.height);
        assert!(board.width > 0.0 && board.height > 0.0);
    }

    #[test]
    fn embedded_cursor_assets_decode() {
        for bytes in [
            include_bytes!("../assets/cursors/grab.png").as_slice(),
            include_bytes!("../assets/cursors/grabbing.png").as_slice(),
        ] {
            let image = image::load_from_memory(bytes).expect("cursor PNG must decode");
            assert!(image.width() >= 64);
            assert_eq!(image.width(), image.height());
        }
    }
}
