//! Pure effect-repair logic, kept free of FFI so it can be unit-tested.
//!
//! The problem: Windows.Gaming.Input (via ffbroker.dll) asks pid.dll to download
//! a 2-axis effect (X + Z, polar) on a wheel whose only DirectInput force-feedback
//! actuator is X. pid.dll rejects the Z axis with E_UNEXPECTED because its object
//! ID carries neither DIDFT_FFACTUATOR nor DIDFT_FFEFFECTTRIGGER, and its error
//! path then calls DestroyEffect(0) and crashes. The repair drops every axis
//! pid.dll would reject, before pid.dll sees the effect.

/// `DIDFT_FFACTUATOR | DIDFT_FFEFFECTTRIGGER`. pid.dll's object-ID translation
/// (`sub_591c` in 10.0.26100.8115) returns E_UNEXPECTED when neither bit is set.
pub const PID_ACCEPTED_AXIS: u32 = 0x0300_0000;

pub const DIEFF_CARTESIAN: u32 = 0x10;
pub const DIEFF_POLAR: u32 = 0x20;
pub const DIEFF_SPHERICAL: u32 = 0x40;
const DIEFF_COORDS: u32 = DIEFF_CARTESIAN | DIEFF_POLAR | DIEFF_SPHERICAL;

/// Length of the cartesian direction vector written for multi-axis effects
/// (an arbitrary magnitude; only the ratio between the components matters).
const DIRECTION_SCALE: f64 = 10_000.0;

/// `sizeof(DICONDITION)`: six 32-bit fields. Condition effects (spring, damper,
/// inertia, friction) may carry one per axis.
pub const DICONDITION_SIZE: usize = 24;

#[derive(Debug, PartialEq)]
pub struct Repaired {
    /// New `DIEFFECT.dwFlags` (coordinate system switched to cartesian).
    pub flags: u32,
    pub axes: Vec<u32>,
    /// New direction, one entry per kept axis. `None` when the caller passed none.
    pub dirs: Option<Vec<i32>>,
    /// Per-axis DICONDITION array cut down to the kept axes, if there was one.
    pub type_specific: Option<Vec<u8>>,
    /// The original direction's component along each kept axis (for the log).
    pub components: Vec<f64>,
}

/// Returns `None` when the effect needs no change or cannot be repaired
/// (no axis survives the filter); the caller tells those apart by `kept_axes`.
pub fn repair(flags: u32, axes: &[u32], dirs: Option<&[i32]>, type_specific: Option<&[u8]>) -> Option<Repaired> {
    let keep = kept_axes(axes);
    if keep.is_empty() || keep.len() == axes.len() {
        return None;
    }

    let components = dirs.map(|d| direction_components(flags, d, axes.len()));
    let new_dirs = components.as_ref().map(|c| {
        if keep.len() == 1 {
            // A single-axis effect only needs the sign. Zero (the force had no
            // component along this axis) falls back to positive: see the README.
            vec![if c[keep[0]] < 0.0 { -1 } else { 1 }]
        } else {
            // Renormalise over the kept axes so the remaining force keeps full strength.
            let len = keep.iter().map(|&i| c[i] * c[i]).sum::<f64>().sqrt();
            let scale = if len > 0.0 { DIRECTION_SCALE / len } else { 0.0 };
            keep.iter().map(|&i| (c[i] * scale).round() as i32).collect()
        }
    });

    // A per-axis condition array shrinks with the axes. Any other size (a single
    // condition applied along the direction, periodic or constant-force params)
    // does not depend on the axis count and passes through untouched.
    let type_specific = type_specific.and_then(|ts| {
        (ts.len() == axes.len() * DICONDITION_SIZE).then(|| {
            keep.iter()
                .flat_map(|&i| ts[i * DICONDITION_SIZE..(i + 1) * DICONDITION_SIZE].iter().copied())
                .collect()
        })
    });

    Some(Repaired {
        flags: (flags & !DIEFF_COORDS) | DIEFF_CARTESIAN,
        axes: keep.iter().map(|&i| axes[i]).collect(),
        dirs: new_dirs,
        type_specific,
        components: components.map(|c| keep.iter().map(|&i| c[i]).collect()).unwrap_or_default(),
    })
}

/// Indices of the axes pid.dll accepts, in their original order.
pub fn kept_axes(axes: &[u32]) -> Vec<usize> {
    axes.iter()
        .enumerate()
        .filter(|(_, &a)| a & PID_ACCEPTED_AXIS != 0)
        .map(|(i, _)| i)
        .collect()
}

