// SPDX-License-Identifier: Apache-2.0 OR MIT

use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    braced,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
    token, Error, Expr, ExprCall, ExprPath, Path, Result, Token, Type,
};

pub(crate) struct InPlaceInitializer {
    this: Option<This>,
    pin: Option<kw::pin>,
    path: Path,
    _brace_token: token::Brace,
    fields: Punctuated<FieldInitializer, Token![,]>,
    rest: Option<(Token![..], Expr)>,
    _question: Token![?],
    error: Type,
}

struct This {
    _and_token: Token![&],
    ident: Ident,
    _in_token: Token![in],
}

mod kw {
    syn::custom_keyword!(pin);
}

enum FieldInitializer {
    Value {
        ident: Ident,
        value: Option<(Token![:], Expr)>,
    },
    Init {
        ident: Ident,
        _larrow: Token![<-],
        value: Expr,
    },
}

impl FieldInitializer {
    fn ident(&self) -> &Ident {
        match self {
            FieldInitializer::Value { ident, .. } | FieldInitializer::Init { ident, .. } => ident,
        }
    }
}

enum InitKind {
    Normal,
    Zeroing,
}

pub(crate) fn primitive_init(
    InPlaceInitializer {
        this,
        pin,
        path,
        fields,
        rest,
        error,
        ..
    }: InPlaceInitializer,
) -> Result<TokenStream> {
    let (has_data_trait, data_trait, get_data, from_closure, use_data) = match pin {
        Some(_) => (
            format_ident!("HasPinData"),
            format_ident!("PinData"),
            format_ident!("__pin_data"),
            format_ident!("pin_init_from_closure"),
            true,
        ),
        None => (
            format_ident!("HasInitData"),
            format_ident!("InitData"),
            format_ident!("__init_data"),
            format_ident!("init_from_closure"),
            false,
        ),
    };

    let init_kind = get_init_kind(rest)?;
    let zeroable_check = match init_kind {
        InitKind::Normal => quote! {},
        InitKind::Zeroing => quote! {
            // The user specified `..Zeroable::zeroed()` at the end of the list of fields.
            // Therefore we check if the struct implements `Zeroable` and then zero the memory.
            // This allows us to also remove the check that all fields are present (since we
            // already set the memory to zero and that is a valid bit pattern).
            fn assert_zeroable<T: ?::core::marker::Sized>(_: *mut T)
            where T: ::kernel::init::Zeroable
            {}
            // Ensure that the struct is indeed `Zeroable`.
            assert_zeroable(slot);
            // SAFETY: The type implements `Zeroable` by the check above.
            unsafe { ::core::ptr::write_bytes(slot, 0, 1) };
        },
    };

    let this = match this {
        None => quote!(),
        Some(This { ident, .. }) => quote! {
            // Create the `this` so it can be referenced by the user inside of the
            // expressions creating the individual fields.
            let #ident = unsafe { ::core::ptr::NonNull::new_unchecked(slot) };
        },
    };
    let init_fields = init_fields(&fields, use_data);
    let field_check = make_field_check(&fields, init_kind, &path);
    Ok(quote! {{
        // We do not want to allow arbitrary returns, so we declare this type as the `Ok` return
        // type and shadow it later when we insert the arbitrary user code. That way there will be
        // no possibility of returning without `unsafe`.
        struct __InitOk;
        // Get the data about fields from the supplied type.
        let data = unsafe {
            use ::kernel::init::__internal::#has_data_trait;
            // Can't use `<#path as #has_data_trait>::#get_data`, since the user is able to omit
            // generics (which need to be present with that syntax).
            #path::#get_data()
        };
        // Ensure that `data` really is of type `data` and help with type inference:
        let init = ::kernel::init::__internal::#data_trait::make_closure::<_, __InitOk, #error>(
            data,
            move |slot| {
                {
                    // Shadow the structure so it cannot be used to return early.
                    struct __InitOk;

                    #zeroable_check

                    #this

                    #init_fields

                    #field_check
                }
                Ok(__InitOk)
            }
        );
        let init = move |slot| -> ::core::result::Result<(), #error> {
            init(slot).map(|__InitOk| ())
        };
        let init = unsafe { ::kernel::init::#from_closure::<_, #error>(init) };
        init
    }})
}

