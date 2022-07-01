// SPDX-License-Identifier: GPL-2.0

use proc_macro::{token_stream, Group, TokenTree};

pub(crate) fn try_ident(it: &mut token_stream::IntoIter) -> Option<String> {
    if let Some(TokenTree::Ident(ident)) = it.next() {
        Some(ident.to_string())
    } else {
        None
    }
}

pub(crate) fn try_literal(it: &mut token_stream::IntoIter) -> Option<String> {
    if let Some(TokenTree::Literal(literal)) = it.next() {
        Some(literal.to_string())
    } else {
        None
    }
}

pub(crate) fn try_byte_string(it: &mut token_stream::IntoIter) -> Option<String> {
    try_literal(it).filter(|bytes| bytes.starts_with("b\"") && bytes.ends_with('\"'))
}

pub(crate) trait TryFromRadix {
    type Primitive;
    fn try_from_radix(code: &str) -> Result<Self::Primitive, std::num::ParseIntError>;
}

macro_rules! try_from_radix_impl {
    ($($t:ty)*) => {$(
        impl TryFromRadix for $t {
            type Primitive = $t;
            fn try_from_radix(marked: &str) -> Result<$t, std::num::ParseIntError> {
                let code = marked.strip_suffix(stringify!($t)).unwrap_or(&marked).replace("_", "");
                let radix = match code.get(..2) {
                     Some("0x") => 16,
                     Some("0o") => 8,
                     Some("0b") => 2,
                     _ => return code.parse::<$t>(),
                };
                <$t>::from_str_radix(code.get(2..).expect("Invalid radix"), radix)
            }
        }
    )*}
}

try_from_radix_impl! { i8 u8 i16 u16 i32 u32 i64 u64 usize isize }

pub(crate) fn expect_ident(it: &mut token_stream::IntoIter) -> String {
    try_ident(it).expect("Expected Ident")
}

pub(crate) fn expect_punct(it: &mut token_stream::IntoIter) -> char {
    if let TokenTree::Punct(punct) = it.next().expect("Reached end of token stream for Punct") {
        punct.as_char()
    } else {
        panic!("Expected Punct");
    }
}

pub(crate) fn expect_literal(it: &mut token_stream::IntoIter) -> String {
    try_literal(it).expect("Expected Literal")
}

pub(crate) fn expect_group(it: &mut token_stream::IntoIter) -> Group {
    if let TokenTree::Group(group) = it.next().expect("Reached end of token stream for Group") {
        group
    } else {
        panic!("Expected Group");
    }
}

pub(crate) fn expect_byte_string(it: &mut token_stream::IntoIter) -> String {
    try_byte_string(it).expect("Expected byte string")
}

pub(crate) fn expect_end(it: &mut token_stream::IntoIter) {
    if it.next().is_some() {
        panic!("Expected end");
    }
}

pub(crate) fn get_literal(it: &mut token_stream::IntoIter, expected_name: &str) -> String {
    assert_eq!(expect_ident(it), expected_name);
    assert_eq!(expect_punct(it), ':');
    let literal = expect_literal(it);
    assert_eq!(expect_punct(it), ',');
    literal
}

pub(crate) fn get_byte_string(it: &mut token_stream::IntoIter, expected_name: &str) -> String {
    assert_eq!(expect_ident(it), expected_name);
    assert_eq!(expect_punct(it), ':');
    let byte_string = expect_byte_string(it);
    assert_eq!(expect_punct(it), ',');
    byte_string
}
