//! Scaling bitmaps (`urushi_ui::ImageData`) to the pixel size they are drawn at, so they
//! stay crisp without mipmaps. Box filtering in premultiplied space: transparent edges
//! don't bleed dark fringes into the colour.

use urushi_ui::ImageData;

/// `image` resized to `width` × `height` (straight RGBA, like the input).
pub(crate) fn resample(image: &ImageData, width: u32, height: u32) -> Vec<u8> {
    let (sw, sh) = (f64::from(image.width), f64::from(image.height));
    let (dw, dh) = (f64::from(width), f64::from(height));
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        let (y0, y1) = (f64::from(y) * sh / dh, f64::from(y + 1) * sh / dh);
        for x in 0..width {
            let (x0, x1) = (f64::from(x) * sw / dw, f64::from(x + 1) * sw / dw);
            out.extend(average(image, (x0, x1), (y0, y1)));
        }
    }
    out
}

/// The area-weighted average of the source pixels under `[x0, x1) × [y0, y1)`.
fn average(image: &ImageData, (x0, x1): (f64, f64), (y0, y1): (f64, f64)) -> [u8; 4] {
    let mut sum = [0.0f64; 4];
    let mut area = 0.0;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel coordinates are small and non-negative"
    )]
    let (first_x, last_x, first_y, last_y) = (
        x0.floor() as u32,
        (x1.ceil() as u32).min(image.width),
        y0.floor() as u32,
        (y1.ceil() as u32).min(image.height),
    );
    for sy in first_y..last_y {
        let wy = (y1.min(f64::from(sy + 1)) - y0.max(f64::from(sy))).max(0.0);
        for sx in first_x..last_x {
            let wx = (x1.min(f64::from(sx + 1)) - x0.max(f64::from(sx))).max(0.0);
            let w = wx * wy;
            let i = ((sy * image.width + sx) * 4) as usize;
            let a = f64::from(image.rgba[i + 3]) / 255.0;
            for (c, s) in sum.iter_mut().take(3).enumerate() {
                *s += f64::from(image.rgba[i + c]) * a * w;
            }
            sum[3] += a * w;
            area += w;
        }
    }
    if area == 0.0 || sum[3] == 0.0 {
        return [0; 4];
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255"
    )]
    let byte = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    [
        byte(sum[0] / sum[3]),
        byte(sum[1] / sum[3]),
        byte(sum[2] / sum[3]),
        byte(sum[3] / area * 255.0),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    #[test]
    fn halving_averages_and_keeps_colour_at_transparent_edges() {
        // 2×2: one opaque red pixel, three fully transparent black ones.
        let mut rgba = vec![0u8; 16];
        rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
        let image = ImageData::new(1, 2, 2, rgba).unwrap();
        let px = resample(&image, 1, 1);
        assert_eq!(
            px,
            [255, 0, 0, 64],
            "red at a quarter coverage, not dark red"
        );
    }

    #[test]
    fn same_size_is_a_copy_and_upscaling_repeats() {
        let rgba = vec![10, 20, 30, 255, 40, 50, 60, 128];
        let image = ImageData::new(2, 2, 1, rgba.clone()).unwrap();
        assert_eq!(resample(&image, 2, 1), rgba);
        let up = resample(&image, 4, 1);
        assert_eq!(&up[..8], &[10, 20, 30, 255, 10, 20, 30, 255]);
        assert_eq!(ImageData::new(3, 2, 2, vec![0; 3]), None);
    }
}
