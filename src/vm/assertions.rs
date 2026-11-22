//! `ASSERT`, `ASSERTEQ` and `ASSERTNEAR`: ordinary catchable runtime errors
//! that additionally carry the assertion flag (see `Error::assertion`).

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use super::{Vm, number};
use crate::CollectionKind;
use crate::heap::ObjectRef;
use crate::value::{Value, cmp_numeric};

/// Longest rendering of a single string value in a failure message.
const MAX_STRING_CHARS: usize = 60;
/// Longest rendering of any value in a failure message.
const MAX_RENDER_CHARS: usize = 160;
/// Most elements or fields shown per collection or record.
const MAX_ELEMENTS: usize = 8;
/// Deepest nesting of collections shown in full.
const MAX_DEPTH: usize = 3;

impl Vm {
    /// Builds the failure error: the assertion flag is picked up by
    /// `run_instructions` when the fault is created.
    fn assertion_failed(&mut self, head: String, message: Option<String>) -> String {
        self.assertion_failure = true;
        match message {
            Some(message) if !message.is_empty() => format!("{head} - {message}"),
            _ => head,
        }
    }

    fn pop_assert_message(&mut self, who: &str) -> Result<String, String> {
        match self.pop()? {
            Value::String(reference) => Ok(self.heap.get(reference)?.to_string()),
            other => Err(format!(
                "{who} message must be a string, found {}",
                self.describe(other)
            )),
        }
    }

    pub(super) fn assert_op(&mut self, has_message: bool) -> Result<(), String> {
        let message = has_message
            .then(|| self.pop_assert_message("ASSERT"))
            .transpose()?;
        let condition = number(self.pop()?).map_err(|e| format!("ASSERT condition: {e}"))?;
        if condition != 0.0 {
            return Ok(());
        }
        Err(self.assertion_failed("ASSERT failed".into(), message))
    }

    pub(super) fn assert_eq_op(&mut self, has_message: bool) -> Result<(), String> {
        let message = has_message
            .then(|| self.pop_assert_message("ASSERTEQ"))
            .transpose()?;
        let actual = self.pop()?;
        let expected = self.pop()?;
        if self.deep_equal(expected, actual)? {
            return Ok(());
        }
        let head = format!(
            "ASSERTEQ failed: expected {}, got {}",
            self.render(expected),
            self.render(actual)
        );
        Err(self.assertion_failed(head, message))
    }

    pub(super) fn assert_near_op(&mut self, has_message: bool) -> Result<(), String> {
        let message = has_message
            .then(|| self.pop_assert_message("ASSERTNEAR"))
            .transpose()?;
        let epsilon_value = self.pop()?;
        let actual_value = self.pop()?;
        let expected_value = self.pop()?;
        let operand =
            |value: Value, what: &str| number(value).map_err(|e| format!("ASSERTNEAR {what}: {e}"));
        let epsilon = operand(epsilon_value, "epsilon")?;
        let actual = operand(actual_value, "actual value")?;
        let expected = operand(expected_value, "expected value")?;
        if !epsilon.is_finite() || epsilon < 0.0 {
            return Err("ASSERTNEAR epsilon must be a finite number >= 0".into());
        }
        let difference = match (expected_value, actual_value) {
            // Exact for integers beyond 2^53.
            (Value::Int(a), Value::Int(b)) => (i128::from(a) - i128::from(b)).abs() as f64,
            _ => (expected - actual).abs(),
        };
        // `<=` is false for NaN, so an undefined difference never passes.
        if difference <= epsilon {
            return Ok(());
        }
        let head = format!(
            "ASSERTNEAR failed: expected {}, got {} (difference {difference}, allowed {epsilon})",
            self.render(expected_value),
            self.render(actual_value),
        );
        Err(self.assertion_failed(head, message))
    }

