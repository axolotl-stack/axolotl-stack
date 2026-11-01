//! Executable WASI stdout/stderr and panic coverage for the component boundary.

use unastar_api::{Handler, host};

struct Stdio;
unastar_api::export_handler!(Stdio);

impl Handler for Stdio {
    fn on_load() {
        println!("fixture stdout on-load");
        eprintln!("fixture stderr on-load");
        assert_ne!(host::configuration(), b"load-trap", "fixture load panic");
    }

    fn on_tick(_tick_id: u64) {
        println!("fixture stdout on-tick");
        eprintln!("fixture stderr on-tick");
        assert_ne!(host::configuration(), b"tick-trap", "fixture tick panic");
    }
}
