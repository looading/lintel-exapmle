#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod background;
mod cities;
mod city_dial;
mod media;
#[cfg(target_os = "macos")]
mod native_menu;
mod shortcuts;
mod tools;
#[cfg(any(target_os = "windows", test))]
mod windows_audio;
mod world_map;

use shortcuts::Shortcut;
use tools::{Page, Tools};

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Days, Local, Offset, Timelike, Utc};
use chrono_tz::Tz;
use directories::ProjectDirs;
use lintel::iced::{
    Alignment, Color, Element, Event, Fill, Font, Size, Subscription, Task, Theme, border, color,
    event, keyboard, mouse, time,
    widget::{
        button, column, container, grid, mouse_area, opaque, responsive, row, scrollable, space,
        stack, text, text_input, themer,
    },
    window,
};

const INK: Color = color!(0xe3eef3);
const MUTED: Color = color!(0xa6bac5);
const PAPER: Color = color!(0x101821);
const SURFACE: Color = color!(0x1b2836);
const MINT: Color = color!(0x96dfce);
const RULE: Color = color!(0x2d414e);
const WARNING: Color = color!(0xefb38a);
const CITY_RESULTS_ID: &str = "city-results";

static CLOCK_THEME: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(|| {
    Theme::custom(
        "世界时钟",
        lintel::iced::theme::Palette {
            background: SURFACE,
            text: INK,
            primary: MINT,
            success: color!(0x79c8aa),
            warning: WARNING,
            danger: color!(0xe98c86),
        },
    )
});

fn config_path(file: &str) -> Option<PathBuf> {
    ProjectDirs::from("dev", "Lintel", "World Clock").map(|dirs| dirs.config_dir().join(file))
}

fn main() -> lintel::Result<()> {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("--clock-agent") => {
            let argument = args.next();
            #[cfg(target_os = "windows")]
            if argument.as_deref() == Some(std::ffi::OsStr::new("--windows-startup")) {
                let Some(_claim) = background::claim_agent()? else {
                    return Ok(());
                };
                let path = config_path("clock.json")
                    .ok_or_else(|| io::Error::other("cannot locate clock settings"))?;
                return tools::run_agent(path, background::agent_registered).map_err(Into::into);
            }
            let path = argument
                .map(PathBuf::from)
                .or_else(|| config_path("clock.json"))
                .ok_or_else(|| io::Error::other("cannot locate clock settings"))?;
            return tools::run_agent(path, || true).map_err(Into::into);
        }
        _ => {}
    }
    lintel::application(State::load, update, view)
        .id("dev.lintel.world-clock")
        .title("世界时钟")
        .on_activation(|event| {
            use lintel::runtime::ActivationEvent;
            let urls = match event {
                ActivationEvent::Opened { urls } | ActivationEvent::Launch { urls, .. } => urls,
                _ => Vec::new(),
            };
            let request = urls.iter().find_map(|url| Shortcut::from_url(url));
            Message::ShortcutRequest(request)
        })
        .window(clock_window())
        .subscription(subscription)
        .run()
}

fn clock_window() -> window::Settings {
    window::Settings {
        size: Size::new(1024.0, 768.0),
        min_size: Some(Size::new(440.0, 480.0)),
        position: window::Position::Centered,
        #[cfg(target_os = "macos")]
        platform_specific: window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        },
        ..Default::default()
    }
}

