//! Изолированный Windows worker/IPC-стенд этапа 11.
#[cfg(windows)]
mod inference_worker;
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    inference_worker::main()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This experimental harness requires Windows.");
}
