use std::time::Duration;

use lintel::iced::{
    Alignment, Color, Element, Fill, Font, border, color,
    widget::{
        button, canvas, column, container, pick_list, responsive, row, rule, scrollable, text,
    },
};

use super::{Message, StopwatchDial, StopwatchFace, Tools, clock};
use crate::{INK, MINT, MUTED, PAPER, RULE, action_style};

impl Tools {
    pub(super) fn stopwatch_view(&self, has_native_menu: bool) -> Element<'_, Message> {
        let running = self.stopwatch_running();
        let elapsed = self.stopwatch.elapsed();
        responsive(move |size| {
            let short_window = size.height < 500.0;
            let display = if self.saved.analog_stopwatch {
                let diameter = (size.height * 0.40).clamp(138.0, 238.0);
                container(
                    canvas(StopwatchDial(elapsed))
                        .width(diameter)
                        .height(diameter),
                )
                .padding([12, 0])
                .center_x(Fill)
            } else {
                let seconds = elapsed.as_secs();
                let whole = if seconds < 3600 {
                    format!("{:02}:{:02}", seconds / 60, seconds % 60)
                } else {
                    super::countdown(seconds)
                };
                let max_width = (size.width - 30.0).max(200.0);
                let character_units = whole.len() as f32 + 2.1;
                let main_size = (max_width / (character_units * 0.67))
                    .min(size.height * 0.15)
                    .clamp(32.0, 88.0);
                let fractions = format!(".{:02}", elapsed.subsec_millis() / 10);

                container(
                    row![
                        text(whole).font(Font::MONOSPACE).size(main_size).color(INK),
                        text(fractions)
                            .font(Font::MONOSPACE)
                            .size(main_size * 0.66)
                            .color(MINT),
                    ]
                    .align_y(Alignment::End),
                )
                .center_x(Fill)
                .center_y((size.height * 0.24).clamp(100.0, 172.0))
            };

            let left = if running {
                stopwatch_button("分段", Some(Message::Lap), false, running)
            } else if elapsed.is_zero() {
                stopwatch_button("分段", None, false, running)
            } else {
                stopwatch_button("重置", Some(Message::ResetStopwatch), false, running)
            };
            let right = stopwatch_button(
                if running { "停止" } else { "启动" },
                Some(Message::StopwatchToggle),
                true,
                running,
            );
            let mut controls = column![].spacing(10).align_x(Alignment::Center);
            if !has_native_menu {
                controls = controls.push(pick_list(
                    [StopwatchFace::Digital, StopwatchFace::Analog],
                    Some(self.stopwatch_face()),
                    Message::SetStopwatchFace,
                ));
            }
            controls = controls.push(row![left, right].spacing(12).width(Fill));

            column![
                display,
                lap_history(&self.stopwatch.laps),
                container(controls).width(Fill).max_width(342),
            ]
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Center)
            .spacing(if short_window { 12 } else { 20 })
            .padding([if short_window { 12 } else { 26 }, 16])
            .into()
        })
        .into()
    }
}

fn lap_history(laps: &[Duration]) -> Element<'static, Message> {
    let header = row![
        text("分段").size(13).color(MUTED).width(Fill),
        text("单段")
            .size(13)
            .color(MUTED)
            .width(Fill)
            .align_x(Alignment::End),
        text("累计")
            .size(13)
            .color(MUTED)
            .width(Fill)
            .align_x(Alignment::End),
    ]
    .spacing(8);
    let mut rows = column![].width(Fill);
    let intervals: Vec<_> = laps
        .iter()
        .copied()
        .scan(Duration::ZERO, |previous, total| {
            let interval = total - *previous;
            *previous = total;
            Some(interval)
        })
        .collect();
    let fastest = intervals.iter().min();
    let slowest = intervals.iter().max();

    for (index, (&interval, &total)) in intervals.iter().zip(laps).enumerate().rev() {
        let (note, tint) = if fastest != slowest && Some(&interval) == fastest {
            (" · 最快", MINT)
        } else if fastest != slowest && Some(&interval) == slowest {
            (" · 最慢", color!(0xe1ad90))
        } else {
            ("", INK)
        };
        let lap = row![
            text(format!("第 {} 段{note}", index + 1))
                .size(14)
                .color(tint)
                .width(Fill),
            text(clock(interval))
                .font(Font::MONOSPACE)
                .size(14)
                .color(tint)
                .width(Fill)
                .align_x(Alignment::End),
            text(clock(total))
                .font(Font::MONOSPACE)
                .size(14)
                .color(MUTED)
                .width(Fill)
                .align_x(Alignment::End),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        rows = rows
            .push(container(lap).padding([10, 10]).width(Fill))
            .push(separator());
    }

    container(column![
        container(header).padding([7, 10]).width(Fill),
        separator(),
        scrollable(rows)
            .id("clock-scroll")
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(4).scroller_width(3),
            ))
            .height(Fill)
            .width(Fill),
    ])
    .width(Fill)
    .max_width(536)
    .height(Fill)
    .into()
}

fn separator() -> rule::Rule<'static> {
    rule::horizontal(1).style(|_| rule::Style {
        color: RULE,
        radius: 0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    })
}

fn stopwatch_button(
    label: &'static str,
    message: Option<Message>,
    primary: bool,
    running: bool,
) -> Element<'static, Message> {
    button(text(label).size(15).width(Fill).align_x(Alignment::Center))
        .width(Fill)
        .padding([11, 14])
        .on_press_maybe(message)
        .style(move |_, status| {
            if primary && !running {
                let background = match status {
                    button::Status::Hovered => color!(0xb8e9df),
                    button::Status::Pressed => color!(0x76b6aa),
                    _ => MINT,
                };
                button::Style {
                    background: Some(background.into()),
                    text_color: PAPER,
                    border: border::rounded(9),
                    ..Default::default()
                }
            } else {
                let (base, hovered, label) = if primary {
                    (color!(0x513437), color!(0x634248), color!(0xf6c3bc))
                } else {
                    (color!(0x253642), color!(0x314b57), INK)
                };
                let mut style = action_style(status, base, hovered);
                style.text_color = if status == button::Status::Disabled {
                    Color::from_rgb8(0x74, 0x8a, 0x98)
                } else {
                    label
                };
                style.border = border::rounded(9).width(1).color(RULE);
                style
            }
        })
        .into()
}
