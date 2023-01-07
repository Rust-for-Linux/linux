// SPDX-License-Identifier: GPL-2.0

use proc_macro::{token_stream, Delimiter, Group, Literal, TokenStream, TokenTree};
use std::fmt::Write;

use crate::helpers::*;

#[derive(Clone, PartialEq)]
enum ParamType {
    Ident(String),
    Array { vals: String, max_length: usize },
}

fn expect_array_fields(it: &mut token_stream::IntoIter) -> ParamType {
    assert_eq!(expect_punct(it), '<');
    let vals = expect_ident(it);
    assert_eq!(expect_punct(it), ',');
    let max_length_str = expect_literal(it);
    let max_length = max_length_str
        .parse::<usize>()
        .expect("Expected usize length");
    assert_eq!(expect_punct(it), '>');
    ParamType::Array { vals, max_length }
}

fn expect_type(it: &mut token_stream::IntoIter) -> ParamType {
    if let TokenTree::Ident(ident) = it
        .next()
        .expect("Reached end of token stream for param type")
    {
        match ident.to_string().as_ref() {
            "ArrayParam" => expect_array_fields(it),
            _ => ParamType::Ident(ident.to_string()),
        }
    } else {
        panic!("Expected Param Type")
    }
}

struct ModInfoBuilder<'a> {
    module: &'a str,
    counter: usize,
    buffer: String,
}

impl<'a> ModInfoBuilder<'a> {
    fn new(module: &'a str) -> Self {
        ModInfoBuilder {
            module,
            counter: 0,
            buffer: String::new(),
        }
    }

    fn emit_base(&mut self, field: &str, content: &str, builtin: bool) {
        let string = if builtin {
            // Built-in modules prefix their modinfo strings by `module.`.
            format!(
                "{module}.{field}={content}\0",
                module = self.module,
                field = field,
                content = content
            )
        } else {
            // Loadable modules' modinfo strings go as-is.
            format!("{field}={content}\0", field = field, content = content)
        };

        write!(
            &mut self.buffer,
            "
                {cfg}
                #[doc(hidden)]
                #[link_section = \".modinfo\"]
                #[used]
                pub static __{module}_{counter}: [u8; {length}] = *{string};
            ",
            cfg = if builtin {
                "#[cfg(not(MODULE))]"
            } else {
                "#[cfg(MODULE)]"
            },
            module = self.module.to_uppercase(),
            counter = self.counter,
            length = string.len(),
            string = Literal::byte_string(string.as_bytes()),
        )
        .unwrap();

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

    fn emit_param(&mut self, field: &str, param: &str, content: &str) {
        let content = format!("{param}:{content}", param = param, content = content);
        self.emit(field, &content);
    }
}

fn permissions_are_readonly(perms: &str) -> bool {
    let (radix, digits) = if let Some(n) = perms.strip_prefix("0x") {
        (16, n)
    } else if let Some(n) = perms.strip_prefix("0o") {
        (8, n)
    } else if let Some(n) = perms.strip_prefix("0b") {
        (2, n)
    } else {
        (10, perms)
    };
    match u32::from_str_radix(digits, radix) {
        Ok(perms) => perms & 0o222 == 0,
        Err(_) => false,
    }
}

fn param_ops_path(param_type: &str) -> &'static str {
    match param_type {
        "bool" => "kernel::module_param::PARAM_OPS_BOOL",
        "i8" => "kernel::module_param::PARAM_OPS_I8",
        "u8" => "kernel::module_param::PARAM_OPS_U8",
        "i16" => "kernel::module_param::PARAM_OPS_I16",
        "u16" => "kernel::module_param::PARAM_OPS_U16",
        "i32" => "kernel::module_param::PARAM_OPS_I32",
        "u32" => "kernel::module_param::PARAM_OPS_U32",
        "i64" => "kernel::module_param::PARAM_OPS_I64",
        "u64" => "kernel::module_param::PARAM_OPS_U64",
        "isize" => "kernel::module_param::PARAM_OPS_ISIZE",
        "usize" => "kernel::module_param::PARAM_OPS_USIZE",
        "str" => "kernel::module_param::PARAM_OPS_STR",
        t => panic!("Unrecognized type {}", t),
    }
}

