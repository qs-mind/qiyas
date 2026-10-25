//! An island needs explicit geometric support; symbol outlines alone do not remove floor area.

/// A traced edge may combine several collinear source edges. Check their combined coverage.
pub(super) fn supports_edge(
    a: [f64; 2],
    b: [f64; 2],
    sources: &[[f64; 4]],
    tolerance: f64,
) -> bool {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0].hypot(d[1]);
    if length <= tolerance {
        return true;
    }
    let unit = [d[0] / length, d[1] / length];
    let project = |p: [f64; 2]| (p[0] - a[0]) * unit[0] + (p[1] - a[1]) * unit[1];
    let distance = |p: [f64; 2]| ((p[0] - a[0]) * unit[1] - (p[1] - a[1]) * unit[0]).abs();
    let mut intervals = Vec::new();
    for s in sources {
        let (p, q) = ([s[0], s[1]], [s[2], s[3]]);
        if distance(p) > tolerance || distance(q) > tolerance {
            continue;
        }
        let (p, q) = (project(p), project(q));
        let lo = p.min(q).max(0.0);
        let hi = p.max(q).min(length);
        if hi + tolerance >= lo {
            intervals.push([lo, hi]);
        }
    }
    intervals.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut covered = 0.0;
    for [lo, hi] in intervals {
        if lo > covered + tolerance {
            break;
        }
        covered = covered.max(hi);
        if covered >= length - tolerance {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collinear_support_can_be_split_but_nearby_parallel_symbols_do_not_count() {
        let source = [[0., 0., 1., 0.], [2., 0., 1., 0.]];
        assert!(supports_edge([0., 0.], [2., 0.], &source, 1e-6));
        assert!(!supports_edge([0., 0.1], [2., 0.1], &source, 1e-6));
        assert!(!supports_edge([0., 0.], [3., 0.], &source, 1e-6));
    }
}
