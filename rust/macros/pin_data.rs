// SPDX-License-Identifier: GPL-2.0

use proc_macro::{Delimiter, Group, Ident, Punct, Spacing, Span, TokenStream, TokenTree};

pub(crate) fn pin_data(args: TokenStream, input: TokenStream) -> TokenStream {
    let mut impl_generics = vec![];
    let mut ty_generics = vec![];
    let mut rest = vec![];
    let mut nesting = 0;
    let mut toks = input.into_iter();
    let mut at_start = true;
    for tt in &mut toks {
        match tt.clone() {
            TokenTree::Punct(p) if p.as_char() == '<' => {
                if nesting >= 1 {
                    impl_generics.push(tt);
                }
                nesting += 1;
            }
            TokenTree::Punct(p) if p.as_char() == '>' => {
                if nesting == 0 {
                    break;
                } else {
                    nesting -= 1;
                    if nesting >= 1 {
                        impl_generics.push(tt);
                    }
                    if nesting == 0 {
                        break;
                    }
                }
            }
            tt => {
                if nesting == 1 {
                    match &tt {
                        TokenTree::Ident(i) if i.to_string() == "const" => {}
                        TokenTree::Ident(_) if at_start => {
                            ty_generics.push(tt.clone());
                            ty_generics.push(TokenTree::Punct(Punct::new(',', Spacing::Alone)));
                            at_start = false;
                        }
                        TokenTree::Punct(p) if p.as_char() == ',' => at_start = true,
                        TokenTree::Punct(p) if p.as_char() == '\'' && at_start => {
                            ty_generics.push(tt.clone());
                        }
                        _ => {}
                    }
                }
                if nesting >= 1 {
                    impl_generics.push(tt);
                } else if nesting == 0 {
                    rest.push(tt);
                }
            }
        }
    }
    rest.extend(toks);
    let last = rest.pop();
    let mut ret = vec![];
    ret.extend("::kernel::_pin_data!".parse::<TokenStream>().unwrap());
    ret.push(TokenTree::Group(Group::new(
        Delimiter::Brace,
        TokenStream::from_iter(vec![
            TokenTree::Ident(Ident::new("parse_input", Span::call_site())),
            TokenTree::Punct(Punct::new(':', Spacing::Alone)),
            TokenTree::Punct(Punct::new('@', Spacing::Alone)),
            TokenTree::Ident(Ident::new("args", Span::call_site())),
            TokenTree::Group(Group::new(
                Delimiter::Parenthesis,
                TokenStream::from_iter(args),
            )),
            TokenTree::Punct(Punct::new(',', Spacing::Alone)),
            TokenTree::Punct(Punct::new('@', Spacing::Alone)),
            TokenTree::Ident(Ident::new("sig", Span::call_site())),
            TokenTree::Group(Group::new(
                Delimiter::Parenthesis,
                TokenStream::from_iter(rest),
            )),
            TokenTree::Punct(Punct::new(',', Spacing::Alone)),
            TokenTree::Punct(Punct::new('@', Spacing::Alone)),
            TokenTree::Ident(Ident::new("impl_generics", Span::call_site())),
            TokenTree::Group(Group::new(
                Delimiter::Parenthesis,
                TokenStream::from_iter(impl_generics),
            )),
            TokenTree::Punct(Punct::new(',', Spacing::Alone)),
            TokenTree::Punct(Punct::new('@', Spacing::Alone)),
            TokenTree::Ident(Ident::new("ty_generics", Span::call_site())),
            TokenTree::Group(Group::new(
                Delimiter::Parenthesis,
                TokenStream::from_iter(ty_generics),
            )),
            TokenTree::Punct(Punct::new(',', Spacing::Alone)),
            TokenTree::Punct(Punct::new('@', Spacing::Alone)),
            TokenTree::Ident(Ident::new("body", Span::call_site())),
            TokenTree::Group(Group::new(
                Delimiter::Parenthesis,
                TokenStream::from_iter(last),
            )),
            TokenTree::Punct(Punct::new(',', Spacing::Alone)),
        ]),
    )));
    TokenStream::from_iter(ret)
}
