// SPDX-License-Identifier: GPL-2.0

use std::iter;

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::{
    bracketed,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    token, Error, Ident, LitStr, Result, Token,
};

struct ModInfoBuilder<'a> {
    module: &'a str,
    counter: usize,
    ts: TokenStream,
}

impl<'a> ModInfoBuilder<'a> {
    fn new(module: &'a str) -> Self {
        ModInfoBuilder {
            module,
            counter: 0,
            ts: TokenStream::new(),
        }
    }

    fn emit_base(&mut self, field: &str, content: &str, builtin: bool) {
        let string = if builtin {
            // Built-in modules prefix their modinfo strings by `module.`.
            format!("{module}.{field}={content}\0", module = self.module)
        } else {
            // Loadable modules' modinfo strings go as-is.
            format!("{field}={content}\0")
        };
        let length = string.len();
        let string = Literal::byte_string(string.as_bytes());
        let cfg = if builtin {
            quote!(#[cfg(not(MODULE))])
        } else {
            quote!(#[cfg(MODULE)])
        };

        let counter = format_ident!(
            "__{module}_{counter}",
            module = self.module,
            counter = self.counter
        );
        self.ts.extend(quote! {
            #cfg
            #[doc(hidden)]
            #[link_section = ".modinfo"]
            #[used]
            pub static #counter: [u8; #length] = *#string;
        });

        self.counter += 1;
    }

    fn emit_only_builtin(&mut self, field: &str, content: &str) {
        self.emit_base(field, content, true)
    }

    fn emit_only_loadable(&mut self, field: &str, content: &str) {
        self.emit_base(field, content, false)
    }

    fn emit(&mut self, field: &str, content: &str) {
        self.emit_only_builtin(field, content);
        self.emit_only_loadable(field, content);
    }
}

mod kw {
    syn::custom_keyword!(name);
    syn::custom_keyword!(author);
    syn::custom_keyword!(description);
    syn::custom_keyword!(license);
    syn::custom_keyword!(alias);
}

enum Field {
    Type(Token![type], Token![:], Ident),
    Name(kw::name, Token![:], AsciiLitStr),
    Author(kw::author, Token![:], LitStr),
    Description(kw::description, Token![:], LitStr),
    License(kw::license, Token![:], AsciiLitStr),
    Alias(
        kw::alias,
        Token![:],
        token::Bracket,
        Punctuated<AsciiLitStr, Token![,]>,
    ),
}

impl Parse for Field {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let lookahead = input.lookahead1();
        Ok(if lookahead.peek(Token![type]) {
            Field::Type(input.parse()?, input.parse()?, input.parse()?)
        } else if lookahead.peek(kw::name) {
            Field::Name(input.parse()?, input.parse()?, input.parse()?)
        } else if lookahead.peek(kw::author) {
            Field::Author(input.parse()?, input.parse()?, input.parse()?)
        } else if lookahead.peek(kw::description) {
            Field::Description(input.parse()?, input.parse()?, input.parse()?)
        } else if lookahead.peek(kw::license) {
            Field::License(input.parse()?, input.parse()?, input.parse()?)
        } else if lookahead.peek(kw::alias) {
            let content;
            Field::Alias(
                input.parse()?,
                input.parse()?,
                bracketed!(content in input),
                Punctuated::parse_terminated(&content)?,
            )
        } else {
            return Err(lookahead.error());
        })
    }
}

impl ToTokens for Field {
    fn to_tokens(&self, ts: &mut TokenStream) {
        match self {
            Field::Type(kw, colon, value) => {
                kw.to_tokens(ts);
                colon.to_tokens(ts);
                value.to_tokens(ts);
            }
            Field::Name(kw, colon, value) => {
                kw.to_tokens(ts);
                colon.to_tokens(ts);
                value.to_tokens(ts);
            }
            Field::Author(kw, colon, value) => {
                kw.to_tokens(ts);
                colon.to_tokens(ts);
                value.to_tokens(ts);
            }
            Field::Description(kw, colon, value) => {
                kw.to_tokens(ts);
                colon.to_tokens(ts);
                value.to_tokens(ts);
            }
            Field::License(kw, colon, value) => {
                kw.to_tokens(ts);
                colon.to_tokens(ts);
                value.to_tokens(ts);
            }
            Field::Alias(kw, colon, bracket, value) => {
                kw.to_tokens(ts);
                colon.to_tokens(ts);
                bracket.surround(ts, |ts| value.to_tokens(ts));
            }
        }
    }
}

impl Field {
    fn order(&self) -> usize {
        match self {
            Field::Type(..) => 0,
            Field::Name(..) => 1,
            Field::Author(..) => 2,
            Field::Description(..) => 3,
            Field::License(..) => 4,
            Field::Alias(..) => 5,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Field::Type(..) => "type",
            Field::Name(..) => "name",
            Field::Author(..) => "author",
            Field::Description(..) => "description",
            Field::License(..) => "license",
            Field::Alias(..) => "alias",
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ModuleInfo {
    type_: Option<Ident>, // Not actually optional.
    name: String,
    author: Option<String>,
    description: Option<String>,
    license: String,
    aliases: Vec<String>,
}

// when `slice::is_sorted_by_key` becomes stable, use it, see
// https://github.com/rust-lang/rust/issues/53485
fn is_sorted_by_key(slice: &[Field]) -> bool {
    let Some(mut last) = slice.first() else {
        return true;
    };
    for e in &slice[1..] {
        if last.order() > e.order() {
            return false;
        }
        last = e;
    }
    true
}

impl Parse for ModuleInfo {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut info = Self::default();
        let fields: Punctuated<Field, Token![,]> = Punctuated::parse_terminated(input)?;
        let tokens = quote!(#fields);
        let mut fields: Vec<Field> = fields.into_iter().collect();
        let mut errors = vec![];
        if !fields.iter().any(|f| matches!(f, Field::Type(..))) {
            errors.push(Error::new_spanned(
                tokens.clone(),
                "Missing required key \"type\".",
            ));
        }
        if !fields.iter().any(|f| matches!(f, Field::Name(..))) {
            errors.push(Error::new_spanned(
                tokens.clone(),
                "Missing required key \"name\".",
            ));
        }
        if !fields.iter().any(|f| matches!(f, Field::License(..))) {
            errors.push(Error::new_spanned(
                tokens.clone(),
                "Missing required key \"license\".",
            ));
        }
        if !is_sorted_by_key(&fields) {
            fields.sort_by_key(|f| f.order());
            let ordered = fields.iter().map(|f| f.name()).collect::<Vec<_>>();
            errors.push(Error::new_spanned(
                tokens,
                format!("Keys are not ordered as expected. Order them like: {ordered:?}."),
            ));
        }
        if let Some(err) = errors.into_iter().reduce(|mut e1, e2| {
            e1.combine(e2);
            e1
        }) {
            return Err(err);
        }

        for field in fields {
            match field {
                Field::Type(.., type_) => info.type_ = Some(type_),
                Field::Name(.., name) => info.name = name.value(),
                Field::Author(.., author) => info.author = Some(author.value()),
                Field::Description(.., desc) => info.description = Some(desc.value()),
                Field::License(.., license) => info.license = license.value(),
                Field::Alias(.., aliases) => {
                    info.aliases = aliases.into_iter().map(|s| s.value()).collect()
                }
            }
        }
        Ok(info)
    }
}

#[derive(Debug)]
struct AsciiLitStr(LitStr);

impl Parse for AsciiLitStr {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let s: LitStr = input.parse()?;
        if !s.value().is_ascii() {
            return Err(Error::new_spanned(s, "Expected ASCII-only string."));
        }
        Ok(Self(s))
    }
}

impl ToTokens for AsciiLitStr {
    fn to_tokens(&self, ts: &mut TokenStream) {
        self.0.to_tokens(ts);
    }
}

impl AsciiLitStr {
    fn value(&self) -> String {
        self.0.value()
    }
}

pub(crate) fn module(
    ModuleInfo {
        type_,
        name,
        author,
        description,
        license,
        aliases,
        ..
    }: ModuleInfo,
) -> TokenStream {
    let Some(type_) = type_ else { unreachable!() };
    let mut modinfo = ModInfoBuilder::new(&name);
    if let Some(author) = author {
        modinfo.emit("author", &author);
    }
    if let Some(description) = description {
        modinfo.emit("description", &description);
    }
    modinfo.emit("license", &license);
    for alias in aliases {
        modinfo.emit("alias", &alias);
    }

    // Built-in modules also export the `file` modinfo string.
    let file =
        std::env::var("RUST_MODFILE").expect("Unable to fetch RUST_MODFILE environmental variable");
    modinfo.emit_only_builtin("file", &file);
    let modinfo = modinfo.ts;

    let name_init = format_ident!("__{name}_init");
    let name_exit = format_ident!("__{name}_exit");
    let name_initcall = format_ident!("__{name}_initcall");
    let global_asm = format!(
        r#".section ".initcall6.init", "a"
        __{name}_initcall:
            .long   __{name}_init - .
            .previous
        "#
    );
    let log_prefix = {
        let bytes = name.bytes().chain(iter::once(0)).collect::<Vec<_>>();
        Literal::byte_string(&bytes)
    };
    quote! {
        /// The module name.
        ///
        /// Used by the printing macros, e.g. [`info!`].
        const __LOG_PREFIX: &[u8] = #log_prefix;

        // Double nested modules, since then nobody can access the public items inside.
        mod __module_init {
            mod __module_init {
                use super::super::#type_;

                /// The "Rust loadable module" mark.
                //
                // This may be best done another way later on, e.g. as a new modinfo
                // key or a new section. For the moment, keep it simple.
                #[cfg(MODULE)]
                #[doc(hidden)]
                #[used]
                static __IS_RUST_MODULE: () = ();

                static mut __MOD: Option<#type_> = None;

                // SAFETY: `__this_module` is constructed by the kernel at load time and will not be
                // freed until the module is unloaded.
                #[cfg(MODULE)]
                static THIS_MODULE: kernel::ThisModule = unsafe {
                    kernel::ThisModule::from_ptr(&kernel::bindings::__this_module as *const _ as *mut _)
                };
                #[cfg(not(MODULE))]
                static THIS_MODULE: kernel::ThisModule = unsafe {
                    kernel::ThisModule::from_ptr(core::ptr::null_mut())
                };

                // Loadable modules need to export the `{init,cleanup}_module` identifiers.
                /// # Safety
                ///
                /// This function must not be called after module initialization, because it may be
                /// freed after that completes.
                #[cfg(MODULE)]
                #[doc(hidden)]
                #[no_mangle]
                #[link_section = ".init.text"]
                pub unsafe extern "C" fn init_module() -> core::ffi::c_int {
                    // SAFETY: This function is inaccessible to the outside due to the double
                    // module wrapping it. It is called exactly once by the C side via its
                    // unique name.
                    unsafe { __init() }
                }

                #[cfg(MODULE)]
                #[doc(hidden)]
                #[no_mangle]
                pub extern "C" fn cleanup_module() {
                    // SAFETY:
                    // - This function is inaccessible to the outside due to the double
                    //   module wrapping it. It is called exactly once by the C side via its
                    //   unique name,
                    // - furthermore it is only called after `init_module` has returned `0`
                    //   (which delegates to `__init`).
                    unsafe { __exit() }
                }

                // Built-in modules are initialized through an initcall pointer
                // and the identifiers need to be unique.
                #[cfg(not(MODULE))]
                #[cfg(not(CONFIG_HAVE_ARCH_PREL32_RELOCATIONS))]
                #[doc(hidden)]
                #[link_section = ".initcall6.init"]
                #[used]
                pub static #name_initcall: extern "C" fn() -> core::ffi::c_int = #name_init;

                #[cfg(not(MODULE))]
                #[cfg(CONFIG_HAVE_ARCH_PREL32_RELOCATIONS)]
                core::arch::global_asm!(#global_asm);

                #[cfg(not(MODULE))]
                #[doc(hidden)]
                #[no_mangle]
                pub extern "C" fn #name_init() -> core::ffi::c_int {
                    // SAFETY: This function is inaccessible to the outside due to the double
                    // module wrapping it. It is called exactly once by the C side via its
                    // placement above in the initcall section.
                    unsafe { __init() }
                }

                #[cfg(not(MODULE))]
                #[doc(hidden)]
                #[no_mangle]
                pub extern "C" fn #name_exit() {
                    // SAFETY:
                    // - This function is inaccessible to the outside due to the double
                    //   module wrapping it. It is called exactly once by the C side via its
                    //   unique name,
                    // - furthermore it is only called after `#name_init` has returned `0`
                    //   (which delegates to `__init`).
                    unsafe { __exit() }
                }

                /// # Safety
                ///
                /// This function must only be called once.
                unsafe fn __init() -> core::ffi::c_int {
                    match <#type_ as kernel::Module>::init(&THIS_MODULE) {
                        Ok(m) => {
                            // SAFETY: No data race, since `__MOD` can only be accessed by this
                            // module and there only `__init` and `__exit` access it. These
                            // functions are only called once and `__exit` cannot be called
                            // before or during `__init`.
                            unsafe {
                                __MOD = Some(m);
                            }
                            return 0;
                        }
                        Err(e) => {
                            return e.to_errno();
                        }
                    }
                }

                /// # Safety
                ///
                /// This function must
                /// - only be called once,
                /// - be called after `__init` has been called and returned `0`.
                unsafe fn __exit() {
                    // SAFETY: No data race, since `__MOD` can only be accessed by this module
                    // and there only `__init` and `__exit` access it. These functions are only
                    // called once and `__init` was already called.
                    unsafe {
                        // Invokes `drop()` on `__MOD`, which should be used for cleanup.
                        __MOD = None;
                    }
                }

                #modinfo
            }
        }
    }
}
