// SPDX-License-Identifier: GPL-2.0

use proc_macro::{token_stream, Ident, TokenStream, TokenTree};

use crate::helpers::expect_punct;

fn expect_ident(it: &mut token_stream::IntoIter) -> Ident {
    if let Some(TokenTree::Ident(ident)) = it.next() {
        ident
    } else {
        panic!("Expected Ident")
    }
}

pub(crate) fn concat_idents(ts: TokenStream) -> TokenStream {
    let mut it = ts.into_iter();
    let mut out = TokenStream::new();
    let a = loop {
        let ident = expect_ident(&mut it);
        let punct = expect_punct(&mut it);
        match punct.as_char() {
            ',' => break ident,
            ':' => {
                let punct2 = expect_punct(&mut it);
                assert_eq!(punct2.as_char(), ':');
                out.extend([
                    TokenTree::Ident(ident),
                    TokenTree::Punct(punct),
                    TokenTree::Punct(punct2),
                ]);
            }
            _ => panic!("Expected , or ::"),
        }
    };

    let b = expect_ident(&mut it);
    assert!(it.next().is_none(), "only two idents can be concatenated");
    let res = Ident::new(&format!("{a}{b}"), b.span());
    out.extend([TokenTree::Ident(res)]);
    out
}
