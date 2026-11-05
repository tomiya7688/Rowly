use super::CalculationFailure;

// ```text
// 責務: [reduce_fraction: 割り算の余りと除数を最大公約数で約分する]
// 引数: [numerator: signed remainder, denominator: signed divisor]
// 戻り値: [(i128, i128): 約分後の分子と分母]
// ```
pub(super) fn reduce_fraction(numerator: i128, denominator: i128) -> (i128, i128) {
    let mut left = numerator.unsigned_abs();
    let mut right = denominator.unsigned_abs();
    while right != 0 {
        (left, right) = (right, left % right);
    }
    let greatest_common_divisor = left as i128;
    (
        numerator / greatest_common_divisor,
        denominator / greatest_common_divisor,
    )
}

// {
//   責務: [round_integer_ratio_as_f64: i64の比を中間丸めなしで最近接f64へ変換する]
//   処理: [符号と二進指数を求め、53桁の仮数とguard/sticky bitでties-to-even丸めを行う]
//   引数: [numerator: 分子, denominator: 0以外の分母]
//   戻り値: [Result<f64, CalculationFailure>: 最近接のfinite値または0除算error]
// }
pub(super) fn round_integer_ratio_as_f64(
    numerator: i64,
    denominator: i64,
) -> Result<f64, CalculationFailure> {
    if denominator == 0 {
        return Err(CalculationFailure::DivisionByZero);
    }
    if numerator == 0 {
        return Ok(0.0);
    }

    let numerator_magnitude = u128::from(numerator.unsigned_abs());
    let denominator_magnitude = u128::from(denominator.unsigned_abs());
    let mut exponent = (127 - numerator_magnitude.leading_zeros() as i32)
        - (127 - denominator_magnitude.leading_zeros() as i32);

    // Bit lengths give an exponent that is at most one too large.
    let ratio_is_below_exponent = if exponent >= 0 {
        numerator_magnitude < (denominator_magnitude << exponent as u32)
    } else {
        (numerator_magnitude << (-exponent) as u32) < denominator_magnitude
    };
    if ratio_is_below_exponent {
        exponent -= 1;
    }

    // Scale the exact ratio into [1, 2) before generating its binary significand.
    let (scaled_numerator, scaled_denominator) = if exponent >= 0 {
        (
            numerator_magnitude,
            denominator_magnitude << exponent as u32,
        )
    } else {
        (
            numerator_magnitude << (-exponent) as u32,
            denominator_magnitude,
        )
    };
    let mut remainder = scaled_numerator - scaled_denominator;
    let mut significand = 1_u64 << 52;

    // Generate the 52 stored fraction bits from the exact integer remainder.
    for bit in (0..52).rev() {
        remainder <<= 1;
        if remainder >= scaled_denominator {
            remainder -= scaled_denominator;
            significand |= 1_u64 << bit;
        }
    }

    // The next bit and remaining fraction decide round-to-nearest, ties-to-even.
    remainder <<= 1;
    let guard_bit = remainder >= scaled_denominator;
    if guard_bit {
        remainder -= scaled_denominator;
    }
    let sticky_bit = remainder != 0;
    if guard_bit && (sticky_bit || significand & 1 == 1) {
        significand += 1;
    }
    if significand == 1_u64 << 53 {
        significand >>= 1;
        exponent += 1;
    }

    let magnitude = (significand as f64) * 2.0_f64.powi(exponent - 52);
    Ok(if numerator.is_negative() ^ denominator.is_negative() {
        -magnitude
    } else {
        magnitude
    })
}
