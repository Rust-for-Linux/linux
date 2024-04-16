// SPDX-License-Identifier: GPL-2.0

use proc_macro2::{Ident, TokenStream};
use quote::quote;
use std::collections::HashSet;
use syn::{parse_quote, Error, ImplItem, Item, Result, TraitItem};

macro_rules! handle_item {
    ($item_type:ident, $item:ident, $with_comment:literal) => {{
        let mut functions = Vec::new();
        let mut consts = HashSet::new();
        for item in &$item.items {
            match item {
                $item_type::Fn(fn_) => functions.push(fn_.sig.ident.clone()),
                $item_type::Const(const_) => {
                    consts.insert(const_.ident.clone());
                }
                _ => {}
            }
        }
        let item = if $with_comment {
            parse_quote! {
                /// A marker to prevent implementors from forgetting to use the [`#[vtable]`](vtable)
                /// attribute macro when implementing this trait.
                const USE_VTABLE_ATTR: () = ();
            }
        } else {
            parse_quote!(const USE_VTABLE_ATTR: () = ();)
        };
        $item.items.push(item);
        for func in functions {
            let gen_const_name = Ident::new(
                &format!("HAS_{}", func.to_string().to_uppercase()),
                func.span(),
            );
            // Skip if it's declared already -- this allows user override.
            if consts.contains(&gen_const_name) {
                continue;
            }
            // We don't know on the implementation-site whether a method is required or provided
            // so we have to generate a const for all methods.
            let comment = format!("Indicates if the `{func}` method is overridden by the implementor.");
            let item = if $with_comment {
                parse_quote!(
                    #[doc = #comment]
                    const #gen_const_name: bool = false;
                )
            } else {
                parse_quote!(const #gen_const_name: bool = false;)
            };
            $item.items.push(item);
            consts.insert(gen_const_name);
        }
        quote!(#$item)
    }}
}

pub(crate) fn vtable(input: Item) -> Result<TokenStream> {
    match input {
        Item::Impl(mut impl_) => Ok(handle_item!(ImplItem, impl_, false)),
        Item::Trait(mut trait_) => Ok(handle_item!(TraitItem, trait_, true)),
        other => Err(Error::new_spanned(
            other,
            "`#[vtable]` expects a `trait` or `impl`.",
        )),
    }
}