struct State {
    now: DateTime<Utc>,
    zones: Vec<Tz>,
    storage: Option<PathBuf>,
    query: String,
    adding: bool,
    status: Option<String>,
    drag: Option<Drag>,
    page: Page,
    tools: Tools,
    background_enabled: bool,
    background_busy: bool,
    background_error: Option<String>,
    shortcut: Option<Shortcut>,
    shortcut_error: Option<String>,
    #[cfg(target_os = "macos")]
    native_menu: NativeMenuState,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct NativeMenuState {
    window: Option<window::Id>,
    ready: bool,
    recent: Vec<tools::TimerSpec>,
}

#[derive(Clone, Copy)]
struct Drag {
    source: Tz,
    target: Option<Tz>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    StopwatchFrame,
    SelectPage(Page),
    NewItem,
    OpenTimerMenu,
    #[cfg(target_os = "macos")]
    MenuWindow(Option<window::Id>),
    #[cfg(target_os = "macos")]
    MenuInstalled(Result<(), String>),
    #[cfg(target_os = "macos")]
    NativeMenuItem(String),
    #[cfg(target_os = "macos")]
    TimerMenuClosed(Result<(), String>),
    CloseEditor,
    MoveFocus(bool),
    DragWindow,
    Tools(tools::Message),
    RefreshBackground,
    SetBackground(bool),
    BackgroundChanged(Result<bool, String>),
    ShortcutRequest(Option<Shortcut>),
    ResolveShortcut(bool),
    ToggleSearch,
    Query(String),
    Add(Tz),
    Remove(Tz),
    StartDrag(Tz),
    HoverCard(Tz),
    LeaveCard(Tz),
    DropOn(Tz),
    CancelDrag,
}

impl State {
    fn has_native_menu(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.native_menu.ready
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn load() -> Self {
        let storage = config_path("cities.txt");
        let mut status = None;
        let zones = match storage.as_deref().map(fs::read_to_string) {
            Some(Ok(contents)) => {
                let mut zones = Vec::new();
                for zone in contents.lines().filter_map(|line| line.parse::<Tz>().ok()) {
                    if !zones.contains(&zone) {
                        zones.push(zone);
                    }
                }
                zones
            }
            Some(Err(error)) if error.kind() == io::ErrorKind::NotFound => defaults(),
            _ => {
                status = Some("无法读取城市列表，已显示默认城市。".into());
                defaults()
            }
        };

        let tools = Tools::load(
            storage
                .as_ref()
                .map(|path| path.with_file_name("clock.json")),
        );
        let background = background::status();
        Self {
            now: Utc::now(),
            zones,
            storage,
            query: String::new(),
            adding: false,
            status,
            drag: None,
            page: Page::World,
            tools,
            background_enabled: background.as_ref().copied().unwrap_or(false),
            background_busy: false,
            background_error: background.err(),
            shortcut: None,
            shortcut_error: None,
            #[cfg(target_os = "macos")]
            native_menu: NativeMenuState::default(),
        }
    }

    fn save(&mut self) {
        self.status = match self.storage.as_deref() {
            Some(path) if save_cities(path, &self.zones).is_ok() => None,
            _ => Some("无法保存城市列表，本次更改仅在当前窗口生效。".into()),
        };
    }
}

fn defaults() -> Vec<Tz> {
    use chrono_tz::{America, Asia, Europe};
    vec![
        Asia::Shanghai,
        Asia::Tokyo,
        Europe::London,
        America::New_York,
    ]
}

fn save_cities(path: &Path, zones: &[Tz]) -> io::Result<()> {
    write_config(path, |file| {
        for zone in zones {
            writeln!(file, "{zone}")?;
        }
        Ok(())
    })
}

fn write_config(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let directory = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    write(file.as_file_mut())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

fn day_label(local: chrono::NaiveDate, other: chrono::NaiveDate) -> &'static str {
    if local == other {
        "今天"
    } else if local.checked_add_days(Days::new(1)) == Some(other) {
        "明天"
    } else if local.checked_sub_days(Days::new(1)) == Some(other) {
        "昨天"
    } else {
        "当地日期"
    }
}

fn subscription(state: &State) -> Subscription<Message> {
    Subscription::batch([
        time::every(Duration::from_secs(1)).map(|_| Message::Tick),
        if state.page == Page::Stopwatch && state.tools.stopwatch_running() {
            time::every(Duration::from_millis(16)).map(|_| Message::StopwatchFrame)
        } else {
            Subscription::none()
        },
        #[cfg(target_os = "macos")]
        native_menu::events().map(Message::NativeMenuItem),
        event::listen_with(|event, status, _id| match event {
            #[cfg(target_os = "macos")]
            Event::Window(window::Event::Opened { .. }) => Some(Message::MenuWindow(Some(_id))),
            Event::Window(window::Event::Focused) => Some(Message::RefreshBackground),
            Event::Window(window::Event::Unfocused) | Event::Mouse(mouse::Event::CursorLeft) => {
                Some(Message::CancelDrag)
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat: false,
                ..
            }) => match key {
                keyboard::Key::Character(character) if modifiers.command() => {
                    match character.as_str() {
                        "1" => Some(Message::SelectPage(Page::World)),
                        "2" => Some(Message::SelectPage(Page::Alarms)),
                        "3" => Some(Message::SelectPage(Page::Stopwatch)),
                        "4" => Some(Message::SelectPage(Page::Timers)),
                        "n" if !modifiers.shift() => Some(Message::NewItem),
                        _ => None,
                    }
                }
                keyboard::Key::Named(keyboard::key::Named::Tab)
                    if status == event::Status::Ignored && !modifiers.command() =>
                {
                    Some(Message::MoveFocus(modifiers.shift()))
                }
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::CloseEditor),
                _ => None,
            },
            _ => None,
        }),
    ])
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            state.now = Utc::now();
            let tick = state.tools.tick(state.now).map(Message::Tools);
            #[cfg(target_os = "macos")]
            if state.native_menu.window.is_none() {
                return Task::batch([tick, window::latest().map(Message::MenuWindow)]);
            }
            return tick;
        }
        Message::StopwatchFrame => {}
        Message::SelectPage(page) => {
            if state.adding || state.tools.alarm_editor_open() {
                return Task::none();
            }
            state.page = page;
            state.drag = None;
            state.tools.clear_error();
            return Task::batch([reset_page_scroll(), sync_native_menu(state)]);
        }
        Message::DragWindow =>
        {
            #[cfg(target_os = "macos")]
            if let Some(id) = state.native_menu.window {
                return window::drag(id);
            }
        }
        Message::MoveFocus(backwards) => {
            return if backwards {
                lintel::iced::widget::operation::focus_previous()
            } else {
                lintel::iced::widget::operation::focus_next()
            };
        }
        Message::NewItem => {
            return match state.page {
                Page::World if !state.adding => update(state, Message::ToggleSearch),
                Page::World => lintel::iced::widget::operation::focus("city-search"),
                Page::Alarms if !state.tools.alarm_editor_open() => {
                    update(state, Message::Tools(tools::Message::NewAlarm))
                }
                Page::Alarms => Task::none(),
                Page::Timers => update(state, Message::Tools(tools::Message::NewTimer)),
                Page::Stopwatch => Task::none(),
            };
        }
        Message::OpenTimerMenu => {
            #[cfg(target_os = "macos")]
            if state.native_menu.ready
                && let Some(id) = state.native_menu.window
            {
                let entries = state.tools.recent_timer_entries();
                let mut menu = Vec::with_capacity(entries.len());
                state.native_menu.recent.clear();
                for (index, (label, spec)) in entries.into_iter().enumerate() {
                    menu.push((format!("timer:recent:{index}"), label));
                    state.native_menu.recent.push(spec);
                }
                return window::run(id, move |window| native_menu::pop_timer(window, &menu))
                    .map(Message::TimerMenuClosed);
            }
            return update(state, Message::Tools(tools::Message::ToggleTimerAddMenu));
        }
        #[cfg(target_os = "macos")]
        Message::MenuWindow(Some(id)) if state.native_menu.window.is_none() => {
            state.native_menu.window = Some(id);
            return window::run(id, |_| native_menu::install()).map(Message::MenuInstalled);
        }
        #[cfg(target_os = "macos")]
        Message::MenuWindow(_) => {}
        #[cfg(target_os = "macos")]
        Message::MenuInstalled(result) => match result {
            Ok(()) => {
                state.native_menu.ready = true;
                return sync_native_menu(state);
            }
            Err(error) => eprintln!("clock menu: {error}"),
        },
        #[cfg(target_os = "macos")]
        Message::NativeMenuItem(id) => {
            return match route_native_menu(state, &id) {
                Some(message) => update(state, message),
                // AppKit toggles checkable items before notifying us. Restore
                // the current checkmark if a sheet blocked the command.
                None => sync_native_menu(state),
            };
        }
        #[cfg(target_os = "macos")]
        Message::TimerMenuClosed(Err(error)) => {
            eprintln!("clock timer menu: {error}");
            return update(state, Message::Tools(tools::Message::ToggleTimerAddMenu));
        }
        #[cfg(target_os = "macos")]
        Message::TimerMenuClosed(Ok(())) => {}
        Message::CloseEditor => {
            return match state.page {
                Page::World if state.adding => update(state, Message::ToggleSearch),
                Page::Alarms => update(state, Message::Tools(tools::Message::CloseAlarm)),
                Page::Timers => update(state, Message::Tools(tools::Message::CancelTimerEdit)),
                _ => Task::none(),
            };
        }
        Message::Tools(tools::Message::RequestMediaAuthorization) => {
            return state.tools.authorize_media().map(Message::Tools);
        }
        Message::Tools(message) => {
            state.now = Utc::now();
            let updates_menu = matches!(message, tools::Message::SetStopwatchFace(_));
            let opens_alarm = matches!(
                message,
                tools::Message::NewAlarm | tools::Message::EditAlarm(_)
            );
            let opens_panel = matches!(
                message,
                tools::Message::NewAlarm
                    | tools::Message::EditAlarm(_)
                    | tools::Message::NewTimer
                    | tools::Message::EditTimer(_)
                    | tools::Message::ShowTimerPresets
                    | tools::Message::ToggleTimerAddMenu
            );
            state.tools.update(message, state.now);
            if updates_menu {
                return sync_native_menu(state);
            }
            if opens_alarm && state.tools.alarm_editor_open() {
                use lintel::iced::widget::operation::{focus, select_all};
                return focus("alarm-hour").chain(select_all("alarm-hour"));
            }
            if opens_panel {
                return reset_page_scroll();
            }
        }
        Message::RefreshBackground => {
            if !state.background_busy {
                state.background_busy = true;
                return Task::perform(async { background::status() }, Message::BackgroundChanged);
            }
        }
        Message::SetBackground(enable) => {
            if !state.background_busy {
                state.background_busy = true;
                return Task::perform(
                    async move {
                        if enable {
                            background::enable()
                        } else {
                            background::disable()
                        }
                    },
                    Message::BackgroundChanged,
                );
            }
        }
        Message::BackgroundChanged(result) => {
            state.background_busy = false;
            match result {
                Ok(enabled) => {
                    state.background_enabled = enabled;
                    state.background_error = None;
                }
                Err(error) => state.background_error = Some(error),
            }
        }
        Message::ShortcutRequest(request) => {
            if state.shortcut.is_none() && request.is_some() {
                state.shortcut = request;
                state.shortcut_error = None;
            }
        }
        Message::ResolveShortcut(confirmed) => {
            if let Some(request) = state.shortcut.take()
                && confirmed
            {
                let page = match request {
                    Shortcut::Timer { .. } => Page::Timers,
                    Shortcut::Alarm { .. } => Page::Alarms,
                };
                match state.tools.perform_shortcut(request, Utc::now()) {
                    Ok(()) => return update(state, Message::SelectPage(page)),
                    Err(error) => state.shortcut_error = Some(error),
                }
            }
        }
        Message::ToggleSearch => {
            state.adding = !state.adding;
            state.query.clear();
            if state.adding {
                return lintel::iced::widget::operation::focus("city-search");
            }
        }
        Message::Query(query) => {
            state.query = query;
            return lintel::iced::widget::operation::snap_to(
                CITY_RESULTS_ID,
                scrollable::RelativeOffset::START,
            );
        }
        Message::Add(zone) => {
            if !state.zones.contains(&zone) {
                state.zones.push(zone);
                state.save();
            }
            state.query.clear();
            state.adding = false;
        }
        Message::Remove(zone) => {
            state.drag = None;
            state.zones.retain(|&current| current != zone);
            state.save();
        }
        Message::StartDrag(source) => {
            state.drag = Some(Drag {
                source,
                target: None,
            })
        }
        Message::HoverCard(zone) => {
            if let Some(drag) = &mut state.drag {
                drag.target = Some(zone);
            }
        }
        Message::LeaveCard(zone) => {
            if let Some(drag) = &mut state.drag
                && drag.target == Some(zone)
            {
                drag.target = None;
            }
        }
        Message::DropOn(target) => {
            if let Some(drag) = state.drag.take()
                && drag.source != target
                && let Some(source) = state.zones.iter().position(|&zone| zone == drag.source)
                && let Some(destination) = state.zones.iter().position(|&zone| zone == target)
            {
                let zone = state.zones.remove(source);
                state.zones.insert(destination, zone);
                state.save();
            }
        }
        Message::CancelDrag => state.drag = None,
    }
    Task::none()
}

