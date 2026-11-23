//! Platform media control for timer end actions.
#[derive(Default)]
pub struct PlaybackReport {
    authorizing: bool,
    completed: Vec<String>,
    needs_permission: Vec<&'static str>,
    failed: Vec<String>,
}

impl PlaybackReport {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.completed.is_empty() {
            let action = match (self.authorizing, cfg!(target_os = "windows")) {
                (true, true) => "找到可控制的播放器",
                (true, false) => "已授权",
                (false, _) => "已停止播放",
            };
            parts.push(format!("{action}（{}）", self.completed.join("、")));
        }
        if !self.needs_permission.is_empty() {
            parts.push(format!(
                "需要自动化授权（{}）",
                self.needs_permission.join("、")
            ));
        }
        if !self.failed.is_empty() {
            parts.push(format!("无法控制（{}）", self.failed.join("、")));
        }
        if parts.is_empty() {
            let message = match (self.authorizing, cfg!(target_os = "windows")) {
                (true, true) => "没有发现可暂停或停止的 Windows 媒体会话。",
                (true, false) => "请先打开音乐、Apple TV 或 QuickTime Player，再授权。",
                (false, _) => "没有检测到受支持的媒体在播放。",
            };
            return message.into();
        }
        parts.join("。")
    }
}

pub fn support_hint() -> &'static str {
    if cfg!(target_os = "windows") {
        "支持接入 Windows 系统媒体控件的播放器。"
    } else if cfg!(target_os = "macos") {
        "支持音乐、Apple TV 和 QuickTime Player。"
    } else {
        "当前平台不支持停止媒体播放。"
    }
}

pub fn authorization_label() -> &'static str {
    if cfg!(target_os = "windows") {
        "检查可控制的播放器"
    } else {
        "授权控制已打开的播放器"
    }
}

/// Processes only running players and never brings up an authorization prompt.
pub fn stop_playback() -> PlaybackReport {
    run(false)
}

/// Call this after an explicit user action on a worker thread. macOS may ask
/// the user to approve Automation access for each running player.
pub fn authorize_playback() -> PlaybackReport {
    run(true)
}

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
use self::windows::run;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn run(authorizing: bool) -> PlaybackReport {
    PlaybackReport {
        authorizing,
        failed: vec!["当前平台".into()],
        ..Default::default()
    }
}

