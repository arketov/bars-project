//! Исследовательский Windows-спайк; не вызывается воркером и не загружает модели.
use serde_json::{Value, json};
use std::{mem::size_of, time::Instant};
use windows::{
    Win32::{
        Graphics::{DXCore::*, Direct3D::D3D_FEATURE_LEVEL_11_0, Direct3D12::*, Dxgi::*},
        System::{LibraryLoader::*, SystemInformation::*},
    },
    core::{GUID, Interface, PCSTR, w},
};

// Runtime-agnostic GUID из Microsoft dxcore-adapter-attribute-guids;
// windows 0.62.2 ещё не содержит эти константы.
const GPU: GUID = GUID::from_u128(0xb69eb219_3ded_4464_979f_a00bd4687006);
const NPU: GUID = GUID::from_u128(0xd46140c4_add7_451b_9e56_06fe8c3b58ed);

fn error(e: impl std::fmt::Display) -> Value {
    json!({"status": "unknown", "error": e.to_string()})
}

fn ram() -> windows::core::Result<Value> {
    let mut m = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // Корректный размер и живой выходной буфер, без изменения состояния ОС.
    unsafe { GlobalMemoryStatusEx(&mut m)? };
    Ok(
        json!({"total_phys": m.ullTotalPhys, "available_phys": m.ullAvailPhys,
        "commit_limit": m.ullTotalPageFile, "commit_available": m.ullAvailPageFile}),
    )
}

fn core_bool(a: &IDXCoreAdapter, p: DXCoreAdapterProperty) -> Value {
    let mut value = false;
    // Только bool-свойства IsHardware/IsIntegrated; не произвольный тип буфера.
    match unsafe { a.GetProperty(p, size_of::<bool>(), (&mut value as *mut bool).cast()) } {
        Ok(()) => json!(value),
        Err(e) => error(e),
    }
}

fn core_driver(a: &IDXCoreAdapter) -> Value {
    let mut v = 0_u64;
    match unsafe { a.GetProperty(DriverVersion, size_of::<u64>(), (&mut v as *mut u64).cast()) } {
        Ok(()) => json!(format!(
            "{}.{}.{}.{}",
            v >> 48,
            (v >> 32) & 65535,
            (v >> 16) & 65535,
            v & 65535
        )),
        Err(e) => error(e),
    }
}

fn core_description(a: &IDXCoreAdapter) -> windows::core::Result<String> {
    let size = unsafe { a.GetPropertySize(DriverDescription)? };
    if size == 0 || size > 65536 {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            0x80070057_u32 as i32,
        )));
    }
    let mut bytes = vec![0_u8; size];
    unsafe { a.GetProperty(DriverDescription, size, bytes.as_mut_ptr().cast())? };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

fn core_list(factory: &IDXCoreAdapterFactory, attribute: GUID) -> windows::core::Result<Value> {
    let list: IDXCoreAdapterList = unsafe { factory.CreateAdapterList(&[attribute])? };
    let mut values = Vec::new();
    for i in 0..unsafe { list.GetAdapterCount() } {
        let a: IDXCoreAdapter = unsafe { list.GetAdapter(i)? };
        values.push(
            json!({"description": core_description(&a).map_or_else(error, |v| json!(v)),
            "hardware": core_bool(&a, IsHardware), "integrated": core_bool(&a, IsIntegrated),
            "driver_version": core_driver(&a)}),
        );
    }
    Ok(json!({"status": "enumerated", "adapters": values}))
}

fn budget(a: &IDXGIAdapter1, segment: DXGI_MEMORY_SEGMENT_GROUP) -> windows::core::Result<Value> {
    let a: IDXGIAdapter3 = a.cast()?;
    let mut m = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    unsafe { a.QueryVideoMemoryInfo(0, segment, &mut m)? };
    Ok(json!({"budget": m.Budget, "process_usage": m.CurrentUsage,
        "available_for_reservation": m.AvailableForReservation}))
}

fn uma(a: &IDXGIAdapter1) -> windows::core::Result<Value> {
    let mut device: Option<ID3D12Device> = None;
    unsafe { D3D12CreateDevice(a, D3D_FEATURE_LEVEL_11_0, &mut device)? };
    let device = device.ok_or_else(|| {
        windows::core::Error::from_hresult(windows::core::HRESULT(0x80004005_u32 as i32))
    })?;
    let mut arch = D3D12_FEATURE_DATA_ARCHITECTURE1::default();
    unsafe {
        device.CheckFeatureSupport(
            D3D12_FEATURE_ARCHITECTURE1,
            (&mut arch as *mut D3D12_FEATURE_DATA_ARCHITECTURE1).cast(),
            size_of_val(&arch) as u32,
        )?
    };
    Ok(json!({"uma": arch.UMA.as_bool(), "cache_coherent_uma": arch.CacheCoherentUMA.as_bool()}))
}

