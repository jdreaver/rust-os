//! This crate exists to export proc macros for kernel tests.

#![feature(proc_macro_span)]

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn};

#[proc_macro_attribute]
pub fn kernel_test(args: TokenStream, item: TokenStream) -> TokenStream {
    assert!(args.is_empty(), "kernel_test attribute takes no arguments");
    let original_item = item.clone();

    let parsed_item = parse_macro_input!(item as ItemFn);
    let fn_name_ident = parsed_item.sig.ident;
    let fn_name_str = fn_name_ident.to_string();
    let struct_ident = format_ident!("TEST_{}", fn_name_ident);

    let span = proc_macro::Span::call_site();
    let source_location = format!(
        "{}:{}",
        span.source_file().path().display(),
        span.start().line
    );

    let test_struct: TokenStream = quote! {
        #[used]
        #[link_section = ".init_test_array"]
        #[allow(non_upper_case_globals)]
        static #struct_ident: ::test_infra::SimpleTest = ::test_infra::SimpleTest {
            source_location: #source_location,
            name: #fn_name_str,
            test_fn: #fn_name_ident,
        };
    }
    .into();

    original_item.into_iter().chain(test_struct).collect()
}
