use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    Number(f64),
    /// Exact integer literal (`123&` or `&H7F`).
    Int(i64),
    /// The literal `9223372036854775808&`, valid only directly after `-`.
    MinInt,
    String(String),
    Word(String),
    Plus,
    Minus,
    Star,
    Slash,
    Backslash,
    Caret,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Semicolon,
    Hash,
    /// Field access (`record.field`); a `.` before a digit is part of a number.
    Dot,
    /// `:=`, the named-argument marker of a call.
    ColonEqual,
}

pub(crate) fn lex(source: &str, line: usize) -> Result<Vec<Token>, Error> {
    let mut chars = source.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.next() {
        let token = match ch {
            ch if ch.is_whitespace() => continue,
            '\'' => break,
            '"' => {
                let mut value = String::new();
                loop {
                    match chars.next() {
                        Some('"') if chars.peek() == Some(&'"') => {
                            chars.next();
                            value.push('"');
                        }
                        Some('"') => break,
                        Some(ch) => value.push(ch),
                        None => return Err(Error::new(line, "unterminated string")),
                    }
                }
                Token::String(value)
            }
            '.' if chars
                .peek()
                .is_some_and(|next| next.is_ascii_alphabetic() || *next == '_') =>
            {
                Token::Dot
            }
            ch if ch.is_ascii_digit() || ch == '.' => {
                let mut text = String::from(ch);
                while chars
                    .peek()
                    .is_some_and(|ch| ch.is_ascii_digit() || *ch == '.')
                {
                    text.push(chars.next().unwrap());
                }
                if chars.peek().is_some_and(|ch| *ch == 'e' || *ch == 'E') {
                    text.push(chars.next().unwrap());
                    if chars.peek().is_some_and(|ch| *ch == '+' || *ch == '-') {
                        text.push(chars.next().unwrap());
                    }
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        text.push(chars.next().unwrap());
                    }
                }
                if chars.peek() == Some(&'&') {
                    chars.next();
                    if !text.chars().all(|ch| ch.is_ascii_digit()) {
                        return Err(Error::new(
                            line,
                            format!(
                                "integer literal {text:?}& must not have a fraction or exponent"
                            ),
                        ));
                    }
                    match text.parse::<u64>() {
                        Ok(value) if value <= i64::MAX as u64 => Token::Int(value as i64),
                        Ok(value) if value == i64::MAX as u64 + 1 => Token::MinInt,
                        _ => {
                            return Err(Error::new(
                                line,
                                format!("integer literal {text}& is out of the 64-bit range"),
                            ));
                        }
                    }
                } else {
                    let value = text
                        .parse::<f64>()
                        .map_err(|_| Error::new(line, format!("invalid number {text:?}")))?;
                    if !value.is_finite() {
                        return Err(Error::new(line, "number must be finite"));
                    }
                    Token::Number(value)
                }
            }
            '&' => {
                if !matches!(chars.next(), Some('H' | 'h')) {
                    return Err(Error::new(line, "unexpected character '&'"));
                }
                let mut digits = String::new();
                while chars.peek().is_some_and(char::is_ascii_hexdigit) {
                    digits.push(chars.next().unwrap());
                }
                if digits.is_empty() || digits.len() > 16 {
                    return Err(Error::new(
                        line,
                        "hexadecimal literal needs 1 to 16 digits after &H",
                    ));
                }
                // Two's complement: &HFFFFFFFFFFFFFFFF is -1.
                Token::Int(u64::from_str_radix(&digits, 16).unwrap() as i64)
            }
            ch if ch.is_ascii_alphabetic() || ch == '_' => {
                let mut word = String::from(ch);
                while chars
                    .peek()
                    .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                {
                    word.push(chars.next().unwrap());
                }
                if chars.peek() == Some(&'$') {
                    word.push(chars.next().unwrap());
                }
                let word = word.to_ascii_uppercase();
                if word == "REM" {
                    break;
                }
                Token::Word(word)
            }
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '\\' => Token::Backslash,
            '^' => Token::Caret,
            '=' => Token::Equal,
            '(' => Token::LeftParen,
            ')' => Token::RightParen,
            '[' => Token::LeftBracket,
            ']' => Token::RightBracket,
            ',' => Token::Comma,
            ';' => Token::Semicolon,
            '#' => Token::Hash,
            ':' if chars.peek() == Some(&'=') => {
                chars.next();
                Token::ColonEqual
            }
            '<' => match chars.peek() {
                Some('=') => {
                    chars.next();
                    Token::LessEqual
                }
                Some('>') => {
                    chars.next();
                    Token::NotEqual
                }
                _ => Token::Less,
            },
            '>' => {
                if chars.peek() == Some(&'=') {
                    chars.next();
                    Token::GreaterEqual
                } else {
                    Token::Greater
                }
            }
            _ => return Err(Error::new(line, format!("unexpected character {ch:?}"))),
        };
        tokens.push(token);
    }
    Ok(tokens)
}
