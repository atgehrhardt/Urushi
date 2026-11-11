//! Urushi on Android, loaded by `NativeActivity` as `liburushi.so`. Android apps can't
//! run local shells in a useful way, so this is a remote client: it goes back to the
//! host it used last, or opens with the "connect to a host" dialog. Package with
//! `scripts/build-android.sh`.

#[cfg(target_os = "android")]
mod keyboard;
#[cfg(target_os = "android")]
mod log;

/// Entry point called by `android-activity` (through winit) on the app's main thread.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
#[allow(
    unsafe_code,
    reason = "the symbol must be exported unmangled for NativeActivity"
)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    log::init();
    urushi_app::log_panics();
    tracing_log("starting");

    // winit doesn't report Android's system-bar insets; android-activity does.
    // The app's private storage keeps the LAN keyring and recent hosts (ADR 0041, 0043).
    let data_dir = app.internal_data_path();
    let insets_app = app.clone();
    let soft_keyboard = keyboard::switch(app.clone());
    let safe_area: urushi_app::SafeAreaFn = std::sync::Arc::new(move || {
        let r = insets_app.content_rect();
        Some([r.left, r.top, r.right, r.bottom])
    });
    let event_loop = winit::event_loop::EventLoop::<urushi_app::UserEvent>::with_user_event()
        .with_android_app(app)
        .build();
    match event_loop {
        Ok(event_loop) => {
            let mut config = urushi_app::AppConfig::new(urushi_app::ConnectTarget::Ask);
            config.safe_area = Some(safe_area);
            config.soft_keyboard = Some(soft_keyboard);
            config.data_dir = data_dir;
            if let Err(e) = urushi_app::run_with(event_loop, config) {
                tracing_log(&e.to_string());
            }
        }
        Err(e) => tracing_log(&e.to_string()),
    }
    // The activity is gone (Back, or Android reclaimed it), but the process may live on,
    // and Android would call `android_main` again in it on the next launch. winit can't
    // build a second event loop in one process, so end it here: the next launch starts
    // fresh and goes back to the remembered host (ADR 0043).
    tracing_log("activity destroyed; exiting");
    std::process::exit(0);
}

#[cfg(target_os = "android")]
fn tracing_log(message: &str) {
    let _ = std::io::Write::write_all(
        &mut log::Logcat::default(),
        format!("{message}\n").as_bytes(),
    );
}
