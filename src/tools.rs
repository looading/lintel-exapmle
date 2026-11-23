mod alarms_view;
mod stopwatch_view;
mod timers_view;

use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, Days, Local, NaiveTime, TimeZone, Utc};
use lintel::iced::{
    Alignment, Color, Element, Fill, Point, Rectangle, Renderer, Task, Theme, mouse,
    widget::{canvas, column, container, pick_list, row, text, text_input},
};
use serde::{Deserialize, Serialize};

use crate::{INK, MINT, MUTED, SURFACE, action, panel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    World,
    Alarms,
    Stopwatch,
    Timers,
}

impl Page {
    pub const ALL: [(Self, &str); 4] = [
        (Self::World, "世界时钟"),
        (Self::Alarms, "闹钟"),
        (Self::Stopwatch, "秒表"),
        (Self::Timers, "计时器"),
    ];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sound {
    #[default]
    Ping,
    Glass,
    Hero,
    Silent,
    StopMedia,
}

impl Sound {
    const ALARM_OPTIONS: [Self; 4] = [Self::Ping, Self::Glass, Self::Hero, Self::Silent];
    const TIMER_OPTIONS: [Self; 5] = [
        Self::Ping,
        Self::Glass,
        Self::Hero,
        Self::Silent,
        Self::StopMedia,
    ];

