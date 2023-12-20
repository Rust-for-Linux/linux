// SPDX-License-Identifier: GPL-2.0

use proc_macro::{
    Delimiter, TokenStream,
    TokenTree::{self, Group, Ident, Literal},
};
use std::ops::Range;

fn process_group(var: &str, i: isize, tokens: impl Iterator<Item = TokenTree>) -> Vec<TokenTree> {
    let mut tt = Vec::<TokenTree>::new();
    let mut tokens = tokens.peekable();

    while let Some(token) = tokens.next() {
        match token {
            Group(ref group) => {
                let group_tokens = process_group(var, i, group.stream().into_iter());
                let stream = FromIterator::from_iter(group_tokens.into_iter());
                let new_group = proc_macro::Group::new(group.delimiter(), stream);
                tt.push(TokenTree::Group(new_group));
            }
            TokenTree::Punct(ref punct) => {
                if punct.to_string() == "$" {
                    if let Some(TokenTree::Ident(ident)) = tokens.peek() {
                        if ident.to_string() == var {
                            tt.push(TokenTree::Literal(proc_macro::Literal::isize_unsuffixed(i)));
                            tokens.next();
                            continue;
                        }
                    }
                }

                tt.push(token);
            }
            _ => tt.push(token),
        }
    }

    tt
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let mut tokens = input.into_iter().peekable();

    let var = if let Some(Ident(i)) = tokens.next() {
        i.to_string()
    } else {
        panic!("foreach! first token should be an identifier");
    };

    let token = tokens.next().expect("missing token, expecting ident 'in'");
    assert!(matches!(token, TokenTree::Ident(x) if x.to_string() == "in"));

    let token = tokens
        .next()
        .expect("foreach!: missing token, expecting range");
    let token = if let Group(group) = token {
        group
            .stream()
            .into_iter()
            .next()
            .expect("foreach: missing token, expecting integer")
    } else {
        token
    };

    let start = if let Literal(lit) = token {
        lit.to_string()
            .parse::<isize>()
            .expect("Failed to convert literal to isize")
    } else {
        panic!("foreach!: unexpected token '{token}'");
    };

    let token = tokens
        .next()
        .expect("foreach!: missing token, expecting '.'");
    assert!(matches!(token, TokenTree::Punct(x) if x == '.'));
    let token = tokens
        .next()
        .expect("foreach!: missing token, expecting '.'");
    assert!(matches!(token, TokenTree::Punct(x) if x == '.'));

    let is_inclusive_range = if let Some(TokenTree::Punct(p)) = tokens.peek() {
        if p.as_char() == '=' {
            tokens.next();
            true
        } else {
            false
        }
    } else {
        false
    };

    let token = tokens
        .next()
        .expect("foreach!: missing token, expecting integer");
    let token = if let Group(group) = token {
        group
            .stream()
            .into_iter()
            .next()
            .expect("foreach: missing token, expecting integer")
    } else {
        token
    };

    let end = if let Literal(lit) = token {
        lit.to_string()
            .parse::<isize>()
            .expect("Failed to convert literal to isize")
    } else {
        panic!("foreach!: unexpected token '{token}'");
    };
    let range = Range {
        start,
        end: end + if is_inclusive_range { 1 } else { 0 },
    };

    let tokens = if let Some(Group(group)) = tokens.next() {
        if group.delimiter() != Delimiter::Brace {
            panic!("foreach! expected brace");
        }

        group.stream().into_iter()
    } else {
        panic!("foreach! missing opening brace");
    };

    let tokens: Vec<TokenTree> = tokens.collect();
    let mut tt = Vec::<TokenTree>::new();

    for i in range {
        tt.extend_from_slice(&process_group(&var, i, tokens.clone().into_iter()));
    }

    FromIterator::from_iter(tt)
}
