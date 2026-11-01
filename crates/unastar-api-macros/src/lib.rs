//! Attribute registration for WIT Component Model plugins.
//!
//! Implement `unastar_api::Handler` and annotate it with [`plugin`]. Registration
//! delegates to `unastar_api::export_handler!`, which supplies scoped players and
//! transactions. Low-level `Plugin` implementations use `export_plugin!` instead.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemImpl};

/// Register a concrete scoped `Handler` or canonical `Plugin` as a component.
///
/// Use this attribute instead of a separate export macro invocation. Only one plugin may be registered per component.
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
            "plugin requires a concrete unastar_api::Handler or Plugin trait implementation",
        )
        .to_compile_error()
        .into();
    }
    let self_ty = &input.self_ty;
    let is_handler = input.trait_.as_ref().is_some_and(|(_, path, _)| {
        path.segments
            .last()
            .is_some_and(|segment| segment.ident == "Handler")
    });
    let registration = if is_handler {
        quote! { unastar_api::export_handler!(#self_ty); }
    } else {
        quote! { unastar_api::export_plugin!(#self_ty); }
    };
    quote! { #input #registration }.into()
}
