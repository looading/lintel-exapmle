//! Geographic outlines from Natural Earth 1:110m Land, v5.1.2 (public domain).
//! Source: https://www.naturalearthdata.com/downloads/110m-physical-vectors/110m-land/

use std::{cell::Cell, f64::consts::TAU, sync::OnceLock};

use chrono::{DateTime, Datelike, Timelike, Utc};
use lintel::iced::{
    Color, Element, Fill, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, color, mouse,
    widget::{canvas, responsive},
};

use crate::Message;

const OCEAN: Color = color!(0x101f30);
const LAND: Color = color!(0x334f62);
const COAST: Color = color!(0x527284);
const MINT: Color = color!(0x91e5cf);
const LABEL: Color = color!(0xe5f0f4);

#[derive(Clone)]
pub struct Pin {
    pub latitude: f64,
    pub longitude: f64,
    pub label: String,
    pub time: String,
}

pub fn view(now: DateTime<Utc>, pins: Vec<Pin>) -> Element<'static, Message> {
    responsive(move |size| {
        canvas(WorldMap {
            now,
            pins: pins.clone(),
        })
        .width(Fill)
        .height((size.width * 0.46).clamp(196.0, 500.0))
        .into()
    })
    .height(Length::Shrink)
    .into()
}

pub fn daylight(now: DateTime<Utc>, latitude: f64, longitude: f64) -> bool {
    sun_is_up(subsolar_point(now), latitude, longitude)
}

struct WorldMap {
    now: DateTime<Utc>,
    pins: Vec<Pin>,
}

#[derive(Default)]
struct MapState {
    base: canvas::Cache,
    night: canvas::Cache,
    night_minute: Cell<Option<i64>>,
}

impl canvas::Program<Message> for WorldMap {
    type State = MapState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let base = state.base.draw(renderer, size, draw_base);
        let minute = self.now.timestamp().div_euclid(60);
        if state.night_minute.replace(Some(minute)) != Some(minute) {
            state.night.clear();
        }
        let sun = subsolar_point(self.now);
        let night = state
            .night
            .draw(renderer, size, |frame| draw_night(frame, sun));
        let mut foreground = canvas::Frame::new(renderer, size);
        draw_pins(&mut foreground, &self.pins, sun);
        draw_legend(&mut foreground);
        vec![base, night, foreground.into_geometry()]
    }
}

fn project(size: Size, longitude: f64, latitude: f64) -> Point {
    Point::new(
        ((longitude + 180.0) / 360.0) as f32 * size.width,
        ((90.0 - latitude) / 180.0) as f32 * size.height,
    )
}

fn draw_base(frame: &mut canvas::Frame) {
    let size = frame.size();
    frame.fill_rectangle(Point::ORIGIN, size, OCEAN);

    let land = canvas::Path::new(|path| {
        for polygon in land_polygons() {
            for ring in polygon {
                if let Some(([x, y], remainder)) = ring.split_first() {
                    path.move_to(project(size, *x, *y));
                    for [x, y] in remainder {
                        path.line_to(project(size, *x, *y));
                    }
                    path.close();
                }
            }
        }
    });
    frame.fill(&land, LAND);
    frame.stroke(
        &land,
        canvas::Stroke::default().with_color(COAST).with_width(0.45),
    );

    let graticule = canvas::Path::new(|path| {
        for longitude in (-150..=150).step_by(30) {
            path.move_to(project(size, f64::from(longitude), 90.0));
            path.line_to(project(size, f64::from(longitude), -90.0));
        }
        for latitude in [-60, -30, 0, 30, 60] {
            path.move_to(project(size, -180.0, f64::from(latitude)));
            path.line_to(project(size, 180.0, f64::from(latitude)));
        }
    });
    frame.stroke(
        &graticule,
        canvas::Stroke::default()
            .with_color(Color::from_rgba8(138, 183, 202, 0.26))
            .with_width(0.6),
    );
}