/// Unit vector of the effect direction, one component per axis. Conventions are
/// those of the DIEFFECT documentation (angles in hundredths of a degree):
/// - polar: measured from (0, -1) towards (1, 0), so the vector is (sin t, -cos t);
/// - spherical: the first angle from (1, 0) towards (0, 1), each further angle
///   towards the next axis;
/// - cartesian: `dirs` as given.
/// Missing `dirs` entries count as 0. A zero vector stays zero.
pub fn direction_components(flags: u32, dirs: &[i32], n: usize) -> Vec<f64> {
    let at = |i: usize| dirs.get(i).copied().unwrap_or(0);
    let angle = |i: usize| (at(i) as f64 / 100.0).to_radians();

    let mut v: Vec<f64> = if flags & DIEFF_POLAR != 0 {
        vec![angle(0).sin(), -angle(0).cos()]
    } else if flags & DIEFF_SPHERICAL != 0 {
        let mut v = vec![angle(0).cos(), angle(0).sin()];
        for k in 1..n.saturating_sub(1) {
            let a = angle(k);
            v.iter_mut().for_each(|x| *x *= a.cos());
            v.push(a.sin());
        }
        v
    } else {
        (0..n).map(|i| at(i) as f64).collect()
    };
    v.resize(n, 0.0);

    let len = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if len > 0.0 {
        v.iter_mut().for_each(|x| *x /= len);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DirectInput object IDs as the broker sends them for the Cammus C5:
    /// X is an FF actuator, Z (instance 7) is not.
    const X: u32 = 0x0100_0002;
    const Z: u32 = 0x0000_0702;

    fn polar_xz(angle: i32) -> Option<Repaired> {
        repair(DIEFF_POLAR, &[X, Z], Some(&[angle, 0]), None)
    }

    #[test]
    fn drops_z_and_switches_to_cartesian() {
        let r = polar_xz(9000).unwrap();
        assert_eq!(r.flags, DIEFF_CARTESIAN);
        assert_eq!(r.axes, vec![X]);
        assert_eq!(r.dirs, Some(vec![1]));
    }

    #[test]
    fn single_axis_keeps_the_sign_of_its_component() {
        assert_eq!(polar_xz(27000).unwrap().dirs, Some(vec![-1])); // west
        assert_eq!(polar_xz(4500).unwrap().dirs, Some(vec![1])); // north-east
    }

    #[test]
    fn zero_component_falls_back_to_positive() {
        assert_eq!(polar_xz(0).unwrap().dirs, Some(vec![1])); // due north: all on Z
        assert_eq!(polar_xz(18000).unwrap().dirs, Some(vec![1]));
    }

    #[test]
    fn no_direction_stays_none() {
        let r = repair(DIEFF_POLAR, &[X, Z], None, None).unwrap();
        assert_eq!(r.dirs, None);
        assert!(r.components.is_empty());
    }

    #[test]
    fn clean_effects_are_left_alone() {
        assert_eq!(repair(DIEFF_CARTESIAN, &[X], Some(&[1]), None), None);
        assert_eq!(repair(DIEFF_POLAR, &[X, X], Some(&[9000, 0]), None), None);
    }

    #[test]
    fn unrepairable_effects_return_none_and_kept_axes_says_why() {
        assert_eq!(repair(DIEFF_POLAR, &[Z, Z], Some(&[0, 0]), None), None);
        assert!(kept_axes(&[Z, Z]).is_empty());
    }

    #[test]
    fn condition_array_is_cut_to_the_kept_axes() {
        let mut ts = vec![0u8; 2 * DICONDITION_SIZE];
        ts[..DICONDITION_SIZE].fill(1);
        ts[DICONDITION_SIZE..].fill(2);
        let r = repair(DIEFF_CARTESIAN, &[Z, X], Some(&[5, 7]), Some(&ts)).unwrap();
        assert_eq!(r.axes, vec![X]);
        assert_eq!(r.type_specific, Some(vec![2; DICONDITION_SIZE]));
    }

    #[test]
    fn other_type_specific_sizes_pass_through() {
        let periodic = [0u8; 16];
        let r = repair(DIEFF_POLAR, &[X, Z], Some(&[9000, 0]), Some(&periodic)).unwrap();
        assert_eq!(r.type_specific, None);
    }

    #[test]
    fn three_axes_keep_relative_direction() {
        let r = repair(DIEFF_CARTESIAN, &[X, Z, X + 1], Some(&[3, 9, 4]), None).unwrap();
        assert_eq!(r.axes, vec![X, X + 1]);
        assert_eq!(r.dirs, Some(vec![6000, 8000])); // 3:4 normalised, scaled to 10000
        assert_eq!(r.components.len(), 2);
    }

    #[test]
    fn spherical_first_angle_runs_from_axis_0_towards_axis_1() {
        let c = direction_components(DIEFF_SPHERICAL, &[0, 0], 2);
        assert!((c[0] - 1.0).abs() < 1e-9 && c[1].abs() < 1e-9);
        let c = direction_components(DIEFF_SPHERICAL, &[9000, 0], 2);
        assert!(c[0].abs() < 1e-9 && (c[1] - 1.0).abs() < 1e-9);
    }
}
