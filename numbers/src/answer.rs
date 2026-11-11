//! What a number can be typed as, and reading one.
//!
//! The host chooses per round: whole numbers, decimals, or expressions
//! such as `1,001/3` or `2^10 - 1`. Whatever is typed, the phone shows
//! how it was read before it is sent, which matters most for commas:
//! `1,001` is a thousand and one, while `1,5` is one and a half.

use serde::{Deserialize, Serialize};

/// The longest answer a phone can send.
pub const MAX_TEXT: usize = 80;

/// How deep brackets and signs can nest.
const MAX_DEPTH: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Answers {
    Whole,
    Decimal,
    Expression,
}

impl Answers {
    /// Reads what a player typed, as this kind of answer.
    pub fn read(self, text: &str) -> Result<f64, &'static str> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Type a number first.");
        }
        if text.chars().count() > MAX_TEXT {
            return Err("That is longer than an answer can be.");
        }
        let value = match self {
            Answers::Whole | Answers::Decimal => {
                let mut reader = Reader::new(text);
                let value = reader.signed_literal()?;
                if !reader.done() {
                    return Err("This round takes a plain number, not a sum.");
                }
                value
            }
            Answers::Expression => evaluate(text)?,
        };
        if !value.is_finite() {
            return Err("That does not come to a number.");
        }
        if self == Answers::Whole && value.fract() != 0.0 {
            return Err("This round takes whole numbers.");
        }
        Ok(value)
    }
}

/// Works out an expression: numbers, `+ - * / ^`, brackets, `pi` and `e`,
/// and `sqrt`, `ln`, `log` and `abs`.
pub fn evaluate(text: &str) -> Result<f64, &'static str> {
    let mut reader = Reader::new(text);
    let value = reader.sum(0)?;
    if !reader.done() {
        return Err(match reader.peek() {
            Some(')') => "There is a closing bracket too many.",
            _ => "Part of that is not something this can work out.",
        });
    }
    Ok(value)
}

/// Reads one number as written, with a comma or a dot in it. Commas
/// between groups of three digits are thousands, as in `1,001`; any other
/// comma is a decimal point, as in `1,5`. With both, the last one is the
/// decimal point: `1,234.5` and `1.234,5` are the same number.
fn literal(written: &str) -> Result<f64, &'static str> {
    const BAD: &str = "That number is written in a way this cannot read.";
    let comma = written.rfind(',');
    let dot = written.rfind('.');
    let (thousands, point) = match (comma, dot) {
        (Some(c), Some(d)) => {
            if c > d {
                ('.', Some(','))
            } else {
                (',', Some('.'))
            }
        }
        (Some(_), None) => {
            if grouped(written, ',') {
                (',', None)
            } else {
                ('_', Some(','))
            }
        }
        (None, Some(_)) => {
            if written.matches('.').count() > 1 && grouped(written, '.') {
                ('.', None)
            } else {
                ('_', Some('.'))
            }
        }
        (None, None) => ('_', None),
    };
    let (whole, fraction) = match point {
        Some(p) => {
            let at = written.rfind(p).expect("found above");
            (&written[..at], Some(&written[at + 1..]))
        }
        None => (written, None),
    };
    if thousands != '_' && whole.contains(thousands) && !grouped(whole, thousands) {
        return Err(BAD);
    }
    let digits: String = whole.chars().filter(|c| *c != thousands).collect();
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(BAD);
    }
    if let Some(fraction) = fraction {
        if !fraction.chars().all(|c| c.is_ascii_digit())
            || (digits.is_empty() && fraction.is_empty())
        {
            return Err(BAD);
        }
    } else if digits.is_empty() {
        return Err(BAD);
    }
    let plain = format!(
        "{}.{}",
        if digits.is_empty() { "0" } else { &digits },
        fraction.unwrap_or("0")
    );
    plain.parse().map_err(|_| BAD)
}

