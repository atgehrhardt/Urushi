//! Opens and closes Android's on-screen keyboard (ADR 0042).
//!
//! winit asks `android-activity` to show it, which asks the `InputMethodManager` for the
//! window's decor view, from the app's native thread, as an implicit request. The IME
//! ignores that: `NativeActivity`'s focused view is its content view, not the decor view,
//! and an implicit request doesn't open the keyboard. So this asks on the Java UI thread,
//! for the focused view, explicitly. With no `InputConnection` there, the keyboard types
//! by sending key events, which reach winit like a hardware keyboard's.

use jni::objects::{JObject, JValue};
use jni::{Env, JavaVM, jni_sig, jni_str};
use urushi_app::SoftKeyboardFn;
use winit::platform::android::activity::AndroidApp;

/// The switch the app calls with `true` to show the keyboard and `false` to hide it.
pub(crate) fn switch(app: AndroidApp) -> SoftKeyboardFn {
    std::sync::Arc::new(move |show| {
        let ui_app = app.clone();
        app.run_on_java_main_thread(Box::new(move || {
            if let Err(e) = toggle(&ui_app, show) {
                crate::tracing_log(&format!("can't switch the soft keyboard: {e}"));
            }
        }));
    })
}

/// Shows or hides the keyboard. Must run on the UI thread, which owns the views.
fn toggle(app: &AndroidApp, show: bool) -> jni::errors::Result<()> {
    // SAFETY: `vm_as_ptr` is the process's `JavaVM`, valid for its whole life.
    #[allow(
        unsafe_code,
        reason = "android-activity hands out the JavaVM as a raw pointer"
    )]
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| {
        // SAFETY: `activity_as_ptr` is a global reference to the activity that
        // android-activity keeps alive while the app runs; this borrows it and never
        // deletes it.
        #[allow(
            unsafe_code,
            reason = "android-activity hands out the activity as a raw jobject"
        )]
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
        env.with_local_frame(8, |env| request(env, &activity, show))
    })
}

/// Makes the `InputMethodManager` call with local references that the frame frees.
fn request(env: &mut Env<'_>, activity: &JObject<'_>, show: bool) -> jni::errors::Result<()> {
    let service = env.new_string("input_method")?;
    let imm = env
        .call_method(
            activity,
            jni_str!("getSystemService"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
            &[JValue::Object(&service)],
        )?
        .l()?;
    let window = env
        .call_method(
            activity,
            jni_str!("getWindow"),
            jni_sig!("()Landroid/view/Window;"),
            &[],
        )?
        .l()?;
    let decor = env
        .call_method(
            &window,
            jni_str!("getDecorView"),
            jni_sig!("()Landroid/view/View;"),
            &[],
        )?
        .l()?;
    if show {
        let focused = env
            .call_method(
                &decor,
                jni_str!("findFocus"),
                jni_sig!("()Landroid/view/View;"),
                &[],
            )?
            .l()?;
        let view = if focused.is_null() { &decor } else { &focused };
        env.call_method(
            &imm,
            jni_str!("showSoftInput"),
            jni_sig!("(Landroid/view/View;I)Z"),
            &[JValue::Object(view), JValue::Int(0)],
        )?;
    } else {
        let token = env
            .call_method(
                &decor,
                jni_str!("getWindowToken"),
                jni_sig!("()Landroid/os/IBinder;"),
                &[],
            )?
            .l()?;
        env.call_method(
            &imm,
            jni_str!("hideSoftInputFromWindow"),
            jni_sig!("(Landroid/os/IBinder;I)Z"),
            &[JValue::Object(&token), JValue::Int(0)],
        )?;
    }
    Ok(())
}