fn sync_native_menu(state: &State) -> Task<Message> {
    #[cfg(target_os = "macos")]
    if state.native_menu.ready
        && let Some(id) = state.native_menu.window
    {
        let page = state.page;
        let face = state.tools.stopwatch_face();
        return window::run(id, move |_| native_menu::update_view(page, face)).discard();
    }
    let _ = state;
    Task::none()
}

#[cfg(target_os = "macos")]
fn route_native_menu(state: &State, id: &str) -> Option<Message> {
    if state.adding || state.tools.alarm_editor_open() {
        return None;
    }
    let message = match id {
        "page:world" => Message::SelectPage(Page::World),
        "page:alarms" => Message::SelectPage(Page::Alarms),
        "page:stopwatch" => Message::SelectPage(Page::Stopwatch),
        "page:timers" => Message::SelectPage(Page::Timers),
        "face:digital" if state.page == Page::Stopwatch => Message::Tools(
            tools::Message::SetStopwatchFace(tools::StopwatchFace::Digital),
        ),
        "face:analog" if state.page == Page::Stopwatch => Message::Tools(
            tools::Message::SetStopwatchFace(tools::StopwatchFace::Analog),
        ),
        "timer:custom" if state.page == Page::Timers => Message::Tools(tools::Message::NewTimer),
        id if state.page == Page::Timers && id.starts_with("timer:preset:") => {
            let seconds = id.strip_prefix("timer:preset:")?.parse().ok()?;
            tools::TIMER_PRESETS
                .into_iter()
                .flatten()
                .any(|(_, duration)| duration == seconds)
                .then_some(Message::Tools(tools::Message::TimerPreset(seconds)))?
        }
        id if state.page == Page::Timers && id.starts_with("timer:recent:") => {
            let index: usize = id.strip_prefix("timer:recent:")?.parse().ok()?;
            Message::Tools(tools::Message::StartRecent(
                state.native_menu.recent.get(index)?.clone(),
            ))
        }
        _ => return None,
    };
    Some(message)
}

