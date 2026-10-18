/// Safe execution metadata; DirectML identifies the session EP, not per-node placement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionInfo {
    pub backend: &'static str,
    pub actual: Option<&'static str>,
    pub fallback_reason: Option<String>,
}

pub(super) fn load_session<T, E: std::fmt::Display>(
    accelerator: super::Accelerator,
    load: impl Fn(bool) -> Result<T, E>,
) -> Result<(T, super::Device, Option<String>), E> {
    match accelerator {
        super::Accelerator::Cpu => Ok((load(false)?, super::Device::Cpu, None)),
        super::Accelerator::Auto => match load(true) {
            Ok(model) => Ok((model, super::Device::DirectMl, None)),
            Err(error) => {
                let reason = error.to_string();
                eprintln!("model: GPU unavailable ({reason}); using CPU");
                Ok((load(false)?, super::Device::Cpu, Some(reason)))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn fallback_records_cpu_and_reason_without_claiming_a_successful_gpu_session() {
        let calls = RefCell::new(Vec::new());
        let (_, device, reason) = load_session(super::super::Accelerator::Auto, |gpu| {
            calls.borrow_mut().push(gpu);
            if gpu {
                Err("injected GPU failure")
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(device, super::super::Device::Cpu);
        assert_eq!(reason.as_deref(), Some("injected GPU failure"));
        assert_eq!(*calls.borrow(), [true, false]);
        let failed: Result<((), _, _), _> =
            load_session(super::super::Accelerator::Auto, |_| Err("all failed"));
        assert!(failed.is_err());
    }
}
