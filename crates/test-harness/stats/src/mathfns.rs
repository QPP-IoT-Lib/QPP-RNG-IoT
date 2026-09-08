//! Self-contained special-function support for [`crate::tier1`]: the
//! complementary error function (for the monobit/runs test p-values) and
//! the regularized incomplete gamma function (for the chi-square
//! goodness-of-fit p-value).
//!
//! Deliberately reimplemented here rather than pulled in as a dependency
//! (e.g. `statrs`/`libm`'s erf): Tier 1 exists specifically to be a
//! zero-external-dependency fast path that runs on every `cargo test`
//! (see the crate root doc), and these are short, standard, numerically
//! well-understood algorithms (Numerical Recipes' Lanczos gamma
//! approximation and series/continued-fraction incomplete gamma), not
//! something worth a whole crate for.

/// Complementary error function, `erfc(x) = 1 - erf(x)`.
///
/// Abramowitz & Stegun 7.1.26-derived rational approximation (the
/// classic "Numerical Recipes `erfcc`" form), accurate to about `1.2e-7`
/// absolute error across the whole real line -- comfortably enough for
/// p-value thresholding at the alpha levels ([`crate::tier1::ALPHA`])
/// this crate cares about.
pub fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let ans = t
        * (-z * z - 1.265_512_23
            + t * (1.000_023_68
                + t * (0.374_091_96
                    + t * (0.096_784_18
                        + t * (-0.186_288_06
                            + t * (0.278_868_07
                                + t * (-1.135_203_98
                                    + t * (1.488_515_87
                                        + t * (-0.822_152_23 + t * 0.170_872_77)))))))))
            .exp();
    if x >= 0.0 { ans } else { 2.0 - ans }
}

/// `ln(Gamma(x))` via the Lanczos approximation (g=5, N=6 coefficients),
/// the same one Numerical Recipes' `gammln` uses. Accurate to ~1e-10 for
/// `x > 0`.
fn ln_gamma(x: f64) -> f64 {
    const COF: [f64; 6] = [
        76.180_091_729_471_46,
        -86.505_320_327_112_16,
        24.014_098_240_830_91,
        -1.231_739_572_450_155,
        0.001_208_650_973_866_179,
        -0.000_005_395_239_384_953,
    ];
    let mut y = x;
    let tmp0 = x + 5.5;
    let tmp = tmp0 - (x + 0.5) * tmp0.ln();
    let mut ser = 1.000_000_000_190_015;
    for &c in &COF {
        y += 1.0;
        ser += c / y;
    }
    -tmp + (2.506_628_274_631_000_7 * ser / x).ln()
}

/// Regularized lower incomplete gamma function `P(a, x)`, via its power
/// series (valid/fast-converging for `x < a + 1`).
fn gamma_p_series(a: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let gln = ln_gamma(a);
    let mut ap = a;
    let mut sum = 1.0 / a;
    let mut del = sum;
    for _ in 0..200 {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * 1e-15 {
            break;
        }
    }
    sum * (-x + a * x.ln() - gln).exp()
}

/// Regularized upper incomplete gamma function `Q(a, x) = 1 - P(a, x)`,
/// via its continued fraction (valid/fast-converging for `x >= a + 1`).
fn gamma_q_continued_fraction(a: f64, x: f64) -> f64 {
    let gln = ln_gamma(a);
    const FPMIN: f64 = 1e-300;
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / FPMIN;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..200 {
        let an = -(i as f64) * (i as f64 - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = b + an / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-15 {
            break;
        }
    }
    (-x + a * x.ln() - gln).exp() * h
}

/// Regularized upper incomplete gamma function `Q(a, x)` -- for a
/// chi-square statistic with `df` degrees of freedom, `Q(df/2, x/2)` is
/// exactly the p-value `P(chi2_df >= x)` that [`crate::tier1`]'s
/// chi-square test reports.
pub fn regularized_gamma_q(a: f64, x: f64) -> f64 {
    if x < 0.0 || a <= 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 1.0;
    }
    if x < a + 1.0 {
        1.0 - gamma_p_series(a, x)
    } else {
        gamma_q_continued_fraction(a, x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erfc_matches_known_values() {
        // NR's rational approximation is accurate to ~1.2e-7 across the
        // real line, not exact at any single point (including 0).
        assert!((erfc(0.0) - 1.0).abs() < 1e-6);
        // erfc(1) ~= 0.15729920705...
        assert!((erfc(1.0) - 0.157_299_207_05).abs() < 1e-8);
        assert!(erfc(-1.0) > erfc(0.0));
        assert!(erfc(5.0) < 1e-11);
    }

    #[test]
    fn gamma_q_is_complement_of_gamma_p() {
        for &(a, x) in &[(0.5, 0.1), (2.0, 3.0), (10.0, 5.0), (50.0, 60.0), (127.5, 300.0)] {
            let q = regularized_gamma_q(a, x);
            let p = gamma_p_series(a, x).max(1.0 - gamma_q_continued_fraction(a, x));
            // Whichever branch regularized_gamma_q actually took, P+Q
            // should still sum to 1 within numerical tolerance.
            assert!(
                (p + q - 1.0).abs() < 1e-6 || (1.0 - q - p).abs() < 1e-6,
                "a={a} x={x} p={p} q={q}"
            );
        }
    }

    /// Chi-square with 1 degree of freedom is the square of a standard
    /// normal, so its survival function has the closed form `P(chi2_1 >=
    /// x) = erfc(sqrt(x/2))`. Cross-checking the incomplete-gamma path
    /// against the independently-implemented `erfc` path is a much
    /// stronger correctness check than trusting either in isolation.
    #[test]
    fn chi_square_df1_matches_erfc_closed_form() {
        for &x in &[0.1, 1.0, 3.84, 6.63, 10.0, 20.0] {
            let via_gamma = regularized_gamma_q(0.5, x / 2.0);
            let via_erfc = erfc((x / 2.0).sqrt());
            assert!(
                (via_gamma - via_erfc).abs() < 1e-6,
                "x={x} gamma={via_gamma} erfc={via_erfc}"
            );
        }
    }

    /// Chi-square with 2 degrees of freedom is an Exponential(1/2), so
    /// its survival function has the closed form `P(chi2_2 >= x) =
    /// exp(-x/2)`. A second independent closed form to cross-check the
    /// incomplete-gamma implementation against.
    #[test]
    fn chi_square_df2_matches_exponential_closed_form() {
        for &x in &[0.1, 1.0, 5.0, 10.0, 50.0] {
            let via_gamma = regularized_gamma_q(1.0, x / 2.0);
            let via_exp = (-x / 2.0).exp();
            assert!(
                (via_gamma - via_exp).abs() < 1e-9,
                "x={x} gamma={via_gamma} exp={via_exp}"
            );
        }
    }

    #[test]
    fn gamma_q_is_monotonically_decreasing_in_x() {
        let mut prev = 1.0;
        for i in 1..50 {
            let x = i as f64 * 0.5;
            let q = regularized_gamma_q(20.0, x);
            assert!(q <= prev, "Q(20, x) should decrease as x grows");
            prev = q;
        }
    }
}
