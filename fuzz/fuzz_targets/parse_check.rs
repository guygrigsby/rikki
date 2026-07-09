#![no_main]
use libfuzzer_sys::fuzz_target;

// The no-panic invariant: garbage source may error, never panic. Front end
// only; executing fuzzed programs could loop forever, and the checker gate
// is what user source must pass anyway.
fuzz_target!(|src: &str| {
    if let Ok(prog) = rikki::parser::parse(src) {
        let _ = rikki::typecheck::check(&prog);
    }
});
