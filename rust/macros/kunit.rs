// SPDX-License-Identifier: GPL-2.0

//! Procedural macro to run KUnit tests using a user-space like syntax.
//!
//! Copyright (c) 2023 José Expósito <jose.exposito89@gmail.com>

use proc_macro::{Delimiter, Group, TokenStream, TokenTree};
use std::fmt::Write;

pub(crate) fn kunit_tests(attr: TokenStream, ts: TokenStream) -> TokenStream {
    if attr.to_string().is_empty() {
        panic!("Missing test name in #[kunit_tests(test_name)] macro")
    }

    let mut tokens: Vec<_> = ts.into_iter().collect();

    // Scan for the "mod" keyword.
    tokens
        .iter()
        .find_map(|token| match token {
            TokenTree::Ident(ident) => match ident.to_string().as_str() {
                "mod" => Some(true),
                _ => None,
            },
            _ => None,
        })
        .expect("#[kunit_tests(test_name)] attribute should only be applied to modules");

    // Retrieve the main body. The main body should be the last token tree.
    let body = match tokens.pop() {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => group,
        _ => panic!("cannot locate main body of module"),
    };

    // Get the functions set as tests. Search for `[test]` -> `fn`.
    let mut body_it = body.stream().into_iter();
    let mut tests = Vec::new();
    while let Some(token) = body_it.next() {
        match token {
            TokenTree::Group(ident) if ident.to_string() == "[test]" => match body_it.next() {
                Some(TokenTree::Ident(ident)) if ident.to_string() == "fn" => {
                    let test_name = match body_it.next() {
                        Some(TokenTree::Ident(ident)) => ident.to_string(),
                        _ => continue,
                    };
                    tests.push(test_name);
                }
                _ => continue,
            },
            _ => (),
        }
    }

    // Add `#[cfg(CONFIG_KUNIT)]` before the module declaration.
    let config_kunit = "#[cfg(CONFIG_KUNIT)]".to_owned().parse().unwrap();
    tokens.insert(
        0,
        TokenTree::Group(Group::new(Delimiter::None, config_kunit)),
    );

    // Generate the test KUnit test suite and a test case for each `#[test]`.
    // The code generated for the following test module:
    //
    // ```
    // #[kunit_tests(kunit_test_suit_name)]
    // mod tests {
    //     #[test]
    //     fn foo() {
    //         assert_eq!(1, 1);
    //     }
    //
    //     #[test]
    //     fn bar() {
    //         assert_eq!(2, 2);
    //     }
    // ```
    //
    // Looks like:
    //
    // ```
    // unsafe extern "C" fn kunit_rust_wrapper_foo(_test: *mut kernel::bindings::kunit) {
    //     foo();
    // }
    // static mut KUNIT_CASE_FOO: kernel::bindings::kunit_case =
    //     kernel::kunit_case!(foo, kunit_rust_wrapper_foo);
    //
    // unsafe extern "C" fn kunit_rust_wrapper_bar(_test: * mut kernel::bindings::kunit) {
    //     bar();
    // }
    // static mut KUNIT_CASE_BAR: kernel::bindings::kunit_case =
    //     kernel::kunit_case!(bar, kunit_rust_wrapper_bar);
    //
    // static mut KUNIT_CASE_NULL: kernel::bindings::kunit_case = kernel::kunit_case!();
    //
    // static mut TEST_CASES : &mut[kernel::bindings::kunit_case] = unsafe {
    //     &mut [KUNIT_CASE_FOO, KUNIT_CASE_BAR, KUNIT_CASE_NULL]
    // };
    //
    // kernel::kunit_unsafe_test_suite!(kunit_test_suit_name, TEST_CASES);
    // ```
    let mut kunit_macros = "".to_owned();
    let mut test_cases = "".to_owned();
    for test in tests {
        let kunit_wrapper_fn_name = format!("kunit_rust_wrapper_{}", test);
        let kunit_case_name = format!("KUNIT_CASE_{}", test.to_uppercase());
        let kunit_wrapper = format!(
            "unsafe extern \"C\" fn {}(_test: *mut kernel::bindings::kunit) {{ {}(); }}",
            kunit_wrapper_fn_name, test
        );
        let kunit_case = format!(
            "static mut {}: kernel::bindings::kunit_case = kernel::kunit_case!({}, {});",
            kunit_case_name, test, kunit_wrapper_fn_name
        );
        writeln!(kunit_macros, "{kunit_wrapper}").unwrap();
        writeln!(kunit_macros, "{kunit_case}").unwrap();
        writeln!(test_cases, "{kunit_case_name},").unwrap();
    }

    writeln!(
        kunit_macros,
        "static mut KUNIT_CASE_NULL: kernel::bindings::kunit_case = kernel::kunit_case!();"
    )
    .unwrap();

    writeln!(
        kunit_macros,
        "static mut TEST_CASES : &mut[kernel::bindings::kunit_case] = unsafe {{ &mut[{test_cases} KUNIT_CASE_NULL] }};"
    )
    .unwrap();

    writeln!(
        kunit_macros,
        "kernel::kunit_unsafe_test_suite!({attr}, TEST_CASES);"
    )
    .unwrap();

    let new_body: TokenStream = vec![body.stream(), kunit_macros.parse().unwrap()]
        .into_iter()
        .collect();

    // Remove the `#[test]` macros.
    let new_body = new_body.to_string().replace("#[test]", "");
    tokens.push(TokenTree::Group(Group::new(
        Delimiter::Brace,
        new_body.parse().unwrap(),
    )));

    tokens.into_iter().collect()
}
