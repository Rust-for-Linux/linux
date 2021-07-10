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
    try_literal(it).and_then(|byte_string| {
        if byte_string.starts_with("b\"") && byte_string.ends_with('\"') {
            Some(byte_string[2..byte_string.len() - 1].to_string())
        } else {
            None
        }
    })
}

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

#[derive(Clone, PartialEq)]
pub(crate) enum ParamType {
    Ident(String),
    Array { vals: String, max_length: usize },
}

pub(crate) fn expect_array_fields(it: &mut token_stream::IntoIter) -> ParamType {
    assert_eq!(expect_punct(it), '<');
    let vals = expect_ident(it);
    assert_eq!(expect_punct(it), ',');
    let max_length_str = expect_literal(it);
    let max_length = max_length_str
        .parse::<usize>()
        .expect("Expected usize length");
    assert_eq!(expect_punct(it), '>');
    ParamType::Array { vals, max_length }
}

pub(crate) fn expect_type(it: &mut token_stream::IntoIter) -> ParamType {
    if let TokenTree::Ident(ident) = it
        .next()
        .expect("Reached end of token stream for param type")
    {
        match ident.to_string().as_ref() {
            "ArrayParam" => expect_array_fields(it),
            _ => ParamType::Ident(ident.to_string()),
        }
    } else {
        panic!("Expected Param Type")
    }
}

pub(crate) fn expect_end(it: &mut token_stream::IntoIter) {
    if it.next().is_some() {
        panic!("Expected end");
    }
}

pub(crate) fn get_ident(it: &mut token_stream::IntoIter, expected_name: &str) -> String {
    assert_eq!(expect_ident(it), expected_name);
    assert_eq!(expect_punct(it), ':');
    let ident = expect_ident(it);
    assert_eq!(expect_punct(it), ',');
    ident
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
