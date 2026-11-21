use std::cmp::Ordering;

use crate::heap::ObjectRef;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Value {
    /// A finite double-precision number (the type of every legacy number).
    Number(f64),
    /// An exact 64-bit signed integer.
    Int(i64),
    String(ObjectRef),
    Collection(ObjectRef),
    /// A record instance (a managed object holding its field values).
    Record(ObjectRef),
    /// The null reference of record-typed variables and fields.
    Nothing,
}

impl Value {
    pub(crate) fn reference(self) -> Option<ObjectRef> {
        match self {
            Self::Number(_) | Self::Int(_) | Self::Nothing => None,
            Self::String(reference) | Self::Collection(reference) | Self::Record(reference) => {
                Some(reference)
            }
        }
    }
}

/// 2^63 as a double: the first value above the `i64` range.
const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;

/// Exactly compares an integer with a finite double, without converting the
/// integer to a (lossy) double.
pub(crate) fn cmp_int_float(integer: i64, float: f64) -> Ordering {
    if float >= TWO_POW_63 {
        return Ordering::Less;
    }
    if float < -TWO_POW_63 {
        return Ordering::Greater;
    }
    let whole = float.trunc();
    // `whole` lies within the i64 range here, so the cast is exact.
    match integer.cmp(&(whole as i64)) {
        Ordering::Equal => 0.0f64
            .partial_cmp(&(float - whole))
            .unwrap_or(Ordering::Equal),
        other => other,
    }
}

/// Exact numeric ordering of two numeric values; `None` for non-numbers.
pub(crate) fn cmp_numeric(left: Value, right: Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(a.cmp(&b)),
        (Value::Number(a), Value::Number(b)) => a.partial_cmp(&b),
        (Value::Int(a), Value::Number(b)) => Some(cmp_int_float(a, b)),
        (Value::Number(a), Value::Int(b)) => Some(cmp_int_float(b, a).reverse()),
        _ => None,
    }
}

/// The integer a double holds when it is whole and within the `i64` range.
pub(crate) fn float_to_int(float: f64) -> Option<i64> {
    (float.fract() == 0.0 && (-TWO_POW_63..TWO_POW_63).contains(&float)).then_some(float as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_compare_exactly_with_doubles() {
        let big = 9_007_199_254_740_993i64; // 2^53 + 1
        assert_eq!(
            cmp_int_float(big, 9_007_199_254_740_992.0),
            Ordering::Greater
        );
        assert_eq!(
            cmp_int_float(big - 1, 9_007_199_254_740_992.0),
            Ordering::Equal
        );
        assert_eq!(cmp_int_float(i64::MAX, TWO_POW_63), Ordering::Less);
        assert_eq!(cmp_int_float(i64::MIN, -TWO_POW_63), Ordering::Equal);
        assert_eq!(cmp_int_float(3, 3.5), Ordering::Less);
        assert_eq!(cmp_int_float(-3, -3.5), Ordering::Greater);
        assert_eq!(cmp_int_float(0, -0.25), Ordering::Greater);
        assert_eq!(float_to_int(-TWO_POW_63), Some(i64::MIN));
        assert_eq!(float_to_int(TWO_POW_63), None);
        assert_eq!(float_to_int(1.5), None);
    }
}
