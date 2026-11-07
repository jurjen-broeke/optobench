//! Complex numbers, for thin-film optics (complex refractive indices and angles).

use core::ops::{Add, Div, Mul, Neg, Sub};

/// A complex number re + i·im.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl Complex {
    /// Zero.
    pub const ZERO: Self = Self::new(0.0, 0.0);
    /// One.
    pub const ONE: Self = Self::new(1.0, 0.0);
    /// The imaginary unit.
    pub const I: Self = Self::new(0.0, 1.0);

    /// re + i·im.
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// A real number.
    pub const fn real(re: f64) -> Self {
        Self::new(re, 0.0)
    }

    /// |z|².
    pub fn norm_squared(self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    /// |z|.
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// The complex conjugate.
    pub fn conjugate(self) -> Self {
        Self::new(self.re, -self.im)
    }

    /// The principal square root (real part ≥ 0).
    pub fn sqrt(self) -> Self {
        let r = self.abs();
        let re = ((r + self.re) / 2.0).max(0.0).sqrt();
        let im = ((r - self.re) / 2.0).max(0.0).sqrt();
        Self::new(re, if self.im < 0.0 { -im } else { im })
    }

    /// e^z.
    pub fn exp(self) -> Self {
        let (s, c) = self.im.sin_cos();
        let m = self.re.exp();
        Self::new(m * c, m * s)
    }

    /// sin z = sin a cosh b + i cos a sinh b.
    pub fn sin(self) -> Self {
        let (s, c) = self.re.sin_cos();
        Self::new(s * self.im.cosh(), c * self.im.sinh())
    }

    /// cos z = cos a cosh b − i sin a sinh b.
    pub fn cos(self) -> Self {
        let (s, c) = self.re.sin_cos();
        Self::new(c * self.im.cosh(), -s * self.im.sinh())
    }
}

impl Add for Complex {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for Complex {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
}

impl Mul for Complex {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

impl Mul<f64> for Complex {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.re * s, self.im * s)
    }
}

impl Div for Complex {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let d = o.norm_squared();
        Self::new(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }
}

impl Neg for Complex {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.re, -self.im)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-14;

    fn close(a: Complex, b: Complex) -> bool {
        (a - b).abs() < EPS
    }

    #[test]
    fn arithmetic_and_identities() {
        let a = Complex::new(1.5, -0.25);
        let b = Complex::new(-0.3, 2.0);
        assert!(close((a * b) / b, a));
        assert!(close(a.sqrt() * a.sqrt(), a));
        assert!(Complex::new(-4.0, 0.0).sqrt() == Complex::new(0.0, 2.0));
        // sin² + cos² = 1 for complex arguments too.
        let z = Complex::new(0.7, 0.4);
        assert!(close(z.sin() * z.sin() + z.cos() * z.cos(), Complex::ONE));
        // Euler: e^(iπ) = −1.
        assert!(close(
            (Complex::I * core::f64::consts::PI).exp(),
            -Complex::ONE
        ));
    }
}
