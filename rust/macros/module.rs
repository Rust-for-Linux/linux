// SPDX-License-Identifier: GPL-2.0

#![deny(clippy::complexity)]
#![deny(clippy::correctness)]
#![deny(clippy::perf)]
#![deny(clippy::style)]

use crate::syn::{
    buffer::{Cursor, TokenBuffer},
    Lit,
};
use proc_macro::{Delimiter, Literal, TokenStream};

fn expect_ident(it: &mut Cursor<'_>) -> String {
    let (ident, next) = it.ident().expect("Expected Ident");
    *it = next;
    ident.to_string()
}

fn expect_punct(it: &mut Cursor<'_>) -> char {
    let (punct, next) = it.punct().expect("Expected Punct");
    *it = next;
    punct.as_char()
}

fn expect_literal(it: &mut Cursor<'_>) -> Literal {
    let (lit, next) = it.literal().expect("Expected Literal");
    *it = next;
    lit
}

fn expect_group<'a>(it: &mut Cursor<'a>, delim: Delimiter) -> Cursor<'a> {
    let (inner, _, next) = it.group(delim).expect("Expected Group");
    *it = next;
    inner
}

fn expect_string(it: &mut Cursor<'_>) -> String {
    let lit = expect_literal(it);
    let lit = Lit::new(lit);
    match &lit {
        Lit::Str(s) => {
            assert!(s.suffix().is_empty(), "Unexpected suffix");
            s.value()
        }
        _ => panic!("Expected string"),
    }
}

#[derive(Clone, PartialEq)]
enum ParamType {
    Ident(String),
    Array { vals: String, max_length: usize },
}

fn expect_array_fields(it: &mut Cursor<'_>) -> ParamType {
    assert_eq!(expect_punct(it), '<');
    let vals = expect_ident(it);
    assert_eq!(expect_punct(it), ',');
    let max_length_str = expect_literal(it).to_string();
    let max_length = max_length_str
        .parse::<usize>()
        .expect("Expected usize length");
    assert_eq!(expect_punct(it), '>');
    ParamType::Array { vals, max_length }
}

fn expect_type(it: &mut Cursor<'_>) -> ParamType {
    let (ident, next) = it.ident().expect("Expected Param Type");
    *it = next;
    match ident.to_string().as_ref() {
        "ArrayParam" => expect_array_fields(it),
        _ => ParamType::Ident(ident.to_string()),
    }
}

fn expect_end(it: &mut Cursor<'_>) {
    assert!(it.eof(), "Expected end");
}

