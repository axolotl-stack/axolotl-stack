//! Runs the unit tests of `codegen/parser/surface_rule.rs`. `build.rs`
//! compiles the codegen modules without `cfg(test)`, so their `#[cfg(test)]`
//! modules only run when a test target includes the file like this.

#[allow(dead_code)]
#[path = "../codegen/parser/surface_rule.rs"]
mod surface_rule;