fn try_simple_param_val(
    param_type: &str,
) -> Box<dyn Fn(&mut token_stream::IntoIter) -> Option<String>> {
    match param_type {
        "bool" => Box::new(try_ident),
        "str" => Box::new(|param_it| {
            try_byte_string(param_it)
                .map(|s| format!("kernel::module_param::StringParam::Ref(b\"{}\")", s))
        }),
        _ => Box::new(try_literal),
    }
}

fn get_default(param_type: &ParamType, param_it: &mut token_stream::IntoIter) -> String {
    let try_param_val = match param_type {
        ParamType::Ident(ref param_type)
        | ParamType::Array {
            vals: ref param_type,
            max_length: _,
        } => try_simple_param_val(param_type),
    };
    assert_eq!(expect_ident(param_it), "default");
    assert_eq!(expect_punct(param_it), ':');
    let default = match param_type {
        ParamType::Ident(_) => try_param_val(param_it).expect("Expected default param value"),
        ParamType::Array {
            vals: _,
            max_length: _,
        } => {
            let group = expect_group(param_it);
            assert_eq!(group.delimiter(), Delimiter::Bracket);
            let mut default_vals = Vec::new();
            let mut it = group.stream().into_iter();

            while let Some(default_val) = try_param_val(&mut it) {
                default_vals.push(default_val);
                match it.next() {
                    Some(TokenTree::Punct(punct)) => assert_eq!(punct.as_char(), ','),
                    None => break,
                    _ => panic!("Expected ',' or end of array default values"),
                }
            }

            let mut default_array = "kernel::module_param::ArrayParam::create(&[".to_string();
            default_array.push_str(
                &default_vals
                    .iter()
                    .map(|val| val.to_string())
                    .collect::<Vec<String>>()
                    .join(","),
            );
            default_array.push_str("])");
            default_array
        }
    };
    assert_eq!(expect_punct(param_it), ',');
    default
}

fn generated_array_ops_name(vals: &str, max_length: usize) -> String {
    format!(
        "__generated_array_ops_{vals}_{max_length}",
        vals = vals,
        max_length = max_length
    )
}

#[derive(Debug, Default)]
struct ModuleInfo {
    type_: String,
    license: String,
    name: String,
    authors: Option<Vec<String>>,
    description: Option<String>,
    alias: Option<String>,
    params: Option<Group>,
}

impl ModuleInfo {
    fn parse(it: &mut token_stream::IntoIter) -> Self {
        let mut info = ModuleInfo::default();

        const EXPECTED_KEYS: &[&str] = &[
            "type",
            "name",
            "authors",
            "description",
            "license",
            "alias",
            "alias_rtnl_link",
            "params",
        ];
        const REQUIRED_KEYS: &[&str] = &["type", "name", "license"];
        let mut seen_keys = Vec::new();

        loop {
            let key = match it.next() {
                Some(TokenTree::Ident(ident)) => ident.to_string(),
                Some(_) => panic!("Expected Ident or end"),
                None => break,
            };

            if seen_keys.contains(&key) {
                panic!(
                    "Duplicated key \"{}\". Keys can only be specified once.",
                    key
                );
            }

            assert_eq!(expect_punct(it), ':');

            match key.as_str() {
                "type" => info.type_ = expect_ident(it),
                "name" => info.name = expect_string_ascii(it),
                "authors" => info.authors = Some(Self::parse_authors(it)),
                "description" => info.description = Some(expect_string(it)),
                "license" => info.license = expect_string_ascii(it),
                "alias" => info.alias = Some(expect_string_ascii(it)),
                "alias_rtnl_link" => {
                    info.alias = Some(format!("rtnl-link-{}", expect_string_ascii(it)))
                }
                "params" => info.params = Some(expect_group(it)),
                _ => panic!(
                    "Unknown key \"{}\". Valid keys are: {:?}.",
                    key, EXPECTED_KEYS
                ),
            }

            assert_eq!(expect_punct(it), ',');

            seen_keys.push(key);
        }

        expect_end(it);

        for key in REQUIRED_KEYS {
            if !seen_keys.iter().any(|e| e == key) {
                panic!("Missing required key \"{}\".", key);
            }
        }

        let mut ordered_keys: Vec<&str> = Vec::new();
        for key in EXPECTED_KEYS {
            if seen_keys.iter().any(|e| e == key) {
                ordered_keys.push(key);
            }
        }

        if seen_keys != ordered_keys {
            panic!(
                "Keys are not ordered as expected. Order them like: {:?}.",
                ordered_keys
            );
        }

        info
    }