#[cfg(target_os = "macos")]
fn run(authorizing: bool) -> PlaybackReport {
    macos::run(authorizing)
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{rc::Retained, rc::autoreleasepool};
    use objc2_app_kit::NSRunningApplication;
    use objc2_core_services::{
        AEDeterminePermissionToAutomateTarget, cDocument, cProperty, formAbsolutePosition,
        formPropertyID, kAECoreSuite, kAECountElements, kAEGetData, kAnyTransactionID,
        kAutoGenerateReturnID, keyAEContainer, keyAEDesiredClass, keyAEKeyData, keyAEKeyForm,
        keyAEObjectClass, keyDirectObject, keyErrorNumber, typeAlias, typeFSRef, typeFileURL,
        typeNull, typeObjectSpecifier,
    };
    use objc2_foundation::{
        NSAppleEventDescriptor as Descriptor, NSAppleEventSendOptions as SendOptions, NSString,
    };

    use super::PlaybackReport;

    // Command and property codes from the apps' installed scripting dictionaries.
    const MUSIC_SUITE: u32 = u32::from_be_bytes(*b"hook");
    const PAUSE: u32 = u32::from_be_bytes(*b"Paus");
    const PLAYER_STATE: u32 = u32::from_be_bytes(*b"pPlS");
    const PLAYING: u32 = u32::from_be_bytes(*b"kPSP");
    const QUICKTIME_SUITE: u32 = u32::from_be_bytes(*b"MVWR");
    const STOP_MOVIE: u32 = u32::from_be_bytes(*b"stop");
    const MOVIE_PLAYING: u32 = u32::from_be_bytes(*b"play");
    const DOCUMENT_FILE: u32 = u32::from_be_bytes(*b"file");

    const PERMISSION_DENIED: i32 = -1743;
    const CONSENT_REQUIRED: i32 = -1744;
    const PROCESS_GONE: i32 = -600;
    const CONNECTION_GONE: i32 = -609;
    const MISSING_RESULT: i32 = -1701;
    const NO_SUCH_OBJECT: i32 = -1728;
    const MISSING_VALUE: u32 = u32::from_be_bytes(*b"msng");

    #[derive(Debug)]
    struct Failure {
        step: &'static str,
        status: i32,
    }

    fn failure(step: &'static str) -> impl FnOnce(i32) -> Failure {
        move |status| Failure { step, status }
    }

    struct Player {
        name: &'static str,
        bundle_id: &'static str,
        pause: fn(&Descriptor) -> Result<bool, Failure>,
        command: (u32, u32),
    }

    const PLAYERS: [Player; 3] = [
        Player {
            name: "音乐",
            bundle_id: "com.apple.Music",
            pause: pause_player,
            command: (MUSIC_SUITE, PAUSE),
        },
        Player {
            name: "Apple TV",
            bundle_id: "com.apple.TV",
            pause: pause_player,
            command: (MUSIC_SUITE, PAUSE),
        },
        Player {
            name: "QuickTime Player",
            bundle_id: "com.apple.QuickTimePlayerX",
            pause: stop_movies,
            command: (QUICKTIME_SUITE, STOP_MOVIE),
        },
    ];

    pub(super) fn run(authorizing: bool) -> PlaybackReport {
        autoreleasepool(|_| {
            let mut report = PlaybackReport {
                authorizing,
                ..Default::default()
            };
            for player in PLAYERS {
                let bundle_id = NSString::from_str(player.bundle_id);
                let running =
                    NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id);
                for index in 0..running.count() {
                    let application = running.objectAtIndex(index);
                    // Address the existing process, so a player quitting during a timer expiry
                    // does not cause a new instance to launch.
                    let pid = application.processIdentifier();
                    if pid <= 0 {
                        continue;
                    }
                    let target = Descriptor::descriptorWithProcessIdentifier(pid);
                    let result = check_permission(&target, kAECoreSuite, kAEGetData, authorizing)
                        .map_err(failure("读取权限"))
                        .and_then(|()| {
                            check_permission(
                                &target,
                                player.command.0,
                                player.command.1,
                                authorizing,
                            )
                            .map_err(failure("播放控制权限"))
                        })
                        .and_then(|()| {
                            if authorizing {
                                Ok(true)
                            } else {
                                (player.pause)(&target)
                            }
                        });
                    match result {
                        Ok(true) => report.completed.push(player.name.into()),
                        Ok(false)
                        | Err(Failure {
                            status: PROCESS_GONE | CONNECTION_GONE,
                            ..
                        }) => {}
                        Err(Failure {
                            status: PERMISSION_DENIED | CONSENT_REQUIRED,
                            ..
                        }) => report.needs_permission.push(player.name),
                        Err(Failure { step, status }) => {
                            eprintln!("clock media: {} {step} ({status})", player.bundle_id);
                            report
                                .failed
                                .push(format!("{}，{step} {status}", player.name));
                        }
                    }
                }
            }
            report
        })
    }

    fn check_permission(
        target: &Descriptor,
        class: u32,
        command: u32,
        ask_user: bool,
    ) -> Result<(), i32> {
        // SAFETY: The live descriptor owns the address pointer for this synchronous call.
        let status = unsafe {
            AEDeterminePermissionToAutomateTarget(target.aeDesc(), class, command, ask_user)
        };
        if status == 0 { Ok(()) } else { Err(status) }
    }

    fn pause_player(target: &Descriptor) -> Result<bool, Failure> {
        let state = get(
            target,
            &property(PLAYER_STATE, &Descriptor::nullDescriptor()),
        )
        .map_err(failure("读取播放状态"))?;
        if state.enumCodeValue() != PLAYING {
            return Ok(false);
        }
        send(target, MUSIC_SUITE, PAUSE, None).map_err(failure("暂停播放"))?;
        Ok(true)
    }

    fn stop_movies(target: &Descriptor) -> Result<bool, Failure> {
        let count = document_count(target).map_err(failure("读取影片数量"))?;
        let mut stopped = false;
        for index in 1..=count {
            let document = object(
                cDocument,
                formAbsolutePosition,
                &Descriptor::descriptorWithInt32(index),
                &Descriptor::nullDescriptor(),
            );
            // QuickTime uses one document type for movies and recordings.
            // No script command is sent to a document without a file on disk.
            let file = match get(target, &property(DOCUMENT_FILE, &document)) {
                Ok(file) => file,
                Err(NO_SUCH_OBJECT | MISSING_RESULT) => continue,
                Err(status) => return Err(failure("读取影片文件")(status)),
            };
            if file.descriptorType() == typeNull || file.typeCodeValue() == MISSING_VALUE {
                continue;
            }
            if ![typeFileURL, typeAlias, typeFSRef].contains(&file.descriptorType()) {
                let descriptor_type =
                    String::from_utf8_lossy(&file.descriptorType().to_be_bytes()).into_owned();
                eprintln!(
                    "clock media: QuickTime Player returned unexpected file type {descriptor_type}"
                );
                continue;
            }
            let playing = get(target, &property(MOVIE_PLAYING, &document))
                .map_err(failure("读取影片播放状态"))?;
            if playing.booleanValue() != 0 {
                send(target, QUICKTIME_SUITE, STOP_MOVIE, Some(&document))
                    .map_err(failure("停止影片播放"))?;
                stopped = true;
            }
        }
        Ok(stopped)
    }

    fn document_count(target: &Descriptor) -> Result<i32, i32> {
        let event = new_event(target, kAECoreSuite, kAECountElements);
        event.setParamDescriptor_forKeyword(&Descriptor::nullDescriptor(), keyDirectObject);
        event.setParamDescriptor_forKeyword(
            &Descriptor::descriptorWithTypeCode(cDocument),
            keyAEObjectClass,
        );
        send_event(&event)?
            .paramDescriptorForKeyword(keyDirectObject)
            .map(|value| value.int32Value())
            .ok_or(MISSING_RESULT)
    }

    fn object(
        class: u32,
        form: u32,
        key: &Descriptor,
        container: &Descriptor,
    ) -> Retained<Descriptor> {
        let object = Descriptor::recordDescriptor();
        object.setDescriptor_forKeyword(
            &Descriptor::descriptorWithTypeCode(class),
            keyAEDesiredClass,
        );
        object.setDescriptor_forKeyword(&Descriptor::descriptorWithEnumCode(form), keyAEKeyForm);
        object.setDescriptor_forKeyword(key, keyAEKeyData);
        object.setDescriptor_forKeyword(container, keyAEContainer);
        object
            .coerceToDescriptorType(typeObjectSpecifier)
            .expect("valid Apple event object specifier")
    }

    fn property(code: u32, container: &Descriptor) -> Retained<Descriptor> {
        object(
            cProperty,
            formPropertyID,
            &Descriptor::descriptorWithTypeCode(code),
            container,
        )
    }

    fn get(target: &Descriptor, selector: &Descriptor) -> Result<Retained<Descriptor>, i32> {
        send(target, kAECoreSuite, kAEGetData, Some(selector))?
            .paramDescriptorForKeyword(keyDirectObject)
            .ok_or(MISSING_RESULT)
    }

    fn send(
        target: &Descriptor,
        class: u32,
        command: u32,
        parameter: Option<&Descriptor>,
    ) -> Result<Retained<Descriptor>, i32> {
        let event = new_event(target, class, command);
        if let Some(parameter) = parameter {
            event.setParamDescriptor_forKeyword(parameter, keyDirectObject);
        }
        send_event(&event)
    }

    fn new_event(target: &Descriptor, class: u32, command: u32) -> Retained<Descriptor> {
        Descriptor::appleEventWithEventClass_eventID_targetDescriptor_returnID_transactionID(
            class,
            command,
            Some(target),
            kAutoGenerateReturnID as i16,
            kAnyTransactionID,
        )
    }

    fn send_event(event: &Descriptor) -> Result<Retained<Descriptor>, i32> {
        let reply = event
            .sendEventWithOptions_timeout_error(
                SendOptions::WaitForReply | SendOptions::NeverInteract,
                3.0,
            )
            .map_err(|error| error.code() as i32)?;
        if let Some(error) = reply.paramDescriptorForKeyword(keyErrorNumber)
            && error.int32Value() != 0
        {
            return Err(error.int32Value());
        }
        Ok(reply)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn apple_event_specifiers_encode_the_requested_property() {
            let specifier = property(PLAYER_STATE, &Descriptor::nullDescriptor());
            assert_eq!(specifier.descriptorType(), typeObjectSpecifier);
            assert_eq!(
                specifier
                    .descriptorForKeyword(keyAEKeyData)
                    .unwrap()
                    .typeCodeValue(),
                PLAYER_STATE
            );
        }
    }
}
