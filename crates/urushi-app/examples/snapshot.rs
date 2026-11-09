//! Renders the app's UI offscreen to a BMP, without a window or daemon. Handy for
//! checking layouts at phone, tablet and desktop sizes:
//!
//! ```sh
//! cargo run -p urushi-app --example snapshot -- out.bmp 402 874 3 [connect|empty|<scenario>]
//! ```
//! Scenarios (see `snapshot::SCENARIOS`) show screens of a sample workspace, e.g.
//! `launcher`, `picker`, `add-project`, `new-thread`, `settings`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "dev tool"
)]

use std::io::Write;

use urushi_app::{ConnectTarget, snapshot};
use urushi_render::Offscreen;
use urushi_ui::{TextSystem, Theme};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "snapshot.bmp".into());
    let width: f32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1280.0);
    let height: f32 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(820.0);
    let scale: f32 = args.get(4).and_then(|a| a.parse().ok()).unwrap_or(2.0);
    let mode = args.get(5).map_or("empty", String::as_str);
    let target = if mode == "connect" {
        ConnectTarget::Ask
    } else {
        ConnectTarget::Local(None)
    };

    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    let scene = snapshot::scenario(mode, &theme, &mut text, width, height)
        .unwrap_or_else(|| snapshot::scene(target, &theme, &mut text, width, height));
    let mut gpu = Offscreen::new().expect("a GPU adapter");
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "window sizes"
    )]
    let (pw, ph) = ((width * scale) as u32, (height * scale) as u32);
    let image = gpu
        .render(&scene, &mut text, pw, ph, scale, theme.background)
        .unwrap();
    write_bmp(&out, image.width, image.height, &image.rgba).unwrap();
    eprintln!("wrote {out} ({pw}x{ph})");
}

/// Minimal 32-bit BMP writer (no image crate needed).
fn write_bmp(path: &str, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    let data_len = width * height * 4;
    let mut header = Vec::with_capacity(54);
    header.extend_from_slice(b"BM");
    header.extend_from_slice(&(54 + data_len).to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&54u32.to_le_bytes());
    header.extend_from_slice(&40u32.to_le_bytes());
    header.extend_from_slice(&width.to_le_bytes());
    header.extend_from_slice(&(-i32::try_from(height).unwrap_or(i32::MAX)).to_le_bytes()); // top-down
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&32u16.to_le_bytes());
    header.extend_from_slice(&[0u8; 24]);
    f.write_all(&header)?;
    let bgra: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();
    f.write_all(&bgra)
}
