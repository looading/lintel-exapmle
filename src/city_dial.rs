use std::f32::consts::TAU;

use chrono::{NaiveTime, Timelike};
use lintel::iced::{
    Color, Element, Font, Point, Rectangle, Renderer, Theme, alignment, color, mouse,
    widget::canvas,
};

use crate::Message;

pub fn view(time: NaiveTime, daylight: bool) -> Element<'static, Message> {
    canvas(Dial { time, daylight })
        .width(104)
        .height(104)
        .into()
}

struct Dial {
    time: NaiveTime,
    daylight: bool,
}

struct Palette {
    face: Color,
    ring: Color,
    track: Color,
    minute_mark: Color,
    hour_mark: Color,
    hands: Color,
    seconds: Color,
}

const DAY: Palette = Palette {
    face: color!(0xe9dcc1),
    ring: color!(0x8c795c),
    track: color!(0xcdbea1),
    minute_mark: color!(0x9c896c),
    hour_mark: color!(0x5a4f3f),
    hands: color!(0x243b3e),
    seconds: color!(0x00766d),
};

const NIGHT: Palette = Palette {
    face: color!(0x122c40),
    ring: color!(0x467387),
    track: color!(0x25475b),
    minute_mark: color!(0x668d9c),
    hour_mark: color!(0xc6d8d6),
    hands: color!(0xe7f1f1),
    seconds: color!(0x52dfc5),
};

impl canvas::Program<Message> for Dial {
    type State = ();

    fn draw(
        &self,
        _: &Self::State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let center = frame.center();
        let radius = bounds.width.min(bounds.height) / 2.0 - 3.0;
        let palette = if self.daylight { &DAY } else { &NIGHT };

        let dial = canvas::Path::circle(center, radius);
        frame.fill(&dial, palette.face);
        frame.stroke(&dial, stroke(palette.ring, 1.25));
        frame.stroke(
            &canvas::Path::circle(center, radius * 0.72),
            stroke(palette.track, 0.6),
        );

        for major in [false, true] {
            let marks = canvas::Path::new(|path| {
                for index in 0..60 {
                    if (index % 5 == 0) == major {
                        let turn = index as f32 / 60.0;
                        let outer = radius * 0.91;
                        let inner = radius * if major { 0.79 } else { 0.86 };
                        path.move_to(point(center, turn, outer));
                        path.line_to(point(center, turn, inner));
                    }
                }
            });
            let (color, width) = if major {
                (palette.hour_mark, 1.35)
            } else {
                (palette.minute_mark, 0.7)
            };
            frame.stroke(&marks, stroke(color, width));
        }

        for (turn, text) in [(0.0, "12"), (0.25, "3"), (0.5, "6"), (0.75, "9")] {
            frame.fill_text(canvas::Text {
                content: text.into(),
                position: point(center, turn, radius * 0.60),
                color: palette.hour_mark,
                size: 9.0.into(),
                font: Font::MONOSPACE,
                align_x: lintel::iced::alignment::Horizontal::Center.into(),
                align_y: alignment::Vertical::Center,
                ..Default::default()
            });
        }

        let second = self.time.second() as f32 + self.time.nanosecond() as f32 / 1_000_000_000.0;
        let minute = self.time.minute() as f32 + second / 60.0;
        let hour = (self.time.hour() % 12) as f32 + minute / 60.0;
        for (turn, length, width) in [(hour / 12.0, 0.46, 3.5), (minute / 60.0, 0.69, 2.2)] {
            frame.stroke(
                &canvas::Path::line(center, point(center, turn, radius * length)),
                stroke(palette.hands, width),
            );
        }

        let sweep = second / 60.0;
        frame.stroke(
            &canvas::Path::line(
                point(center, sweep + 0.5, radius * 0.16),
                point(center, sweep, radius * 0.82),
            ),
            stroke(palette.seconds, 1.2),
        );
        frame.fill(&canvas::Path::circle(center, 2.6), palette.hands);
        frame.fill(&canvas::Path::circle(center, 1.35), palette.seconds);

        vec![frame.into_geometry()]
    }
}

fn point(center: Point, turn: f32, radius: f32) -> Point {
    let (x, y) = (turn * TAU).sin_cos();
    Point::new(center.x + x * radius, center.y - y * radius)
}

fn stroke(color: Color, width: f32) -> canvas::Stroke<'static> {
    canvas::Stroke::default()
        .with_color(color)
        .with_width(width)
        .with_line_cap(canvas::LineCap::Round)
}
