// SPDX-License-Identifier: GPL-2.0

use proc_macro::{Delimiter, Group, Ident, Punct, Spacing, Span, TokenStream, TokenTree};

pub(crate) fn pinned_drop(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut toks = input.into_iter().collect::<Vec<_>>();
    assert!(!toks.is_empty());
    // ensure that we have an impl item
    assert!(matches!(&toks[0], TokenTree::Ident(i) if i.to_string() == "impl"));
    // ensure that we are implementing `PinnedDrop`
    let mut nesting: usize = 0;
    let mut pinned_drop_idx = None;
    for (i, tt) in toks.iter().enumerate() {
        match tt {
            TokenTree::Punct(p) if p.as_char() == '<' => {
                nesting += 1;
            }
            TokenTree::Punct(p) if p.as_char() == '>' => {
                nesting = nesting.checked_sub(1).unwrap();
                continue;
            }
            _ => {}
        }
        if i >= 1 && nesting == 0 {
            assert!(
                matches!(tt, TokenTree::Ident(i) if i.to_string() == "PinnedDrop"),
                "expected 'PinnedDrop', found: '{:?}'",
                tt
            );
            pinned_drop_idx = Some(i);
            break;
        }
    }
    let idx = pinned_drop_idx.unwrap();
    // fully qualify the `PinnedDrop`.
    toks.splice(idx..idx, "::kernel::init::".parse::<TokenStream>().unwrap());
    // take the {} body.
    if let Some(TokenTree::Group(last)) = toks.pop() {
        TokenStream::from_iter(vec![
            TokenTree::Punct(Punct::new(':', Spacing::Joint)),
            TokenTree::Punct(Punct::new(':', Spacing::Alone)),
            TokenTree::Ident(Ident::new("kernel", Span::call_site())),
            TokenTree::Punct(Punct::new(':', Spacing::Joint)),
            TokenTree::Punct(Punct::new(':', Spacing::Alone)),
            TokenTree::Ident(Ident::new("pinned_drop", Span::call_site())),
            TokenTree::Punct(Punct::new('!', Spacing::Alone)),
            TokenTree::Group(Group::new(
                Delimiter::Brace,
                TokenStream::from_iter(vec![
                    TokenTree::Punct(Punct::new('@', Spacing::Alone)),
                    TokenTree::Ident(Ident::new("impl_sig", Span::call_site())),
                    TokenTree::Group(Group::new(
                        Delimiter::Parenthesis,
                        TokenStream::from_iter(toks),
                    )),
                    TokenTree::Punct(Punct::new(',', Spacing::Alone)),
                    TokenTree::Punct(Punct::new('@', Spacing::Alone)),
                    TokenTree::Ident(Ident::new("impl_body", Span::call_site())),
                    TokenTree::Group(Group::new(
                        Delimiter::Parenthesis,
                        TokenStream::from_iter(last.stream()),
                    )),
                    TokenTree::Punct(Punct::new(',', Spacing::Alone)),
                ]),
            )),
        ])
    } else {
        TokenStream::from_iter(toks)
    }
}
