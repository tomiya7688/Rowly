use super::{CalculationFailure, CalculationValue, NumericValue, finite_decimal, numeric_value};

// {
//   責務: [RoundingMode: Round/Floor/Ceilの丸め方向を表す]
// }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RoundingMode {
    /// 0から遠い方向へ四捨五入する。
    HalfAwayFromZero,
    /// 負の無限大方向へ切り捨てる。
    Floor,
    /// 正の無限大方向へ切り上げる。
    Ceil,
}

// {
//   責務: [round_to_integer: numeric scalarを指定方向でInteger値へ丸める]
//   処理: [Integerはそのまま返し、Decimalは丸めた結果がi64へ正確に収まる場合だけ返す]
//   引数: [value: 丸める値, mode: 丸め方向]
//   戻り値: [Result<CalculationValue, CalculationFailure>: Integer結果または型・overflow error]
// }
pub(super) fn round_to_integer(
    value: CalculationValue,
    mode: RoundingMode,
) -> Result<CalculationValue, CalculationFailure> {
    match numeric_value(value)? {
        NumericValue::Integer(value) => Ok(CalculationValue::Integer(value)),
        NumericValue::Decimal(value) => {
            let rounded = match mode {
                RoundingMode::HalfAwayFromZero => value.round(),
                RoundingMode::Floor => value.floor(),
                RoundingMode::Ceil => value.ceil(),
            };
            decimal_to_integer(rounded).map(CalculationValue::Integer)
        }
    }
}

// {
//   責務: [decimal_to_integer: 整数値のf64をi64範囲内でだけInteger化する]
//   引数: [value: 丸め済みのfinite f64]
//   戻り値: [Result<i64, CalculationFailure>: 範囲内のinteger、範囲外はArithmeticOverflow]
// }
fn decimal_to_integer(value: f64) -> Result<i64, CalculationFailure> {
    // i64::MINは-2^63でf64へ正確に表せる。上限2^63はi64外なので未満だけ許可する。
    const LOWER: f64 = -9_223_372_036_854_775_808.0;
    const UPPER: f64 = 9_223_372_036_854_775_808.0;
    if value.is_finite() && (LOWER..UPPER).contains(&value) {
        Ok(value as i64)
    } else {
        Err(CalculationFailure::ArithmeticOverflow)
    }
}

// {
//   責務: [modulo: 除数と同じ符号を持つ剰余を返す（Excel互換）]
//   処理: [Integer同士はchecked演算、それ以外はf64の剰余を除数の符号へ補正する]
//   引数: [dividend: 被除数, divisor: 除数]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 剰余、0除算・型・overflow error]
// }
pub(super) fn modulo(
    dividend: CalculationValue,
    divisor: CalculationValue,
) -> Result<CalculationValue, CalculationFailure> {
    match (numeric_value(dividend)?, numeric_value(divisor)?) {
        (NumericValue::Integer(dividend), NumericValue::Integer(divisor)) => {
            if divisor == 0 {
                return Err(CalculationFailure::DivisionByZero);
            }
            let remainder = dividend
                .checked_rem(divisor)
                .ok_or(CalculationFailure::ArithmeticOverflow)?;
            if remainder != 0 && (remainder < 0) != (divisor < 0) {
                return remainder
                    .checked_add(divisor)
                    .map(CalculationValue::Integer)
                    .ok_or(CalculationFailure::ArithmeticOverflow);
            }
            Ok(CalculationValue::Integer(remainder))
        }
        (dividend, divisor) => {
            let (dividend, divisor) = (dividend.checked_f64()?, divisor.checked_f64()?);
            if divisor == 0.0 {
                return Err(CalculationFailure::DivisionByZero);
            }
            let remainder = dividend % divisor;
            if remainder != 0.0 && (remainder < 0.0) != (divisor < 0.0) {
                return finite_decimal(remainder + divisor);
            }
            finite_decimal(remainder)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(value: i64) -> CalculationValue {
        CalculationValue::Integer(value)
    }

    fn dec(value: f64) -> CalculationValue {
        CalculationValue::Decimal(value)
    }

    fn text(value: &str) -> CalculationValue {
        CalculationValue::Text(value.to_owned())
    }

    // {
    //   責務: [rounding_modes_follow_the_documented_direction: 3種類の丸め方向と負数の扱いを検証する]
    //   戻り値: [(): 丸め結果が仕様どおりなら成功する]
    // }
    #[test]
    fn rounding_modes_follow_the_documented_direction() {
        use RoundingMode::{Ceil, Floor, HalfAwayFromZero};
        assert_eq!(round_to_integer(dec(2.5), HalfAwayFromZero), Ok(int(3)));
        assert_eq!(round_to_integer(dec(-2.5), HalfAwayFromZero), Ok(int(-3)));
        assert_eq!(round_to_integer(dec(-2.1), Floor), Ok(int(-3)));
        assert_eq!(round_to_integer(dec(2.1), Ceil), Ok(int(3)));
        assert_eq!(round_to_integer(dec(-2.1), Ceil), Ok(int(-2)));
        assert_eq!(round_to_integer(int(7), Floor), Ok(int(7)));
        assert_eq!(round_to_integer(text("2.5"), Ceil), Ok(int(3)));
    }

    // {
    //   責務: [rounding_rejects_text_and_out_of_range_results: 数値でない入力とi64範囲外を明示errorにする]
    //   戻り値: [(): errorがsilentに丸められなければ成功する]
    // }
    #[test]
    fn rounding_rejects_text_and_out_of_range_results() {
        assert_eq!(
            round_to_integer(text("abc"), RoundingMode::Floor),
            Err(CalculationFailure::ExpectedNumber("abc".to_owned()))
        );
        assert_eq!(
            round_to_integer(dec(1.0e19), RoundingMode::Floor),
            Err(CalculationFailure::ArithmeticOverflow)
        );
        assert_eq!(
            round_to_integer(dec(-9_223_372_036_854_775_808.0), RoundingMode::Ceil),
            Ok(int(i64::MIN))
        );
        assert_eq!(
            round_to_integer(dec(9_223_372_036_854_775_808.0), RoundingMode::Ceil),
            Err(CalculationFailure::ArithmeticOverflow)
        );
    }

    // {
    //   責務: [modulo_takes_the_sign_of_the_divisor: 剰余が除数の符号になることと境界errorを検証する]
    //   戻り値: [(): Excel互換の剰余と0除算・overflow errorが得られれば成功する]
    // }
    #[test]
    fn modulo_takes_the_sign_of_the_divisor() {
        assert_eq!(modulo(int(7), int(3)), Ok(int(1)));
        assert_eq!(modulo(int(-7), int(3)), Ok(int(2)));
        assert_eq!(modulo(int(7), int(-3)), Ok(int(-2)));
        assert_eq!(modulo(int(-6), int(3)), Ok(int(0)));
        assert_eq!(modulo(dec(5.5), int(2)), Ok(dec(1.5)));
        assert_eq!(modulo(dec(-5.5), int(2)), Ok(dec(0.5)));
        assert_eq!(
            modulo(int(1), int(0)),
            Err(CalculationFailure::DivisionByZero)
        );
        assert_eq!(
            modulo(dec(1.0), dec(0.0)),
            Err(CalculationFailure::DivisionByZero)
        );
        assert_eq!(
            modulo(int(i64::MIN), int(-1)),
            Err(CalculationFailure::ArithmeticOverflow)
        );
    }
}