    /// Deep equality as defined for `ASSERTEQ`; never fails for a mismatch
    /// of kinds and terminates on cyclic structures.
    fn deep_equal(&self, expected: Value, actual: Value) -> Result<bool, String> {
        let mut pending = vec![(expected, actual)];
        // Pairs already compared or being compared: assuming equality for a
        // pair seen again is what makes cycles terminate.
        let mut seen: HashSet<(ObjectRef, ObjectRef)> = HashSet::new();
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (Value::Nothing, Value::Nothing) => {}
                (Value::Int(_) | Value::Number(_), Value::Int(_) | Value::Number(_)) => {
                    if cmp_numeric(left, right) != Some(Ordering::Equal) {
                        return Ok(false);
                    }
                }
                (Value::String(a), Value::String(b)) => {
                    if a != b && self.heap.get(a)? != self.heap.get(b)? {
                        return Ok(false);
                    }
                }
                (Value::Collection(a), Value::Collection(b)) => {
                    if a == b || !seen.insert((a, b)) {
                        continue;
                    }
                    let (left_kind, left_values) = self.heap.collection(a)?;
                    let (right_kind, right_values) = self.heap.collection(b)?;
                    if left_kind != right_kind || left_values.len() != right_values.len() {
                        return Ok(false);
                    }
                    if left_kind == CollectionKind::Map {
                        let mut entries: HashMap<&str, Value> = HashMap::new();
                        for pair in right_values.as_chunks::<2>().0 {
                            let Value::String(key) = pair[0] else {
                                return Err("invalid map key payload".into());
                            };
                            entries.insert(self.heap.get(key)?, pair[1]);
                        }
                        for pair in left_values.as_chunks::<2>().0 {
                            let Value::String(key) = pair[0] else {
                                return Err("invalid map key payload".into());
                            };
                            match entries.get(self.heap.get(key)?) {
                                Some(other) => pending.push((pair[1], *other)),
                                None => return Ok(false),
                            }
                        }
                    } else {
                        pending.extend(left_values.into_iter().zip(right_values));
                    }
                }
                (Value::Record(a), Value::Record(b)) => {
                    if a == b {
                        continue;
                    }
                    if self.heap.record_type(a)? != self.heap.record_type(b)? {
                        return Ok(false);
                    }
                    // Records are mutable objects shared by reference, so
                    // cycles through fields are possible.
                    if !seen.insert((a, b)) {
                        continue;
                    }
                    let left_fields = self.heap.record_fields(a)?;
                    let right_fields = self.heap.record_fields(b)?;
                    pending.extend(left_fields.into_iter().zip(right_fields));
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    /// A compact, bounded rendering of a value for failure messages.
    fn render(&self, value: Value) -> String {
        let mut out = String::new();
        self.render_into(value, 0, &mut out);
        if out.chars().count() > MAX_RENDER_CHARS {
            out = out.chars().take(MAX_RENDER_CHARS).collect();
            out.push_str("...");
        }
        out
    }

    fn render_into(&self, value: Value, depth: usize, out: &mut String) {
        match value {
            Value::Number(n) => out.push_str(&n.to_string()),
            Value::Int(n) => out.push_str(&n.to_string()),
            Value::Nothing => out.push_str("NOTHING"),
            Value::String(reference) => match self.heap.get(reference) {
                Ok(text) => quote(text, out),
                Err(_) => out.push_str("<invalid string>"),
            },
            Value::Collection(reference) => {
                let Ok((kind, values)) = self.heap.collection(reference) else {
                    out.push_str("<invalid collection>");
                    return;
                };
                let name = match kind {
                    CollectionKind::Array => "ARRAY",
                    CollectionKind::List => "LIST",
                    CollectionKind::Map => "MAP",
                };
                if depth >= MAX_DEPTH {
                    out.push_str(name);
                    out.push_str("(...)");
                    return;
                }
                out.push_str(name);
                out.push('(');
                let step = if kind == CollectionKind::Map { 2 } else { 1 };
                let total = values.len() / step;
                for (position, items) in values.chunks(step).enumerate() {
                    if position == MAX_ELEMENTS || out.len() > MAX_RENDER_CHARS {
                        out.push_str(&format!(", ...{} more", total - position));
                        break;
                    }
                    if position > 0 {
                        out.push_str(", ");
                    }
                    if let [key, item] = items {
                        self.render_into(*key, depth + 1, out);
                        out.push_str(": ");
                        self.render_into(*item, depth + 1, out);
                    } else if let [item] = items {
                        self.render_into(*item, depth + 1, out);
                    }
                }
                out.push(')');
            }
            Value::Record(reference) => {
                let (Ok(id), Ok(values)) = (
                    self.heap.record_type(reference),
                    self.heap.record_fields(reference),
                ) else {
                    out.push_str("<invalid record>");
                    return;
                };
                let record = &self.program.types[id];
                out.push_str(&record.name);
                if depth >= MAX_DEPTH {
                    out.push_str("(...)");
                    return;
                }
                out.push('(');
                for (position, (field, item)) in record.fields.iter().zip(values).enumerate() {
                    if position == MAX_ELEMENTS || out.len() > MAX_RENDER_CHARS {
                        out.push_str(&format!(", ...{} more", record.fields.len() - position));
                        break;
                    }
                    if position > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&field.name);
                    out.push_str(" = ");
                    self.render_into(item, depth + 1, out);
                }
                out.push(')');
            }
        }
    }
}

/// Appends `text` in double quotes (an embedded quote is doubled, control
/// characters are escaped), cut to `MAX_STRING_CHARS` characters.
fn quote(text: &str, out: &mut String) {
    out.push('"');
    for (position, ch) in text.chars().enumerate() {
        if position == MAX_STRING_CHARS {
            out.push_str("...");
            break;
        }
        if ch == '"' {
            out.push_str("\"\"");
        } else {
            crate::testing::escape_char_into(ch, out);
        }
    }
    out.push('"');
}