fn reset_page_scroll() -> Task<Message> {
    lintel::iced::widget::operation::snap_to("clock-scroll", scrollable::RelativeOffset::START)
}

fn view(state: &State) -> Element<'_, Message> {
    let mut header = column![top_bar(state.page, state.adding)].spacing(8);
    if let Some(alerts) = state.tools.messages() {
        header = header.push(
            container(alerts.map(Message::Tools))
                .padding([0, 18])
                .width(Fill),
        );
    }

    if let Some(request) = &state.shortcut {
        header = header.push(
            column![
                text(request.description()).color(INK),
                row![
                    action("确认", Message::ResolveShortcut(true)),
                    action("取消", Message::ResolveShortcut(false)),
                ]
                .spacing(8),
            ]
            .spacing(6),
        );
    }
    if let Some(error) = &state.shortcut_error {
        header = header.push(text(error).color(INK));
    }
    let page = match state.page {
        Page::World => world_view(state),
        page => state
            .tools
            .view(page, state.now, state.has_native_menu())
            .map(Message::Tools),
    };
    let body: Element<'_, Message> = match state.page {
        Page::World | Page::Timers => scrollable(container(page).padding([12, 18]).width(Fill))
            .id("clock-scroll")
            .width(Fill)
            .height(Fill)
            .into(),
        Page::Alarms | Page::Stopwatch => container(page)
            .width(Fill)
            .height(Fill)
            .padding([12, 18])
            .into(),
    };
    let mut content = column![header.width(Fill), body].width(Fill).height(Fill);
    if matches!(state.page, Page::Alarms | Page::Timers) {
        content = content.push(background_footer(state));
    }
    let base = mouse_area(
        container(content)
            .width(Fill)
            .height(Fill)
            .style(|_| panel(PAPER, 0)),
    )
    .on_release(Message::CancelDrag)
    .into();

    let rendered = match state.page {
        Page::World if state.adding => sheet(base, city_picker(state), Alignment::Start),
        Page::Alarms => match state.tools.alarm_editor() {
            Some(editor) => sheet(
                base,
                scrollable(editor.map(Message::Tools))
                    .height(lintel::iced::Length::Shrink)
                    .into(),
                Alignment::Center,
            ),
            None => base,
        },
        _ => base,
    };
    themer(Some(CLOCK_THEME.clone()), rendered)
        .text_color(|_| INK)
        .into()
}

