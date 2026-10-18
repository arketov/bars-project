//! Драйвер P0: готовые векторы из snapshot → Rust kNN; без модели и зависимостей.
#[path = "../../crates/bars-core/src/graph/similarity.rs"]
mod similarity;

use similarity::{KnnOptions, VectorNode, build_knn};
use std::io::Read;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("knn <vectors.bin> <k> <min_cosine> <max_comparisons>".into());
    }
    let mut file = std::fs::File::open(&args[0])?;
    if file.metadata()?.len() > 64 * 1024 * 1024 {
        return Err("опыт ограничен входом до 64 МиБ".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() < 12 || &bytes[..4] != b"BGV1" || bytes.len() > 64 * 1024 * 1024 {
        return Err("неверный заголовок snapshot".into());
    }
    let n = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
    let dim = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
    if n > 20_000
        || dim == 0
        || dim > 65_536
        || n.checked_mul(dim)
            .and_then(|x| x.checked_mul(4))
            .and_then(|x| x.checked_add(12))
            != Some(bytes.len())
    {
        return Err("размер snapshot не совпадает с заголовком или превышает предел опыта".into());
    }
    let nodes: Vec<_> = bytes[12..]
        .chunks_exact(dim * 4)
        .enumerate()
        .map(|(i, blob)| VectorNode {
            id: format!("{i:08}"),
            vector: blob
                .chunks_exact(4)
                .map(|part| f32::from_le_bytes(part.try_into().unwrap()))
                .collect(),
        })
        .collect();
    let options = KnnOptions {
        k: args[1].parse()?,
        min_cosine: args[2].parse()?,
        max_comparisons: args[3].parse()?,
    };
    let start = Instant::now();
    let graph = build_knn(&nodes, options, || false)?;
    print!(
        "{{\"seconds\":{},\"comparisons\":{},\"total_nodes\":{},\"completed_nodes\":{},\"partial\":{},\"graph\":[",
        start.elapsed().as_secs_f64(),
        graph.comparisons,
        graph.total_nodes,
        graph.neighbors.len(),
        graph.stop_reason.is_some()
    );
    for (i, (source, neighbors)) in graph.neighbors.iter().enumerate() {
        if i > 0 {
            print!(",");
        }
        print!("{{\"source\":{},\"neighbors\":[", source.parse::<usize>()?);
        for (j, neighbor) in neighbors.iter().enumerate() {
            if j > 0 {
                print!(",");
            }
            print!("[{},{}]", neighbor.id.parse::<usize>()?, neighbor.cosine);
        }
        print!("]}}");
    }
    println!("]}}");
    Ok(())
}