type LandPolygon = Vec<Vec<[f64; 2]>>;

fn land_polygons() -> &'static [LandPolygon] {
    static POLYGONS: OnceLock<Vec<LandPolygon>> = OnceLock::new();
    POLYGONS.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/land-110m.json"))
            .expect("bundled Natural Earth land coordinates")
    })
}

/// Solar latitude and longitude in degrees. The fractional-year approximation
/// comes from NOAA's General Solar Position Calculations.
/// https://gml.noaa.gov/grad/solcalc/solareqns.PDF
fn subsolar_point(now: DateTime<Utc>) -> (f64, f64) {
    let minutes = f64::from(now.num_seconds_from_midnight()) / 60.0;
    let fractional_year =
        TAU / f64::from(if now.date_naive().leap_year() {
            366
        } else {
            365
        }) * (f64::from(now.ordinal0()) + minutes / 1_440.0 - 0.5);
    let (sin, cos) = fractional_year.sin_cos();
    let (sin_2, cos_2) = (2.0 * fractional_year).sin_cos();
    let (sin_3, cos_3) = (3.0 * fractional_year).sin_cos();
    let equation_of_time =
        229.18 * (0.000075 + 0.001868 * cos - 0.032077 * sin - 0.014615 * cos_2 - 0.040849 * sin_2);
    let declination = 0.006918 - 0.399912 * cos + 0.070257 * sin - 0.006758 * cos_2
        + 0.000907 * sin_2
        - 0.002697 * cos_3
        + 0.00148 * sin_3;
    let longitude = (360.0 - (minutes + equation_of_time) / 4.0).rem_euclid(360.0) - 180.0;
    (declination.to_degrees(), longitude)
}

fn draw_night(frame: &mut canvas::Frame, (latitude, longitude): (f64, f64)) {
    let size = frame.size();
    let dark_pole = if latitude >= 0.0 { -90.0 } else { 90.0 };
    // Clamp the declination near the equinox, where the boundary becomes nearly
    // vertical. Sampling more closely there avoids visible stair steps.
    let declination = if latitude >= 0.0 {
        latitude.max(0.015)
    } else {
        latitude.min(-0.015)
    };
    let slope = declination.to_radians().tan();
    let samples = if latitude.abs() < 0.5 { 1440 } else { 360 };
    let edge = (0..=samples).map(|step| {
        let x = 360.0 * f64::from(step) / f64::from(samples) - 180.0;
        let y = (-(x - longitude).to_radians().cos() / slope)
            .atan()
            .to_degrees();
        project(size, x, y)
    });
    let mut path_points = edge.clone();
    let first = path_points
        .next()
        .expect("solar edge starts at the antimeridian");

    let shade = canvas::Path::new(|path| {
        path.move_to(first);
        for point in path_points {
            path.line_to(point);
        }
        path.line_to(project(size, 180.0, dark_pole));
        path.line_to(project(size, -180.0, dark_pole));
        path.close();
    });
    frame.fill(&shade, Color::from_rgba8(5, 12, 25, 0.62));
    let boundary = canvas::Path::new(|path| {
        path.move_to(first);
        for point in edge.skip(1) {
            path.line_to(point);
        }
    });
    frame.stroke(
        &boundary,
        canvas::Stroke::default()
            .with_color(Color::from_rgba8(134, 216, 190, 0.68))
            .with_width(1.0),
    );
}

fn sun_is_up((sun_latitude, sun_longitude): (f64, f64), latitude: f64, longitude: f64) -> bool {
    let (sun_sin, sun_cos) = sun_latitude.to_radians().sin_cos();
    let (sin, cos) = latitude.to_radians().sin_cos();
    sin * sun_sin + cos * sun_cos * (longitude - sun_longitude).to_radians().cos() >= 0.0
}