    /// Parse ["First", "Second"] into Some(vec!["First", "Second"])
    fn parse_authors(it: &mut token_stream::IntoIter) -> Vec<String> {
        let mut authors: Vec<String> = vec![];
        let group = expect_group(it);
        assert_eq!(group.delimiter(), Delimiter::Bracket);
        let mut stream = group.stream().into_iter();
        loop {
            let author = expect_string(&mut stream);
            authors.push(author);
            if let Some(punct) = try_punct(&mut stream) {
                assert_eq!(punct, ',');
            } else {
                assert!(stream.next().is_none());
                break;
            }
        }

        authors
    }
}

pub(crate) fn module(ts: TokenStream) -> TokenStream {
    let mut it = ts.into_iter();

    let info = ModuleInfo::parse(&mut it);

    let mut modinfo = ModInfoBuilder::new(info.name.as_ref());
    if let Some(authors) = &info.authors {
        for author in authors {
            modinfo.emit("author", author);
        }
    }
    if let Some(description) = info.description {
        modinfo.emit("description", &description);
    }
    modinfo.emit("license", &info.license);
    if let Some(alias) = info.alias {
        modinfo.emit("alias", &alias);
    }

    // Built-in modules also export the `file` modinfo string.
    let file =
        std::env::var("RUST_MODFILE").expect("Unable to fetch RUST_MODFILE environmental variable");
    modinfo.emit_only_builtin("file", &file);

    let mut array_types_to_generate = Vec::new();
    if let Some(params) = info.params {
        assert_eq!(params.delimiter(), Delimiter::Brace);

        let mut it = params.stream().into_iter();

        loop {
            let param_name = match it.next() {
                Some(TokenTree::Ident(ident)) => ident.to_string(),
                Some(_) => panic!("Expected Ident or end"),
                None => break,
            };

            assert_eq!(expect_punct(&mut it), ':');
            let param_type = expect_type(&mut it);
            let group = expect_group(&mut it);
            assert_eq!(expect_punct(&mut it), ',');

            assert_eq!(group.delimiter(), Delimiter::Brace);

            let mut param_it = group.stream().into_iter();
            let param_default = get_default(&param_type, &mut param_it);
            let param_permissions = get_literal(&mut param_it, "permissions");
            let param_description = get_string(&mut param_it, "description");
            expect_end(&mut param_it);

            // TODO: More primitive types.
            // TODO: Other kinds: unsafes, etc.
            let (param_kernel_type, ops): (String, _) = match param_type {
                ParamType::Ident(ref param_type) => (
                    param_type.to_string(),
                    param_ops_path(param_type).to_string(),
                ),
                ParamType::Array {
                    ref vals,
                    max_length,
                } => {
                    array_types_to_generate.push((vals.clone(), max_length));
                    (
                        format!("__rust_array_param_{}_{}", vals, max_length),
                        generated_array_ops_name(vals, max_length),
                    )
                }
            };

            modinfo.emit_param("parmtype", &param_name, &param_kernel_type);
            modinfo.emit_param("parm", &param_name, &param_description);
            let param_type_internal = match param_type {
                ParamType::Ident(ref param_type) => match param_type.as_ref() {
                    "str" => "kernel::module_param::StringParam".to_string(),
                    other => other.to_string(),
                },
                ParamType::Array {
                    ref vals,
                    max_length,
                } => format!(
                    "kernel::module_param::ArrayParam<{vals}, {max_length}>",
                    vals = vals,
                    max_length = max_length
                ),
            };
            let read_func = if permissions_are_readonly(&param_permissions) {
                format!(
                    "
                        fn read(&self)
                            -> &<{param_type_internal} as kernel::module_param::ModuleParam>::Value {{
                            // SAFETY: Parameters do not need to be locked because they are
                            // read only or sysfs is not enabled.
                            unsafe {{
                                <{param_type_internal} as kernel::module_param::ModuleParam>::value(
                                    &__{name}_{param_name}_value
                                )
                            }}
                        }}
                    ",
                    name = info.name,
                    param_name = param_name,
                    param_type_internal = param_type_internal,
                )
            } else {
                format!(
                    "
                        fn read<'lck>(&self, lock: &'lck kernel::KParamGuard)
                            -> &'lck <{param_type_internal} as kernel::module_param::ModuleParam>::Value {{
                            // SAFETY: Parameters are locked by `KParamGuard`.
                            unsafe {{
                                <{param_type_internal} as kernel::module_param::ModuleParam>::value(
                                    &__{name}_{param_name}_value
                                )
                            }}
                        }}
                    ",
                    name = info.name,
                    param_name = param_name,
                    param_type_internal = param_type_internal,
                )
            };
            let kparam = format!(
                "
                    kernel::bindings::kernel_param__bindgen_ty_1 {{
                        arg: unsafe {{ &__{name}_{param_name}_value }}
                            as *const _ as *mut core::ffi::c_void,
                    }},
                ",
                name = info.name,
                param_name = param_name,
            );
            write!(
                modinfo.buffer,
                "
                static mut __{name}_{param_name}_value: {param_type_internal} = {param_default};

                struct __{name}_{param_name};

                impl __{name}_{param_name} {{ {read_func} }}

                const {param_name}: __{name}_{param_name} = __{name}_{param_name};

                // Note: the C macro that generates the static structs for the `__param` section
                // asks for them to be `aligned(sizeof(void *))`. However, that was put in place
                // in 2003 in commit 38d5b085d2a0 (\"[PATCH] Fix over-alignment problem on x86-64\")
                // to undo GCC over-alignment of static structs of >32 bytes. It seems that is
                // not the case anymore, so we simplify to a transparent representation here
                // in the expectation that it is not needed anymore.
                // TODO: Revisit this to confirm the above comment and remove it if it happened.
                #[repr(transparent)]
                struct __{name}_{param_name}_RacyKernelParam(kernel::bindings::kernel_param);

                unsafe impl Sync for __{name}_{param_name}_RacyKernelParam {{
                }}

                #[cfg(not(MODULE))]
                const __{name}_{param_name}_name: *const core::ffi::c_char =
                    b\"{name}.{param_name}\\0\" as *const _ as *const core::ffi::c_char;

                #[cfg(MODULE)]
                const __{name}_{param_name}_name: *const core::ffi::c_char =
                    b\"{param_name}\\0\" as *const _ as *const core::ffi::c_char;

                #[link_section = \"__param\"]
                #[used]
                static __{name}_{param_name}_struct: __{name}_{param_name}_RacyKernelParam =
                    __{name}_{param_name}_RacyKernelParam(kernel::bindings::kernel_param {{
                        name: __{name}_{param_name}_name,
                        // SAFETY: `__this_module` is constructed by the kernel at load time
                        // and will not be freed until the module is unloaded.
                        #[cfg(MODULE)]
                        mod_: unsafe {{ &kernel::bindings::__this_module as *const _ as *mut _ }},
                        #[cfg(not(MODULE))]
                        mod_: core::ptr::null_mut(),
                        ops: unsafe {{ &{ops} }} as *const kernel::bindings::kernel_param_ops,
                        perm: {permissions},
                        level: -1,
                        flags: 0,
                        __bindgen_anon_1: {kparam}
                    }});
                ",
                name = info.name,
                param_type_internal = param_type_internal,
                read_func = read_func,
                param_default = param_default,
                param_name = param_name,
                ops = ops,
                permissions = param_permissions,
                kparam = kparam,
            )
            .unwrap();
        }
    }

    let mut generated_array_types = String::new();

    for (vals, max_length) in array_types_to_generate {
        let ops_name = generated_array_ops_name(&vals, max_length);
        write!(
            generated_array_types,
            "
                kernel::make_param_ops!(
                    {ops_name},
                    kernel::module_param::ArrayParam<{vals}, {{ {max_length} }}>
                );
            ",
            ops_name = ops_name,
            vals = vals,
            max_length = max_length,
        )
        .unwrap();
    }

    format!(
        "
            /// The module name.
            ///
            /// Used by the printing macros, e.g. [`info!`].
            const __LOG_PREFIX: &[u8] = b\"{name}\\0\";

            /// The \"Rust loadable module\" mark, for `scripts/is_rust_module.sh`.
            //
            // This may be best done another way later on, e.g. as a new modinfo
            // key or a new section. For the moment, keep it simple.
            #[cfg(MODULE)]
            #[doc(hidden)]
            #[used]
            static __IS_RUST_MODULE: () = ();

            static mut __MOD: Option<{type_}> = None;

            // SAFETY: `__this_module` is constructed by the kernel at load time and will not be
            // freed until the module is unloaded.
            #[cfg(MODULE)]
            static THIS_MODULE: kernel::ThisModule = unsafe {{
                kernel::ThisModule::from_ptr(&kernel::bindings::__this_module as *const _ as *mut _)
            }};
            #[cfg(not(MODULE))]
            static THIS_MODULE: kernel::ThisModule = unsafe {{
                kernel::ThisModule::from_ptr(core::ptr::null_mut())
            }};

            // Loadable modules need to export the `{{init,cleanup}}_module` identifiers.
            #[cfg(MODULE)]
            #[doc(hidden)]
            #[no_mangle]
            pub extern \"C\" fn init_module() -> core::ffi::c_int {{
                __init()
            }}

            #[cfg(MODULE)]
            #[doc(hidden)]
            #[no_mangle]
            pub extern \"C\" fn cleanup_module() {{
                __exit()
            }}

            // Built-in modules are initialized through an initcall pointer
            // and the identifiers need to be unique.
            #[cfg(not(MODULE))]
            #[cfg(not(CONFIG_HAVE_ARCH_PREL32_RELOCATIONS))]
            #[doc(hidden)]
            #[link_section = \"{initcall_section}\"]
            #[used]
            pub static __{name}_initcall: extern \"C\" fn() -> core::ffi::c_int = __{name}_init;

            #[cfg(not(MODULE))]
            #[cfg(CONFIG_HAVE_ARCH_PREL32_RELOCATIONS)]
            core::arch::global_asm!(
                r#\".section \"{initcall_section}\", \"a\"
                __{name}_initcall:
                    .long   __{name}_init - .
                    .previous
                \"#
            );

            #[cfg(not(MODULE))]
            #[doc(hidden)]
            #[no_mangle]
            pub extern \"C\" fn __{name}_init() -> core::ffi::c_int {{
                __init()
            }}

            #[cfg(not(MODULE))]
            #[doc(hidden)]
            #[no_mangle]
            pub extern \"C\" fn __{name}_exit() {{
                __exit()
            }}

            fn __init() -> core::ffi::c_int {{
                match <{type_} as kernel::Module>::init(kernel::c_str!(\"{name}\"), &THIS_MODULE) {{
                    Ok(m) => {{
                        unsafe {{
                            __MOD = Some(m);
                        }}
                        return 0;
                    }}
                    Err(e) => {{
                        return e.to_kernel_errno();
                    }}
                }}
            }}

            fn __exit() {{
                unsafe {{
                    // Invokes `drop()` on `__MOD`, which should be used for cleanup.
                    __MOD = None;
                }}
            }}

            {modinfo}

            {generated_array_types}
        ",
        type_ = info.type_,
        name = info.name,
        modinfo = modinfo.buffer,
        generated_array_types = generated_array_types,
        initcall_section = ".initcall6.init"
    )
    .parse()
    .expect("Error parsing formatted string into token stream.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permissions_are_readonly() {
        assert!(permissions_are_readonly("0b000000000"));
        assert!(permissions_are_readonly("0o000"));
        assert!(permissions_are_readonly("000"));
        assert!(permissions_are_readonly("0x000"));

        assert!(!permissions_are_readonly("0b111111111"));
        assert!(!permissions_are_readonly("0o777"));
        assert!(!permissions_are_readonly("511"));
        assert!(!permissions_are_readonly("0x1ff"));

        assert!(permissions_are_readonly("0o014"));
        assert!(permissions_are_readonly("0o015"));

        assert!(!permissions_are_readonly("0o214"));
        assert!(!permissions_are_readonly("0o024"));
        assert!(!permissions_are_readonly("0o012"));

        assert!(!permissions_are_readonly("0o315"));
        assert!(!permissions_are_readonly("0o065"));
        assert!(!permissions_are_readonly("0o017"));
    }
}