fn background_footer(state: &State) -> Element<'_, Message> {
    let label = if state.background_busy {
        "正在更新后台提醒状态"
    } else if state.background_enabled {
        "退出后提醒已开启"
    } else {
        "退出后提醒未开启"
    };
    let dot = if state.background_enabled {
        MINT
    } else {
        MUTED
    };
    let toggle = action(
        if state.background_enabled {
            "停用"
        } else {
            "启用"
        },
        Message::SetBackground(!state.background_enabled),
    )
    .padding([4, 9])
    .on_press_maybe(
        (!state.background_busy).then_some(Message::SetBackground(!state.background_enabled)),
    );
    let mut bar = column![
        container(space::horizontal())
            .width(Fill)
            .height(1)
            .style(|_| panel(RULE, 0)),
        container(
            row![
                text("•").size(13).color(dot),
                text(label).size(12).color(MUTED).width(Fill),
                toggle,
            ]
            .spacing(7)
            .align_y(Alignment::Center)
            .width(Fill)
        )
        .padding([6, 18]),
    ];
    if let Some(error) = &state.background_error {
        bar = bar.push(container(text(error).size(12).color(WARNING)).padding([0, 18]));
    }
    container(bar)
        .width(Fill)
        .style(|_| panel(color!(0x131d28), 0))
        .into()
}