    fn play(self) {
        #[cfg(target_os = "windows")]
        crate::windows_audio::play(self);

        #[cfg(target_os = "macos")]
        {
            let name = match self {
                Self::Ping => "Ping",
                Self::Glass => "Glass",
                Self::Hero => "Hero",
                Self::Silent | Self::StopMedia => return,
            };
            match std::process::Command::new("/usr/bin/afplay")
                .arg(format!("/System/Library/Sounds/{name}.aiff"))
                .status()
            {
                Ok(status) if status.success() => {}
                result => eprintln!("clock audio could not play {name}: {result:?}"),
            }
        }
    }
}

impl std::fmt::Display for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ping => "清脆",
            Self::Glass => "玻璃",
            Self::Hero => "号角",
            Self::Silent => "静音",
            Self::StopMedia => "停止媒体播放",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Alarm {
    id: u64,
    time: NaiveTime,
    label: String,
    weekdays: u8,
    snooze: bool,
    sound: Sound,
    next: Option<i64>,
}

#[derive(Default)]
struct AlarmForm {
    id: Option<u64>,
    hour: String,
    minute: String,
    label: String,
    weekdays: u8,
    snooze: bool,
    sound: Sound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerSpec {
    seconds: u64,
    label: String,
    sound: Sound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Timer {
    id: u64,
    spec: TimerSpec,
    remaining_ms: i64,
    deadline_ms: Option<i64>,
}

impl Timer {
    fn remaining(&self, now: DateTime<Utc>) -> i64 {
        self.deadline_ms
            .map_or(self.remaining_ms, |deadline| {
                deadline.saturating_sub(now.timestamp_millis())
            })
            .max(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerPanel {
    Overview,
    Custom,
    Presets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerPart {
    Hours,
    Minutes,
    Seconds,
}

impl TimerPart {
    const ALL: [Self; 3] = [Self::Hours, Self::Minutes, Self::Seconds];

    const fn unit(self) -> &'static str {
        match self {
            Self::Hours => "小时",
            Self::Minutes => "分钟",
            Self::Seconds => "秒",
        }
    }

    const fn max(self) -> u64 {
        match self {
            Self::Hours => 23,
            _ => 59,
        }
    }
}

struct TimerDuration {
    parts: [String; 3],
}

impl TimerDuration {
    const MAX: u64 = 24 * 60 * 60 - 1;

    fn new(seconds: u64) -> Self {
        Self {
            parts: [seconds / 3600, seconds / 60 % 60, seconds % 60]
                .map(|part| format!("{part:02}")),
        }
    }

    fn value(&self, part: TimerPart) -> u64 {
        self.parts[part as usize].parse().unwrap_or(0)
    }

    fn set(&mut self, part: TimerPart, value: String) {
        let field = &mut self.parts[part as usize];
        if value.is_empty()
            || (value.len() <= 2
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && value.parse::<u64>().is_ok_and(|value| value <= part.max()))
        {
            *field = value;
        }
    }

    fn step(&mut self, part: TimerPart, increase: bool) {
        let value = self.value(part);
        let next = if increase {
            (value + 1).min(part.max())
        } else {
            value.saturating_sub(1)
        };
        self.parts[part as usize] = format!("{next:02}");
    }

    fn seconds(&self) -> u64 {
        self.value(TimerPart::Hours) * 3600
            + self.value(TimerPart::Minutes) * 60
            + self.value(TimerPart::Seconds)
    }
}

impl Default for TimerDuration {
    fn default() -> Self {
        Self::new(300)
    }
}

#[derive(Default)]
struct TimerForm {
    id: Option<u64>,
    duration: TimerDuration,
    label: String,
    sound: Sound,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    next_id: u64,
    alarms: Vec<Alarm>,
    timers: Vec<Timer>,
    recent: Vec<TimerSpec>,
    snoozed: Vec<(u64, i64)>,
    analog_stopwatch: bool,
    alerts: VecDeque<Alert>,
    last_media_result: Option<String>,
}

impl Saved {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopwatchFace {
    Digital,
    Analog,
}

impl std::fmt::Display for StopwatchFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Digital => "数字表盘",
            Self::Analog => "指针表盘",
        })
    }
}

#[derive(Default)]
struct Stopwatch {
    elapsed: Duration,
    started: Option<Instant>,
    laps: Vec<Duration>,
}

impl Stopwatch {
    fn elapsed(&self) -> Duration {
        self.elapsed + self.started.map_or(Duration::ZERO, |start| start.elapsed())
    }

    fn toggle(&mut self) {
        if let Some(start) = self.started.take() {
            self.elapsed += start.elapsed();
        } else {
            self.started = Some(Instant::now());
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
enum Alert {
    Alarm(Alarm),
    Timer(TimerSpec),
}

impl Alert {
    fn details(&self) -> (&'static str, &str, Sound) {
        match self {
            Self::Alarm(alarm) => ("闹钟响了", &alarm.label, alarm.sound),
            Self::Timer(spec) => ("计时结束", &spec.label, spec.sound),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    NewAlarm,
    EditAlarm(u64),
    CloseAlarm,
    AlarmHour(String),
    AlarmMinute(String),
    AlarmLabel(String),
    AlarmDay(u8),
    AlarmSnooze(bool),
    AlarmSound(Sound),
    SaveAlarm,
    ToggleAlarm(u64, bool),
    DeleteAlarm(u64),
    StopwatchToggle,
    Lap,
    ResetStopwatch,
    SetStopwatchFace(StopwatchFace),
    ToggleTimerAddMenu,
    NewTimer,
    ShowTimerPresets,
    TimerPartChanged(TimerPart, String),
    StepTimerPart(TimerPart, bool),
    TimerLabel(String),
    TimerSound(Sound),
    StartTimer,
    TimerPreset(u64),
    StartRecent(TimerSpec),
    DeleteRecent(TimerSpec),
    EditTimer(u64),
    CancelTimerEdit,
    ToggleTimer(u64),
    DeleteTimer(u64),
    Dismiss,
    Snooze,
    RepeatTimer,
    RequestMediaAuthorization,
    MediaAuthorization(String),
    Delivered,
}

pub struct Tools {
    saved: Saved,
    storage: Option<PathBuf>,
    alarm_form: Option<AlarmForm>,
    timer_form: TimerForm,
    timer_panel: TimerPanel,
    timer_add_menu: bool,
    stopwatch: Stopwatch,
    form_error: Option<&'static str>,
    storage_error: Option<&'static str>,
    media_pending: bool,
    media_authorization: Option<String>,
}

impl Tools {
    pub fn load(storage: Option<PathBuf>) -> Self {
        let result = storage.as_deref().map(read_saved);
        let (saved, storage_error) = match result {
            Some(Ok(saved)) => (saved, None),
            None => (Saved::default(), None),
            Some(Err(_)) => (Saved::default(), Some("无法读取闹钟和计时器设置。")),
        };
        let timer_form = timer_draft(&saved);
        Self {
            saved,
            storage,
            alarm_form: None,
            timer_form,
            timer_panel: TimerPanel::Overview,
            timer_add_menu: false,
            stopwatch: Stopwatch::default(),
            form_error: None,
            storage_error,
            media_pending: false,
            media_authorization: None,
        }
    }

    fn save(&mut self) -> bool {
        let saved = self.storage.as_deref().is_none_or(|path| {
            crate::write_config(path, |file| {
                serde_json::to_writer(file, &self.saved).map_err(io::Error::from)
            })
            .is_ok()
        });
        self.storage_error = (!saved).then_some("无法保存闹钟和计时器设置。");
        saved
    }

    fn refresh_locked(&mut self) -> Option<Option<fs::File>> {
        let Some(path) = &self.storage else {
            return Some(None);
        };
        match lock_store(path).and_then(|lock| read_saved(path).map(|saved| (lock, saved))) {
            Ok((lock, saved)) => {
                self.saved = saved;
                self.storage_error = None;
                Some(Some(lock))
            }
            Err(_) => {
                self.storage_error = Some("无法读取闹钟和计时器设置。");
                None
            }
        }
    }

    pub fn stopwatch_running(&self) -> bool {
        self.stopwatch.started.is_some()
    }

    pub fn clear_error(&mut self) {
        self.form_error = None;
    }

    pub fn tick(&mut self, now: DateTime<Utc>) -> Task<Message> {
        let _owner = if let Some(path) = &self.storage {
            let result = open_lock(&path.with_extension("agent.lock")).and_then(|lock| match lock
                .try_lock()
            {
                Ok(()) => Ok(Some(lock)),
                Err(fs::TryLockError::WouldBlock) => Ok(None),
                Err(fs::TryLockError::Error(error)) => Err(error),
            });
            match result {
                Ok(Some(lock)) => Some(lock),
                Ok(None) => {
                    let _store = self.refresh_locked();
                    self.close_expired_editor();
                    return Task::none();
                }
                Err(_) => {
                    self.storage_error = Some("无法访问闹钟和计时器的后台状态。");
                    return Task::none();
                }
            }
        } else {
            None
        };
        let notices = self.collect_due(now);
        if notices.is_empty() {
            return Task::none();
        }
        let storage = self.storage.clone();
        Task::perform(async move { deliver_in_thread(notices, storage) }, |_| {
            Message::Delivered
        })
    }

    fn collect_due(&mut self, now: DateTime<Utc>) -> Vec<Alert> {
        // The same lock protects reads and replacements in the window, adapter and login agent.
        let Some(_lock) = self.refresh_locked() else {
            return Vec::new();
        };
        let mut triggered = Vec::new();
        let mut changed = false;
        for alarm in &mut self.saved.alarms {
            if alarm.next.is_some_and(|next| next <= now.timestamp()) {
                alarm.next = (alarm.weekdays != 0)
                    .then(|| next_alarm(now, alarm.time, alarm.weekdays))
                    .flatten();
                triggered.push(Alert::Alarm(alarm.clone()));
            }
        }
        self.saved.snoozed.retain(|&(id, due)| {
            if due > now.timestamp() {
                return true;
            }
            changed = true;
            if let Some(alarm) = self.saved.alarms.iter().find(|alarm| alarm.id == id) {
                if !triggered
                    .iter()
                    .any(|alert| matches!(alert, Alert::Alarm(item) if item.id == id))
                {
                    triggered.push(Alert::Alarm(alarm.clone()));
                }
            }
            false
        });
        self.saved.timers.retain(|timer| {
            if timer.deadline_ms.is_some() && timer.remaining(now) == 0 {
                triggered.push(Alert::Timer(timer.spec.clone()));
                false
            } else {
                true
            }
        });
        self.close_expired_editor();
        if triggered.is_empty() && !changed {
            return triggered;
        }

        self.saved.alerts.extend(triggered.iter().cloned());
        while self.saved.alerts.len() > 64 {
            self.saved.alerts.pop_front();
        }
        if self.save() { triggered } else { Vec::new() }
    }

    fn close_expired_editor(&mut self) {
        if self
            .timer_form
            .id
            .is_some_and(|id| !self.saved.timers.iter().any(|timer| timer.id == id))
        {
            self.reset_timer_form();
        }
    }

    fn reset_timer_form(&mut self) {
        self.timer_panel = TimerPanel::Overview;
        self.timer_add_menu = false;
        self.timer_form = timer_draft(&self.saved);
    }

    fn start_timer(&mut self, spec: TimerSpec, now: DateTime<Utc>) {
        let id = self.saved.id();
        let remaining_ms = (spec.seconds * 1000) as i64;
        self.saved.timers.push(Timer {
            id,
            spec: spec.clone(),
            remaining_ms,
            deadline_ms: Some(now.timestamp_millis() + remaining_ms),
        });
        self.saved.recent.retain(|item| item != &spec);
        self.saved.recent.insert(0, spec);
        self.saved.recent.truncate(8);
        self.save();
    }

    pub fn update(&mut self, message: Message, now: DateTime<Utc>) {
        self.form_error = None;
        let _lock = match message {
            Message::EditAlarm(_)
            | Message::SaveAlarm
            | Message::ToggleAlarm(..)
            | Message::DeleteAlarm(_)
            | Message::SetStopwatchFace(_)
            | Message::StartTimer
            | Message::TimerPreset(_)
            | Message::StartRecent(_)
            | Message::DeleteRecent(_)
            | Message::EditTimer(_)
            | Message::ToggleTimer(_)
            | Message::DeleteTimer(_)
            | Message::Dismiss
            | Message::Snooze
            | Message::RepeatTimer => {
                let Some(lock) = self.refresh_locked() else {
                    return;
                };
                lock
            }
            _ => None,
        };
        match message {
            Message::NewAlarm => {
                let local = now.with_timezone(&Local);
                self.alarm_form = Some(AlarmForm {
                    hour: local.format("%H").to_string(),
                    minute: local.format("%M").to_string(),
                    snooze: true,
                    ..Default::default()
                })
            }
            Message::EditAlarm(id) => {
                self.alarm_form =
                    self.saved
                        .alarms
                        .iter()
                        .find(|item| item.id == id)
                        .map(|alarm| AlarmForm {
                            id: Some(id),
                            hour: alarm.time.format("%H").to_string(),
                            minute: alarm.time.format("%M").to_string(),
                            label: alarm.label.clone(),
                            weekdays: alarm.weekdays,
                            snooze: alarm.snooze,
                            sound: alarm.sound,
                        });
            }
            Message::CloseAlarm => self.alarm_form = None,
            Message::AlarmHour(value) => {
                if let Some(form) = &mut self.alarm_form {
                    update_alarm_component(&mut form.hour, value, 23);
                }
            }
            Message::AlarmMinute(value) => {
                if let Some(form) = &mut self.alarm_form {
                    update_alarm_component(&mut form.minute, value, 59);
                }
            }
            Message::AlarmLabel(value) => {
                if let Some(form) = &mut self.alarm_form {
                    form.label = value;
                }
            }
            Message::AlarmDay(day) => {
                if let Some(form) = &mut self.alarm_form {
                    form.weekdays ^= 1 << day;
                }
            }
            Message::AlarmSnooze(value) => {
                if let Some(form) = &mut self.alarm_form {
                    form.snooze = value;
                }
            }
            Message::AlarmSound(value) => {
                if let Some(form) = &mut self.alarm_form {
                    form.sound = value;
                }
            }
            Message::SaveAlarm => self.commit_alarm(now),
            Message::ToggleAlarm(id, enabled) => {
                if let Some(alarm) = self.saved.alarms.iter_mut().find(|alarm| alarm.id == id) {
                    alarm.next = enabled
                        .then(|| next_alarm(now, alarm.time, alarm.weekdays))
                        .flatten();
                    self.saved.snoozed.retain(|&(alarm_id, _)| alarm_id != id);
                    self.save();
                }
            }
            Message::DeleteAlarm(id) => {
                self.saved.alarms.retain(|alarm| alarm.id != id);
                self.saved.snoozed.retain(|&(alarm_id, _)| alarm_id != id);
                self.saved
                    .alerts
                    .retain(|alert| !matches!(alert, Alert::Alarm(alarm) if alarm.id == id));
                self.save();
            }
            Message::StopwatchToggle => self.stopwatch.toggle(),
            Message::Lap => {
                if self.stopwatch_running() {
                    self.stopwatch.laps.push(self.stopwatch.elapsed());
                }
            }
            Message::ResetStopwatch => self.stopwatch = Stopwatch::default(),
            Message::SetStopwatchFace(face) => {
                self.saved.analog_stopwatch = face == StopwatchFace::Analog;
                self.save();
            }
            Message::ToggleTimerAddMenu => self.timer_add_menu = !self.timer_add_menu,
            Message::NewTimer => {
                self.timer_add_menu = false;
                if self.timer_form.id.is_some() {
                    self.reset_timer_form();
                }
                self.timer_panel = TimerPanel::Custom;
            }
            Message::ShowTimerPresets => {
                self.timer_add_menu = false;
                self.timer_panel = TimerPanel::Presets;
            }
            Message::TimerPartChanged(part, value) => self.timer_form.duration.set(part, value),
            Message::StepTimerPart(part, increase) => self.timer_form.duration.step(part, increase),
            Message::TimerLabel(value) => self.timer_form.label = value,
            Message::TimerSound(value) => self.timer_form.sound = value,
            Message::StartTimer => self.commit_timer(now),
            Message::TimerPreset(seconds) => {
                self.start_timer(
                    TimerSpec {
                        seconds,
                        label: "计时器".into(),
                        sound: Sound::default(),
                    },
                    now,
                );
                self.reset_timer_form();
            }
            Message::StartRecent(spec) => {
                self.start_timer(spec, now);
                self.reset_timer_form();
            }
            Message::DeleteRecent(spec) => {
                self.saved.recent.retain(|recent| recent != &spec);
                self.save();
            }
            Message::EditTimer(id) => {
                if let Some(timer) = self.saved.timers.iter().find(|timer| timer.id == id) {
                    self.timer_add_menu = false;
                    self.timer_panel = TimerPanel::Custom;
                    self.timer_form = TimerForm {
                        id: Some(id),
                        duration: TimerDuration::default(),
                        label: timer.spec.label.clone(),
                        sound: timer.spec.sound,
                    };
                }
            }
            Message::CancelTimerEdit => {
                if self.timer_add_menu {
                    self.timer_add_menu = false;
                } else {
                    self.reset_timer_form();
                }
            }
            Message::ToggleTimer(id) => {
                if let Some(timer) = self.saved.timers.iter_mut().find(|timer| timer.id == id) {
                    if timer.deadline_ms.is_some() {
                        timer.remaining_ms = timer.remaining(now);
                        timer.deadline_ms = None;
                    } else {
                        timer.deadline_ms = Some(now.timestamp_millis() + timer.remaining_ms);
                    }
                    self.save();
                }
            }
            Message::DeleteTimer(id) => {
                self.saved.timers.retain(|timer| timer.id != id);
                if self.timer_form.id == Some(id) {
                    self.reset_timer_form();
                }
                self.save();
            }
            Message::Dismiss => {
                if self.saved.alerts.pop_front().is_some() {
                    self.save();
                }
            }
            Message::Snooze => {
                if let Some(Alert::Alarm(alarm)) = self.saved.alerts.pop_front()
                    && self.saved.alarms.iter().any(|item| item.id == alarm.id)
                {
                    self.saved
                        .snoozed
                        .push((alarm.id, now.timestamp() + 9 * 60));
                    self.save();
                }
            }
            Message::RepeatTimer => {
                if let Some(Alert::Timer(spec)) = self.saved.alerts.pop_front() {
                    self.start_timer(spec, now);
                }
            }
            Message::MediaAuthorization(result) => {
                self.media_pending = false;
                self.media_authorization = Some(result);
            }
            Message::RequestMediaAuthorization | Message::Delivered => {}
        }
    }

    pub fn authorize_media(&mut self) -> Task<Message> {
        if self.media_pending {
            return Task::none();
        }
        self.media_pending = true;
        Task::perform(
            async { crate::media::authorize_playback().summary() },
            Message::MediaAuthorization,
        )
    }

    pub fn perform_shortcut(
        &mut self,
        shortcut: crate::shortcuts::Shortcut,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let Some(_lock) = self.refresh_locked() else {
            return Err("无法读取闹钟和计时器设置。".into());
        };
        match shortcut {
            crate::shortcuts::Shortcut::Timer {
                seconds,
                label: name,
            } => {
                self.start_timer(
                    TimerSpec {
                        seconds,
                        label: label(&name, "计时器"),
                        sound: Sound::default(),
                    },
                    now,
                );
            }
            crate::shortcuts::Shortcut::Alarm { time, label: name } => {
                let id = self.saved.id();
                self.saved.alarms.push(Alarm {
                    id,
                    time,
                    label: label(&name, "闹钟"),
                    weekdays: 0,
                    snooze: true,
                    sound: Sound::default(),
                    next: next_alarm(now, time, 0),
                });
                self.save();
            }
        }
        self.storage_error.map_or(Ok(()), |error| Err(error.into()))
    }

    fn commit_alarm(&mut self, now: DateTime<Utc>) {
        let Some(form) = self.alarm_form.as_ref() else {
            return;
        };
        let Some(time) = form
            .hour
            .parse()
            .ok()
            .zip(form.minute.parse().ok())
            .and_then(|(hour, minute)| NaiveTime::from_hms_opt(hour, minute, 0))
        else {
            self.form_error = Some("请填写小时和分钟。");
            return;
        };
        let form = self.alarm_form.take().unwrap();
        let existing = form
            .id
            .and_then(|id| self.saved.alarms.iter().position(|alarm| alarm.id == id));
        let id = form.id.unwrap_or_else(|| self.saved.id());
        let enabled = existing.is_none_or(|index| self.saved.alarms[index].next.is_some());
        let alarm = Alarm {
            id,
            time,
            label: label(&form.label, "闹钟"),
            weekdays: form.weekdays,
            snooze: form.snooze,
            sound: form.sound,
            next: enabled
                .then(|| next_alarm(now, time, form.weekdays))
                .flatten(),
        };
        if let Some(index) = existing {
            self.saved.alarms[index] = alarm;
            self.saved.snoozed.retain(|&(alarm_id, _)| alarm_id != id);
        } else {
            self.saved.alarms.push(alarm);
        }
        self.save();
    }

    fn commit_timer(&mut self, now: DateTime<Utc>) {
        let name = label(&self.timer_form.label, "计时器");
        if let Some(id) = self.timer_form.id {
            if let Some(timer) = self.saved.timers.iter_mut().find(|timer| timer.id == id) {
                timer.spec.label = name;
                timer.spec.sound = self.timer_form.sound;
                self.save();
            }
            self.reset_timer_form();
            return;
        }
        let seconds = self.timer_form.duration.seconds();
        if seconds == 0 {
            self.form_error = Some("请输入至少 1 秒的时长。");
            return;
        }
        let sound = self.timer_form.sound;
        self.start_timer(
            TimerSpec {
                seconds,
                label: name,
                sound,
            },
            now,
        );
        self.reset_timer_form();
    }

    pub fn messages(&self) -> Option<Element<'_, Message>> {
        let mut content = column![].spacing(8);
        if let Some(error) = self.storage_error {
            content = content.push(text(error).color(INK));
        }
        if let Some(alert) = self.saved.alerts.front() {
            let (title, name, _) = alert.details();
            let mut buttons = row![control("停止", Message::Dismiss)].spacing(8);
            match alert {
                Alert::Alarm(alarm) if alarm.snooze => {
                    buttons = buttons.push(control("9 分钟后提醒", Message::Snooze));
                }
                Alert::Timer(_) => {
                    buttons = buttons.push(control("再计一次", Message::RepeatTimer))
                }
                _ => {}
            }
            content = content.push(card(
                column![
                    text(title).size(20).color(INK),
                    text(name).color(MUTED),
                    buttons
                ]
                .spacing(8),
            ));
        }
        (self.storage_error.is_some() || !self.saved.alerts.is_empty()).then(|| content.into())
    }

    #[cfg(target_os = "macos")]
    pub fn recent_timer_entries(&self) -> Vec<(String, TimerSpec)> {
        self.saved
            .recent
            .iter()
            .map(|spec| (timer_title(spec), spec.clone()))
            .collect()
    }

    pub fn stopwatch_face(&self) -> StopwatchFace {
        if self.saved.analog_stopwatch {
            StopwatchFace::Analog
        } else {
            StopwatchFace::Digital
        }
    }

    pub fn view(
        &self,
        page: Page,
        now: DateTime<Utc>,
        has_native_menu: bool,
    ) -> Element<'_, Message> {
        match page {
            Page::Alarms => self.alarms_view(),
            Page::Stopwatch => self.stopwatch_view(has_native_menu),
            Page::Timers => self.timers_view(now),
            Page::World => column![].into(),
        }
    }

    pub fn alarm_editor_open(&self) -> bool {
        self.alarm_form.is_some()
    }
}

pub(crate) const TIMER_PRESETS: [[(&str, u64); 3]; 4] = [
    [("1 分钟", 60), ("2 分钟", 120), ("3 分钟", 180)],
    [("4 分钟", 240), ("5 分钟", 300), ("10 分钟", 600)],
    [("15 分钟", 900), ("20 分钟", 1200), ("30 分钟", 1800)],
    [("45 分钟", 2700), ("1 小时", 3600), ("2 小时", 7200)],
];

fn timer_presets() -> Element<'static, Message> {
    let mut body = column![
        row![
            control("返回计时器", Message::CancelTimerEdit),
            control("自定计时器", Message::NewTimer),
        ]
        .spacing(8)
    ]
    .spacing(12);
    for group in TIMER_PRESETS {
        let mut choices = row![].spacing(8);
        for (title, duration) in group {
            choices = choices.push(action(title, Message::TimerPreset(duration)).width(Fill));
        }
        body = body.push(choices);
    }
    card(body)
}

fn timer_title(spec: &TimerSpec) -> String {
    format!("{}  ·  {}", spec.label, countdown(spec.seconds))
}

fn timer_draft(saved: &Saved) -> TimerForm {
    TimerForm {
        duration: saved
            .recent
            .iter()
            .find(|spec| spec.seconds <= TimerDuration::MAX)
            .map_or_else(TimerDuration::default, |spec| {
                TimerDuration::new(spec.seconds)
            }),
        ..Default::default()
    }
}

const ALARM_DAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

fn update_alarm_component(field: &mut String, value: String, maximum: u32) {
    if value.is_empty()
        || (value.len() <= 2
            && value.bytes().all(|digit| digit.is_ascii_digit())
            && value.parse::<u32>().is_ok_and(|number| number <= maximum))
    {
        *field = value;
    }
}

fn open_lock(path: &Path) -> io::Result<fs::File> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(parent)?;
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn lock_store(path: &Path) -> io::Result<fs::File> {
    let lock = open_lock(&path.with_extension("lock"))?;
    lock.lock()?;
    Ok(lock)
}

fn read_saved(path: &Path) -> io::Result<Saved> {
    match fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).map_err(io::Error::from),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Saved::default()),
        Err(error) => Err(error),
    }
}

fn deliver_in_thread(alerts: Vec<Alert>, storage: Option<PathBuf>) {
    if alerts
        .iter()
        .any(|alert| matches!(alert, Alert::Timer(spec) if spec.sound == Sound::StopMedia))
    {
        let _ = std::thread::Builder::new()
            .name("clock-media".into())
            .spawn(move || {
                let result = crate::media::stop_playback().summary();
                if let Some(path) = storage {
                    let written = lock_store(&path).and_then(|_lock| {
                        let mut saved = read_saved(&path)?;
                        saved.last_media_result = Some(result);
                        crate::write_config(&path, |file| {
                            serde_json::to_writer(file, &saved).map_err(io::Error::from)
                        })
                    });
                    if let Err(error) = written {
                        eprintln!("clock media result: {error}");
                    }
                }
            });
    }
    let _ = std::thread::Builder::new()
        .name("clock-alerts".into())
        .spawn(move || {
            #[cfg(target_os = "macos")]
            let _ = notify_rust::set_application("dev.lintel.world-clock");
            for alert in alerts {
                let (title, label, sound) = alert.details();
                sound.play();
                if let Err(error) = notify_rust::Notification::new()
                    .summary(title)
                    .body(label)
                    .show()
                {
                    eprintln!("clock notification: {error}");
                }
            }
        });
}

pub fn run_agent(path: PathBuf, keep_running: impl Fn() -> bool) -> io::Result<()> {
    // Prefer the login agent while it is alive, even when the GUI is open.
    let owner = open_lock(&path.with_extension("agent.lock"))?;
    owner.lock()?;
    let mut tools = Tools::load(Some(path));
    while keep_running() {
        let alerts = tools.collect_due(Utc::now());
        if !alerts.is_empty() {
            deliver_in_thread(alerts, tools.storage.clone());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

struct StopwatchDial(Duration);

impl canvas::Program<Message> for StopwatchDial {
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
        let scale = bounds.width.min(bounds.height) / 210.0;
        frame.stroke(
            &canvas::Path::circle(center, 103.0 * scale),
            canvas::Stroke::default()
                .with_color(crate::RULE)
                .with_width(scale),
        );
        frame.stroke(
            &canvas::Path::circle(center, 74.0 * scale),
            canvas::Stroke::default()
                .with_color(crate::RULE)
                .with_width(scale),
        );
        let point = |turn: f32, radius: f32| {
            let angle = turn * std::f32::consts::TAU;
            let radius = radius * scale;
            Point::new(
                center.x + angle.sin() * radius,
                center.y - angle.cos() * radius,
            )
        };
        for tick in 0..60 {
            let major = tick % 5 == 0;
            let turn = tick as f32 / 60.0;
            let mark = canvas::Path::line(
                point(turn, 96.0),
                point(turn, if major { 83.0 } else { 91.0 }),
            );
            frame.stroke(
                &mark,
                canvas::Stroke::default()
                    .with_color(Color::WHITE)
                    .with_width(if major { 2.0 * scale } else { scale }),
            );
        }
        let seconds = self.0.as_secs_f64() % 3600.0;
        let minutes = canvas::Path::line(center, point((seconds / 3600.0) as f32, 53.0));
        frame.stroke(
            &minutes,
            canvas::Stroke::default()
                .with_color(Color::WHITE)
                .with_width(4.0 * scale),
        );
        let hand = canvas::Path::line(center, point((seconds / 60.0) as f32, 80.0));
        frame.stroke(
            &hand,
            canvas::Stroke::default()
                .with_color(MINT)
                .with_width(2.0 * scale),
        );
        frame.fill(&canvas::Path::circle(center, 4.0 * scale), MINT);
        vec![frame.into_geometry()]
    }
}

fn label(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.chars().take(80).collect()
    }
}

fn next_alarm(now: DateTime<Utc>, time: NaiveTime, weekdays: u8) -> Option<i64> {
    let today = now.with_timezone(&Local).date_naive();
    (0..=14).find_map(|offset| {
        let date = today.checked_add_days(Days::new(offset))?;
        if weekdays != 0 && weekdays & (1 << date.weekday().num_days_from_monday()) == 0 {
            return None;
        }
        let choices = Local.from_local_datetime(&date.and_time(time));
        choices
            .earliest()
            .map(|candidate| candidate.timestamp())
            .filter(|&candidate| candidate > now.timestamp())
    })
}

fn countdown(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn clock(elapsed: Duration) -> String {
    format!(
        "{}.{:02}",
        countdown(elapsed.as_secs()),
        elapsed.subsec_millis() / 10
    )
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Fill)
        .padding(16)
        .style(|_| panel(SURFACE, 16))
        .into()
}

fn control<'a>(title: impl Into<String>, message: Message) -> Element<'a, Message> {
    action(title.into(), message).into()
}

fn field<'a>(
    label: &'a str,
    placeholder: &str,
    value: &str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(label).size(14).color(MUTED),
        text_input(placeholder, value).padding(9).on_input(on_input),
    ]
    .spacing(4)
    .into()
}

fn sound_picker(
    label: &'static str,
    options: &'static [Sound],
    sound: Sound,
    on_select: fn(Sound) -> Message,
) -> Element<'static, Message> {
    row![
        text(label).size(14).color(MUTED),
        pick_list(options, Some(sound), on_select),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_leaves_delivery_to_running_agent() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clock.json");
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut foreground = Tools::load(Some(path.clone()));
        foreground.timer_form.duration = TimerDuration::new(1);
        foreground.update(Message::StartTimer, now);
        let owner = open_lock(&path.with_extension("agent.lock")).unwrap();
        owner.lock().unwrap();

        let due = now + chrono::TimeDelta::seconds(1);
        let _ = foreground.tick(due);
        assert!(foreground.saved.alerts.is_empty());
        let mut background = Tools::load(Some(path));
        assert_eq!(background.collect_due(due).len(), 1);
        let _ = foreground.tick(due);
        assert_eq!(foreground.saved.alerts.len(), 1);
    }

