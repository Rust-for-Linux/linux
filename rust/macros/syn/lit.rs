// SPDX-License-Identifier: MIT or Apache-2.0

// Adapted from https://github.com/dtolnay/syn/blob/1.0.72/src/lit.rs
// Changes made compared to upstream:
// * Removed code that depends on types and macros defined elsewhere in syn.
// * Removed dependency on `BigInt` and `unicode-xid`.
// * Removed `From<Literal>` and `new` from `LitInt` and `LitFloat` which are
//   unlikely to be used by us.

use proc_macro::{Literal, Span};

/// A Rust literal such as a string or integer or boolean.
///
/// # Syntax tree enum
///
/// This type is a [syntax tree enum].
///
/// [syntax tree enum]: crate::Expr#syntax-tree-enums
pub enum Lit {
    /// A UTF-8 string literal: `"foo"`.
    Str(LitStr),

    /// A byte string literal: `b"foo"`.
    ByteStr(LitByteStr),

    /// A byte literal: `b'f'`.
    Byte(LitByte),

    /// A character literal: `'a'`.
    Char(LitChar),

    /// An integer literal: `1` or `1u16`.
    Int(LitInt),

    /// A floating point literal: `1f64` or `1.0e10f64`.
    ///
    /// Must be finite. May not be infinite or NaN.
    Float(LitFloat),

    /// A boolean literal: `true` or `false`.
    Bool(LitBool),

    /// A raw token literal not interpreted by Syn.
    Verbatim(Literal),
}

/// A UTF-8 string literal: `"foo"`.
pub struct LitStr {
    repr: Box<LitRepr>,
}

/// A byte string literal: `b"foo"`.
pub struct LitByteStr {
    repr: Box<LitRepr>,
}

/// A byte literal: `b'f'`.
pub struct LitByte {
    repr: Box<LitRepr>,
}

/// A character literal: `'a'`.
pub struct LitChar {
    repr: Box<LitRepr>,
}

struct LitRepr {
    token: Literal,
    suffix: Box<str>,
}

/// An integer literal: `1` or `1u16`.
pub struct LitInt {
    repr: Box<LitIntRepr>,
}

struct LitIntRepr {
    token: Literal,
    digits: Box<str>,
    suffix: Box<str>,
}

/// A floating point literal: `1f64` or `1.0e10f64`.
///
/// Must be finite. May not be infinite or NaN.
pub struct LitFloat {
    repr: Box<LitFloatRepr>,
}

struct LitFloatRepr {
    token: Literal,
    digits: Box<str>,
    suffix: Box<str>,
}

/// A boolean literal: `true` or `false`.
pub struct LitBool {
    pub value: bool,
    pub span: Span,
}

impl LitStr {
    pub fn new(value: &str, span: Span) -> Self {
        let mut token = Literal::string(value);
        token.set_span(span);
        LitStr {
            repr: Box::new(LitRepr {
                token,
                suffix: Box::<str>::default(),
            }),
        }
    }

    pub fn value(&self) -> String {
        let repr = self.repr.token.to_string();
        let (value, _suffix) = value::parse_lit_str(&repr);
        String::from(value)
    }

    pub fn span(&self) -> Span {
        self.repr.token.span()
    }

    pub fn set_span(&mut self, span: Span) {
        self.repr.token.set_span(span)
    }

    pub fn suffix(&self) -> &str {
        &self.repr.suffix
    }
}

impl LitByteStr {
    pub fn new(value: &[u8], span: Span) -> Self {
        let mut token = Literal::byte_string(value);
        token.set_span(span);
        LitByteStr {
            repr: Box::new(LitRepr {
                token,
                suffix: Box::<str>::default(),
            }),
        }
    }

    pub fn value(&self) -> Vec<u8> {
        let repr = self.repr.token.to_string();
        let (value, _suffix) = value::parse_lit_byte_str(&repr);
        value
    }

    pub fn span(&self) -> Span {
        self.repr.token.span()
    }

