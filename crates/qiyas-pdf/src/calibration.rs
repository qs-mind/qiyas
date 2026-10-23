//! Refine approximate scale-bar anchors against raster ink, without interpreting a drawing.
use image::RgbaImage;

#[derive(Debug, Clone)]
pub struct RasterSpan {
    pub a: [f64; 2],
    pub b: [f64; 2],
    /// Relative endpoint uncertainty, including disagreement between the two tick halves.
    pub uncertainty: f64,
}

/// Find the centres of the transverse ticks near two supplied page anchors. Integrating ink
/// coverage gives sub-pixel centres; subtracting the background rejects the horizontal bar.
/// Multiple nearby ticks, absent ink or a plain unmarked edge are not a calibration.
pub fn refine_raster_span(
    image: &RgbaImage,
    size: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
) -> Option<RasterSpan> {
    if !size.iter().all(|v| v.is_finite() && *v > 0.0)
        || !a.iter().chain(b.iter()).all(|v| v.is_finite())
        || image.width() == 0
        || image.height() == 0
    {
        return None;
    }
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    let pixel = (size[0] / f64::from(image.width())).max(size[1] / f64::from(image.height()));
    if length < 12.0 * pixel {
        return None;
    }
    let u = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
    let v = [-u[1], u[0]];
    let radius = (length * 0.06).max(3.0 * pixel).min(24.0 * pixel);
    let reach = (length * 0.10).max(4.0 * pixel).min(32.0 * pixel);
    // Locate the bar itself first. Approximate anchors may lie off its centreline; otherwise
    // the bar contaminates one half of the tick and biases the endpoint towards the interior.
    let m = (reach / pixel).ceil() as i32;
    let baseline: Vec<f64> = (-m..=m)
        .map(|j| {
            (1..=9)
                .map(|i| {
                    let along = length * (0.2 + 0.06 * f64::from(i));
                    ink(
                        image,
                        size,
                        std::array::from_fn(|axis| {
                            a[axis] + along * u[axis] + f64::from(j) * pixel * v[axis]
                        }),
                    )
                })
                .sum()
        })
        .collect();
    let shift = peak(&baseline, m, pixel)?;
    let a = std::array::from_fn(|axis| a[axis] + shift * v[axis]);
    let b = std::array::from_fn(|axis| b[axis] + shift * v[axis]);
    let tick = |at: [f64; 2]| -> Option<([f64; 2], f64)> {
        let n = (radius / pixel).ceil() as i32;
        let m = (reach / pixel).ceil() as i32;
        let profiles: Vec<Vec<f64>> = [-1.0, 1.0]
            .into_iter()
            .map(|side| {
                (-n..=n)
                    .map(|i| {
                        (3..=m)
                            .map(|j| {
                                let p = std::array::from_fn(|axis| {
                                    at[axis]
                                        + pixel
                                            * (f64::from(i) * u[axis]
                                                + side * f64::from(j) * v[axis])
                                });
                                ink(image, size, p)
                            })
                            .sum()
                    })
                    .collect()
            })
            .collect();
        let centres: Option<Vec<_>> = profiles
            .iter()
            .map(|profile| peak(profile, n, pixel))
            .collect();
        let centres = centres?;
        let offset = (centres[0] + centres[1]) * 0.5;
        Some((
            std::array::from_fn(|axis| at[axis] + offset * u[axis]),
            (centres[0] - centres[1]).abs().max(pixel * 0.5),
        ))
    };
    let (a, ea) = tick(a)?;
    let (b, eb) = tick(b)?;
    let refined = (b[0] - a[0]).hypot(b[1] - a[1]);
    (refined > 0.0).then_some(RasterSpan {
        a,
        b,
        uncertainty: ea.hypot(eb) / refined,
    })
}

fn peak(profile: &[f64], n: i32, pixel: f64) -> Option<f64> {
    let baseline = profile.iter().copied().min_by(f64::total_cmp)?;
    let weights: Vec<_> = profile.iter().map(|v| (v - baseline).max(0.0)).collect();
    let maximum = weights.iter().copied().max_by(f64::total_cmp)?;
    if maximum < 0.5 {
        return None;
    }
    let occupied: Vec<_> = weights
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > maximum * 0.1)
        .map(|(i, _)| i)
        .collect();
    if occupied.windows(2).any(|w| w[1] != w[0] + 1)
        || *occupied.first()? == 0
        || *occupied.last()? + 1 == weights.len()
    {
        return None;
    }
    let total: f64 = weights.iter().sum();
    Some(
        weights
            .iter()
            .enumerate()
            .map(|(i, w)| (i as f64 - f64::from(n)) * pixel * w)
            .sum::<f64>()
            / total,
    )
}

fn ink(image: &RgbaImage, size: [f64; 2], p: [f64; 2]) -> f64 {
    let x = p[0] / size[0] * f64::from(image.width()) - 0.5;
    let y = (size[1] - p[1]) / size[1] * f64::from(image.height()) - 0.5;
    let mut value = 0.0;
    for dx in 0..=1 {
        for dy in 0..=1 {
            let px = x.floor() as i64 + dx;
            let py = y.floor() as i64 + dy;
            if px < 0 || py < 0 || px >= i64::from(image.width()) || py >= i64::from(image.height())
            {
                continue;
            }
            let c = image.get_pixel(px as u32, py as u32).0;
            let darkness = 1.0 - (f64::from(c[0]) + f64::from(c[1]) + f64::from(c[2])) / 765.0;
            value += darkness * (1.0 - (x - px as f64).abs()) * (1.0 - (y - py as f64).abs());
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn antialiased_tick_centres_and_missing_or_ambiguous_ticks() {
        let mut image = RgbaImage::from_pixel(240, 100, image::Rgba([255; 4]));
        for x in [20, 220] {
            for y in 43..57 {
                image.put_pixel(x, y, image::Rgba([64, 64, 64, 255]));
                image.put_pixel(x + 1, y, image::Rgba([191, 191, 191, 255]));
            }
        }
        for x in 20..=221 {
            for y in 49..51 {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        let result =
            refine_raster_span(&image, [240.0, 100.0], [19.0, 50.0], [222.0, 50.0]).unwrap();
        assert!((result.a[0] - 20.75).abs() < 0.01);
        assert!((result.b[0] - 220.75).abs() < 0.01);
        assert!(result.uncertainty < 0.005);
        let rotated = image::imageops::rotate90(&image);
        let vertical =
            refine_raster_span(&rotated, [100.0, 240.0], [50.0, 221.0], [50.0, 18.0]).unwrap();
        assert!(((vertical.b[1] - vertical.a[1]).abs() - 200.0).abs() < 0.01);
        for y in 43..57 {
            image.put_pixel(15, y, image::Rgba([0, 0, 0, 255]));
        }
        assert!(refine_raster_span(&image, [240.0, 100.0], [19.0, 50.0], [222.0, 50.0]).is_none());
        assert!(refine_raster_span(&image, [240.0, 100.0], [100.0, 10.0], [200.0, 10.0]).is_none());
    }
}
