//! Urushi on iOS. iOS can't spawn local shells, so the app always connects to a remote
//! daemon, over Tailscale or as a paired LAN device: it goes back to the host it used
//! last, or opens with the "connect to a host" dialog. Bundle with
//! `scripts/bundle-ios.sh`.

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            "info,winit=error,urushi_app=debug,wgpu_core=warn,wgpu_hal=warn,naga=warn",
        ))
        .with_writer(std::io::stderr)
        .init();
    urushi_app::log_panics();
    // `URUSHI_HOST` (e.g. set by MDM or `SIMCTL_CHILD_URUSHI_HOST`) skips the connect dialog.
    let target = match std::env::var("URUSHI_HOST") {
        Ok(host) if !host.trim().is_empty() => urushi_app::ConnectTarget::Remote(host),
        _ => urushi_app::ConnectTarget::Ask,
    };
    let mut config = urushi_app::AppConfig::new(target);
    // The app's sandboxed Application Support folder keeps the LAN keyring and recent
    // hosts (ADR 0041, 0043).
    config.data_dir = std::env::var_os("HOME").map(|home| {
        std::path::PathBuf::from(home)
            .join("Library")
            .join("Application Support")
    });
    if let Err(e) = urushi_app::run(config) {
        // There's no console on device. The message shows in the Xcode or simulator log.
        let _ =
            std::io::Write::write_all(&mut std::io::stderr(), format!("urushi: {e}\n").as_bytes());
    }
}