    pub fn set_span(&mut self, span: Span) {
        self.repr.token.set_span(span)
    }

    pub fn suffix(&self) -> &str {
        &self.repr.suffix
    }
}

impl LitByte {
    pub fn new(value: u8, span: Span) -> Self {
        let mut token = Literal::u8_suffixed(value);
        token.set_span(span);
        LitByte {
            repr: Box::new(LitRepr {
                token,
                suffix: Box::<str>::default(),
            }),
        }
    }

    pub fn value(&self) -> u8 {
        let repr = self.repr.token.to_string();
        let (value, _suffix) = value::parse_lit_byte(&repr);
        value
    }

    pub fn span(&self) -> Span {
        self.repr.token.span()
    }

    pub fn set_span(&mut self, span: Span) {
        self.repr.token.set_span(span)
    }

    pub fn suffix(&self) -> &str {
        &self.repr.suffix
    }
}

impl LitChar {
    pub fn new(value: char, span: Span) -> Self {
        let mut token = Literal::character(value);
        token.set_span(span);
        LitChar {
            repr: Box::new(LitRepr {
                token,
                suffix: Box::<str>::default(),
            }),
        }
    }

    pub fn value(&self) -> char {
        let repr = self.repr.token.to_string();
        let (value, _suffix) = value::parse_lit_char(&repr);
        value
    }

    pub fn span(&self) -> Span {
        self.repr.token.span()
    }

    pub fn set_span(&mut self, span: Span) {
        self.repr.token.set_span(span)
    }

    pub fn suffix(&self) -> &str {
        &self.repr.suffix
    }
}

impl LitInt {
    pub fn base10_digits(&self) -> &str {
        &self.repr.digits
    }

    pub fn suffix(&self) -> &str {
        &self.repr.suffix
    }

    pub fn span(&self) -> Span {
        self.repr.token.span()
    }

    pub fn set_span(&mut self, span: Span) {
        self.repr.token.set_span(span)
    }
}

impl LitFloat {
    pub fn suffix(&self) -> &str {
        &self.repr.suffix
    }

    pub fn span(&self) -> Span {
        self.repr.token.span()
    }

    pub fn set_span(&mut self, span: Span) {
        self.repr.token.set_span(span)
    }
}

impl LitBool {
    pub fn new(value: bool, span: Span) -> Self {
        LitBool { value, span }
    }

    pub fn value(&self) -> bool {
        self.value
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn set_span(&mut self, span: Span) {
        self.span = span;
    }
}

mod value {
    use super::*;
    use std::char;
    use std::ops::{Index, RangeFrom};

    impl Lit {
        /// Interpret a Syn literal from a proc-macro2 literal.
        pub fn new(token: Literal) -> Self {
            let repr = token.to_string();

            match byte(&repr, 0) {
                b'"' | b'r' => {
                    let (_, suffix) = parse_lit_str(&repr);
                    return Lit::Str(LitStr {
                        repr: Box::new(LitRepr { token, suffix }),
                    });
                }
                b'b' => match byte(&repr, 1) {
                    b'"' | b'r' => {
                        let (_, suffix) = parse_lit_byte_str(&repr);
                        return Lit::ByteStr(LitByteStr {
                            repr: Box::new(LitRepr { token, suffix }),
                        });
                    }
                    b'\'' => {
                        let (_, suffix) = parse_lit_byte(&repr);
                        return Lit::Byte(LitByte {
                            repr: Box::new(LitRepr { token, suffix }),
                        });
                    }
                    _ => {}
                },
                b'\'' => {
                    let (_, suffix) = parse_lit_char(&repr);
                    return Lit::Char(LitChar {
                        repr: Box::new(LitRepr { token, suffix }),
                    });
                }
                b'0'..=b'9' | b'-' => {
                    if let Some((digits, suffix)) = parse_lit_int(&repr) {
                        return Lit::Int(LitInt {
                            repr: Box::new(LitIntRepr {
                                token,
                                digits,
                                suffix,
                            }),
                        });
                    }
                    if let Some((digits, suffix)) = parse_lit_float(&repr) {
                        return Lit::Float(LitFloat {
                            repr: Box::new(LitFloatRepr {
                                token,
                                digits,
                                suffix,
                            }),
                        });
                    }
                }
                b't' | b'f' => {
                    if repr == "true" || repr == "false" {
                        return Lit::Bool(LitBool {
                            value: repr == "true",
                            span: token.span(),
                        });
                    }
                }
                _ => {}
            }

            panic!("Unrecognized literal: `{}`", repr);
        }

