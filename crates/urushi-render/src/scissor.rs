//! Turning a clip rectangle into a scissor rectangle the GPU accepts.
//!
//! wgpu rejects a scissor that reaches outside the render target, and a rejected
//! command panics the app. A clip can lie wholly off screen (a board card scrolled below
//! the bottom), and at fractional scales (Android's 2.625) its edges fall between pixels,
//! so rounding outwards could turn an empty clip into a 1-pixel one outside the target
//! (UR-55). Every scissor is therefore clamped to the target, and empty clips stay empty.

use urushi_ui::Rect;

/// The pixels `clip` covers in a `target` of `(width, height)` pixels, as
/// `[x, y, width, height]`, rounded outwards and always inside the target. A clip with
/// no area, or none on the target, gives a zero size, which draws nothing.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "clamped to non-negative pixel coordinates within the target, far below 2^24"
)]
pub(crate) fn scissor_of(clip: Rect, (width, height): (u32, u32)) -> [u32; 4] {
    if clip.width <= 0.0 || clip.height <= 0.0 || clip.width.is_nan() || clip.height.is_nan() {
        return [0; 4];
    }
    let (w, h) = (width as f32, height as f32);
    let x = clip.x.floor().clamp(0.0, w);
    let y = clip.y.floor().clamp(0.0, h);
    let right = clip.right().ceil().clamp(x, w);
    let bottom = clip.bottom().ceil().clamp(y, h);
    [x as u32, y as u32, (right - x) as u32, (bottom - y) as u32]
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: (u32, u32) = (1080, 2400);

    /// wgpu's rule: the scissor lies within the target.
    fn inside([x, y, w, h]: [u32; 4]) -> bool {
        x + w <= TARGET.0 && y + h <= TARGET.1
    }

    #[test]
    fn a_clip_inside_the_target_is_rounded_outwards() {
        let s = scissor_of(Rect::new(10.4, 20.6, 100.2, 50.0), TARGET);
        assert_eq!(s, [10, 20, 101, 51]);
    }

    #[test]
    fn an_empty_clip_below_the_target_draws_nothing() {
        // What the board clipped a card below the screen to, on an Android phone.
        let s = scissor_of(Rect::new(39.375, 2516.4, 178.5, 0.0), TARGET);
        assert!(s[2] == 0 || s[3] == 0, "{s:?}");
        assert!(inside(s), "{s:?}");
    }

    #[test]
    fn clips_reaching_outside_are_cut_to_the_target() {
        for clip in [
            Rect::new(-5.5, -5.5, 20.0, 20.0),
            Rect::new(1000.5, 2390.5, 200.0, 200.0),
            Rect::new(2000.0, 3000.0, 10.0, 10.0),
            Rect::new(-100.0, -100.0, 50.0, 50.0),
            Rect::new(1079.6, 2399.6, 0.2, 0.2),
        ] {
            let s = scissor_of(clip, TARGET);
            assert!(inside(s), "{clip:?} gave {s:?}");
        }
        assert_eq!(
            scissor_of(Rect::new(-5.5, -5.5, 20.0, 20.0), TARGET),
            [0, 0, 15, 15]
        );
        assert_eq!(
            scissor_of(Rect::new(1000.5, 2390.5, 200.0, 200.0), TARGET),
            [1000, 2390, 80, 10]
        );
    }

    #[test]
    fn a_clip_with_no_area_draws_nothing() {
        for clip in [
            Rect::new(10.5, 10.5, 0.0, 30.0),
            Rect::new(10.5, 10.5, 30.0, 0.0),
            Rect::new(10.5, 10.5, -3.0, 30.0),
            Rect::new(10.5, 10.5, f32::NAN, 30.0),
        ] {
            let [_, _, w, h] = scissor_of(clip, TARGET);
            assert!(w == 0 || h == 0, "{clip:?}");
        }
    }
}
