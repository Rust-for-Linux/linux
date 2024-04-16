// SPDX-License-Identifier: Apache-2.0 OR MIT

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse_quote, Data, DataStruct, DeriveInput, Error, GenericParam, Result, TypeParam,
    WherePredicate,
};

pub(crate) fn derive(
    DeriveInput {
        ident,
        mut generics,
        data,
        ..
    }: DeriveInput,
    raw_input: TokenStream,
) -> Result<TokenStream> {
    let Data::Struct(DataStruct { fields, .. }) = data else {
        return Err(Error::new_spanned(
            raw_input,
            "`Zeroable` can only be derived for structs.",
        ));
    };
    let field_ty = fields.iter().map(|f| &f.ty);
    let zeroable_bounds = generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(TypeParam { ident, .. }) => {
                Some(parse_quote!(#ident: ::kernel::init::Zeroable))
            }
            _ => None,
        })
        .collect::<Vec<WherePredicate>>();
    generics
        .make_where_clause()
        .predicates
        .extend(zeroable_bounds);
    let (impl_generics, ty_generics, whr) = generics.split_for_impl();
    Ok(quote! {
        // SAFETY: Every field type implements `Zeroable` and padding bytes may be zero.
        #[automatically_derived]
        unsafe impl #impl_generics ::kernel::init::Zeroable for #ident #ty_generics
            #whr
        {}
        const _: () = {
            fn assert_zeroable<T: ?::core::marker::Sized + ::kernel::init::Zeroable>() {}
            fn ensure_zeroable #impl_generics ()
                #whr
            {
                #(assert_zeroable::<#field_ty>();)*
            }
        };
    })
}
