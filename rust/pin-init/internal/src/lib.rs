// SPDX-License-Identifier: Apache-2.0 OR MIT

// When fixdep scans this, it will find this string `CONFIG_RUSTC_VERSION_TEXT`
// and thus add a dependency on `include/config/RUSTC_VERSION_TEXT`, which is
// touched by Kconfig when the version string from the compiler changes.

//! `pin-init` proc macros.

#![cfg_attr(not(RUSTC_LINT_REASONS_IS_STABLE), feature(lint_reasons))]
// Documentation is done in the pin-init crate instead.
#![allow(missing_docs)]

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod init;
mod pin_data;
mod pinned_drop;
mod zeroable;

#[proc_macro_attribute]
pub fn pin_data(args: TokenStream, input: TokenStream) -> TokenStream {
    ok_or_compile_error(pin_data::pin_data(
        parse_macro_input!(args),
        parse_macro_input!(input),
    ))
}

#[proc_macro_attribute]
pub fn pinned_drop(args: TokenStream, input: TokenStream) -> TokenStream {
    pinned_drop::pinned_drop(parse_macro_input!(args), parse_macro_input!(input)).into()
}

#[proc_macro_derive(Zeroable)]
pub fn derive_zeroable(input: TokenStream) -> TokenStream {
    ok_or_compile_error(zeroable::derive(parse_macro_input!(input)))
}

#[proc_macro_derive(MaybeZeroable)]
pub fn maybe_derive_zeroable(input: TokenStream) -> TokenStream {
    ok_or_compile_error(zeroable::maybe_derive(parse_macro_input!(input)))
}

#[proc_macro]
pub fn init(input: TokenStream) -> TokenStream {
    init::expand(
        parse_macro_input!(input),
        Some("::core::convert::Infallible"),
        false,
    )
    .into()
}

#[proc_macro]
pub fn pin_init(input: TokenStream) -> TokenStream {
    init::expand(
        parse_macro_input!(input),
        Some("::core::convert::Infallible"),
        true,
    )
    .into()
}

fn ok_or_compile_error(res: syn::Result<proc_macro2::TokenStream>) -> TokenStream {
    match res {
        Ok(stream) => stream,
        Err(err) => err.into_compile_error(),
    }
    .into()
}

pub(crate) struct Error(Option<syn::Error>);

impl From<syn::Error> for Error {
    fn from(value: syn::Error) -> Self {
        Self(Some(value))
    }
}

impl Error {
    pub(crate) fn none() -> Self {
        Self(None)
    }

    pub(crate) fn combine(&mut self, error: impl Into<Self>) {
        let error = error.into();
        if let Some(this) = self.0.as_mut() {
            if let Some(error) = error.0 {
                this.combine(error);
            }
        } else {
            self.0 = error.0;
        }
    }
}

impl quote::ToTokens for Error {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        if let Some(error) = self.0.as_ref() {
            quote::ToTokens::to_tokens(&error.to_compile_error(), tokens);
        }
    }
}