    #[test]
    fn two_schedulers_claim_due_timer_once() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clock.json");
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut foreground = Tools::load(Some(path.clone()));
        foreground.timer_form.duration = TimerDuration::new(1);
        foreground.update(Message::StartTimer, now);
        let due = now + chrono::TimeDelta::seconds(1);
        let barrier = std::sync::Barrier::new(2);
        let total = std::thread::scope(|scope| {
            let other = scope.spawn(|| {
                let mut helper = Tools::load(Some(path.clone()));
                barrier.wait();
                helper.collect_due(due).len()
            });
            barrier.wait();
            foreground.collect_due(due).len() + other.join().unwrap()
        });
        assert_eq!(total, 1);
        assert_eq!(Tools::load(Some(path)).saved.alerts.len(), 1);
    }

    #[test]
    fn creating_custom_timer_returns_to_overview_and_keeps_end_action() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clock.json");
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut tools = Tools::load(Some(path.clone()));
        tools.update(Message::NewTimer, now);
        tools.update(Message::ShowTimerPresets, now);
        assert_eq!(tools.timer_panel, TimerPanel::Presets);
        tools.update(Message::NewTimer, now);
        tools.update(
            Message::TimerPartChanged(TimerPart::Minutes, "12".into()),
            now,
        );
        tools.update(
            Message::TimerPartChanged(TimerPart::Seconds, "30".into()),
            now,
        );
        tools.update(Message::TimerLabel("煮茶".into()), now);
        tools.update(Message::TimerSound(Sound::StopMedia), now);
        tools.update(Message::StartTimer, now);
        assert_eq!(tools.timer_panel, TimerPanel::Overview);

        let reopened = Tools::load(Some(path));
        let spec = &reopened.saved.timers[0].spec;
        assert_eq!(spec.seconds, 750);
        assert_eq!(spec.label, "煮茶");
        assert_eq!(spec.sound, Sound::StopMedia);
    }

    #[test]
    fn paused_timer_survives_restart_and_finishes_after_resuming() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clock.json");
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut tools = Tools::load(Some(path.clone()));
        tools.timer_form.duration = TimerDuration::new(10);
        tools.update(Message::TimerSound(Sound::Silent), now);
        tools.update(Message::StartTimer, now);
        let id = tools.saved.timers[0].id;
        tools.update(
            Message::ToggleTimer(id),
            now + chrono::TimeDelta::seconds(3),
        );

        let reopened = now + chrono::TimeDelta::minutes(1);
        let mut tools = Tools::load(Some(path.clone()));
        assert_eq!(tools.saved.timers[0].remaining(reopened), 7000);
        assert_eq!(tools.saved.recent[0].seconds, 10);
        tools.update(Message::ToggleTimer(id), reopened);
        let _ = tools.tick(reopened + chrono::TimeDelta::seconds(7));
        assert!(matches!(tools.saved.alerts.front(), Some(Alert::Timer(_))));
        assert!(Tools::load(Some(path)).saved.timers.is_empty());
    }

    #[test]
    fn one_shot_alarm_snoozes_once_and_remains_disabled() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clock.json");
        let now = Utc::now();
        let mut tools = Tools::load(Some(path.clone()));
        tools.update(Message::NewAlarm, now);
        tools.update(Message::AlarmSound(Sound::Silent), now);
        tools.update(Message::SaveAlarm, now);
        let due = DateTime::from_timestamp(tools.saved.alarms[0].next.unwrap(), 0).unwrap();

        let mut tools = Tools::load(Some(path.clone()));
        let _ = tools.tick(due);
        assert!(matches!(tools.saved.alerts.front(), Some(Alert::Alarm(_))));
        assert!(tools.saved.alarms[0].next.is_none());
        tools.update(Message::Snooze, due);

        let mut tools = Tools::load(Some(path));
        let _ = tools.tick(due + chrono::TimeDelta::minutes(8));
        assert!(tools.saved.alerts.is_empty());
        let _ = tools.tick(due + chrono::TimeDelta::minutes(9));
        assert!(matches!(tools.saved.alerts.front(), Some(Alert::Alarm(_))));
        assert!(tools.saved.snoozed.is_empty());
    }

    #[test]
    fn deleting_recent_timer_does_not_cancel_running_timer_or_change_legacy_duration() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clock.json");
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut tools = Tools::load(Some(path.clone()));
        let legacy = TimerSpec {
            seconds: TimerDuration::MAX + 1,
            label: "旧计时器".into(),
            sound: Sound::Silent,
        };

        tools.update(Message::StartRecent(legacy.clone()), now);
        assert_eq!(tools.saved.recent, vec![legacy.clone()]);
        assert_eq!(tools.timer_form.duration.seconds(), 300);
        tools.update(Message::DeleteRecent(legacy.clone()), now);

        let reopened = Tools::load(Some(path));
        assert!(reopened.saved.recent.is_empty());
        assert_eq!(reopened.saved.timers[0].spec, legacy);
    }

    #[test]
    fn timer_parts_accept_only_two_digits_in_their_own_range() {
        let mut duration = TimerDuration::default();
        duration.set(TimerPart::Hours, "24".into());
        duration.set(TimerPart::Minutes, "60".into());
        duration.set(TimerPart::Seconds, "+1".into());
        duration.set(TimerPart::Seconds, "010".into());
        assert_eq!(duration.seconds(), 300);
        duration.set(TimerPart::Hours, "23".into());
        duration.set(TimerPart::Minutes, "59".into());
        duration.set(TimerPart::Seconds, "59".into());
        duration.step(TimerPart::Seconds, true);
        assert_eq!(duration.seconds(), TimerDuration::MAX);
        duration.set(TimerPart::Hours, String::new());
        duration.step(TimerPart::Seconds, false);
        assert_eq!(duration.seconds(), 3598);
    }
}