        pub fn suffix(&self) -> &str {
            match self {
                Lit::Str(lit) => lit.suffix(),
                Lit::ByteStr(lit) => lit.suffix(),
                Lit::Byte(lit) => lit.suffix(),
                Lit::Char(lit) => lit.suffix(),
                Lit::Int(lit) => lit.suffix(),
                Lit::Float(lit) => lit.suffix(),
                Lit::Bool(_) | Lit::Verbatim(_) => "",
            }
        }

        pub fn span(&self) -> Span {
            match self {
                Lit::Str(lit) => lit.span(),
                Lit::ByteStr(lit) => lit.span(),
                Lit::Byte(lit) => lit.span(),
                Lit::Char(lit) => lit.span(),
                Lit::Int(lit) => lit.span(),
                Lit::Float(lit) => lit.span(),
                Lit::Bool(lit) => lit.span,
                Lit::Verbatim(lit) => lit.span(),
            }
        }

        pub fn set_span(&mut self, span: Span) {
            match self {
                Lit::Str(lit) => lit.set_span(span),
                Lit::ByteStr(lit) => lit.set_span(span),
                Lit::Byte(lit) => lit.set_span(span),
                Lit::Char(lit) => lit.set_span(span),
                Lit::Int(lit) => lit.set_span(span),
                Lit::Float(lit) => lit.set_span(span),
                Lit::Bool(lit) => lit.span = span,
                Lit::Verbatim(lit) => lit.set_span(span),
            }
        }
    }

    /// Get the byte at offset idx, or a default of `b'\0'` if we're looking
    /// past the end of the input buffer.
    pub fn byte<S: AsRef<[u8]> + ?Sized>(s: &S, idx: usize) -> u8 {
        let s = s.as_ref();
        if idx < s.len() {
            s[idx]
        } else {
            0
        }
    }

    fn next_chr(s: &str) -> char {
        s.chars().next().unwrap_or('\0')
    }

    // Returns (content, suffix).
    pub fn parse_lit_str(s: &str) -> (Box<str>, Box<str>) {
        match byte(s, 0) {
            b'"' => parse_lit_str_cooked(s),
            b'r' => parse_lit_str_raw(s),
            _ => unreachable!(),
        }
    }

    // Clippy false positive
    // https://github.com/rust-lang-nursery/rust-clippy/issues/2329
    #[allow(clippy::needless_continue)]
    fn parse_lit_str_cooked(mut s: &str) -> (Box<str>, Box<str>) {
        assert_eq!(byte(s, 0), b'"');
        s = &s[1..];

        let mut content = String::new();
        'outer: loop {
            let ch = match byte(s, 0) {
                b'"' => break,
                b'\\' => {
                    let b = byte(s, 1);
                    s = &s[2..];
                    match b {
                        b'x' => {
                            let (byte, rest) = backslash_x(s);
                            s = rest;
                            assert!(byte <= 0x80, "Invalid \\x byte in string literal");
                            char::from_u32(u32::from(byte)).unwrap()
                        }
                        b'u' => {
                            let (chr, rest) = backslash_u(s);
                            s = rest;
                            chr
                        }
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'\\' => '\\',
                        b'0' => '\0',
                        b'\'' => '\'',
                        b'"' => '"',
                        b'\r' | b'\n' => loop {
                            let ch = next_chr(s);
                            if ch.is_whitespace() {
                                s = &s[ch.len_utf8()..];
                            } else {
                                continue 'outer;
                            }
                        },
                        b => panic!("unexpected byte {:?} after \\ character in byte literal", b),
                    }
                }
                b'\r' => {
                    assert_eq!(byte(s, 1), b'\n', "Bare CR not allowed in string");
                    s = &s[2..];
                    '\n'
                }
                _ => {
                    let ch = next_chr(s);
                    s = &s[ch.len_utf8()..];
                    ch
                }
            };
            content.push(ch);
        }

