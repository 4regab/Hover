//! main.js's small helpers, with JavaScript's number rules where they matter: the
//! seeded generator must give the page's exact sequence, since the room's colours and
//! the books on the shelves come from it.

pub const TAU: f64 = std::f64::consts::PI * 2.0;

/// `rng(s)`: mulberry32, in JS's 32-bit integer arithmetic.
#[derive(Clone)]
pub struct Rng(i32);

impl Rng {
    pub fn new(seed: i32) -> Rng { Rng(seed) }

    /// `s = s + 0x6D2B79F5 | 0; let t = Math.imul(s ^ s >>> 15, 1 | s); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
    /// return ((t ^ t >>> 14) >>> 0) / 4294967296`.
    pub fn next(&mut self) -> f64 {
        let s = self.0.wrapping_add(0x6D2B79F5);
        self.0 = s;
        let ush = |x: i32, n: u32| ((x as u32) >> n) as i32;
        let mut t = (s ^ ush(s, 15)).wrapping_mul(1 | s);
        t = t.wrapping_add((t ^ ush(t, 7)).wrapping_mul(61 | t)) ^ t;
        ((t ^ ush(t, 14)) as u32) as f64 / 4294967296.0
    }
}

/// `ease(v, to, k, dt)`: exponential approach.
pub fn ease(v: f64, to: f64, k: f64, dt: f64) -> f64 { v + (to - v) * (1.0 - (-k * dt).exp()) }

/// `angTo`: the shortest way round, then eased. JS's `%` keeps the sign of the dividend.
pub fn ang_to(a: f64, b: f64, k: f64, dt: f64) -> f64 {
    let pi = std::f64::consts::PI;
    let d = ((b - a + pi) % TAU + TAU) % TAU - pi;
    a + d * (1.0 - (-k * dt).exp())
}

/// `x | 0` for the non-negative numbers main.js floors this way.
pub fn floor_i(x: f64) -> i64 { x.trunc() as i64 }

#[cfg(test)]
mod tests {
    use super::*;

    /// The first values of rng(11) and rng(3), from the page's own function run in Node.
    #[test]
    fn mulberry32_gives_the_pages_sequence() {
        let mut r = Rng::new(11);
        let got: Vec<f64> = (0..4).map(|_| r.next()).collect();
        assert_eq!(got, RNG11);
        let mut r = Rng::new(3);
        assert_eq!(r.next(), RNG3_FIRST);
    }

    // Filled in from Node (see the test's doc comment).
    const RNG11: [f64; 4] = include!("../tests/rng11.txt");
    const RNG3_FIRST: f64 = include!("../tests/rng3.txt");

    #[test]
    fn angles_ease_the_short_way() {
        assert!((ang_to(3.0, -3.0, 1e9, 1.0) - (-3.0 + 2.0 * std::f64::consts::PI)).abs() < 1e-9);
        assert!((ease(0.0, 1.0, 10.0, 0.1) - (1.0 - (-1f64).exp())).abs() < 1e-12);
    }
}
