use chrono::{DateTime, Utc};
use lintel::iced::{
    Alignment, Element, Fill, Font, Length, Theme, border, color,
    widget::{
        button, column, container, grid, progress_bar, responsive, row, space, text, text_input,
    },
};

use crate::{INK, MINT, MUTED, PAPER, RULE, SURFACE, WARNING, action, panel};

use super::{
    Message, Sound, Timer, TimerDuration, TimerPanel, TimerPart, TimerSpec, Tools, card, control,
    countdown, field, sound_picker, timer_presets, timer_title,
};

impl Tools {
    pub(super) fn timers_view(&self, now: DateTime<Utc>) -> Element<'_, Message> {
        let content = match self.timer_panel {
            TimerPanel::Custom => self.timer_editor(now, true),
            TimerPanel::Presets => container(timer_presets())
                .max_width(600)
                .center_x(Fill)
                .into(),
            TimerPanel::Overview => self.timer_overview(now),
        };
        if self.timer_add_menu {
            column![self.timer_add_menu(), content]
                .width(Fill)
                .spacing(14)
                .into()
        } else {
            content
        }
    }

    fn timer_add_menu(&self) -> Element<'_, Message> {
        let mut menu = column![
            row![
                control("自定计时器", Message::NewTimer),
                control("预设", Message::ShowTimerPresets),
            ]
            .spacing(8)
        ]
        .spacing(8);
        if !self.saved.recent.is_empty() {
            menu = menu.push(text("最近使用").size(14).color(MUTED));
        }
        for spec in &self.saved.recent {
            menu = menu
                .push(action(timer_title(spec), Message::StartRecent(spec.clone())).width(Fill));
        }
        container(card(menu)).max_width(600).center_x(Fill).into()
    }

    fn timer_editor(&self, now: DateTime<Utc>, detail: bool) -> Element<'_, Message> {
        let form = &self.timer_form;
        let timer = form
            .id
            .and_then(|id| self.saved.timers.iter().find(|timer| timer.id == id));
        let valid_duration = form.duration.seconds() > 0;
        let mut instrument = column![].align_x(Alignment::Center).spacing(18);
        if detail {
            instrument = instrument.push(
                row![
                    control("返回", Message::CancelTimerEdit),
                    space::horizontal(),
                    text(if timer.is_some() {
                        "编辑计时器"
                    } else {
                        "自定计时器"
                    })
                    .size(15)
                    .color(MUTED),
                ]
                .width(Fill)
                .align_y(Alignment::Center),
            );
        }

        if let Some(timer) = timer {
            let remaining = timer.remaining(now);
            let display = countdown((remaining as u64).div_ceil(1000));
            instrument = instrument
                .push(
                    responsive(move |size| {
                        text(display.clone())
                            .font(Font::MONOSPACE)
                            .size((size.width * 0.14).clamp(38.0, 77.0))
                            .color(INK)
                            .width(Fill)
                            .align_x(Alignment::Center)
                            .into()
                    })
                    .height(Length::Shrink),
                )
                .push(timer_progress(timer, remaining))
                .push(
                    action(timer_control_title(timer), Message::ToggleTimer(timer.id))
                        .width(136)
                        .padding([10, 14]),
                );
        } else {
            instrument = instrument.push(duration_fields(&form.duration));
        }

        let mut options = column![
            field("名称", "计时器", &form.label, Message::TimerLabel),
            sound_picker(
                "计时结束时",
                Sound::TIMER_OPTIONS.as_slice(),
                form.sound,
                Message::TimerSound,
            ),
        ]
        .width(Fill)
        .spacing(12);

        if form.sound == Sound::StopMedia {
            options = options
                .push(text(crate::media::support_hint()).size(13).color(MUTED))
                .push(
                    action(
                        crate::media::authorization_label(),
                        Message::RequestMediaAuthorization,
                    )
                    .on_press_maybe(
                        (!self.media_pending).then_some(Message::RequestMediaAuthorization),
                    ),
                );
            if let Some(result) = &self.media_authorization {
                options = options.push(text(result).size(13).color(MUTED));
            }
        }

        instrument = instrument.push(container(options).max_width(328).center_x(Fill));

        let editing = form.id.is_some();
        let primary = primary_button(
            if editing { "保存" } else { "开始计时" },
            (editing || valid_duration).then_some(Message::StartTimer),
        );
        let (secondary_title, secondary_message) = match form.id {
            Some(id) => ("取消计时", Some(Message::DeleteTimer(id))),
            None => ("取消", detail.then_some(Message::CancelTimerEdit)),
        };
        let secondary = button(
            text(secondary_title)
                .size(15)
                .width(Fill)
                .align_x(Alignment::Center),
        )
        .width(Fill)
        .padding([11, 14])
        .on_press_maybe(secondary_message)
        .style(|_, status| crate::action_style(status, color!(0x243843), color!(0x314e58)));

        instrument = instrument.push(
            container(row![secondary, primary].spacing(12).width(Fill))
                .max_width(328)
                .center_x(Fill),
        );
        if let Some(error) = &self.form_error {
            instrument = instrument.push(text(*error).size(14).color(WARNING));
        }

        container(
            container(instrument.width(Fill))
                .max_width(620)
                .padding([22, 16])
                .style(|_| instrument_style()),
        )
        .center_x(Fill)
        .padding([if detail { 22 } else { 46 }, 0])
        .into()
    }

    fn timer_overview(&self, now: DateTime<Utc>) -> Element<'_, Message> {
        let mut body = column![].spacing(22).width(Fill);
        if self.saved.timers.is_empty() {
            body = body.push(self.timer_editor(now, false));
        } else {
            body = body.push(
                column![
                    text("正在计时").size(18).color(INK),
                    responsive(move |size| {
                        let columns = grid_columns(size.width, self.saved.timers.len(), 268.0);
                        grid(
                            self.saved
                                .timers
                                .iter()
                                .map(|timer| active_timer_card(timer, now)),
                        )
                        .columns(columns)
                        .spacing(12)
                        .height(Length::Shrink)
                        .into()
                    })
                    .height(Length::Shrink),
                ]
                .spacing(12)
                .width(Fill),
            );
        }
        if !self.saved.recent.is_empty() {
            body = body.push(
                column![
                    text("最近使用").size(18).color(INK),
                    responsive(move |size| {
                        let columns = grid_columns(size.width, self.saved.recent.len(), 204.0);
                        grid(self.saved.recent.iter().map(recent_timer_card))
                            .columns(columns)
                            .spacing(12)
                            .height(Length::Shrink)
                            .into()
                    })
                    .height(Length::Shrink),
                ]
                .spacing(12)
                .width(Fill),
            );
        }
        if let Some(result) = &self.saved.last_media_result {
            body = body.push(
                container(
                    text(format!("上次媒体操作：{result}"))
                        .size(12)
                        .color(MUTED),
                )
                .center_x(Fill),
            );
        }
        body.into()
    }
}

