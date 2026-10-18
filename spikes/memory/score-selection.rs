//! Изолированный замер хранения/выбора оценок, без SQLite, dot product и моделей.
#[path = "../../crates/bars-core/src/search/vector_top.rs"]
mod vector_top;
use std::{collections::HashMap, time::Instant};

#[repr(C)]
#[derive(Default)]
struct Counters { cb:u32, faults:u32, peak_ws:usize, ws:usize, peak_paged:usize, paged:usize, peak_nonpaged:usize, nonpaged:usize, pagefile:usize, peak_pagefile:usize, private:usize }
#[link(name="kernel32")]
unsafe extern "system" { fn GetCurrentProcess() -> *mut std::ffi::c_void; }
#[link(name="psapi")]
unsafe extern "system" { fn GetProcessMemoryInfo(p:*mut std::ffi::c_void,c:*mut Counters,size:u32)->i32; }
fn memory()->Counters {
    let mut c=Counters {cb:std::mem::size_of::<Counters>() as u32,..Default::default()};
    unsafe { assert_ne!(GetProcessMemoryInfo(GetCurrentProcess(),&mut c,std::mem::size_of::<Counters>() as u32),0); }
    c
}
fn main() {
    let args:Vec<_>=std::env::args().collect();
    let n:i64=args[2].parse().unwrap();
    assert!(n>=30 && matches!(args[1].as_str(),"full"|"bounded"));
    let baseline=memory().private;
    for repeat in 0..3 {
        let start=Instant::now();
        let scores:HashMap<i64,f32>=if args[1]=="full" {
            (0..n).map(|id|(id,((id*7919)%10007) as f32 /10007.)).collect()
        } else {
            let mut top=vector_top::VectorTop::new(30,(0..30).chain(n-30..n));
            for id in 0..n { top.push(id,((id*7919)%10007) as f32 /10007.); }
            top.finish()
        };
        let mut ordered:Vec<_>=scores.iter().map(|(&id,&score)|(id,score)).collect();
        ordered.sort_by(|a,b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let selected:Vec<_>=ordered.iter().take(30).map(|(id,_)|*id).collect();
        drop(ordered);
        let ms=start.elapsed().as_secs_f64()*1000.;
        let c=memory();
        println!("{{\"mode\":\"{}\",\"n\":{},\"repeat\":{},\"retained_scores\":{},\"ms\":{},\"baseline_private\":{},\"private\":{},\"lifetime_peak_commit\":{},\"selected\":{:?}}}",args[1],n,repeat,scores.len(),ms,baseline,c.private,c.peak_pagefile,selected);
        std::hint::black_box(scores);
    }
}
