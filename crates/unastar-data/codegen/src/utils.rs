//! Utility functions for code generation.

use heck::{ToPascalCase, ToSnakeCase};

#[allow(dead_code)]
pub fn component_to_module_name(name: &str) -> String {
    name.strip_prefix("minecraft:")
        .unwrap_or(name)
        .replace('.', "_")
        .to_snake_case()
}

#[allow(dead_code)]
pub fn component_to_struct_name(name: &str) -> String {
    name.strip_prefix("minecraft:")
        .unwrap_or(name)
        .replace('.', "_")
        .to_pascal_case()
}
