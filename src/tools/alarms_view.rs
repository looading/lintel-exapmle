use super::{ALARM_DAYS, Alarm, Message, Sound, Tools, field, sound_picker};
use crate::{INK, MINT, MUTED, PAPER, RULE, SURFACE, action, panel};
use lintel::iced::{
    Alignment, Color, Element, Fill, Font, Length, Point, Rectangle, Renderer, Theme, border,
    color, mouse,
    widget::{
        button, canvas, checkbox, column, container, grid, responsive, row, rule, scrollable,
        space, text, text_input, toggler,
    },
};

impl Tools {
    pub(crate) fn alarm_editor(&self) -> Option<Element<'_, Message>> {
        let form = self.alarm_form.as_ref()?;
        let hour = time_field("小时", "alarm-hour", &form.hour, Message::AlarmHour);
        let minute = time_field("分钟", "alarm-minute", &form.minute, Message::AlarmMinute);
        let time = row![
            hour,
            text(":").font(Font::MONOSPACE).size(32).color(MINT),
            minute,
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let mut weekdays = row![].spacing(5);
        for (day, name) in ALARM_DAYS.iter().enumerate() {
            let selected = form.weekdays & (1 << day) != 0;
            let chip = button(text(*name).size(14).center().width(Fill))
                .padding([7, 0])
                .width(Fill)
                .on_press(Message::AlarmDay(day as u8))
                .style(move |_, status| {
                    let fill = if selected { color!(0x26464d) } else { PAPER };
                    let mut style = crate::action_style(status, fill, color!(0x314e58));
                    style.border = border::rounded(7).width(1).color(if selected {
                        color!(0x5a988f)
                    } else {
                        RULE
                    });
                    if selected {
                        style.text_color = MINT;
                    }
                    style
                });
            weekdays = weekdays.push(chip);
        }

        let mut editor = column![
            text(if form.id.is_some() {
                "编辑闹钟"
            } else {
                "添加闹钟"
            })
            .size(21)
            .color(INK),
            container(time).center_x(Fill).padding([3, 0]),
            field("名称", "闹钟", &form.label, Message::AlarmLabel),
            column![text("重复日期").size(14).color(MUTED), weekdays].spacing(5),
            sound_picker(
                "提示音",
                Sound::ALARM_OPTIONS.as_slice(),
                form.sound,
                Message::AlarmSound,
            ),
            checkbox(form.snooze)
                .label("允许稍后提醒")
                .on_toggle(Message::AlarmSnooze),
        ]
        .spacing(11)
        .width(Fill);
        if let Some(error) = self.form_error {
            editor = editor.push(text(error).size(13).color(crate::WARNING));
        }
        let save = action("保存", Message::SaveAlarm).style(|_, status| {
            let mut style = crate::action_style(status, MINT, color!(0xb5efe0));
            style.text_color = PAPER;
            style
        });
        Some(
            editor
                .push(
                    row![
                        space::horizontal(),
                        action("取消", Message::CloseAlarm),
                        save,
                    ]
                    .spacing(8),
                )
                .into(),
        )
    }

    pub(super) fn alarms_view(&self) -> Element<'_, Message> {
        if self.saved.alarms.is_empty() {
            let empty = column![
                canvas(EmptyAlarm).width(64).height(64),
                text("未设置闹钟").size(16).color(MUTED),
            ]
            .spacing(12)
            .align_x(Alignment::Center);
            return container(empty).center(Fill).into();
        }

        const CARD_WIDTH: f32 = 226.0;
        const GAP: f32 = 12.0;
        let grid = responsive(move |size| {
            let columns = ((size.width + GAP) / (CARD_WIDTH + GAP)).floor() as usize;
            grid(self.saved.alarms.iter().map(alarm_card))
                .columns(columns.max(1))
                .spacing(GAP)
                .height(Length::Shrink)
                .into()
        })
        .height(Length::Shrink);
        scrollable(container(grid).width(Fill).padding([8, 2]))
            .id("clock-scroll")
            .width(Fill)
            .height(Fill)
            .into()
    }
}