fn top_bar(page: Page, adding: bool) -> Element<'static, Message> {
    responsive(move |size| {
        let compact = size.width < 620.0;
        let side = if compact { 82 } else { 132 };
        let mut tabs = row![].spacing(4).align_y(Alignment::Center);
        for (candidate, label) in Page::ALL {
            let selected = candidate == page;
            let tab = button(text(label).size(13))
                .padding([8, if compact { 7 } else { 12 }])
                .on_press(Message::SelectPage(candidate))
                .style(move |_, status| {
                    let background = if selected {
                        color!(0x27434c)
                    } else {
                        Color::TRANSPARENT
                    };
                    let mut style = action_style(status, background, color!(0x243743));
                    if selected {
                        style.text_color = MINT;
                        style.border = border::rounded(6).width(1).color(color!(0x466c73));
                    }
                    style
                });
            tabs = tabs.push(tab);
        }
        let tabs = container(tabs)
            .padding(3)
            .style(|_| panel(color!(0x17222d), 9).border(border::rounded(9).width(1).color(RULE)));
        let entry = match page {
            Page::World if adding => Some(("关闭搜索", Message::ToggleSearch)),
            Page::World => Some(("添加城市", Message::ToggleSearch)),
            Page::Alarms => Some(("添加闹钟", Message::NewItem)),
            Page::Timers => Some(("添加计时器 ▾", Message::OpenTimerMenu)),
            Page::Stopwatch => None,
        };
        let trailing: Element<'_, Message> = match entry {
            Some((label, message)) => {
                let label = if compact {
                    "＋".to_owned()
                } else {
                    format!("＋ {label}")
                };
                container(action(label, message).padding([7, 10]))
                    .width(side)
                    .align_x(Alignment::End)
                    .into()
            }
            None => space::horizontal().width(side).into(),
        };
        let drag_area = mouse_area(container(space::horizontal()).width(side).height(46))
            .on_press(Message::DragWindow);
        container(
            row![
                drag_area,
                space::horizontal(),
                tabs,
                space::horizontal(),
                trailing
            ]
            .width(Fill)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .height(54)
        .padding([4, 14])
        .style(|_| panel(color!(0x131d28), 0))
        .into()
    })
    .height(lintel::iced::Length::Shrink)
    .into()
}

fn sheet<'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    placement: Alignment,
) -> Element<'a, Message> {
    let shade = opaque(
        container(space::horizontal())
            .width(Fill)
            .height(Fill)
            .style(|_| panel(Color::from_rgba(0.04, 0.12, 0.15, 0.38), 0)),
    );
    let card = opaque(
        container(content)
            .padding(18)
            .width(Fill)
            .max_width(524)
            .style(|_| panel(SURFACE, 20).border(border::rounded(20).width(1).color(RULE))),
    );
    let anchored = container(card)
        .center_x(Fill)
        .height(Fill)
        .align_y(placement)
        .padding([24, 18]);
    stack([base, shade, anchored.into()])
        .width(Fill)
        .height(Fill)
        .into()
}

fn city_picker(state: &State) -> Element<'_, Message> {
    let matches = cities::search(&state.query, &state.zones);
    let search = text_input("搜索城市或时区，如 东京、New York", &state.query)
        .id("city-search")
        .padding(10)
        .on_input(Message::Query)
        .on_submit_maybe(matches.first().copied().map(Message::Add));
    let picker = column![
        row![
            text("添加城市").size(22).color(INK).width(Fill),
            action("取消", Message::CloseEditor),
        ]
        .align_y(Alignment::Center),
        search,
    ]
    .spacing(8);
    if matches.is_empty() {
        return picker
            .push(text("没有可添加的匹配城市").color(MUTED))
            .into();
    }

    let mut results = column![].spacing(3).width(Fill);
    for zone in matches {
        results = results.push(
            button(
                column![
                    text(cities::name(zone)).color(INK),
                    text(zone.name()).size(13).color(MUTED),
                ]
                .spacing(3),
            )
            .width(Fill)
            .padding([8, 12])
            .on_press(Message::Add(zone))
            .style(|_, status| action_style(status, Color::TRANSPARENT, color!(0x2b414b))),
        );
    }
    let results = scrollable(results)
        .id(CITY_RESULTS_ID)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(6)
                .scroller_width(4)
                .spacing(4),
        ))
        .height(lintel::iced::Length::Shrink)
        .width(Fill);
    picker
        .push(container(results).width(Fill).max_height(340))
        .into()
}

