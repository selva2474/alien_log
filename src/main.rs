//! `alien_log` CLI entry point.
//!
//! Phase 1 will grow `send` and `search` subcommands (roadmap P1.5). For now this is a
//! placeholder that proves the binary builds and links the library.

use alien_log::LogRecord;

fn main() {
    let sample = LogRecord::new(0, "alien_log: hello from Phase 0 skeleton");
    println!("{}", serde_json::to_string(&sample).unwrap());
    eprintln!(
        "alien_log {} — see docs/ROADMAP.md",
        env!("CARGO_PKG_VERSION")
    );
}