fn time_field<'a>(
    label: &'static str,
    id: &'static str,
    value: &'a str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    let input = text_input("00", value)
        .id(id)
        .on_input(on_input)
        .on_submit(Message::SaveAlarm)
        .font(Font::MONOSPACE)
        .align_x(Alignment::Center)
        .size(36)
        .padding([6, 7])
        .width(96);
    column![input, text(label).size(12).color(MUTED)]
        .spacing(5)
        .align_x(Alignment::Center)
        .into()
}

fn alarm_card(alarm: &Alarm) -> Element<'_, Message> {
    let enabled = alarm.next.is_some();
    let weekdays = match alarm.weekdays {
        0 => "仅一次".into(),
        0b111_1111 => "每天".into(),
        days => format!(
            "周{}",
            ALARM_DAYS
                .iter()
                .enumerate()
                .filter_map(|(day, label)| (days & (1 << day) != 0).then_some(*label))
                .collect::<Vec<_>>()
                .join("、"),
        ),
    };
    let title = container(text(&alarm.label).size(15).color(INK))
        .width(Fill)
        .max_height(23)
        .clip(true);
    let edit = button(
        column![
            title,
            text(alarm.time.format("%H:%M").to_string())
                .font(Font::MONOSPACE)
                .size(37)
                .color(if enabled { INK } else { MUTED }),
            text(format!("{weekdays} · {}", alarm.sound))
                .size(12)
                .color(MUTED),
        ]
        .spacing(5),
    )
    .width(Fill)
    .padding([10, 11])
    .on_press(Message::EditAlarm(alarm.id))
    .style(|_, status| crate::action_style(status, Color::TRANSPARENT, color!(0x29424e)));
    let controls = row![
        toggler(enabled)
            .size(16)
            .label(if enabled { "已开启" } else { "已关闭" })
            .text_size(12)
            .spacing(6)
            .on_toggle(move |value| Message::ToggleAlarm(alarm.id, value)),
        space::horizontal(),
        action("删除", Message::DeleteAlarm(alarm.id))
            .padding([4, 6])
            .style(|_, status| crate::action_style(status, Color::TRANSPARENT, color!(0x41343d),)),
    ]
    .align_y(Alignment::Center);
    container(column![edit, rule::horizontal(1), controls].spacing(7))
        .width(Fill)
        .padding(9)
        .style(move |_| {
            panel(SURFACE, 12).border(border::rounded(12).width(1).color(if enabled {
                color!(0x3b5b62)
            } else {
                RULE
            }))
        })
        .into()
}

struct EmptyAlarm;

impl canvas::Program<Message> for EmptyAlarm {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let center = frame.center();
        let rim = canvas::Path::circle(center, 18.0);
        frame.fill(&rim, color!(0x172831));
        frame.stroke(
            &rim,
            canvas::Stroke::default().with_color(MUTED).with_width(1.5),
        );
        let hands_and_bells = canvas::Path::new(|path| {
            path.move_to(Point::new(center.x, center.y - 9.0));
            path.line_to(center);
            path.line_to(Point::new(center.x + 7.0, center.y));
            for sign in [-1.0f32, 1.0] {
                path.move_to(Point::new(center.x + sign * 14.0, center.y - 20.0));
                path.line_to(Point::new(center.x + sign * 20.0, center.y - 14.0));
                path.move_to(Point::new(center.x + sign * 12.0, center.y + 15.0));
                path.line_to(Point::new(center.x + sign * 16.0, center.y + 20.0));
            }
        });
        frame.stroke(
            &hands_and_bells,
            canvas::Stroke::default()
                .with_color(MINT)
                .with_width(1.65)
                .with_line_cap(canvas::LineCap::Round),
        );
        vec![frame.into_geometry()]
    }
}
