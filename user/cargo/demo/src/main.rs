// Demo binary for the OCTAGRAM Cargo bootstrap driver.
//
// The driver only parses manifests and invokes rustc; this file exists so the
// guest probe has a real target to plan. It will only be compiled once a
// native (or host-provided) rustc is runnable, which is M4.3/M4.4 work.

fn main() {
    println!("demo-app running on OCTAGRAM");
}