fn get_init_kind(rest: Option<(Token![..], Expr)>) -> Result<InitKind> {
    let Some((dotdot, expr)) = rest else {
        return Ok(InitKind::Normal);
    };
    let error = Error::new_spanned(
        quote!(#dotdot #expr),
        "Expected one of the following:\n- Nothing.\n- `..Zeroable::zeroed()`.",
    );
    let Expr::Call(ExprCall {
        func, args, attrs, ..
    }) = expr
    else {
        return Err(error);
    };
    if !args.is_empty() || !attrs.is_empty() {
        return Err(error);
    }
    match *func {
        Expr::Path(ExprPath {
            attrs,
            qself: None,
            path:
                Path {
                    leading_colon: None,
                    segments,
                },
        }) if attrs.is_empty()
            && segments.len() == 2
            && segments[0].ident == "Zeroable"
            && segments[0].arguments.is_none()
            && segments[1].ident == "zeroed"
            && segments[1].arguments.is_none() =>
        {
            Ok(InitKind::Zeroing)
        }
        _ => Err(error),
    }
}

/// Generate the code that initializes the fields of the struct using the initializers in `field`.
fn init_fields(fields: &Punctuated<FieldInitializer, Token![,]>, use_data: bool) -> TokenStream {
    let mut guards = vec![];
    let mut res = TokenStream::new();
    for field in fields {
        let ident = field.ident();
        let guard = format_ident!("__{ident}_guard", span = Span::call_site());
        guards.push(guard.clone());
        let init = match field {
            FieldInitializer::Value { ident, value } => {
                let mut value_ident = ident.clone();
                let value = value.as_ref().map(|value| &value.1).map(|value| {
                    // Setting the span of `value_ident` to `value`'s span improves error messages
                    // when the type of `value` is wrong.
                    value_ident.set_span(value.span());
                    quote!(let #value_ident = #value;)
                });
                // Again span for better diagnostics
                let write = quote_spanned! {ident.span()=>
                    ::core::ptr::write
                };
                quote! {
                    {
                        #value
                        // Initialize the field.
                        //
                        // SAFETY: The memory at `slot` is uninitialized.
                        unsafe { #write(::core::ptr::addr_of_mut!((*slot).#ident), #value_ident) };
                    }
                }
            }
            FieldInitializer::Init { ident, value, .. } => {
                if use_data {
                    quote! {
                        let init = #value;
                        // Call the initializer.
                        //
                        // SAFETY: `slot` is valid, because we are inside of an initializer closure,
                        // we return when an error/panic occurs.
                        // We also use the `data` to require the correct trait (`Init` or `PinInit`)
                        // for `#ident`.
                        unsafe { data.#ident(::core::ptr::addr_of_mut!((*slot).#ident), init)? };
                    }
                } else {
                    quote! {
                        let init = #value;
                        // Call the initializer.
                        //
                        // SAFETY: `slot` is valid, because we are inside of an initializer closure,
                        // we return when an error/panic occurs.
                        unsafe {
                            ::kernel::init::Init::__init(
                                init,
                                ::core::ptr::addr_of_mut!((*slot).#ident),
                            )?
                        };
                    }
                }
            }
        };
        res.extend(init);
        res.extend(quote! {
            // Create the drop guard:
            //
            // We rely on macro hygiene to make it impossible for users to access this local
            // variable.
            // SAFETY: We forget the guard later when initialization has succeeded.
            let #guard = unsafe {
                ::kernel::init::__internal::DropGuard::new(
                    ::core::ptr::addr_of_mut!((*slot).#ident)
                )
            };
        });
    }
    quote! {
        #res
        // If execution reaches this point, all fields have been initialized. Therefore we can now
        // dismiss the guards by forgetting them.
        #(::core::mem::forget(#guards);)*
    }
}

/// Generate the check for ensuring that every field has been initialized.
fn make_field_check(
    fields: &Punctuated<FieldInitializer, Token![,]>,
    init_kind: InitKind,
    path: &Path,
) -> TokenStream {
    let fields = fields.iter().map(|f| f.ident());
    match init_kind {
        InitKind::Normal => quote! {
            // We use unreachable code to ensure that all fields have been mentioned exactly once,
            // this struct initializer will still be type-checked and complain with a very natural
            // error message if a field is forgotten/mentioned more than once.
            #[allow(unreachable_code, clippy::diverging_sub_expression)]
            // SAFETY: this code is never executed.
            let _ = || unsafe {
                ::core::ptr::write(slot, #path {
                    #(
                        #fields: ::core::panic!(),
                    )*
                })
            };
        },
        InitKind::Zeroing => quote! {
            // We use unreachable code to ensure that all fields have been mentioned at most once.
            // Since the user specified `..Zeroable::zeroed()` at the end, all missing fields will
            // be zeroed. This struct initializer will still be type-checked and complain with a
            // very natural error message if a field is mentioned more than once, or doesn't exist.
            #[allow(unreachable_code, clippy::diverging_sub_expression, unused_assignments)]
            // SAFETY: this code is never executed.
            let _ = || unsafe {
                let mut zeroed = ::core::mem::zeroed();
                ::core::ptr::write(slot, zeroed);
                zeroed = ::core::mem::zeroed();
                ::core::ptr::write(slot, #path {
                    #(
                        #fields: ::core::panic!(),
                    )*
                    ..zeroed
                })
            };
        },
    }
}

impl Parse for InPlaceInitializer {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        Ok(Self {
            this: input.peek(Token![&]).then(|| input.parse()).transpose()?,
            pin: input.peek(kw::pin).then(|| input.parse()).transpose()?,
            path: input.parse()?,
            _brace_token: braced!(content in input),
            fields: {
                let mut fields = Punctuated::new();
                loop {
                    let lookahead = content.lookahead1();
                    if content.is_empty() || lookahead.peek(Token![..]) {
                        break;
                    } else if lookahead.peek(syn::Ident) {
                        fields.push_value(content.parse()?);
                        let lookahead = content.lookahead1();
                        if lookahead.peek(Token![,]) {
                            fields.push_punct(content.parse()?);
                        } else if content.is_empty() {
                            break;
                        } else {
                            return Err(lookahead.error());
                        }
                    } else {
                        return Err(lookahead.error());
                    }
                }
                fields
            },
            rest: content
                .peek(Token![..])
                .then(|| -> Result<_> { Ok((content.parse()?, content.parse()?)) })
                .transpose()?,
            _question: input.parse()?,
            error: input.parse()?,
        })
    }
}

impl Parse for This {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self {
            _and_token: input.parse()?,
            ident: input.parse()?,
            _in_token: input.parse()?,
        })
    }
}

impl Parse for FieldInitializer {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident = input.parse()?;
        let lookahead = input.lookahead1();
        Ok(if lookahead.peek(Token![<-]) {
            Self::Init {
                ident,
                _larrow: input.parse()?,
                value: input.parse()?,
            }
        } else if lookahead.peek(Token![:]) {
            Self::Value {
                ident,
                value: Some((input.parse()?, input.parse()?)),
            }
        } else if lookahead.peek(Token![,]) || input.is_empty() {
            Self::Value { ident, value: None }
        } else {
            return Err(lookahead.error());
        })
    }
}
