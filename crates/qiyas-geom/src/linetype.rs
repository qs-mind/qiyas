//! CAD dash patterns, continuous across the vertices of a path. Measurement keeps the source path.
use crate::Path;

pub(crate) fn dashed(path: &Path, pattern: &[f64], budget: usize) -> (Vec<Path>, bool) {
    let period: f64 = pattern.iter().map(|x| x.abs()).sum();
    if pattern.is_empty() || !period.is_finite() || period <= 1e-12 {
        return (vec![path.clone()], false);
    }
    let dot = period * 0.002;
    if !pattern.iter().any(|v| *v >= 0.0) {
        return (Vec::new(), false);
    }
    let mut steps = 0usize;
    let mut out = Vec::new();
    let mut index = 0;
    let mut remaining = pattern[0].abs();
    let n = if path.closed {
        path.points.len()
    } else {
        path.points.len().saturating_sub(1)
    };
    for i in 0..n {
        let (a, b) = (path.points[i], path.points[(i + 1) % path.points.len()]);
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        if length <= 1e-12 {
            continue;
        }
        let at = |t: f64| {
            [
                a[0] + (b[0] - a[0]) * t / length,
                a[1] + (b[1] - a[1]) * t / length,
            ]
        };
        let mut t = 0.0;
        while t < length {
            steps += 1;
            if out.len() >= budget || steps > 100_000 {
                return (out, true);
            }
            if remaining <= 1e-12 {
                if pattern[index] == 0.0 {
                    out.push(Path {
                        points: vec![at(t), at((t + dot).min(length))],
                        closed: false,
                    });
                }
                index = (index + 1) % pattern.len();
                remaining = pattern[index].abs();
                continue;
            }
            let step = remaining.min(length - t);
            if pattern[index] > 0.0 {
                out.push(Path {
                    points: vec![at(t), at(t + step)],
                    closed: false,
                });
            }
            t += step;
            remaining -= step;
        }
    }
    (out, false)
}
