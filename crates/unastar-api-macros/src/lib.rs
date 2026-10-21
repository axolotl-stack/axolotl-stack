//! Attribute registration for WIT Component Model plugins.
//!
//! Implement `unastar_api::Plugin` using its typed event methods and annotate
//! that implementation with [`plugin`]. Registration delegates to
//! `unastar_api::export_plugin!`, which generates the complete `Guest` bridge
//! from the API's WIT world. Event methods need no additional attributes.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemImpl};

/// Register a concrete `unastar_api::Plugin` implementation as a component.
///
/// Use this attribute instead of a separate `unastar_api::export_plugin!`
/// invocation. Only one plugin may be registered per component.
///
/// ```no_run
/// use unastar_api::Plugin;
/// use unastar_api_macros::plugin;
///
/// struct MyPlugin;
///
/// #[plugin]
/// impl Plugin for MyPlugin {
///     fn on_tick(tick_id: u64) {
///         // Handle a typed event from the WIT host.
///         let _ = tick_id;
///     }
/// }
/// ```
///
/// Build the consuming plugin as a `cdylib` for `wasm32-wasip2`.
#[proc_macro_attribute]
pub fn plugin(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro::Span::call_site().into(),
            "plugin takes no arguments",
        )
        .to_compile_error()
        .into();
    }

    let input = parse_macro_input!(item as ItemImpl);
    if input.trait_.is_none() || !input.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &input,
            "plugin requires a concrete unastar_api::Plugin trait implementation",
        )
        .to_compile_error()
        .into();
    }
    let self_ty = &input.self_ty;

    quote! {
        #input
        unastar_api::export_plugin!(#self_ty);
    }
    .into()
}
