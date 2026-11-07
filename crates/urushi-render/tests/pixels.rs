//! Renders scenes on the real GPU and checks pixels. Skipped where no adapter exists
//! (e.g. headless CI without a software rasterizer).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test code"
)]

use urushi_render::Offscreen;
use urushi_ui::{Color, Font, Point, Quad, Rect, Scene, TextSystem};

fn offscreen() -> Option<Offscreen> {
    match Offscreen::new() {
        Ok(o) => Some(o),
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

#[test]
fn quads_clips_and_text_reach_the_framebuffer() {
    let Some(mut gpu) = offscreen() else { return };
    let mut text = TextSystem::bundled_only();
    let mut scene = Scene::new();
    scene.rect(Rect::new(10.0, 10.0, 30.0, 30.0), Color::hex(0xff0000));
    scene.quad(Quad {
        rect: Rect::new(50.0, 10.0, 40.0, 40.0),
        color: Color::hex(0x00ff00),
        radius: 20.0,
        border_width: 0.0,
        border_color: Color::TRANSPARENT,
    });
    scene.push_clip(Rect::new(0.0, 60.0, 50.0, 40.0));
    scene.rect(Rect::new(0.0, 60.0, 100.0, 40.0), Color::hex(0x0000ff));
    scene.pop_clip();
    scene.text(
        Point::new(100.0, 10.0),
        "Hg",
        Font::UiSemibold,
        24.0,
        Color::hex(0xffffff),
    );
    let img = gpu
        .render(&scene, &mut text, 200, 100, 1.0, Color::hex(0x000000))
        .unwrap();

    assert_eq!(img.pixel(20, 20), [255, 0, 0, 255], "plain quad");
    assert_eq!(img.pixel(70, 30)[1], 255, "circle centre is green");
    assert_eq!(
        img.pixel(51, 11),
        [0, 0, 0, 255],
        "circle corner is cut away"
    );
    assert_eq!(img.pixel(25, 80), [0, 0, 255, 255], "inside the clip");
    assert_eq!(img.pixel(75, 80), [0, 0, 0, 255], "outside the clip");
    let text_pixels = (100..160)
        .flat_map(|x| (10..45).map(move |y| (x, y)))
        .filter(|&(x, y)| img.pixel(x, y)[0] > 128)
        .count();
    assert!(
        text_pixels > 50,
        "text was rasterized ({text_pixels} lit pixels)"
    );
}

#[test]
fn high_dpi_scales_geometry() {
    let Some(mut gpu) = offscreen() else { return };
    let mut text = TextSystem::bundled_only();
    let mut scene = Scene::new();
    scene.rect(Rect::new(10.0, 10.0, 10.0, 10.0), Color::hex(0xffffff));
    let img = gpu
        .render(&scene, &mut text, 100, 100, 2.0, Color::hex(0x000000))
        .unwrap();
    assert_eq!(img.pixel(38, 38), [255, 255, 255, 255]);
    assert_eq!(img.pixel(41, 41), [0, 0, 0, 255]);
}