fn draw_pins(frame: &mut canvas::Frame, pins: &[Pin], sun: (f64, f64)) {
    let size = frame.size();
    let visible: Vec<_> = pins
        .iter()
        .filter(|pin| pin.latitude.is_finite() && pin.longitude.is_finite())
        .map(|pin| (pin, project(size, pin.longitude, pin.latitude)))
        .collect();

    for (index, &(pin, center)) in visible.iter().enumerate() {
        let color = if sun_is_up(sun, pin.latitude, pin.longitude) {
            MINT
        } else {
            color!(0x88accd)
        };
        let radius = if index == 0 { 4.2 } else { 3.2 };
        frame.fill(
            &canvas::Path::circle(center, radius + 2.0),
            color!(0x111e30),
        );
        frame.fill(&canvas::Path::circle(center, radius), color);
    }

    let mut occupied = vec![Rectangle::new(
        Point::new(13.0, 11.0),
        Size::new(124.0, 30.0),
    )];
    for (index, &(pin, point)) in visible.iter().enumerate() {
        let content = format!("{}  {}", pin.label, pin.time);
        let width: f32 = content
            .chars()
            .map(|ch| if ch.is_ascii() { 7.0 } else { 12.0 })
            .sum();
        let dimensions = Size::new(width + 12.0, 22.0);
        let choices = [
            Point::new(point.x + 8.0, point.y - 11.0),
            Point::new(point.x - dimensions.width - 8.0, point.y - 11.0),
            Point::new(point.x + 8.0, point.y + 7.0),
            Point::new(
                point.x - dimensions.width - 8.0,
                point.y - dimensions.height - 7.0,
            ),
        ];
        let display = choices
            .into_iter()
            .map(|origin| Rectangle::new(origin, dimensions))
            .find(|rect| {
                rect.x >= 3.0
                    && rect.y >= 3.0
                    && rect.x + rect.width <= size.width - 3.0
                    && rect.y + rect.height <= size.height - 3.0
                    && !occupied.iter().any(|used| used.intersects(rect))
                    && !visible.iter().any(|&(_, mark)| rect.contains(mark))
            });
        if let Some(rect) = display {
            occupied.push(rect);
            let background = if index == 0 {
                color!(0x28465a)
            } else {
                color!(0x16273a)
            };
            frame.fill_rectangle(rect.position(), rect.size(), background);
            map_text(
                frame,
                content,
                Point::new(rect.x + 6.0, rect.y + 3.5),
                LABEL,
            );
        }
    }
}

fn draw_legend(frame: &mut canvas::Frame) {
    let background = Color::from_rgba8(13, 25, 41, 0.9);
    frame.fill_rectangle(Point::new(13.0, 11.0), Size::new(119.0, 27.0), background);
    frame.fill(&canvas::Path::circle(Point::new(26.0, 24.0), 3.2), MINT);
    map_text(frame, "白昼".into(), Point::new(36.0, 16.0), LABEL);
    frame.fill(
        &canvas::Path::circle(Point::new(82.0, 24.0), 3.2),
        color!(0x88accd),
    );
    map_text(frame, "夜晚".into(), Point::new(92.0, 16.0), LABEL);
}

fn map_text(frame: &mut canvas::Frame, content: String, position: Point, color: Color) {
    frame.fill_text(canvas::Text {
        content,
        position,
        color,
        size: Pixels(11.0),
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn solar_meridian_is_near_greenwich_at_utc_noon() {
        let summer = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
        let winter = Utc.with_ymd_and_hms(2026, 12, 21, 12, 0, 0).unwrap();
        let (summer_lat, summer_lon) = subsolar_point(summer);
        let (winter_lat, winter_lon) = subsolar_point(winter);
        assert!((23.0..24.0).contains(&summer_lat));
        assert!((-24.0..-23.0).contains(&winter_lat));
        assert!(summer_lon.abs() < 4.0 && winter_lon.abs() < 4.0);
        assert!(daylight(summer, 51.5, 0.0));
        assert!(!daylight(summer, -43.5, 172.6));
    }
}
