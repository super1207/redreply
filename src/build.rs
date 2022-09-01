use std::env;
use std::process::Command;
use std::path::Path;
 
fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-search=native={}", out_dir);
}