fn dxgi(core: Option<&IDXCoreAdapterFactory>) -> windows::core::Result<Value> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut values = Vec::new();
    for i in 0.. {
        let a = match unsafe { factory.EnumAdapters1(i) } {
            Ok(a) => a,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => return Err(e),
        };
        let d = unsafe { a.GetDesc1()? };
        let end = d
            .Description
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(d.Description.len());
        let software = d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0;
        let matching: Option<IDXCoreAdapter> =
            core.and_then(|f| unsafe { f.GetAdapterByLuid(&d.AdapterLuid).ok() });
        values.push(
            json!({"dxgi_index": i, "name": String::from_utf16_lossy(&d.Description[..end]),
            "vendor_id": d.VendorId, "device_id": d.DeviceId, "software": software,
            "dedicated_video": d.DedicatedVideoMemory, "dedicated_system": d.DedicatedSystemMemory,
            "shared_system_max": d.SharedSystemMemory,
            "integrated": matching.as_ref().map(|a| core_bool(a, IsIntegrated)),
            "driver_version": matching.as_ref().map(core_driver),
            "architecture": if software { json!(null) } else { uma(&a).unwrap_or_else(error) },
            "local": budget(&a, DXGI_MEMORY_SEGMENT_GROUP_LOCAL).unwrap_or_else(error),
            "non_local": budget(&a, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL).unwrap_or_else(error)}),
        );
    }
    Ok(json!(values))
}

fn cuda() -> windows::core::Result<Value> {
    // Только системный драйвер; не подхватывать DLL из cwd/PATH.
    let module = unsafe { LoadLibraryExW(w!("nvcuda.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)? };
    let result = (|| {
        let symbol = |name: &'static [u8]| {
            unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) }
                .ok_or_else(windows::core::Error::from_thread)
        };
        type Version = unsafe extern "system" fn(*mut i32) -> i32;
        type Init = unsafe extern "system" fn(u32) -> i32;
        // Экспорты CUDA Driver API имеют указанные C ABI и сигнатуры.
        let version: Version = unsafe { std::mem::transmute(symbol(b"cuDriverGetVersion\0")?) };
        let init: Init = unsafe { std::mem::transmute(symbol(b"cuInit\0")?) };
        let count: Version = unsafe { std::mem::transmute(symbol(b"cuDeviceGetCount\0")?) };
        let mut v = 0;
        let mut n = 0;
        let v_code = unsafe { version(&mut v) };
        let init_code = unsafe { init(0) };
        let count_code = if init_code == 0 {
            Some(unsafe { count(&mut n) })
        } else {
            None
        };
        Ok(
            json!({"driver_api_version": if v_code == 0 { Some(v) } else { None },
            "version_code": v_code, "init_code": init_code, "count_code": count_code,
            "device_count": if count_code == Some(0) { Some(n) } else { None },
            "model_backend": "not_tested"}),
        )
    })();
    // Function pointers не выходят за время жизни модуля.
    unsafe { windows::Win32::Foundation::FreeLibrary(module)? };
    result
}

fn timed(f: impl FnOnce() -> Value) -> Value {
    let start = Instant::now();
    let value = f();
    json!({"elapsed_us": start.elapsed().as_micros(), "value": value})
}

fn toolkits() -> Value {
    let mut roots = Vec::new();
    if let Some(path) = std::env::var_os("CUDA_PATH") {
        roots.push(std::path::PathBuf::from(path));
    }
    if let Some(base) = std::env::var_os("ProgramFiles") {
        let base = std::path::PathBuf::from(base).join("NVIDIA GPU Computing Toolkit/CUDA");
        if let Ok(entries) = std::fs::read_dir(base) {
            roots.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    roots.sort();
    roots.dedup();
    json!(
        roots
            .into_iter()
            .map(|root| {
                let manifest = std::fs::read(root.join("version.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
                let mut libraries = Vec::new();
                for dir in [root.join("bin"), root.join("bin/x64")] {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for e in entries.flatten() {
                            let name = e.file_name().to_string_lossy().into_owned();
                            if (name.starts_with("cudart64_")
                                || name.starts_with("cublas64_")
                                || name.starts_with("cublasLt64_"))
                                && name.ends_with(".dll")
                            {
                                libraries.push(name);
                            }
                        }
                    }
                }
                json!({"directory": root.file_name().map(|n| n.to_string_lossy().into_owned()),
            "manifest_cuda": manifest.as_ref().and_then(|m| m.get("cuda")),
            "library_filenames": libraries, "loadability": "not_tested"})
            })
            .collect::<Vec<_>>()
    )
}

fn main() {
    let start = Instant::now();
    let core_start = Instant::now();
    let core: windows::core::Result<IDXCoreAdapterFactory> =
        unsafe { DXCoreCreateAdapterFactory() };
    let core_init_us = core_start.elapsed().as_micros();
    let ram = timed(|| ram().unwrap_or_else(error));
    let dxgi = timed(|| dxgi(core.as_ref().ok()).unwrap_or_else(error));
    let core_lists = timed(|| match &core {
        Ok(f) => json!({"gpu": core_list(f, GPU).unwrap_or_else(error),
            "npu": core_list(f, NPU).unwrap_or_else(error)}),
        Err(e) => error(e),
    });
    let cuda = timed(|| cuda().unwrap_or_else(error));
    let toolkits = timed(toolkits);
    let output = json!({"schema": 1, "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "core_init_us": core_init_us, "ram": ram, "dxgi": dxgi, "dxcore": core_lists,
        "cuda": cuda, "toolkits": toolkits, "elapsed_us": start.elapsed().as_micros()});
    println!("{output}");
}