fn duration_fields(duration: &TimerDuration) -> Element<'_, Message> {
    responsive(move |size| {
        let font_size = (size.width * 0.133).clamp(37.0, 65.0);
        let can_start = duration.seconds() > 0;
        let mut fields = row![].spacing(12).width(Fill);
        for part in TimerPart::ALL {
            let number = text_input("00", &duration.parts[part as usize])
                .font(Font::MONOSPACE)
                .size(font_size)
                .align_x(Alignment::Center)
                .padding([8, 4])
                .on_input(move |value| Message::TimerPartChanged(part, value))
                .on_submit_maybe(can_start.then_some(Message::StartTimer))
                .style(digit_style);
            let current = duration.value(part);
            let step = row![
                step_button(part, false, current > 0),
                step_button(part, true, current < part.max()),
            ]
            .width(Fill)
            .spacing(6);
            fields = fields.push(
                column![text(part.unit()).size(13).color(MUTED), number, step,]
                    .spacing(6)
                    .align_x(Alignment::Center)
                    .width(Fill),
            );
        }
        container(container(fields).width(Fill).max_width(500))
            .center_x(Fill)
            .into()
    })
    .height(Length::Shrink)
    .into()
}

fn digit_style(_: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: if focused { color!(0x1d3441) } else { PAPER }.into(),
        border: border::rounded(10)
            .width(1)
            .color(if focused { MINT } else { RULE }),
        icon: MUTED,
        placeholder: MUTED,
        value: INK,
        selection: color!(0x3f6169),
    }
}

