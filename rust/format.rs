//! Original two-decimal median-price labels (HR).
use crate::types::Price;

/// Match `parseFloat(n.toFixed(2)).toString()`, including half-up ties.
pub fn fmt(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value.abs() >= 1e21 {
        let text = format!("{value:e}");
        return text.replace("e", "e+");
    }
    // Scale the exact binary significand, not the rounded f64 product n*100.
    // This preserves JavaScript's 1.125 -> 1.13 but 2.675 -> 2.67 behavior.
    let bits = value.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let significand = (bits & ((1u64 << 52) - 1)) | if exponent == 0 { 0 } else { 1u64 << 52 };
    let shift = if exponent == 0 {
        -1074
    } else {
        exponent - 1023 - 52
    };
    let scaled = u128::from(significand) * 100;
    let cents = if shift >= 0 {
        scaled << shift
    } else if -shift >= 128 {
        0
    } else {
        let divisor = 1u128 << -shift;
        scaled / divisor + u128::from(scaled % divisor >= divisor / 2)
    };
    let decimal = format!("{}.{:02}", cents / 100, cents % 100);
    let rounded: f64 = decimal.parse().expect("rounded decimal");
    if rounded == 0.0 {
        "0".into()
    } else if value.is_sign_negative() {
        format!("-{}", rounded)
    } else {
        rounded.to_string()
    }
}
/// Boundaries validate medians; confidence remains fewer than three samples.
pub fn format_tag(prices: &[Price]) -> Result<String, String> {
    let first = prices.first().ok_or("formatTag: no prices")?;
    let low = fmt(prices.iter().fold(first.median, |n, p| n.min(p.median)));
    let high = fmt(prices.iter().fold(first.median, |n, p| n.max(p.median)));
    let mark = if prices.iter().any(|p| p.median_samples < 3) {
        "?"
    } else {
        ""
    };
    let value = if low == high {
        low
    } else {
        format!("{low} - {high}")
    };
    Ok(format!("{value} HR{mark}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(median: f64, median_samples: u64) -> Price {
        Price {
            median,
            median_samples,
        }
    }

    #[test]
    fn formats_median_groups_with_original_three_sample_confidence() {
        for (prices, expected) in [
            (vec![p(3.0, 10)], "3 HR"),
            (vec![p(0.1, 10)], "0.1 HR"),
            (vec![p(5.0, 2)], "5 HR?"),
            (vec![p(5.0, 3)], "5 HR"),
            (vec![p(120.0, 10), p(5.0, 10)], "5 - 120 HR"),
            (vec![p(2.0, 10), p(2.0, 10)], "2 HR"),
            (vec![p(2.001, 10), p(2.002, 2)], "2 HR?"),
            (vec![p(120.0, 2), p(5.0, 10)], "5 - 120 HR?"),
            (vec![p(0.0, 0)], "0 HR?"),
        ] {
            assert_eq!(format_tag(&prices).unwrap(), expected);
        }
        assert_eq!(format_tag(&[]).unwrap_err(), "formatTag: no prices");
    }

    #[test]
    fn agrees_with_node_to_fixed_output_on_differential_samples() {
        let cases = [
            (0.75, "0.75"),
            (0.1, "0.1"),
            (3.0, "3"),
            (0.05, "0.05"),
            (120.0, "120"),
            (1.125, "1.13"),
            (2.675, "2.67"),
            (1.005, "1"),
            (0.125, "0.13"),
            (1.234, "1.23"),
            (1.239, "1.24"),
            (0.005, "0.01"),
            (2.345, "2.35"),
            (9.995, "9.99"),
            (0.004999, "0"),
            (123456.789, "123456.79"),
            (1e-7, "0"),
            (5e-324, "0"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (1e21, "1e+21"),
            (999999999999999999999.0, "1e+21"),
        ];
        for (value, expected) in cases {
            assert_eq!(fmt(value), expected, "{value:e}");
        }
    }

    #[test]
    fn matches_javascript_to_fixed_rounding_including_binary_half_ties() {
        for (n, expected) in [
            (1.125, "1.13"),
            (2.675, "2.67"),
            (1.005, "1"),
            (0.125, "0.13"),
            (-1.125, "-1.13"),
            (-0.0, "0"),
            (1e21, "1e+21"),
            (1e-10, "0"),
        ] {
            assert_eq!(fmt(n), expected, "{n}");
        }
    }

    #[test]
    fn drops_trailing_zeros_and_caps_at_two_decimals() {
        for (n, expected) in [
            (0.75, "0.75"),
            (0.1, "0.1"),
            (3.0, "3"),
            (0.05, "0.05"),
            (120.0, "120"),
            (1.234, "1.23"),
            (1.239, "1.24"),
        ] {
            assert_eq!(fmt(n), expected);
        }
    }
}