fn parse_list<T>(
    it: &mut Cursor<'_>,
    delim: Delimiter,
    f: impl Fn(&mut Cursor<'_>) -> T,
) -> Vec<T> {
    let mut inner = expect_group(it, delim);
    let mut vec = Vec::new();
    while !inner.eof() {
        let item = f(&mut inner);
        vec.push(item);
        if inner.eof() {
            break;
        }
        assert_eq!(expect_punct(&mut inner), ',');
    }
    assert!(inner.eof(), "Expected end");
    vec
}

fn parse_item_or_list<T>(
    it: &mut Cursor<'_>,
    delim: Delimiter,
    f: impl Fn(&mut Cursor<'_>) -> T,
) -> Vec<T> {
    if it.group(delim).is_some() {
        parse_list(it, delim, f)
    } else {
        vec![f(it)]
    }
}

fn get_literal(it: &mut Cursor<'_>, expected_name: &str) -> Literal {
    assert_eq!(expect_ident(it), expected_name);
    assert_eq!(expect_punct(it), ':');
    let literal = expect_literal(it);
    assert_eq!(expect_punct(it), ',');
    literal
}

fn get_string(it: &mut Cursor<'_>, expected_name: &str) -> String {
    assert_eq!(expect_ident(it), expected_name);
    assert_eq!(expect_punct(it), ':');
    let byte_string = expect_string(it);
    assert_eq!(expect_punct(it), ',');
    byte_string
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
        use std::fmt::Write;

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
                #[link_section = \".modinfo\"]
                #[used]
                pub static __{module}_{counter}: [u8; {length}] = *{string};
            ",
            cfg = if builtin {
                "#[cfg(not(MODULE))]"
            } else {
                "#[cfg(MODULE)]"
            },
            module = self.module,
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

    fn emit_optional(&mut self, field: &str, content: Option<&str>) {
        if let Some(content) = content {
            self.emit(field, content);
        }
    }

    fn emit_param(&mut self, field: &str, param: &str, content: &str) {
        let content = format!("{param}:{content}", param = param, content = content);
        self.emit(field, &content);
    }
}

fn permissions_are_readonly(perms: u32) -> bool {
    perms & 0o222 == 0
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

fn parse_expr(it: &mut Cursor<'_>) -> TokenStream {
    let mut tt = Vec::new();
    while !it.eof() {
        if let Some((punct, _)) = it.punct() {
            if let ',' | ';' = punct.as_char() {
                break;
            }
        }
        let (token, next) = it.token_tree().unwrap();
        tt.push(token);
        *it = next;
    }
    if tt.is_empty() {
        panic!("Expected expression");
    }
    tt.into_iter().collect()
}

fn expect_simple_param_val(param_type: &str) -> Box<dyn Fn(&mut Cursor<'_>) -> String> {
    match param_type {
        "str" => Box::new(|param_it| {
            let s = expect_string(param_it);
            format!(
                "kernel::module_param::StringParam::Ref({})",
                Literal::byte_string(s.as_bytes())
            )
        }),
        _ => Box::new(|param_it| parse_expr(param_it).to_string()),
    }
}

fn get_default(param_type: &ParamType, param_it: &mut Cursor<'_>) -> String {
    let expect_param_val = match param_type {
        ParamType::Ident(ref param_type)
        | ParamType::Array {
            vals: ref param_type,
            max_length: _,
        } => expect_simple_param_val(param_type),
    };
    assert_eq!(expect_ident(param_it), "default");
    assert_eq!(expect_punct(param_it), ':');
    let default = match param_type {
        ParamType::Ident(_) => expect_param_val(param_it),
        ParamType::Array {
            vals: _,
            max_length: _,
        } => {
            let default_vals = parse_list(param_it, Delimiter::Bracket, expect_param_val);
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

struct ParamInfo {
    name: String,
    type_: ParamType,
    default: String,
    permission: u32,
    description: String,
}

impl ParamInfo {
    fn parse(it: &mut Cursor<'_>) -> Self {
        let param_name = expect_ident(it);

        assert_eq!(expect_punct(it), ':');
        let param_type = expect_type(it);
        let mut param_it = expect_group(it, Delimiter::Brace);

        let param_default = get_default(&param_type, &mut param_it);
        let param_permissions = match Lit::new(get_literal(&mut param_it, "permissions")) {
            Lit::Int(i) => i.base10_digits().parse::<u32>().unwrap(),
            _ => panic!("Permission is expected to be an integer literal"),
        };
        let param_description = get_string(&mut param_it, "description");
        expect_end(&mut param_it);

        ParamInfo {
            name: param_name,
            type_: param_type,
            default: param_default,
            permission: param_permissions,
            description: param_description,
        }
    }
}

#[derive(Default)]
struct ModuleInfo {
    type_: String,
    license: String,
    name: String,
    author: Vec<String>,
    description: Option<String>,
    alias: Vec<String>,
    params: Vec<ParamInfo>,
}

impl ModuleInfo {
    fn parse(it: &mut Cursor<'_>) -> Self {
        let mut info = ModuleInfo::default();

        const EXPECTED_KEYS: &[&str] = &[
            "type",
            "name",
            "author",
            "description",
            "license",
            "alias",
            "alias_rtnl_link",
            "params",
        ];
        const REQUIRED_KEYS: &[&str] = &["type", "name", "license"];
        let mut seen_keys = Vec::new();

        loop {
            if it.eof() {
                break;
            }

            let key = expect_ident(it);

            if seen_keys.contains(&key) {
                panic!(
                    "Duplicated key \"{}\". Keys can only be specified once.",
                    key
                );
            }

            assert_eq!(expect_punct(it), ':');

            match key.as_str() {
                "type" => info.type_ = expect_ident(it),
                "name" => info.name = expect_string(it),
                "author" => info.author = parse_item_or_list(it, Delimiter::Bracket, expect_string),
                "description" => info.description = Some(expect_string(it)),
                "license" => info.license = expect_string(it),
                "alias" => info.alias = parse_item_or_list(it, Delimiter::Bracket, expect_string),
                "alias_rtnl_link" => {
                    info.alias = parse_item_or_list(it, Delimiter::Bracket, expect_string)
                        .into_iter()
                        .map(|x| format!("rtnl-link-{}", x))
                        .collect()
                }
                "params" => info.params = parse_list(it, Delimiter::Brace, ParamInfo::parse),
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

    fn generate(&self) -> TokenStream {
        let mut modinfo = ModInfoBuilder::new(&self.name);
        for author in self.author.iter() {
            modinfo.emit("author", author);
        }
        modinfo.emit_optional("description", self.description.as_deref());
        modinfo.emit("license", &self.license);
        for alias in self.alias.iter() {
            modinfo.emit("alias", alias);
        }

        // Built-in modules also export the `file` modinfo string
        let file = std::env::var("RUST_MODFILE")
            .expect("Unable to fetch RUST_MODFILE environmental variable");
        modinfo.emit_only_builtin("file", &file);

        let mut array_types_to_generate = Vec::new();
        for param in self.params.iter() {
            // TODO: more primitive types
            // TODO: other kinds: unsafes, etc.
            let (param_kernel_type, ops): (String, _) = match param.type_ {
                ParamType::Ident(ref param_type) => (
                    param_type.to_string(),
                    param_ops_path(&param_type).to_string(),
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

            modinfo.emit_param("parmtype", &param.name, &param_kernel_type);
            modinfo.emit_param("parm", &param.name, &param.description);
            let param_type_internal = match param.type_ {
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
            let read_func = if permissions_are_readonly(param.permission) {
                format!(
                    "
                        fn read(&self) -> &<{param_type_internal} as kernel::module_param::ModuleParam>::Value {{
                            // SAFETY: Parameters do not need to be locked because they are read only or sysfs is not enabled.
                            unsafe {{ <{param_type_internal} as kernel::module_param::ModuleParam>::value(&__{name}_{param_name}_value) }}
                        }}
                    ",
                    name = self.name,
                    param_name = param.name,
                    param_type_internal = param_type_internal,
                )
            } else {
                format!(
                    "
                        fn read<'lck>(&self, lock: &'lck kernel::KParamGuard) -> &'lck <{param_type_internal} as kernel::module_param::ModuleParam>::Value {{
                            // SAFETY: Parameters are locked by `KParamGuard`.
                            unsafe {{ <{param_type_internal} as kernel::module_param::ModuleParam>::value(&__{name}_{param_name}_value) }}
                        }}
                    ",
                    name = self.name,
                    param_name = param.name,
                    param_type_internal = param_type_internal,
                )
            };
            let kparam = format!(
                "
                    kernel::bindings::kernel_param__bindgen_ty_1 {{
                        arg: unsafe {{ &__{name}_{param_name}_value }} as *const _ as *mut kernel::c_types::c_void,
                    }},
                ",
                name = self.name,
                param_name = param.name,
            );
            modinfo.buffer.push_str(
                &format!(
                    "
                    static mut __{name}_{param_name}_value: {param_type_internal} = {param_default};

                    struct __{name}_{param_name};

                    impl __{name}_{param_name} {{ {read_func} }}

                    const {param_name}: __{name}_{param_name} = __{name}_{param_name};

                    // Note: the C macro that generates the static structs for the `__param` section
                    // asks for them to be `aligned(sizeof(void *))`. However, that was put in place
                    // in 2003 in commit 38d5b085d2 (\"[PATCH] Fix over-alignment problem on x86-64\")
                    // to undo GCC over-alignment of static structs of >32 bytes. It seems that is
                    // not the case anymore, so we simplify to a transparent representation here
                    // in the expectation that it is not needed anymore.
                    // TODO: revisit this to confirm the above comment and remove it if it happened
                    #[repr(transparent)]
                    struct __{name}_{param_name}_RacyKernelParam(kernel::bindings::kernel_param);

                    unsafe impl Sync for __{name}_{param_name}_RacyKernelParam {{
                    }}

                    #[cfg(not(MODULE))]
                    const __{name}_{param_name}_name: *const kernel::c_types::c_char = b\"{name}.{param_name}\\0\" as *const _ as *const kernel::c_types::c_char;

                    #[cfg(MODULE)]
                    const __{name}_{param_name}_name: *const kernel::c_types::c_char = b\"{param_name}\\0\" as *const _ as *const kernel::c_types::c_char;

                    #[link_section = \"__param\"]
                    #[used]
                    static __{name}_{param_name}_struct: __{name}_{param_name}_RacyKernelParam = __{name}_{param_name}_RacyKernelParam(kernel::bindings::kernel_param {{
                        name: __{name}_{param_name}_name,
                        // SAFETY: `__this_module` is constructed by the kernel at load time and will not be freed until the module is unloaded.
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
                    name = self.name,
                    param_type_internal = param_type_internal,
                    read_func = read_func,
                    param_default = param.default,
                    param_name = param.name,
                    ops = ops,
                    permissions = param.permission,
                    kparam = kparam,
                )
            );
        }

        let mut generated_array_types = String::new();

        for (vals, max_length) in array_types_to_generate {
            let ops_name = generated_array_ops_name(&vals, max_length);
            generated_array_types.push_str(&format!(
                "
                    kernel::make_param_ops!(
                        {ops_name},
                        kernel::module_param::ArrayParam<{vals}, {{ {max_length} }}>
                    );
                ",
                ops_name = ops_name,
                vals = vals,
                max_length = max_length,
            ));
        }

        format!(
            "
                /// The module name.
                ///
                /// Used by the printing macros, e.g. [`info!`].
                const __LOG_PREFIX: &[u8] = b\"{name}\\0\";

                static mut __MOD: Option<{type_}> = None;

                // SAFETY: `__this_module` is constructed by the kernel at load time and will not be freed until the module is unloaded.
                #[cfg(MODULE)]
                static THIS_MODULE: kernel::ThisModule = unsafe {{ kernel::ThisModule::from_ptr(&kernel::bindings::__this_module as *const _ as *mut _) }};
                #[cfg(not(MODULE))]
                static THIS_MODULE: kernel::ThisModule = unsafe {{ kernel::ThisModule::from_ptr(core::ptr::null_mut()) }};

                // Loadable modules need to export the `{{init,cleanup}}_module` identifiers
                #[cfg(MODULE)]
                #[no_mangle]
                pub extern \"C\" fn init_module() -> kernel::c_types::c_int {{
                    __init()
                }}

                #[cfg(MODULE)]
                #[no_mangle]
                pub extern \"C\" fn cleanup_module() {{
                    __exit()
                }}

                // Built-in modules are initialized through an initcall pointer
                // and the identifiers need to be unique
                #[cfg(not(MODULE))]
                #[cfg(not(CONFIG_HAVE_ARCH_PREL32_RELOCATIONS))]
                #[link_section = \"{initcall_section}\"]
                #[used]
                pub static __{name}_initcall: extern \"C\" fn() -> kernel::c_types::c_int = __{name}_init;

                #[cfg(not(MODULE))]
                #[cfg(CONFIG_HAVE_ARCH_PREL32_RELOCATIONS)]
                global_asm!(
                    r#\".section \"{initcall_section}\", \"a\"
                    __{name}_initcall:
                        .long   __{name}_init - .
                        .previous
                    \"#
                );

                #[cfg(not(MODULE))]
                #[no_mangle]
                pub extern \"C\" fn __{name}_init() -> kernel::c_types::c_int {{
                    __init()
                }}

                #[cfg(not(MODULE))]
                #[no_mangle]
                pub extern \"C\" fn __{name}_exit() {{
                    __exit()
                }}

                fn __init() -> kernel::c_types::c_int {{
                    match <{type_} as kernel::KernelModule>::init() {{
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
            type_ = self.type_,
            name = self.name,
            modinfo = modinfo.buffer,
            generated_array_types = generated_array_types,
            initcall_section = ".initcall6.init"
        ).parse().expect("Error parsing formatted string into token stream.")
    }
}

pub fn module(ts: TokenStream) -> TokenStream {
    let buffer = TokenBuffer::new(ts);
    let mut it = buffer.begin();

    let info = ModuleInfo::parse(&mut it);
    info.generate()
}

pub fn module_misc_device(ts: TokenStream) -> TokenStream {
    let buffer = TokenBuffer::new(ts);
    let mut it = buffer.begin();

    let mut info = ModuleInfo::parse(&mut it);
    let type_ = info.type_;
    let module = format!("__internal_ModuleFor{}", type_);
    info.type_ = module.clone();

    let extra = format!(
        "
            #[doc(hidden)]
            struct {module} {{
                _dev: core::pin::Pin<alloc::boxed::Box<kernel::miscdev::Registration>>,
            }}

            impl kernel::KernelModule for {module} {{
                fn init() -> kernel::Result<Self> {{
                    Ok(Self {{
                        _dev: kernel::miscdev::Registration::new_pinned::<{type_}>(
                            kernel::c_str!({name}),
                            None,
                            (),
                        )?,
                    }})
                }}
            }}
        ",
        module = module,
        type_ = type_,
        name = Literal::string(&info.name),
    )
    .parse()
    .expect("Error parsing formatted string into token stream.");

    vec![extra, info.generate()].into_iter().collect()
}
