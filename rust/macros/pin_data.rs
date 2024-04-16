// SPDX-License-Identifier: Apache-2.0 OR MIT

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse_quote,
    visit_mut::{visit_path_segment_mut, VisitMut},
    Error, Field, ItemStruct, PathSegment, Result, Type, TypePath, WhereClause,
};

pub(crate) fn pin_data(args: TokenStream, mut struct_: ItemStruct) -> Result<TokenStream> {
    // The generics might contain the `Self` type. Since this macro will define a new type with the
    // same generics and bounds, this poses a problem: `Self` will refer to the new type as opposed
    // to this struct definition. Therefore we have to replace `Self` with the concrete name.
    let mut replacer = {
        let name = &struct_.ident;
        let (_, ty_generics, _) = struct_.generics.split_for_impl();
        SelfReplacer(parse_quote!(#name #ty_generics))
    };
    replacer.visit_generics_mut(&mut struct_.generics);

    let the_pin_data = generate_the_pin_data(&struct_);
    let unpin_impl = unpin_impl(&struct_);
    let drop_impl = drop_impl(&struct_, args)?;

    let mut errors = TokenStream::new();
    for field in &mut struct_.fields {
        if !is_pinned(field) && is_phantom_pinned(&field.ty) {
            let message = format!("The field `{}` of type `PhantomPinned` only has an effect, if it has the `#[pin]` attribute", field.ident.as_ref().unwrap() );
            errors.extend(quote!(::core::compile_error!(#message);));
        }
        field.attrs.retain(|a| !a.path().is_ident("pin"));
    }
    Ok(quote! {
        #struct_
        #errors
        const _: () = {
            #the_pin_data
            #unpin_impl
            #drop_impl
        };
    })
}

struct SelfReplacer(PathSegment);

impl VisitMut for SelfReplacer {
    fn visit_path_segment_mut(&mut self, seg: &mut PathSegment) {
        if seg.ident == "Self" {
            *seg = self.0.clone();
        } else {
            visit_path_segment_mut(self, seg);
        }
    }
    fn visit_item_mut(&mut self, _: &mut syn::Item) {
        // Do not descend into items, since items reset/change what `Self` refers to.
    }
}

fn is_pinned(field: &Field) -> bool {
    field.attrs.iter().any(|a| a.path().is_ident("pin"))
}

fn is_phantom_pinned(ty: &Type) -> bool {
    match ty {
        Type::Path(TypePath { qself: None, path }) => {
            // Cannot possibly refer to `PhantomPinned`.
            if path.segments.len() > 3 {
                return false;
            }
            // If there is a `::`, then the path needs to be `::core::marker::PhantomPinned` or
            // `::std::marker::PhantomPinned`.
            if path.leading_colon.is_some() && path.segments.len() != 3 {
                return false;
            }
            let expected: Vec<&[&str]> = vec![&["PhantomPinned"], &["marker"], &["core", "std"]];
            for (actual, expected) in path.segments.iter().rev().zip(expected) {
                if !actual.arguments.is_empty() || expected.iter().all(|e| actual.ident != e) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn generate_the_pin_data(
    ItemStruct {
        vis,
        ident,
        generics,
        fields,
        ..
    }: &ItemStruct,
) -> TokenStream {
    let (impl_generics, ty_generics, whr) = generics.split_for_impl();

    // For every field, we create a projection function according to its projection type. If a
    // field is structurally pinned, then it must be initialized via `PinInit`, if it is not
    // structurally pinned, then it must be initialized via `Init`.
    let pinned_field_accessors = fields
        .iter()
        .filter(|f| is_pinned(f))
        .map(|Field { vis, ident, ty, .. }| {
            quote! {
                #vis unsafe fn #ident<E>(
                    self,
                    slot: *mut #ty,
                    init: impl ::kernel::init::PinInit<#ty, E>,
                ) -> ::core::result::Result<(), E> {
                    unsafe { ::kernel::init::PinInit::__pinned_init(init, slot) }
                }
            }
        })
        .collect::<TokenStream>();
    let not_pinned_field_accessors = fields
        .iter()
        .filter(|f| !is_pinned(f))
        .map(|Field { vis, ident, ty, .. }| {
            quote! {
                #vis unsafe fn #ident<E>(
                    self,
                    slot: *mut #ty,
                    init: impl ::kernel::init::Init<#ty, E>,
                ) -> ::core::result::Result<(), E> {
                    unsafe { ::kernel::init::Init::__init(init, slot) }
                }
            }
        })
        .collect::<TokenStream>();
    quote! {
        // We declare this struct which will host all of the projection function for our type. It
        // will be invariant over all generic parameters which are inherited from the struct.
        #vis struct __ThePinData #generics
            #whr
        {
            __phantom: ::core::marker::PhantomData<
                fn(#ident #ty_generics) -> #ident #ty_generics
            >,
        }

        impl #impl_generics ::core::clone::Clone for __ThePinData #ty_generics
            #whr
        {
            fn clone(&self) -> Self { *self }
        }

        impl #impl_generics ::core::marker::Copy for __ThePinData #ty_generics
            #whr
        {}

        #[allow(dead_code)] // Some functions might never be used and private.
        impl #impl_generics __ThePinData #ty_generics
            #whr
        {
            #pinned_field_accessors
            #not_pinned_field_accessors
        }

        // SAFETY: We have added the correct projection functions above to `__ThePinData` and
        // we also use the least restrictive generics possible.
        unsafe impl #impl_generics
            ::kernel::init::__internal::HasPinData for #ident #ty_generics
            #whr
        {
            type PinData = __ThePinData #ty_generics;

            unsafe fn __pin_data() -> Self::PinData {
                __ThePinData { __phantom: ::core::marker::PhantomData }
            }
        }

        unsafe impl #impl_generics
            ::kernel::init::__internal::PinData for __ThePinData #ty_generics
            #whr
        {
            type Datee = #ident #ty_generics;
        }
    }
}

fn unpin_impl(
    ItemStruct {
        ident,
        generics,
        fields,
        ..
    }: &ItemStruct,
) -> TokenStream {
    let generics_with_pinlt = {
        let mut g = generics.clone();
        g.params.insert(0, parse_quote!('__pin));
        let _ = g.make_where_clause();
        g
    };
    let (
        impl_generics_with_pinlt,
        ty_generics_with_pinlt,
        Some(WhereClause {
            where_token,
            predicates,
        }),
    ) = generics_with_pinlt.split_for_impl()
    else {
        unreachable!()
    };
    let (_, ty_generics, _) = generics.split_for_impl();
    let mut pinned_fields = fields
        .iter()
        .filter(|f| is_pinned(f))
        .cloned()
        .collect::<Vec<_>>();
    for field in &mut pinned_fields {
        field.attrs.retain(|a| !a.path().is_ident("pin"));
    }
    quote! {
        // This struct will be used for the unpin analysis. It is needed, because only structurally
        // pinned fields are relevant whether the struct should implement `Unpin`.
        #[allow(dead_code)] // The fields below are never used.
        struct __Unpin #generics_with_pinlt
        #where_token
            #predicates
        {
            __phantom_pin: ::core::marker::PhantomData<fn(&'__pin ()) -> &'__pin ()>,
            __phantom: ::core::marker::PhantomData<
                fn(#ident #ty_generics) -> #ident #ty_generics
            >,
            #(#pinned_fields),*
        }

        #[doc(hidden)]
        impl #impl_generics_with_pinlt ::core::marker::Unpin for #ident #ty_generics
        #where_token
            __Unpin #ty_generics_with_pinlt: ::core::marker::Unpin,
            #predicates
        {}
    }
}

fn drop_impl(
    ItemStruct {
        ident, generics, ..
    }: &ItemStruct,
    args: TokenStream,
) -> Result<TokenStream> {
    let (impl_generics, ty_generics, whr) = generics.split_for_impl();
    let has_pinned_drop = match syn::parse2::<Option<syn::Ident>>(args.clone()) {
        Ok(None) => false,
        Ok(Some(ident)) if ident == "PinnedDrop" => true,
        _ => {
            return Err(Error::new_spanned(
                args,
                "Expected nothing or `PinnedDrop` as arguments to `#[pin_data]`.",
            ))
        }
    };
    // We need to disallow normal `Drop` implementation, the exact behavior depends on whether
    // `PinnedDrop` was specified in `args`.
    Ok(if has_pinned_drop {
        // When `PinnedDrop` was specified we just implement `Drop` and delegate.
        quote! {
            impl #impl_generics ::core::ops::Drop for #ident #ty_generics
                #whr
            {
                fn drop(&mut self) {
                    // SAFETY: Since this is a destructor, `self` will not move after this function
                    // terminates, since it is inaccessible.
                    let pinned = unsafe { ::core::pin::Pin::new_unchecked(self) };
                    // SAFETY: Since this is a drop function, we can create this token to call the
                    // pinned destructor of this type.
                    let token = unsafe { ::kernel::init::__internal::OnlyCallFromDrop::new() };
                    ::kernel::init::PinnedDrop::drop(pinned, token);
                }
            }
        }
    } else {
        // When no `PinnedDrop` was specified, then we have to prevent implementing drop.
        quote! {
            // We prevent this by creating a trait that will be implemented for all types implementing
            // `Drop`. Additionally we will implement this trait for the struct leading to a conflict,
            // if it also implements `Drop`
            trait MustNotImplDrop {}
            #[allow(drop_bounds)]
            impl<T: ::core::ops::Drop + ?::core::marker::Sized> MustNotImplDrop for T {}
            impl #impl_generics MustNotImplDrop for #ident #ty_generics
                #whr
            {}
            // We also take care to prevent users from writing a useless `PinnedDrop` implementation.
            // They might implement `PinnedDrop` correctly for the struct, but forget to give
            // `PinnedDrop` as the parameter to `#[pin_data]`.
            #[allow(non_camel_case_types)]
            trait UselessPinnedDropImpl_you_need_to_specify_PinnedDrop {}
            impl<T: ::kernel::init::PinnedDrop + ?::core::marker::Sized>
                UselessPinnedDropImpl_you_need_to_specify_PinnedDrop for T {}
            impl #impl_generics
                UselessPinnedDropImpl_you_need_to_specify_PinnedDrop for #ident #ty_generics
                #whr
            {}
        }
    })
}
