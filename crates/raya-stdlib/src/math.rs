//! Math native implementations
//!
//! Provides the Rust-side handlers for `std:math` native calls.
//! All functions operate on f64 values.

use rand::Rng;

/// Absolute value
pub fn abs(x: f64) -> f64 {
    x.abs()
}

/// Sign of number (-1, 0, or 1)
pub fn sign(x: f64) -> f64 {
    x.signum()
}

/// Round down to nearest integer
pub fn floor(x: f64) -> f64 {
    x.floor()
}

/// Round up to nearest integer
pub fn ceil(x: f64) -> f64 {
    x.ceil()
}

/// Round to nearest integer
pub fn round(x: f64) -> f64 {
    x.round()
}

/// Truncate decimal part
pub fn trunc(x: f64) -> f64 {
    x.trunc()
}

/// Minimum of two numbers
pub fn min(a: f64, b: f64) -> f64 {
    a.min(b)
}

/// Maximum of two numbers
pub fn max(a: f64, b: f64) -> f64 {
    a.max(b)
}

/// Raise base to power exp
pub fn pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

/// Square root
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Sine
pub fn sin(x: f64) -> f64 {
    x.sin()
}

/// Cosine
pub fn cos(x: f64) -> f64 {
    x.cos()
}

/// Tangent
pub fn tan(x: f64) -> f64 {
    x.tan()
}

/// Arcsine
pub fn asin(x: f64) -> f64 {
    x.asin()
}

/// Arccosine
pub fn acos(x: f64) -> f64 {
    x.acos()
}

/// Arctangent
pub fn atan(x: f64) -> f64 {
    x.atan()
}

/// Two-argument arctangent
pub fn atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/// e raised to the power x
pub fn exp(x: f64) -> f64 {
    x.exp()
}

/// Natural logarithm (base e)
pub fn log(x: f64) -> f64 {
    x.ln()
}

/// Base-10 logarithm
pub fn log10(x: f64) -> f64 {
    x.log10()
}

/// Random number in [0, 1)
pub fn random() -> f64 {
    rand::thread_rng().gen::<f64>()
}

/// Pi constant
pub fn pi() -> f64 {
    std::f64::consts::PI
}

/// Euler's number constant
pub fn e() -> f64 {
    std::f64::consts::E
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abs() {
        assert_eq!(abs(-5.0), 5.0);
        assert_eq!(abs(3.0), 3.0);
        assert_eq!(abs(0.0), 0.0);
    }

    #[test]
    fn test_sign() {
        assert_eq!(sign(-5.0), -1.0);
        assert_eq!(sign(3.0), 1.0);
    }

    #[test]
    fn test_floor_ceil_round_trunc() {
        assert_eq!(floor(3.7), 3.0);
        assert_eq!(ceil(3.2), 4.0);
        assert_eq!(round(3.5), 4.0);
        assert_eq!(trunc(3.7), 3.0);
    }

    #[test]
    fn test_min_max() {
        assert_eq!(min(1.0, 2.0), 1.0);
        assert_eq!(max(1.0, 2.0), 2.0);
    }

    #[test]
    fn test_pow_sqrt() {
        assert_eq!(pow(2.0, 10.0), 1024.0);
        assert_eq!(sqrt(16.0), 4.0);
    }

    #[test]
    fn test_trig() {
        assert!((sin(0.0) - 0.0).abs() < 1e-10);
        assert!((cos(0.0) - 1.0).abs() < 1e-10);
        assert!((tan(0.0) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_exp_log() {
        assert!((exp(0.0) - 1.0).abs() < 1e-10);
        assert!((log(1.0) - 0.0).abs() < 1e-10);
        assert!((log10(100.0) - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_random_range() {
        let r = random();
        assert!(r >= 0.0 && r < 1.0);
    }

    #[test]
    fn test_constants() {
        assert!((pi() - std::f64::consts::PI).abs() < 1e-15);
        assert!((e() - std::f64::consts::E).abs() < 1e-15);
    }
}