fn step_button(part: TimerPart, increase: bool, enabled: bool) -> button::Button<'static, Message> {
    button(
        text(if increase { "+" } else { "−" })
            .size(16)
            .width(Fill)
            .align_x(Alignment::Center),
    )
    .padding([3, 7])
    .width(Fill)
    .on_press_maybe(enabled.then_some(Message::StepTimerPart(part, increase)))
    .style(|_, status| crate::action_style(status, color!(0x213343), color!(0x2b4653)))
}

fn primary_button<'a>(title: &'a str, message: Option<Message>) -> button::Button<'a, Message> {
    button(text(title).size(15).width(Fill).align_x(Alignment::Center))
        .padding([11, 14])
        .width(Fill)
        .on_press_maybe(message)
        .style(|_, status| {
            let background = match status {
                button::Status::Hovered => color!(0xb0eadc),
                button::Status::Pressed => color!(0x78c3b2),
                button::Status::Disabled => color!(0x22313e),
                button::Status::Active => MINT,
            };
            button::Style {
                background: Some(background.into()),
                text_color: if status == button::Status::Disabled {
                    MUTED
                } else {
                    PAPER
                },
                border: border::rounded(8),
                ..button::Style::default()
            }
        })
}

fn timer_control_title(timer: &Timer) -> &'static str {
    if timer.deadline_ms.is_some() {
        "暂停"
    } else {
        "继续"
    }
}

fn timer_progress(timer: &Timer, remaining: i64) -> Element<'static, Message> {
    let duration = timer.spec.seconds.max(1) as f32 * 1_000.0;
    let ratio = (remaining as f32 / duration).clamp(0.0, 1.0);
    progress_bar(0.0..=1.0, ratio)
        .length(Fill)
        .girth(4)
        .style(|_| progress_bar::Style {
            background: RULE.into(),
            bar: MINT.into(),
            border: border::rounded(2),
        })
        .into()
}

fn active_timer_card(timer: &Timer, now: DateTime<Utc>) -> Element<'static, Message> {
    let remaining = timer.remaining(now);
    let running = timer.deadline_ms.is_some();
    let status = text(if running { "倒计时中" } else { "已暂停" })
        .size(12)
        .color(if running { MINT } else { MUTED });
    timer_card(
        column![
            row![
                text(timer.spec.label.clone())
                    .size(16)
                    .color(INK)
                    .width(Fill),
                status,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(countdown((remaining as u64).div_ceil(1_000)))
                .font(Font::MONOSPACE)
                .size(34)
                .color(INK),
            timer_progress(timer, remaining),
            row![
                control(timer_control_title(timer), Message::ToggleTimer(timer.id)),
                control("编辑", Message::EditTimer(timer.id)),
                control("取消", Message::DeleteTimer(timer.id)),
            ]
            .spacing(7),
        ]
        .spacing(11)
        .width(Fill),
    )
}

fn recent_timer_card(spec: &TimerSpec) -> Element<'static, Message> {
    timer_card(
        column![
            text(countdown(spec.seconds))
                .font(Font::MONOSPACE)
                .size(29)
                .color(INK),
            text(spec.label.clone()).size(14).color(MUTED),
            row![
                control("开始计时", Message::StartRecent(spec.clone())),
                control("移除", Message::DeleteRecent(spec.clone())),
            ]
            .spacing(7),
        ]
        .spacing(10)
        .width(Fill),
    )
}

fn timer_card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Fill)
        .padding(14)
        .style(|_| instrument_style())
        .into()
}

fn instrument_style() -> container::Style {
    panel(SURFACE, 12).border(border::rounded(12).width(1).color(RULE))
}

fn grid_columns(width: f32, count: usize, card_width: f32) -> usize {
    const GAP: f32 = 12.0;
    (((width + GAP) / (card_width + GAP)).floor() as usize).clamp(1, count.max(1))
}
