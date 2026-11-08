//! Interactive collage canvas.

use iced::widget::canvas::{self, Frame, Geometry, Image, Path, Program, Stroke};
use iced::widget::image::FilterMethod;
use iced::{mouse, Color, Point, Rectangle, Size};

use crate::collage::{self, CollageImage, CANVAS_H, CANVAS_W, GRID_STEP};
use crate::Message;

pub struct CollageCanvas<'a> {
    pub images: &'a [CollageImage],
    pub selected: Option<usize>,
    pub dragging: bool,
    pub grid: bool,
}

impl Program<Message> for CollageCanvas<'_> {
    type State = ();

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
                let point = normalize(cursor, board);

                let index = collage::hit_test(self.images, point.x, point.y)?;
                Some(canvas::Action::publish(Message::DragStart {
                    index,
                    pointer: point,
                }))
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !self.dragging {
                    return None;
                }

                // Keep following the pointer even when it leaves the canvas;
                // snap_center clamps the photo to the board for us.
                let origin = Point::new(bounds.x, bounds.y);
                let cursor = cursor
                    .position_in(bounds)
                    .or_else(|| cursor.position_from(origin))?;

                Some(canvas::Action::publish(Message::DragMove(normalize(
                    cursor, board,
                ))))
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if self.dragging {
                    Some(canvas::Action::publish(Message::DragEnd))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let board = board_rect(bounds);

        // Soft board shadow.
        frame.fill_rectangle(
            Point::new(board.x + 6.0, board.y + 8.0),
            board.size(),
            Color::from_rgba8(0, 0, 0, 0.35),
        );

        let board_path =
            Path::rounded_rectangle(Point::new(board.x, board.y), board.size(), 12.0.into());
        frame.fill(&board_path, Color::WHITE);

        if self.grid {
            let grid_color = Color::from_rgba8(82, 100, 130, 0.125);
            let steps = (1.0 / GRID_STEP).round() as usize;

            // Build every line into a single path. Filling them one by one
            // forces tiny-skia to allocate a full-surface mask per line, which
            // costs more than the rest of the canvas combined.
            let grid_path = Path::new(|path| {
                for step in 1..steps {
                    let x = board.x + step as f32 * GRID_STEP * board.width;
                    path.rectangle(Point::new(x, board.y), Size::new(1.0, board.height));
                }

                for step in 1..steps {
                    let y = board.y + step as f32 * GRID_STEP * board.height;
                    path.rectangle(Point::new(board.x, y), Size::new(board.width, 1.0));
                }
            });

            frame.fill(&grid_path, grid_color);
        }

        for (index, image) in self.images.iter().enumerate() {
            let rect = image_rect(image, board);
            let mut canvas_image = Image::new(image.handle.clone());
            // Linear sampling looks best. Nearest-neighbour avoids the extra
            // filtering work while a photo is being dragged.
            canvas_image.filter_method = if self.dragging {
                FilterMethod::Nearest
            } else {
                FilterMethod::Linear
            };

            frame.draw_image(rect, canvas_image);

            if self.selected == Some(index) {
                frame.stroke_rectangle(
                    Point::new(rect.x, rect.y),
                    rect.size(),
                    Stroke::default()
                        .with_color(Color::from_rgb8(96, 130, 255))
                        .with_width(2.5),
                );
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.dragging {
            return mouse::Interaction::Grabbing;
        }

        let Some(position) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };

        let point = normalize(position, board_rect(bounds));

        if collage::hit_test(self.images, point.x, point.y).is_some() {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
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
}
