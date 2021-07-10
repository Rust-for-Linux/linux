// SPDX-License-Identifier: GPL-2.0
use proc_macro::{TokenStream, TokenTree};

use super::helpers::*;

pub fn rtnl_link_ops(ts: TokenStream) -> TokenStream {
    let mut it = ts.into_iter();
    let literals = &["maxtype", "policy", "slave_maxtype", "slave_policy"];

    let mut found_idents = Vec::new();

    let kind = get_byte_string(&mut it, "kind");
    let netdevice = get_ident(&mut it, "type");

    let mut callbacks = String::new();
    let mut fields = String::new();

    loop {
        let name = match it.next() {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            Some(_) => panic!("Expected Ident or End"),
            None => break,
        };

        assert_eq!(expect_punct(&mut it), ':');

        if literals.contains(&name.as_str()) {
            let literal = expect_literal(&mut it);
            fields.push_str(&format!(
                "{name}: {literal},\n",
                name = name,
                literal = literal
            ));
        } else {
            let func = expect_ident(&mut it);
            callbacks.push_str(&build_rtnl_links_callback(&name, &netdevice, &func, &kind));
            found_idents.push(name);
        }

        assert_eq!(expect_punct(&mut it), ',');
    }
    expect_end(&mut it);

    let callback_fields = found_idents
        .iter()
        .map(|name| format!("{}: Some(__rtnl_link_{}_callback_{}),", name, name, kind))
        .collect::<Vec<String>>()
        .join("\n");

    let ops_struct = format!(
        r#"
             #[doc(hidden)]
             #[used]
             #[no_mangle]
             pub static mut {kind}_LINK_OPS: kernel::net::rtnl::RtnlLinkOps = kernel::net::rtnl::RtnlLinkOps::new_from_inner(kernel::bindings::rtnl_link_ops {{
                 priv_size: core::mem::size_of::<<{netdevice} as kernel::net::device::NetDeviceAdapter>::Inner>(),
                 kind: b"{kind}\0".as_ptr() as *const i8,
                 {callback_fields}
                 {fields}
                 ..kernel::net::rtnl::RTNL_LINK_OPS_EMPTY                
             }});
         "#,
        kind = kind,
        netdevice = netdevice,
        callback_fields = callback_fields,
        fields = fields,
    );

    format!(
        r#"
         {callbacks}
         #[cfg_attr(any(CONFIG_X86, CONFIG_SPARC64), link_section = ".data.read_mostly")]
         {ops_struct}
         "#,
        callbacks = callbacks,
        ops_struct = ops_struct,
    )
    .parse()
    .expect("Error parsing formatted string into token stream.")
}

struct RtnlLinkValues {
    callback_params: String,
    return_type: String,
    wrapper_before: String,
    wrapper_after: String,
    params: String,
}

impl RtnlLinkValues {
    fn new(callback_params: &str, wrapper_before: &str, params: &str) -> Self {
        Self {
            callback_params: callback_params.to_owned(),
            return_type: "()".to_owned(),
            wrapper_before: wrapper_before.to_owned(),
            wrapper_after: "".to_owned(),
            params: params.to_owned(),
        }
    }
}

fn get_rtnl_links_values(name: &str, netdevice: &str) -> RtnlLinkValues {
    let setup_dev = format!(
        "let mut dev = kernel::net::device::NetDevice::<{}>::from_pointer(dev);",
        netdevice
    );
    match name {
        "setup" => RtnlLinkValues::new("dev: *mut kernel::bindings::net_device", &setup_dev, "&mut dev"),
        "validate" => RtnlLinkValues {
            callback_params: "tb: *mut *mut kernel::bindings::nlattr, data: *mut *mut kernel::bindings::nlattr, extack: *mut kernel::bindings::netlink_ext_ack".to_owned(),
            return_type: "kernel::c_types::c_int".to_owned(),
            wrapper_before: r#"kernel::from_kernel_result! {
                 let tb = kernel::net::netlink::NlAttrVec::from_pointer(tb as *const *const kernel::bindings::nlattr);
                 let data = kernel::net::netlink::NlAttrVec::from_pointer(data as *const *const kernel::bindings::nlattr);
                 let extack = kernel::net::netlink::NlExtAck::from_pointer(extack);
                 "#.to_owned(),
            wrapper_after: "?; Ok(0) }".to_owned(),
            params: "&tb, &data, &extack".to_owned(),
        },
        _ => panic!("invalid rtnl_link_ops function '{}'", name),
    }
}

fn build_rtnl_links_callback(name: &str, netdevice: &str, func: &str, kind: &str) -> String {
    let values = get_rtnl_links_values(name, netdevice);
    format!(
        r#"
             #[doc(hidden)]
             pub unsafe extern "C" fn __rtnl_link_{name}_callback_{kind}({cb_params}) -> {cb_return} {{
                 {cb_before}
                 {cb_func}({cb_r_params})
                 {cb_after}
             }}
         "#,
        name = name,
        kind = kind,
        cb_params = values.callback_params,
        cb_return = values.return_type,
        cb_before = values.wrapper_before,
        cb_func = func,
        cb_r_params = values.params,
        cb_after = values.wrapper_after,
    )
}
