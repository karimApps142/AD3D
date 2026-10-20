//! Summed-area table with exact fractional queries (design §16 steps 6–7).
//!
//! Pixels are piecewise constant: pixel `(i, j)` covers `[i, i+1) × [j, j+1)` (y down).
//! `S[j][i]` is the sum over rows `< j` and columns `< i`. The integral
//! `I(x, y) = ∫₀ˣ∫₀ʸ f` is bilinear inside each pixel, so bilinear interpolation of `S`
//! gives it exactly and a rectangle of any fractional size costs four lookups.
//!
//! Polygons use Green's theorem: `∬_P f dA = ∮ F dy` with `F(x, y) = ∫₀ˣ f(t, y) dt`.
//! Inside one pixel row `F` is the row prefix sum, linear between pixel columns, so
//! splitting each edge at row and column lines and applying the trapezoid rule to the
//! pieces is exact.

use glam::DVec2;

/// Summed-area table, `(w + 1) × (h + 1)` sums in f64.
#[derive(Debug, Clone, PartialEq)]
pub struct Sat {
    w: usize,
    h: usize,
    s: Vec<f64>,
}

impl Sat {
    /// Builds the table of `map(values[k])` for a `w × h` row-major image.
    pub fn build<T: Copy>(w: u32, h: u32, values: &[T], map: impl Fn(T) -> f32) -> Self {
        let (w, h) = (w as usize, h as usize);
        assert_eq!(values.len(), w * h, "Sat::build: buffer size");
        let stride = w + 1;
        let mut s = vec![0f64; stride * (h + 1)];
        for y in 0..h {
            let (prev, cur) = s[y * stride..(y + 2) * stride].split_at_mut(stride);
            let mut run = 0f64;
            for x in 0..w {
                run += map(values[y * w + x]) as f64;
                cur[x + 1] = prev[x + 1] + run;
            }
        }
        Sat { w, h, s }
    }

    pub fn width(&self) -> u32 {
        self.w as u32
    }
    pub fn height(&self) -> u32 {
        self.h as u32
    }

    #[inline]
    fn at(&self, i: usize, j: usize) -> f64 {
        self.s[j * (self.w + 1) + i]
    }

    /// Sum of all pixels.
    pub fn total(&self) -> f64 {
        self.at(self.w, self.h)
    }

    /// `∫₀ˣ∫₀ʸ f` for any real `(x, y)`, clamped to the image.
    #[inline]
    pub fn integral_to(&self, x: f64, y: f64) -> f64 {
        let x = x.clamp(0.0, self.w as f64);
        let y = y.clamp(0.0, self.h as f64);
        let i = (x as usize).min(self.w.saturating_sub(1));
        let j = (y as usize).min(self.h.saturating_sub(1));
        let (fx, fy) = (x - i as f64, y - j as f64);
        let (a, b, c, d) = (self.at(i, j), self.at(i + 1, j), self.at(i, j + 1), self.at(i + 1, j + 1));
        a + fx * (b - a) + fy * (c - a) + fx * fy * (d - b - c + a)
    }

    /// Integral of `f` over `[x0, x1] × [y0, y1]` (clamped to the image).
    #[inline]
    pub fn rect_integral(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
        self.integral_to(x1, y1) - self.integral_to(x0, y1) - self.integral_to(x1, y0) + self.integral_to(x0, y0)
    }

    /// Area mean over a rectangle (`NaN` if it has no area inside the image).
    pub fn rect_mean(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
        let cx = |x: f64| x.clamp(0.0, self.w as f64);
        let cy = |y: f64| y.clamp(0.0, self.h as f64);
        let area = (cx(x1) - cx(x0)) * (cy(y1) - cy(y0));
        self.rect_integral(x0, y0, x1, y1) / area
    }