/// Whether `text` is digit groups split by `by`, the first of one to three
/// digits not starting with a nought, and every other of exactly three.
fn grouped(text: &str, by: char) -> bool {
    let mut groups = text.split(by);
    let first = groups.next().unwrap_or("");
    let first_ok = (1..=3).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_digit())
        && !first.starts_with('0');
    let mut rest = groups.peekable();
    first_ok
        && rest.peek().is_some()
        && rest.all(|g| g.len() == 3 && g.chars().all(|c| c.is_ascii_digit()))
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Reader { text, at: 0 }
    }

    fn skip_space(&mut self) {
        while let Some(c) = self.text[self.at..].chars().next() {
            if c.is_whitespace() {
                self.at += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.text[self.at..].chars().next()
    }

    fn bump(&mut self) {
        if let Some(c) = self.text[self.at..].chars().next() {
            self.at += c.len_utf8();
        }
    }

    fn done(&mut self) -> bool {
        self.peek().is_none()
    }

    fn signed_literal(&mut self) -> Result<f64, &'static str> {
        let mut sign = 1.0;
        while let Some(c @ ('-' | '+' | '−')) = self.peek() {
            if c != '+' {
                sign = -sign;
            }
            self.bump();
        }
        match self.peek() {
            Some(c) if c.is_ascii_digit() || c == '.' || c == ',' => Ok(sign * self.number()?),
            _ => Err("Type a number."),
        }
    }

    fn number(&mut self) -> Result<f64, &'static str> {
        let start = self.at;
        while let Some(c) = self.text[self.at..].chars().next() {
            if c.is_ascii_digit() || c == '.' || c == ',' {
                self.at += 1;
            } else {
                break;
            }
        }
        let mut value = literal(&self.text[start..self.at])?;
        // 1e6, as long as the e is followed by digits; otherwise it is
        // the constant, as in 2e for two times e.
        let rest = &self.text[self.at..];
        if let Some(after) = rest.strip_prefix(['e', 'E']) {
            let signed = after.strip_prefix(['-', '+']).unwrap_or(after);
            let digits = signed.chars().take_while(char::is_ascii_digit).count();
            if digits > 0 {
                let used = 1 + (after.len() - signed.len()) + digits;
                let power: i32 = rest[1..used]
                    .parse()
                    .map_err(|_| "That exponent is too big.")?;
                value *= 10f64.powi(power);
                self.at += used;
            }
        }
        Ok(value)
    }

    fn sum(&mut self, depth: usize) -> Result<f64, &'static str> {
        let mut value = self.product(depth)?;
        loop {
            match self.peek() {
                Some('+') => {
                    self.bump();
                    value += self.product(depth)?;
                }
                Some('-' | '−') => {
                    self.bump();
                    value -= self.product(depth)?;
                }
                _ => return Ok(value),
            }
        }
    }

    fn product(&mut self, depth: usize) -> Result<f64, &'static str> {
        let mut value = self.signed(depth)?;
        loop {
            match self.peek() {
                Some('*' | '×' | '·') => {
                    self.bump();
                    value *= self.signed(depth)?;
                }
                Some('/' | '÷' | ':') => {
                    self.bump();
                    let by = self.signed(depth)?;
                    if by == 0.0 {
                        return Err("That divides by nought.");
                    }
                    value /= by;
                }
                // 2pi, 3(4 + 1), 2sqrt(2): a number next to a bracket or
                // a name multiplies it.
                Some(c) if c == '(' || c == 'π' || c == '√' || c.is_ascii_alphabetic() => {
                    value *= self.power(depth)?;
                }
                _ => return Ok(value),
            }
        }
    }

    fn signed(&mut self, depth: usize) -> Result<f64, &'static str> {
        if depth > MAX_DEPTH {
            return Err("That nests deeper than this will work out.");
        }
        match self.peek() {
            Some('-' | '−') => {
                self.bump();
                Ok(-self.signed(depth + 1)?)
            }
            Some('+') => {
                self.bump();
                self.signed(depth + 1)
            }
            _ => self.power(depth),
        }
    }

    /// Powers bind tighter than a sign in front, so -2^2 is -4, and go
    /// from the right, so 2^3^2 is 2^9.
    fn power(&mut self, depth: usize) -> Result<f64, &'static str> {
        let base = self.atom(depth)?;
        if self.peek() == Some('^') {
            self.bump();
            let exponent = self.signed(depth + 1)?;
            return Ok(base.powf(exponent));
        }
        Ok(base)
    }

    fn atom(&mut self, depth: usize) -> Result<f64, &'static str> {
        if depth > MAX_DEPTH {
            return Err("That nests deeper than this will work out.");
        }
        match self.peek() {
            Some('(') => {
                self.bump();
                let value = self.sum(depth + 1)?;
                if self.peek() != Some(')') {
                    return Err("A bracket is not closed.");
                }
                self.bump();
                Ok(value)
            }
            Some(c) if c.is_ascii_digit() || c == '.' || c == ',' => self.number(),
            Some('π') => {
                self.bump();
                Ok(std::f64::consts::PI)
            }
            Some('√') => {
                self.bump();
                root(self.power(depth + 1)?)
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let start = self.at;
                while self.text[self.at..].starts_with(|c: char| c.is_ascii_alphabetic()) {
                    self.at += 1;
                }
                let name = self.text[start..self.at].to_ascii_lowercase();
                match name.as_str() {
                    "pi" => Ok(std::f64::consts::PI),
                    "e" => Ok(std::f64::consts::E),
                    "sqrt" | "ln" | "log" | "abs" => {
                        let x = self.power(depth + 1)?;
                        match name.as_str() {
                            "sqrt" => root(x),
                            "abs" => Ok(x.abs()),
                            _ if x <= 0.0 => Err("A logarithm needs a number above nought."),
                            "ln" => Ok(x.ln()),
                            _ => Ok(x.log10()),
                        }
                    }
                    _ => Err("That has a word in it this does not know."),
                }
            }
            Some(')') => Err("There is a closing bracket with nothing open."),
            None => Err("That ends too soon."),
            Some(_) => Err("That has a sign in it this does not know."),
        }
    }
}

