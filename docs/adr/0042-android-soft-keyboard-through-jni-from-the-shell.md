# 0042. Open Android's soft keyboard with JNI calls from the Rust shell

- **Status:** Accepted
- **Date:** 2026-11-11

## Context
The mobile apps open on the connect dialog, and on a phone the on-screen keyboard is the
only way to type the host (UR-49). The app asked winit for the keyboard once, at window
creation. On iOS that was simply too early (before the window was visible). On Android it
could never work: winit's `set_ime_allowed(true)` reaches `android-activity`, which calls
`InputMethodManager.showSoftInput(decorView, SHOW_IMPLICIT)` from the native thread. The IME
ignores that request, for two reasons. `NativeActivity`'s focused view is its content view,
not the decor view. And an *implicit* request doesn't open the keyboard. The Android build
uses `NativeActivity` with no Java code (`android:hasCode="false"`) and no Gradle
(`scripts/build-android.sh`).

## Decision
- The app decides when the keyboard should be up (`urushi-app/src/soft_keyboard.rs`). A
  platform shell may supply `AppConfig::soft_keyboard`, a show/hide switch the runner calls
  instead of winit's `set_ime_allowed`.
- The Android shell (`apps/android/src/keyboard.rs`) posts each request to the Java UI thread
  (`AndroidApp::run_on_java_main_thread`). There it calls the `InputMethodManager` through the
  `jni` crate: `showSoftInput(<focused view>, 0)` to show, and
  `hideSoftInputFromWindow(<window token>, 0)` to hide.
- Typing needs nothing more. The content view has no `InputConnection`, so the IME sends key
  events. They reach winit's `KeyboardInput` the same way a hardware keyboard's do.
- iOS keeps winit's `becomeFirstResponder`, now called after the window is visible and again
  on each tap.

## Alternatives considered
- **A Java shim** (a `NativeActivity` subclass with a `showSoftInput` helper): needs `javac`
  and `d8` in the Gradle-less build, `hasCode="true"`, and a second language in the shell. It
  makes the same calls the JNI code makes.
- **Switching to `GameActivity`:** it has real IME support (`InputConnection`, composing text),
  but it needs its Java/AAR part, which means Gradle or vendored Java sources, and it changes
  the shell's input path. That's too much for this fix. It may be worth revisiting for
  composing languages.
- **Fixing it upstream in winit/android-activity:** the right long-term fix, but the app
  needs a working keyboard now. If upstream starts targeting the focused view, the shell's
  switch can be removed.
- **`ANativeActivity_showSoftInput`** (the NDK call, which targets the right view on the UI
  thread): `android-activity` doesn't expose the `ANativeActivity` pointer.

## Consequences
- `jni` becomes a direct dependency of the Android shell. It was already in the tree through
  `android-activity`, at the same version.
- The shell has two small `unsafe` conversions, from the raw `JavaVM` and activity pointers
  that `android-activity` hands out.
- Without an `InputConnection`, IME features that need one (autocorrect, composing CJK text,
  swipe typing) don't apply. Plain typing, digits and symbols work.