fn world_view(state: &State) -> Element<'_, Message> {
    let now = state.now;
    let local = now.with_timezone(&Local);
    let local_date = local.date_naive();
    let local_offset = local.offset().local_minus_utc();
    let pins = state
        .zones
        .iter()
        .filter_map(|&zone| {
            let location = cities::coordinates(zone)?;
            Some(world_map::Pin {
                latitude: location.lat(),
                longitude: location.lon(),
                label: cities::name(zone),
                time: now.with_timezone(&zone).format("%H:%M").to_string(),
            })
        })
        .collect();
    let map = container(world_map::view(now, pins))
        .width(Fill)
        .padding(1)
        .style(|_| panel(RULE, 2));
    let local_time = text(local.format("%H:%M:%S").to_string())
        .font(Font::MONOSPACE)
        .size(18)
        .color(INK);
    let context = row![
        text("本地时间").size(13).color(MUTED),
        local_time,
        space::horizontal(),
        text(local.format("%m月%d日 · UTC%:z").to_string())
            .size(13)
            .color(MUTED),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let mut body = column![map, context].spacing(14);
    if let Some(status) = &state.status {
        body = body.push(text(status).size(14).color(WARNING));
    }
    if state.zones.is_empty() {
        body = body.push(
            text("添加城市后，地图和时钟卡片会显示当地时间。")
                .size(14)
                .color(MUTED),
        );
    } else {
        const CARD_MIN_WIDTH: f32 = 190.0;
        const GAP: f32 = 12.0;
        body = body.push(
            responsive(move |size| {
                let columns = ((size.width + GAP) / (CARD_MIN_WIDTH + GAP)).floor() as usize;
                grid(
                    state
                        .zones
                        .iter()
                        .map(|&zone| city_card(zone, now, local_date, local_offset, state.drag)),
                )
                .columns(columns.clamp(1, state.zones.len()))
                .spacing(GAP)
                .height(lintel::iced::Length::Shrink)
                .into()
            })
            .height(lintel::iced::Length::Shrink),
        );
    }
    body.into()
}

fn city_card(
    zone: Tz,
    now: DateTime<Utc>,
    local_date: chrono::NaiveDate,
    local_offset: i32,
    drag: Option<Drag>,
) -> Element<'static, Message> {
    let city_time = now.with_timezone(&zone);
    let date = city_time.date_naive();
    let daylight = cities::coordinates(zone).map_or_else(
        || (6..18).contains(&city_time.hour()),
        |location| world_map::daylight(now, location.lat(), location.lon()),
    );
    let is_source = drag.is_some_and(|drag| drag.source == zone);
    let is_target = drag.is_some_and(|drag| drag.target == Some(zone)) && !is_source;
    let handle = mouse_area(container(text("⠿").size(17).color(MUTED)).padding([4, 4]))
        .on_press(Message::StartDrag(zone))
        .interaction(if is_source {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::Grab
        });
    let title = row![
        handle,
        text(cities::name(zone)).size(16).color(INK).width(Fill),
        action("移除", Message::Remove(zone)).padding([4, 6]),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let dial = container(city_dial::view(city_time.time(), daylight)).center_x(Fill);
    let offset = city_time.offset().fix().local_minus_utc() - local_offset;
    let difference = format!(
        "{} · {}",
        day_label(local_date, date),
        format_difference(offset)
    );
    let solar_line = match cities::solar_events(zone, date) {
        Some((rise, set)) => {
            let format = |instant: Option<DateTime<Utc>>| {
                instant.map_or_else(
                    || "—".to_owned(),
                    |time| time.with_timezone(&zone).format("%H:%M").to_string(),
                )
            };
            format!("日出 {}   日落 {}", format(rise), format(set))
        }
        None => city_time.format("UTC%:z").to_string(),
    };
    let numbers = column![
        text(city_time.format("%H:%M:%S").to_string())
            .font(Font::MONOSPACE)
            .size(22)
            .color(INK),
        text(difference).size(12).color(MUTED),
        text(solar_line).size(11).color(MUTED),
    ]
    .spacing(4)
    .align_x(Alignment::Center);

    let card = container(
        column![title, dial, numbers]
            .spacing(7)
            .align_x(Alignment::Center),
    )
    .width(Fill)
    .padding([10, 12])
    .style(move |_| {
        let (fill, stroke) = if is_target {
            (color!(0x25454d), MINT)
        } else if is_source {
            (color!(0x14212c), color!(0x487482))
        } else {
            (SURFACE, RULE)
        };
        panel(fill, 12).border(border::rounded(12).width(1).color(stroke))
    });

    if drag.is_some() {
        mouse_area(card)
            .on_enter(Message::HoverCard(zone))
            .on_move(move |_| Message::HoverCard(zone))
            .on_exit(Message::LeaveCard(zone))
            .on_release(Message::DropOn(zone))
            .into()
    } else {
        mouse_area(card).into()
    }
}