    /// `∫ F dy` along the segment `a → b` (both inside the image).
    ///
    /// Walks the pixel cells the segment crosses (a DDA over integer row and column
    /// lines, no allocation). Inside one cell `F` is linear in `x`, so the trapezoid
    /// rule on each sub-segment, i.e. `Δy · F(x_mid)`, is exact.
    #[inline]
    fn edge_integral(&self, a: DVec2, b: DVec2) -> f64 {
        let d = b - a;
        if d.y == 0.0 {
            return 0.0;
        }
        // Next integer line in the direction of travel and its parameter t.
        let (mut kx, sx, ix) = step_init(a.x, d.x);
        let (mut ky, sy, iy) = step_init(a.y, d.y);
        let mut tx = if sx == 0.0 { f64::INFINITY } else { (kx - a.x) * ix };
        let mut ty = (ky - a.y) * iy;
        let (wl, hl) = (self.w - 1, self.h - 1);
        let stride = self.w + 1;
        let mut t0 = 0.0;
        let mut acc = 0.0;
        loop {
            let t1 = tx.min(ty).min(1.0);
            if t1 > t0 {
                let tm = 0.5 * (t0 + t1);
                let (mx, my) = (a.x + d.x * tm, a.y + d.y * tm);
                let j = (my.max(0.0) as usize).min(hl);
                let i = (mx.max(0.0) as usize).min(wl);
                let r0 = j * stride + i;
                let r1 = r0 + stride;
                // Row prefix F_j at columns i and i + 1.
                let base = self.s[r1] - self.s[r0];
                let slope = self.s[r1 + 1] - self.s[r0 + 1] - base;
                acc += (t1 - t0) * (base + (mx - i as f64) * slope);
            }
            if t1 >= 1.0 {
                break;
            }
            if tx <= t1 {
                kx += sx;
                tx = (kx - a.x) * ix;
            }
            if ty <= t1 {
                ky += sy;
                ty = (ky - a.y) * iy;
            }
            t0 = t1;
        }
        acc * d.y
    }

    /// Exact `(∬ f, signed area)` over a simple polygon in pixel coordinates, after
    /// clipping it to the image. Both carry the sign of the vertex order (positive for
    /// counter-clockwise in a y-up reading of the coordinates), so their ratio is the
    /// mean for either order.
    pub fn polygon_integral(&self, poly: &[DVec2]) -> (f64, f64) {
        if self.w == 0 || self.h == 0 {
            return (0.0, 0.0);
        }
        let (wf, hf) = (self.w as f64, self.h as f64);
        let owned;
        let clipped: &[DVec2] = if poly.iter().all(|p| p.x >= 0.0 && p.x <= wf && p.y >= 0.0 && p.y <= hf) {
            poly
        } else {
            owned = clip_to_rect(poly, 0.0, 0.0, wf, hf);
            &owned
        };
        if clipped.len() < 3 {
            return (0.0, 0.0);
        }
        let (mut integ, mut area2) = (0.0, 0.0);
        let n = clipped.len();
        for k in 0..n {
            let (a, b) = (clipped[k], clipped[(k + 1) % n]);
            integ += self.edge_integral(a, b);
            area2 += a.x * b.y - b.x * a.y;
        }
        (integ, 0.5 * area2)
    }

    /// Area mean over a polygon (`NaN` if it has no area inside the image).
    pub fn polygon_mean(&self, poly: &[DVec2]) -> f64 {
        let (i, a) = self.polygon_integral(poly);
        i / a
    }
}

/// First integer line strictly past `p` in the direction `d`, the step sign and `1/d`.
#[inline]
fn step_init(p: f64, d: f64) -> (f64, f64, f64) {
    if d > 0.0 {
        (p.floor() + 1.0, 1.0, 1.0 / d)
    } else if d < 0.0 {
        (p.ceil() - 1.0, -1.0, 1.0 / d)
    } else {
        (0.0, 0.0, 0.0)
    }
}