fn root(x: f64) -> Result<f64, &'static str> {
    if x < 0.0 {
        Err("A square root needs a number of at least nought.")
    } else {
        Ok(x.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(text: &str, want: f64) {
        let got = evaluate(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert!((got - want).abs() < 1e-9, "{text} = {got}, not {want}");
    }

    #[test]
    fn commas_between_threes_are_thousands_and_others_are_decimal_points() {
        close("1,001", 1001.0);
        close("1,001/3", 1001.0 / 3.0);
        close("12,345,678", 12_345_678.0);
        close("1,5", 1.5);
        close("0,001", 0.001);
        close("2,71", 2.71);
        close("1,234.5", 1234.5);
        close("1.234,5", 1234.5);
        close("1.000.000", 1_000_000.0);
        close("1.5", 1.5);
        close(".5", 0.5);
        assert!(evaluate("1,23,456").is_err());
        assert!(evaluate("1..2").is_err());
    }

    #[test]
    fn sums_follow_the_usual_order() {
        close("1 + 2 * 3", 7.0);
        close("(1 + 2) * 3", 9.0);
        close("2^10 - 1", 1023.0);
        close("-2^2", -4.0);
        close("2^3^2", 512.0);
        close("2^-1", 0.5);
        close("10 / 4", 2.5);
        close("7 ÷ 2 × 3", 10.5);
        close("--3", 3.0);
        close("1e3", 1000.0);
        close("2.5e-2", 0.025);
    }

    #[test]
    fn names_and_implied_multiplying() {
        close("pi", std::f64::consts::PI);
        close("2pi", 2.0 * std::f64::consts::PI);
        close("2e", 2.0 * std::f64::consts::E);
        close("sqrt(16)", 4.0);
        close("√2 * √2", 2.0);
        close("3(4 + 1)", 15.0);
        close("log(1000)", 3.0);
        close("ln(e)", 1.0);
        close("abs(-3)", 3.0);
    }

    #[test]
    fn nonsense_is_refused_with_a_reason() {
        for text in [
            "", "1 +", "(1", "1)", "1 / 0", "sqrt(-1)", "foo", "1 # 2", "ln(0)",
        ] {
            assert!(Answers::Expression.read(text).is_err(), "{text}");
        }
        assert!(
            Answers::Expression.read(&"1+".repeat(50)).is_err(),
            "too long"
        );
        assert!(
            evaluate(&format!("{}1{}", "(".repeat(40), ")".repeat(40))).is_err(),
            "too deep"
        );
        assert!(Answers::Expression.read("10^400").is_err(), "too big");
    }

    #[test]
    fn plain_rounds_take_one_number() {
        assert_eq!(Answers::Decimal.read(" 1,5 "), Ok(1.5));
        assert_eq!(Answers::Decimal.read("-2.25"), Ok(-2.25));
        assert_eq!(Answers::Whole.read("1,001"), Ok(1001.0));
        assert_eq!(Answers::Whole.read("-7"), Ok(-7.0));
        assert!(Answers::Whole.read("1,5").is_err());
        assert!(Answers::Decimal.read("1/3").is_err());
        assert!(Answers::Decimal.read("pi").is_err());
    }
}