        assert!(s.starts_with('"'));
        let content = content.into_boxed_str();
        let suffix = s[1..].to_owned().into_boxed_str();
        (content, suffix)
    }

    fn parse_lit_str_raw(mut s: &str) -> (Box<str>, Box<str>) {
        assert_eq!(byte(s, 0), b'r');
        s = &s[1..];

        let mut pounds = 0;
        while byte(s, pounds) == b'#' {
            pounds += 1;
        }
        assert_eq!(byte(s, pounds), b'"');
        let close = s.rfind('"').unwrap();
        for end in s[close + 1..close + 1 + pounds].bytes() {
            assert_eq!(end, b'#');
        }

        let content = s[pounds + 1..close].to_owned().into_boxed_str();
        let suffix = s[close + 1 + pounds..].to_owned().into_boxed_str();
        (content, suffix)
    }

    // Returns (content, suffix).
    pub fn parse_lit_byte_str(s: &str) -> (Vec<u8>, Box<str>) {
        assert_eq!(byte(s, 0), b'b');
        match byte(s, 1) {
            b'"' => parse_lit_byte_str_cooked(s),
            b'r' => parse_lit_byte_str_raw(s),
            _ => unreachable!(),
        }
    }

    // Clippy false positive
    // https://github.com/rust-lang-nursery/rust-clippy/issues/2329
    #[allow(clippy::needless_continue)]
    fn parse_lit_byte_str_cooked(mut s: &str) -> (Vec<u8>, Box<str>) {
        assert_eq!(byte(s, 0), b'b');
        assert_eq!(byte(s, 1), b'"');
        s = &s[2..];

        // We're going to want to have slices which don't respect codepoint boundaries.
        let mut v = s.as_bytes();

        let mut out = Vec::new();
        'outer: loop {
            let byte = match byte(v, 0) {
                b'"' => break,
                b'\\' => {
                    let b = byte(v, 1);
                    v = &v[2..];
                    match b {
                        b'x' => {
                            let (b, rest) = backslash_x(v);
                            v = rest;
                            b
                        }
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'\\' => b'\\',
                        b'0' => b'\0',
                        b'\'' => b'\'',
                        b'"' => b'"',
                        b'\r' | b'\n' => loop {
                            let byte = byte(v, 0);
                            let ch = char::from_u32(u32::from(byte)).unwrap();
                            if ch.is_whitespace() {
                                v = &v[1..];
                            } else {
                                continue 'outer;
                            }
                        },
                        b => panic!("unexpected byte {:?} after \\ character in byte literal", b),
                    }
                }
                b'\r' => {
                    assert_eq!(byte(v, 1), b'\n', "Bare CR not allowed in string");
                    v = &v[2..];
                    b'\n'
                }
                b => {
                    v = &v[1..];
                    b
                }
            };
            out.push(byte);
        }

