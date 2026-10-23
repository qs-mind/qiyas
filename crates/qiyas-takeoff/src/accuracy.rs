//! Independent accuracy verdicts. Never combine count, geometry and net-room denominators.
use qiyas_core::{Kind, Measurement, MeasurementBasis};

#[derive(Debug, PartialEq)]
pub struct Accuracy {
    /// In the measurement's displayed unit; None for invalid inputs.
    pub absolute_error: Option<f64>,
    pub absolute_pass: Option<bool>,
    /// None for a zero reference or rounded/invalid inputs; never divide by zero.
    pub within_one_percent: Option<bool>,
    /// Counts must be nonnegative integers and match exactly, regardless of tolerance.
    pub exact_count: Option<bool>,
}

pub fn accuracy(
    actual: f64,
    reference: f64,
    absolute_tolerance: f64,
    kind: Kind,
    rounded: bool,
) -> Accuracy {
    let valid = actual.is_finite()
        && reference.is_finite()
        && absolute_tolerance.is_finite()
        && absolute_tolerance >= 0.0;
    let error = valid
        .then(|| (actual - reference).abs())
        .filter(|e| e.is_finite());
    Accuracy {
        absolute_error: error,
        absolute_pass: error.filter(|_| !rounded).map(|e| e <= absolute_tolerance),
        within_one_percent: error
            .filter(|_| !rounded && reference != 0.0)
            .map(|e| e / reference.abs() <= 0.01),
        exact_count: (kind == Kind::Count && valid && !rounded).then(|| {
            actual >= 0.0
                && reference >= 0.0
                && actual.fract() == 0.0
                && reference.fract() == 0.0
                && actual == reference
        }),
    }
}

/// The only net-room report entry point: unapproved, legacy and wall-face rows enter
/// neither numerator nor denominator. Caller supplies an independent approved net reference
/// in the measurement's displayed unit, plus its absolute tolerance and rounding status.
#[derive(Debug, Default, PartialEq)]
pub struct NetRoomAccuracy {
    pub eligible: usize,
    pub within_one_percent: usize,
    pub outside_one_percent: usize,
    pub indeterminate: usize,
    pub excluded: usize,
}

pub fn net_room_accuracy<'a>(
    rows: impl IntoIterator<Item = (&'a Measurement, f64, f64, bool)>,
) -> NetRoomAccuracy {
    let mut report = NetRoomAccuracy::default();
    for (m, reference, tolerance, rounded) in rows {
        if !m.available()
            || m.kind != Kind::Area
            || m.measurement_basis != MeasurementBasis::ApprovedNet
        {
            report.excluded += 1;
            continue;
        }
        report.eligible += 1;
        match accuracy(m.shown, reference, tolerance, m.kind, rounded).within_one_percent {
            Some(true) => report.within_one_percent += 1,
            Some(false) => report.outside_one_percent += 1,
            None => report.indeterminate += 1,
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use qiyas_core::Units;

    #[test]
    fn absolute_relative_integer_zero_and_rounding_have_separate_verdicts() {
        let a = accuracy(0.0102, 0.01, 0.001, Kind::Area, false);
        assert_eq!(a.absolute_pass, Some(true));
        assert_eq!(a.within_one_percent, Some(false));
        for (actual, reference) in [(30.0, 26.0), (186.0, 182.0), (101.0, 100.0), (1.5, 1.5)] {
            assert_eq!(
                accuracy(actual, reference, 10.0, Kind::Count, false).exact_count,
                Some(false)
            );
        }
        assert_eq!(
            accuracy(26.0, 26.0, 0.0, Kind::Count, false).exact_count,
            Some(true)
        );
        assert_eq!(
            accuracy(0.0, 0.0, 0.0, Kind::Area, false).within_one_percent,
            None
        );
        assert_eq!(
            accuracy(1.0, 1.0, 0.001, Kind::Area, true).absolute_pass,
            None
        );
        assert_eq!(
            accuracy(f64::NAN, 1.0, 1.0, Kind::Area, false).absolute_error,
            None
        );
    }

    #[test]
    fn unapproved_candidates_and_legacy_rows_never_enter_net_accuracy() {
        let mut m = Measurement::new(
            Kind::Area,
            12.0,
            Units::Metres,
            "Fountain on A-ROOM".into(),
            "A-ROOM".into(),
            "*Model_Space".into(),
        );
        for basis in [
            MeasurementBasis::Unspecified,
            MeasurementBasis::Gross,
            MeasurementBasis::WallFace,
            MeasurementBasis::UnapprovedNet,
        ] {
            m.measurement_basis = basis;
            let r = net_room_accuracy([(&m, 12.0, 0.001, false)]);
            assert_eq!(r.eligible, 0);
            assert_eq!(r.within_one_percent, 0);
            assert_eq!(r.excluded, 1);
        }
        m.measurement_basis = MeasurementBasis::ApprovedNet;
        assert_eq!(
            net_room_accuracy([(&m, 12.0, 0.001, false)]).within_one_percent,
            1
        );
    }
}