/// Sutherland–Hodgman clip of a polygon to an axis-aligned rectangle; keeps the vertex
/// order. Exact when nothing crosses the rectangle.
pub fn clip_to_rect(poly: &[DVec2], x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
    if poly.iter().all(|p| p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1) {
        return poly.to_vec();
    }
    let mut out = poly.to_vec();
    // (axis, bound, keep if coordinate ≥ bound when `ge`)
    for (axis, bound, ge) in [(0, x0, true), (0, x1, false), (1, y0, true), (1, y1, false)] {
        if out.is_empty() {
            break;
        }
        let input = std::mem::take(&mut out);
        let coord = |p: DVec2| if axis == 0 { p.x } else { p.y };
        let inside = |p: DVec2| if ge { coord(p) >= bound } else { coord(p) <= bound };
        let n = input.len();
        for k in 0..n {
            let (cur, prev) = (input[k], input[(k + n - 1) % n]);
            let (ci, pi) = (inside(cur), inside(prev));
            if ci != pi {
                let t = (bound - coord(prev)) / (coord(cur) - coord(prev));
                let mut q = prev + (cur - prev) * t;
                if axis == 0 { q.x = bound } else { q.y = bound }
                out.push(q);
            }
            if ci {
                out.push(cur);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> (u32, u32, Vec<f32>) {
        (4, 3, (0..12).map(|k| ((k * 7) % 5) as f32 / 4.0).collect())
    }

    fn brute_rect(w: u32, px: &[f32], x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
        let h = px.len() as u32 / w;
        let mut s = 0.0;
        for j in 0..h {
            for i in 0..w {
                let ox = (x1.min(i as f64 + 1.0) - x0.max(i as f64)).max(0.0);
                let oy = (y1.min(j as f64 + 1.0) - y0.max(j as f64)).max(0.0);
                s += ox * oy * px[(j * w + i) as usize] as f64;
            }
        }
        s
    }

    #[test]
    fn rect_matches_brute_force() {
        let (w, h, px) = img();
        let sat = Sat::build(w, h, &px, |v| v);
        for &(x0, y0, x1, y1) in &[(0.0, 0.0, 4.0, 3.0), (0.3, 0.7, 2.6, 2.2), (1.5, 0.0, 1.75, 3.0), (3.9, 2.9, 4.0, 3.0)] {
            let a = sat.rect_integral(x0, y0, x1, y1);
            let b = brute_rect(w, &px, x0, y0, x1, y1);
            assert!((a - b).abs() < 1e-12, "{a} {b}");
        }
        assert!((sat.total() - px.iter().map(|&v| v as f64).sum::<f64>()).abs() < 1e-12);
    }

    #[test]
    fn polygon_rect_matches_rect_both_orders() {
        let (w, h, px) = img();
        let sat = Sat::build(w, h, &px, |v| v);
        let r = [DVec2::new(0.3, 0.7), DVec2::new(2.6, 0.7), DVec2::new(2.6, 2.2), DVec2::new(0.3, 2.2)];
        let (i, a) = sat.polygon_integral(&r);
        assert!((i - sat.rect_integral(0.3, 0.7, 2.6, 2.2)).abs() < 1e-12);
        assert!((a - 2.3 * 1.5).abs() < 1e-12);
        let mut rr = r;
        rr.reverse();
        let (i2, a2) = sat.polygon_integral(&rr);
        assert!((i + i2).abs() < 1e-12 && (a + a2).abs() < 1e-12);
    }

    #[test]
    fn triangle_halves_of_square_sum_to_square() {
        let (w, h, px) = img();
        let sat = Sat::build(w, h, &px, |v| v);
        let (p0, p1, p2, p3) = (DVec2::new(0.2, 0.1), DVec2::new(3.7, 0.4), DVec2::new(3.3, 2.9), DVec2::new(0.4, 2.5));
        let (a, _) = sat.polygon_integral(&[p0, p1, p2]);
        let (b, _) = sat.polygon_integral(&[p0, p2, p3]);
        let (q, _) = sat.polygon_integral(&[p0, p1, p2, p3]);
        assert!((a + b - q).abs() < 1e-12);
    }

    #[test]
    fn clip_keeps_inside_part() {
        let sq = [DVec2::new(-1.0, -1.0), DVec2::new(2.0, -1.0), DVec2::new(2.0, 2.0), DVec2::new(-1.0, 2.0)];
        let c = clip_to_rect(&sq, 0.0, 0.0, 1.0, 1.0);
        let area: f64 = (0..c.len()).map(|k| c[k].perp_dot(c[(k + 1) % c.len()])).sum::<f64>() * 0.5;
        assert!((area - 1.0).abs() < 1e-12);
    }
}