        assert_eq!(byte(v, 0), b'"');
        let suffix = s[s.len() - v.len() + 1..].to_owned().into_boxed_str();
        (out, suffix)
    }

    fn parse_lit_byte_str_raw(s: &str) -> (Vec<u8>, Box<str>) {
        assert_eq!(byte(s, 0), b'b');
        let (value, suffix) = parse_lit_str_raw(&s[1..]);
        (String::from(value).into_bytes(), suffix)
    }

    // Returns (value, suffix).
    pub fn parse_lit_byte(s: &str) -> (u8, Box<str>) {
        assert_eq!(byte(s, 0), b'b');
        assert_eq!(byte(s, 1), b'\'');

        // We're going to want to have slices which don't respect codepoint boundaries.
        let mut v = s[2..].as_bytes();

        let b = match byte(v, 0) {
            b'\\' => {
                let b = byte(v, 1);
                v = &v[2..];
                match b {
                    b'x' => {
                        let (b, rest) = backslash_x(v);
                        v = rest;
                        b
                    }
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'\\' => b'\\',
                    b'0' => b'\0',
                    b'\'' => b'\'',
                    b'"' => b'"',
                    b => panic!("unexpected byte {:?} after \\ character in byte literal", b),
                }
            }
            b => {
                v = &v[1..];
                b
            }
        };

        assert_eq!(byte(v, 0), b'\'');
        let suffix = s[s.len() - v.len() + 1..].to_owned().into_boxed_str();
        (b, suffix)
    }

    // Returns (value, suffix).
    pub fn parse_lit_char(mut s: &str) -> (char, Box<str>) {
        assert_eq!(byte(s, 0), b'\'');
        s = &s[1..];

        let ch = match byte(s, 0) {
            b'\\' => {
                let b = byte(s, 1);
                s = &s[2..];
                match b {
                    b'x' => {
                        let (byte, rest) = backslash_x(s);
                        s = rest;
                        assert!(byte <= 0x80, "Invalid \\x byte in string literal");
                        char::from_u32(u32::from(byte)).unwrap()
                    }
                    b'u' => {
                        let (chr, rest) = backslash_u(s);
                        s = rest;
                        chr
                    }
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'\\' => '\\',
                    b'0' => '\0',
                    b'\'' => '\'',
                    b'"' => '"',
                    b => panic!("unexpected byte {:?} after \\ character in byte literal", b),
                }
            }
            _ => {
                let ch = next_chr(s);
                s = &s[ch.len_utf8()..];
                ch
            }
        };
        assert_eq!(byte(s, 0), b'\'');
        let suffix = s[1..].to_owned().into_boxed_str();
        (ch, suffix)
    }

    fn backslash_x<S>(s: &S) -> (u8, &S)
    where
        S: Index<RangeFrom<usize>, Output = S> + AsRef<[u8]> + ?Sized,
    {
        let mut ch = 0;
        let b0 = byte(s, 0);
        let b1 = byte(s, 1);
        ch += 0x10
            * match b0 {
                b'0'..=b'9' => b0 - b'0',
                b'a'..=b'f' => 10 + (b0 - b'a'),
                b'A'..=b'F' => 10 + (b0 - b'A'),
                _ => panic!("unexpected non-hex character after \\x"),
            };
        ch += match b1 {
            b'0'..=b'9' => b1 - b'0',
            b'a'..=b'f' => 10 + (b1 - b'a'),
            b'A'..=b'F' => 10 + (b1 - b'A'),
            _ => panic!("unexpected non-hex character after \\x"),
        };
        (ch, &s[2..])
    }

    fn backslash_u(mut s: &str) -> (char, &str) {
        if byte(s, 0) != b'{' {
            panic!("{}", "expected { after \\u");
        }
        s = &s[1..];

        let mut ch = 0;
        let mut digits = 0;
        loop {
            let b = byte(s, 0);
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => 10 + b - b'a',
                b'A'..=b'F' => 10 + b - b'A',
                b'_' if digits > 0 => {
                    s = &s[1..];
                    continue;
                }
                b'}' if digits == 0 => panic!("invalid empty unicode escape"),
                b'}' => break,
                _ => panic!("unexpected non-hex character after \\u"),
            };
            if digits == 6 {
                panic!("overlong unicode escape (must have at most 6 hex digits)");
            }
            ch *= 0x10;
            ch += u32::from(digit);
            digits += 1;
            s = &s[1..];
        }
        assert!(byte(s, 0) == b'}');
        s = &s[1..];

        if let Some(ch) = char::from_u32(ch) {
            (ch, s)
        } else {
            panic!("character code {:x} is not a valid unicode character", ch);
        }
    }

    // Returns base 10 digits and suffix.
    pub fn parse_lit_int(mut s: &str) -> Option<(Box<str>, Box<str>)> {
        let negative = byte(s, 0) == b'-';
        if negative {
            s = &s[1..];
        }

        let base = match (byte(s, 0), byte(s, 1)) {
            (b'0', b'x') => {
                s = &s[2..];
                16
            }
            (b'0', b'o') => {
                s = &s[2..];
                8
            }
            (b'0', b'b') => {
                s = &s[2..];
                2
            }
            (b'0'..=b'9', _) => 10,
            _ => return None,
        };

        let mut value = 0u128;
        'outer: loop {
            let b = byte(s, 0);
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' if base > 10 => b - b'a' + 10,
                b'A'..=b'F' if base > 10 => b - b'A' + 10,
                b'_' => {
                    s = &s[1..];
                    continue;
                }
                // If looking at a floating point literal, we don't want to
                // consider it an integer.
                b'.' if base == 10 => return None,
                b'e' | b'E' if base == 10 => {
                    let mut has_exp = false;
                    for (i, b) in s[1..].bytes().enumerate() {
                        match b {
                            b'_' => {}
                            b'-' | b'+' => return None,
                            b'0'..=b'9' => has_exp = true,
                            _ => {
                                let _suffix = &s[1 + i..];
                                if has_exp {
                                    return None;
                                } else {
                                    break 'outer;
                                }
                            }
                        }
                    }
                    if has_exp {
                        return None;
                    } else {
                        break;
                    }
                }
                _ => break,
            };

            if digit >= base {
                return None;
            }

            value *= base as u128;
            value += digit as u128;
            s = &s[1..];
        }

        let suffix = s;
        let mut repr = value.to_string();
        if negative {
            repr.insert(0, '-');
        }
        Some((repr.into_boxed_str(), suffix.to_owned().into_boxed_str()))
    }

    // Returns base 10 digits and suffix.
    pub fn parse_lit_float(input: &str) -> Option<(Box<str>, Box<str>)> {
        // Rust's floating point literals are very similar to the ones parsed by
        // the standard library, except that rust's literals can contain
        // ignorable underscores. Let's remove those underscores.

        let mut bytes = input.to_owned().into_bytes();

        let start = (*bytes.get(0)? == b'-') as usize;
        match bytes.get(start)? {
            b'0'..=b'9' => {}
            _ => return None,
        }

        let mut read = start;
        let mut write = start;
        let mut has_dot = false;
        let mut has_e = false;
        let mut has_sign = false;
        let mut has_exponent = false;
        while read < bytes.len() {
            match bytes[read] {
                b'_' => {
                    // Don't increase write
                    read += 1;
                    continue;
                }
                b'0'..=b'9' => {
                    if has_e {
                        has_exponent = true;
                    }
                    bytes[write] = bytes[read];
                }
                b'.' => {
                    if has_e || has_dot {
                        return None;
                    }
                    has_dot = true;
                    bytes[write] = b'.';
                }
                b'e' | b'E' => {
                    match bytes[read + 1..]
                        .iter()
                        .find(|b| **b != b'_')
                        .unwrap_or(&b'\0')
                    {
                        b'-' | b'+' | b'0'..=b'9' => {}
                        _ => break,
                    }
                    if has_e {
                        if has_exponent {
                            break;
                        } else {
                            return None;
                        }
                    }
                    has_e = true;
                    bytes[write] = b'e';
                }
                b'-' | b'+' => {
                    if has_sign || has_exponent || !has_e {
                        return None;
                    }
                    has_sign = true;
                    if bytes[read] == b'-' {
                        bytes[write] = bytes[read];
                    } else {
                        // Omit '+'
                        read += 1;
                        continue;
                    }
                }
                _ => break,
            }
            read += 1;
            write += 1;
        }

        if has_e && !has_exponent {
            return None;
        }

        let mut digits = String::from_utf8(bytes).unwrap();
        let suffix = digits.split_off(read);
        digits.truncate(write);
        Some((digits.into_boxed_str(), suffix.into_boxed_str()))
    }
}
