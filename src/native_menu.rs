//! AppKit menus for the iced window. Commands are routed back to the iced update loop.

use std::{
    cell::RefCell,
    sync::{Mutex, Once},
};

use lintel::iced::{
    Subscription,
    futures::channel::mpsc,
    stream,
    window::{Window, raw_window_handle::RawWindowHandle},
};
use muda::{CheckMenuItem, ContextMenu, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use objc2::MainThreadMarker;

use crate::tools::{Page, StopwatchFace, TIMER_PRESETS};

struct AppMenu {
    _menu: Menu,
    pages: Vec<(Page, CheckMenuItem)>,
    digital: CheckMenuItem,
    analog: CheckMenuItem,
}

thread_local! {
    // AppKit menu item handles belong to the thread that creates them.
    static APP_MENU: RefCell<Option<AppMenu>> = const { RefCell::new(None) };
}

static MENU_SENDER: Mutex<Option<mpsc::Sender<String>>> = Mutex::new(None);
static MENU_EVENTS: Once = Once::new();

pub fn events() -> Subscription<String> {
    fn channel() -> impl lintel::iced::futures::Stream<Item = String> {
        stream::channel(64, async |sender| {
            *MENU_SENDER.lock().expect("menu subscription lock") = Some(sender);
            MENU_EVENTS.call_once(|| {
                MenuEvent::set_event_handler(Some(|event: MenuEvent| {
                    if let Ok(mut sender) = MENU_SENDER.lock()
                        && let Some(sender) = sender.as_mut()
                    {
                        let _ = sender.try_send(event.id.0);
                    }
                }));
            });
            std::future::pending::<()>().await;
        })
    }
    Subscription::run(channel)
}

pub fn install() -> Result<(), String> {
    require_main_thread()?;
    APP_MENU.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = Some(build_app_menu().map_err(|error| error.to_string())?);
        }
        Ok(())
    })
}

fn build_app_menu() -> muda::Result<AppMenu> {
    let app = Submenu::with_items(
        "世界时钟",
        true,
        &[
            &PredefinedMenuItem::about(Some("关于世界时钟"), None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(Some("隐藏世界时钟")),
            &PredefinedMenuItem::hide_others(Some("隐藏其他")),
            &PredefinedMenuItem::show_all(Some("全部显示")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(Some("退出世界时钟")),
        ],
    )?;
    let view = Submenu::new("显示", true);
    let mut pages = Vec::with_capacity(Page::ALL.len());
    for (page, title) in Page::ALL {
        let id = match page {
            Page::World => "page:world",
            Page::Alarms => "page:alarms",
            Page::Stopwatch => "page:stopwatch",
            Page::Timers => "page:timers",
        };
        let entry = CheckMenuItem::with_id(id, title, true, page == Page::World, None);
        view.append(&entry)?;
        pages.push((page, entry));
    }
    let digital = CheckMenuItem::with_id("face:digital", "查看数码秒表", false, true, None);
    let analog = CheckMenuItem::with_id("face:analog", "查看指针秒表", false, false, None);
    view.append_items(&[&PredefinedMenuItem::separator(), &digital, &analog])?;
    let windows = Submenu::with_items(
        "窗口",
        true,
        &[
            &PredefinedMenuItem::minimize(Some("最小化")),
            &PredefinedMenuItem::maximize(Some("缩放")),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::bring_all_to_front(Some("全部置于前面")),
        ],
    )?;
    // AppKit edit commands would consume keyboard equivalents before the iced
    // text inputs can handle them. Keep editing shortcuts on the iced side.
    let menu = Menu::with_items(&[&app, &view, &windows])?;
    menu.init_for_nsapp();
    windows.set_as_windows_menu_for_nsapp();
    Ok(AppMenu {
        _menu: menu,
        pages,
        digital,
        analog,
    })
}

pub fn update_view(page: Page, face: StopwatchFace) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    APP_MENU.with_borrow(|slot| {
        if let Some(menu) = slot {
            for (candidate, item) in &menu.pages {
                item.set_checked(*candidate == page);
            }
            let can_select_face = page == Page::Stopwatch;
            menu.digital.set_enabled(can_select_face);
            menu.analog.set_enabled(can_select_face);
            menu.digital.set_checked(face == StopwatchFace::Digital);
            menu.analog.set_checked(face == StopwatchFace::Analog);
        }
    });
}

pub fn pop_timer(window: &dyn Window, recent: &[(String, String)]) -> Result<(), String> {
    require_main_thread()?;
    let handle = window.window_handle().map_err(|error| error.to_string())?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("计时器菜单需要 macOS 窗口。".into());
    };
    let popup = build_timer_menu(recent).map_err(|error| error.to_string())?;
    // SAFETY: AppKit supplies the live NSView pointer for this iced window. The
    // popup is used synchronously before the borrowed window handle expires.
    unsafe { popup.show_context_menu_for_nsview(handle.ns_view.as_ptr(), None) };
    Ok(())
}

fn build_timer_menu(recent: &[(String, String)]) -> muda::Result<Menu> {
    let presets = Submenu::new("预设", true);
    for (title, seconds) in TIMER_PRESETS.into_iter().flatten() {
        presets.append(&MenuItem::with_id(
            format!("timer:preset:{seconds}"),
            title,
            true,
            None,
        ))?;
    }
    let popup = Menu::with_items(&[
        &MenuItem::with_id("timer:custom", "自定计时器…", true, None),
        &presets,
    ])?;
    if !recent.is_empty() {
        popup.append_items(&[
            &PredefinedMenuItem::separator(),
            &MenuItem::new("最近使用", false, None),
        ])?;
        for (id, title) in recent {
            popup.append(&MenuItem::with_id(id, title.replace('&', "&&"), true, None))?;
        }
    }
    Ok(popup)
}

fn require_main_thread() -> Result<(), String> {
    MainThreadMarker::new()
        .map(|_| ())
        .ok_or_else(|| "请在主线程打开 macOS 菜单。".into())
}
