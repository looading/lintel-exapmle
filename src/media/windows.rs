//! Windows system media sessions provide explicit pause and stop commands.
//! <https://learn.microsoft.com/uwp/api/windows.media.control.globalsystemmediatransportcontrolssessionmanager>

use std::{sync::mpsc, time::Duration};

use ::windows::{
    Foundation::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperation},
    Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as SessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
    },
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::RuntimeType,
};

use super::PlaybackReport;

pub(super) fn run(authorizing: bool) -> PlaybackReport {
    let mut report = PlaybackReport {
        authorizing,
        ..Default::default()
    };
    if let Err(error) = scan_sessions(&mut report) {
        report.failed.push(error);
    }
    report
}

fn scan_sessions(report: &mut PlaybackReport) -> Result<(), String> {
    let _apartment = WinRtApartment::initialize()?;
    let manager = wait_for(SessionManager::RequestAsync().map_err(system_error)?)?;
    let sessions = manager.GetSessions().map_err(system_error)?;
    for index in 0..sessions.Size().map_err(system_error)? {
        let session = match sessions.GetAt(index) {
            Ok(session) => session,
            Err(error) => {
                report.failed.push(system_error(error));
                continue;
            }
        };
        let name = session
            .SourceAppUserModelId()
            .ok()
            .map_or_else(|| "媒体会话".into(), |id| session_label(&id.to_string()));
        match process_session(&session, report.authorizing) {
            Ok(true) => {
                if !report.completed.contains(&name) {
                    report.completed.push(name);
                }
            }
            Ok(false) => {}
            Err(error) => {
                report.failed.push(format!("{name}，{error}"));
            }
        }
    }
    Ok(())
}

fn process_session(session: &Session, probe: bool) -> Result<bool, String> {
    let playback = session.GetPlaybackInfo().map_err(system_error)?;
    if !probe && playback.PlaybackStatus().map_err(system_error)? != PlaybackStatus::Playing {
        return Ok(false);
    }

    let controls = playback.Controls().map_err(system_error)?;
    let pause = controls.IsPauseEnabled().map_err(system_error)?;
    let stop = !pause && controls.IsStopEnabled().map_err(system_error)?;
    if probe {
        return Ok(pause || stop);
    }

    let operation = if pause {
        session.TryPauseAsync()
    } else if stop {
        session.TryStopAsync()
    } else {
        return Err("播放器未提供暂停或停止操作".into());
    }
    .map_err(system_error)?;
    if wait_for(operation)? {
        Ok(true)
    } else {
        Err("播放器未接受操作".into())
    }
}

fn wait_for<T: RuntimeType + 'static>(operation: IAsyncOperation<T>) -> Result<T, String> {
    if operation.Status().map_err(system_error)? == AsyncStatus::Started {
        let (sender, receiver) = mpsc::sync_channel(1);
        operation
            .SetCompleted(&AsyncOperationCompletedHandler::new(move |_, _| {
                let _ = sender.try_send(());
                Ok(())
            }))
            .map_err(system_error)?;
        if receiver.recv_timeout(Duration::from_secs(3)).is_err() {
            let _ = operation.Cancel();
            return Err("系统媒体控件响应超时".into());
        }
    }
    operation.GetResults().map_err(system_error)
}

fn session_label(id: &str) -> String {
    id.rsplit(['!', '\\', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or("媒体会话")
        .chars()
        .take(80)
        .collect()
}

fn system_error(error: ::windows::core::Error) -> String {
    format!("Windows 媒体控件错误（0x{:08X}）", error.code().0 as u32)
}

struct WinRtApartment;

impl WinRtApartment {
    fn initialize() -> Result<Self, String> {
        // SAFETY: This initializes WinRT on the calling worker thread. The
        // local guard balances the call on the same thread.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(system_error)?;
        Ok(Self)
    }
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        // SAFETY: An instance is only created after a successful initialization.
        unsafe { RoUninitialize() };
    }
}
