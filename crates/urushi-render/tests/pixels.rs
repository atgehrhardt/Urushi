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
        ..Quad::default()
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

/// Black and white stripes, 4 px wide, over the whole 200×100 image.
fn stripes(scene: &mut Scene) {
    for i in 0..25 {
        #[allow(clippy::cast_precision_loss, reason = "small integers")]
        scene.rect(
            Rect::new(i as f32 * 8.0, 0.0, 4.0, 100.0),
            Color::hex(0xffffff),
        );
    }
}

fn contrast(img: &urushi_render::Image, xs: std::ops::Range<u32>, y: u32) -> u8 {
    let reds: Vec<u8> = xs.map(|x| img.pixel(x, y)[0]).collect();
    reds.iter().max().unwrap() - reds.iter().min().unwrap()
}

#[test]
fn a_backdrop_frosts_what_is_under_it_and_nothing_else() {
    let Some(mut gpu) = offscreen() else { return };
    let mut text = TextSystem::bundled_only();
    let mut scene = Scene::new();
    stripes(&mut scene);
    scene.backdrop(Rect::new(100.0, 20.0, 80.0, 60.0), 8.0, 16.0);
    // Something drawn after the backdrop stays sharp.
    scene.rect(Rect::new(130.0, 45.0, 10.0, 10.0), Color::hex(0xff0000));
    let img = gpu
        .render(&scene, &mut text, 200, 100, 1.0, Color::hex(0x000000))
        .unwrap();

    assert_eq!(contrast(&img, 10..30, 50), 255, "outside: still sharp");
    assert!(
        contrast(&img, 110..128, 30) < 60,
        "inside: stripes blurred ({})",
        contrast(&img, 110..128, 30)
    );
    let grey = img.pixel(115, 30);
    assert!(
        (80..180).contains(&grey[0]),
        "the blur averages to grey: {grey:?}"
    );
    assert_eq!(
        img.pixel(135, 50),
        [255, 0, 0, 255],
        "later primitives on top"
    );
    assert_eq!(contrast(&img, 10..30, 95), 255, "rows below the shape too");
    assert_eq!(img.pixel(100, 20)[3], 255, "opaque everywhere");
}

#[test]
fn soft_quads_fade_and_gradients_blend() {
    let Some(mut gpu) = offscreen() else { return };
    let mut text = TextSystem::bundled_only();
    let mut scene = Scene::new();
    scene.quad(Quad {
        rect: Rect::new(20.0, 20.0, 60.0, 60.0),
        color: Color::hex(0xffffff),
        softness: 20.0,
        ..Quad::default()
    });
    scene.quad(Quad {
        rect: Rect::new(120.0, 0.0, 40.0, 100.0),
        color: Color::hex(0xff0000),
        gradient: Some(Color::hex(0x0000ff)),
        ..Quad::default()
    });
    let img = gpu
        .render(&scene, &mut text, 200, 100, 1.0, Color::hex(0x000000))
        .unwrap();

    let at = |x| img.pixel(x, 50)[0];
    assert_eq!(at(50), 255, "solid in the middle");
    assert!(
        at(25) > at(20) && at(20) > at(15) && at(15) > 0,
        "fades across the edge, inside and out"
    );
    assert!(
        (60..200).contains(&at(20)),
        "about half on the outline: {}",
        at(20)
    );
    assert_eq!(at(8), 0, "nothing beyond the feather");

    let top = img.pixel(140, 2);
    let bottom = img.pixel(140, 97);
    assert!(top[0] > 240 && top[2] < 15, "red at the top: {top:?}");
    assert!(
        bottom[2] > 240 && bottom[0] < 15,
        "blue at the bottom: {bottom:?}"
    );
}
