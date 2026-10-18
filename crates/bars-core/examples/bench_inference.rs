//! Windows-стенд этапа 11; production API не расширяется.
#[cfg(windows)]
mod inference;
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    inference::main()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This experimental harness requires Windows.");
}
