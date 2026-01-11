// SPDX-License-Identifier: Apache-2.0 OR MIT

// When fixdep scans this, it will find this string `CONFIG_RUSTC_VERSION_TEXT`
// and thus add a dependency on `include/config/RUSTC_VERSION_TEXT`, which is
// touched by Kconfig when the version string from the compiler changes.

//! `pin-init` proc macros.

#![cfg_attr(not(RUSTC_LINT_REASONS_IS_STABLE), feature(lint_reasons))]
// Documentation is done in the pin-init crate instead.
#![allow(missing_docs)]

use proc_macro::TokenStream;

mod helpers;
mod pin_data;
mod pinned_drop;
mod zeroable;

#[proc_macro_attribute]
pub fn pin_data(inner: TokenStream, item: TokenStream) -> TokenStream {
    pin_data::pin_data(inner.into(), item.into()).into()
}

#[proc_macro_attribute]
pub fn pinned_drop(args: TokenStream, input: TokenStream) -> TokenStream {
    pinned_drop::pinned_drop(args.into(), input.into()).into()
}

#[proc_macro_derive(Zeroable)]
pub fn derive_zeroable(input: TokenStream) -> TokenStream {
    zeroable::derive(input.into()).into()
}

#[proc_macro_derive(MaybeZeroable)]
pub fn maybe_derive_zeroable(input: TokenStream) -> TokenStream {
    zeroable::maybe_derive(input.into()).into()
}

#[expect(dead_code)]
fn ok_or_compile_error(res: syn::Result<proc_macro2::TokenStream>) -> TokenStream {
    match res {
        Ok(stream) => stream,
        Err(error) => error.into_compile_error(),
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
    #[expect(dead_code)]
    pub(crate) fn none() -> Self {
        Self(None)
    }

    #[expect(dead_code)]
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