fn format_difference(seconds: i32) -> String {
    if seconds == 0 {
        return "与本地无时差".into();
    }
    let direction = if seconds > 0 { "快" } else { "慢" };
    let minutes = seconds.unsigned_abs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{direction} {minutes} 分钟"),
        (hours, 0) => format!("{direction} {hours} 小时"),
        (hours, minutes) => format!("{direction} {hours} 小时 {minutes} 分"),
    }
}

fn panel(background: Color, radius: u32) -> container::Style {
    container::Style::default()
        .background(background)
        .border(border::rounded(radius))
}

fn action<'a, M: Clone + 'a>(
    label: impl text::IntoFragment<'a>,
    message: M,
) -> button::Button<'a, M> {
    button(text(label).size(14))
        .padding([8, 12])
        .on_press(message)
        .style(|_: &Theme, status| action_style(status, color!(0x243843), color!(0x314e58)))
}

fn action_style(status: button::Status, default: Color, hovered: Color) -> button::Style {
    let background = match status {
        button::Status::Hovered => hovered,
        button::Status::Pressed => color!(0x385c63),
        button::Status::Disabled => color!(0x202b34),
        _ => default,
    };
    button::Style {
        background: Some(background.into()),
        text_color: if status == button::Status::Disabled {
            MUTED
        } else {
            INK
        },
        border: border::rounded(8),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn city_dates_and_offsets_follow_daylight_saving() {
        let winter = "2026-01-10T02:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let summer = "2026-07-10T02:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let new_york = chrono_tz::America::New_York;
        let shanghai = chrono_tz::Asia::Shanghai;
        assert_eq!(
            winter.with_timezone(&new_york).format("%:z").to_string(),
            "-05:00"
        );
        assert_eq!(
            summer.with_timezone(&new_york).format("%:z").to_string(),
            "-04:00"
        );
        assert_eq!(
            day_label(
                summer.with_timezone(&shanghai).date_naive(),
                summer.with_timezone(&new_york).date_naive()
            ),
            "昨天"
        );
        let kathmandu = summer.with_timezone(&chrono_tz::Asia::Kathmandu);
        assert_eq!(kathmandu.format("%:z").to_string(), "+05:45");
    }

    #[test]
    fn searches_in_chinese_and_english_and_omits_selected_zones() {
        let tokyo = chrono_tz::Asia::Tokyo;
        assert_eq!(cities::search("东京", &[]), vec![tokyo]);
        assert!(cities::search("东京", &[tokyo]).is_empty());
        assert_eq!(
            cities::search("New York", &[]),
            vec![chrono_tz::America::New_York]
        );
    }

    #[test]
    fn city_search_includes_results_beyond_the_first_page() {
        let results = cities::search("America/", &[]);
        assert!(results.len() > 8);
        assert!(results.contains(&chrono_tz::America::Vancouver));
    }

    #[test]
    fn dropping_a_card_saves_its_new_position_and_cancel_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cities.txt");
        let original = defaults();
        let mut state = State {
            now: Utc::now(),
            zones: original.clone(),
            storage: Some(path.clone()),
            query: String::new(),
            adding: false,
            status: None,
            drag: None,
            page: Page::World,
            tools: Tools::load(None),
            background_enabled: false,
            background_busy: false,
            background_error: None,
            shortcut: None,
            shortcut_error: None,
            #[cfg(target_os = "macos")]
            native_menu: NativeMenuState::default(),
        };

        let _ = update(&mut state, Message::StartDrag(original[0]));
        let _ = update(&mut state, Message::DropOn(original[2]));
        let expected = vec![original[1], original[2], original[0], original[3]];
        assert_eq!(state.zones, expected);

        let _ = update(&mut state, Message::StartDrag(original[0]));
        let _ = update(&mut state, Message::HoverCard(original[1]));
        let _ = update(&mut state, Message::CancelDrag);
        let _ = update(&mut state, Message::DropOn(original[1]));
        assert_eq!(state.zones, expected);
        let contents = fs::read_to_string(path).unwrap();
        let saved = contents
            .lines()
            .map(|line| line.parse().unwrap())
            .collect::<Vec<Tz>>();
        assert_eq!(saved, expected);
    }

    #[test]
    fn saves_an_empty_list_over_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences/cities.txt");
        save_cities(&path, &[chrono_tz::Asia::Shanghai]).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "Asia/Shanghai\n");
        save_cities(&path, &[]).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
    }
}
