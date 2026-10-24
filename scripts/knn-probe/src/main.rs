//! Замер полного kNN близости чанков: перебор и HNSW на 2–50 тысячах чанков и
//! пересчёт после правки одного документа (docs/knowledge/chunk-similarity.md).
//!
//! Запуск: `knn-probe <результат.json> <index.sqlite> [<index.sqlite> ...]`.
//! Индексы открываются только на чтение, модель не нужна. Параметры среды:
//! `SCALES` (по умолчанию 2000,5000,10000,20000,50000), `THREADS` (1,4,8),
//! `EDITS` (20). Векторы больших размеров — синтетика на основе настоящих.

#[path = "../../../crates/bars-core/src/graph/similarity.rs"]
#[allow(dead_code)]
mod similarity;

use rusqlite::{Connection, OpenFlags};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

const K: usize = 5;
const TAU: f32 = 0.5;
const BLOCK: usize = 64;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn normal(&mut self) -> f32 {
        let u1 = self.unit().max(1e-7);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}

#[derive(Clone)]
struct Data {
    dim: usize,
    n: usize,
    v: Vec<f32>,
    doc: Vec<u32>,
}

impl Data {
    fn row(&self, i: usize) -> &[f32] {
        &self.v[i * self.dim..(i + 1) * self.dim]
    }
}

fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter_mut().for_each(|x| *x /= norm);
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0f32; 16];
    let (ca, cb) = (a.chunks_exact(16), b.chunks_exact(16));
    let tail: f32 = ca
        .remainder()
        .iter()
        .zip(cb.remainder())
        .map(|(x, y)| x * y)
        .sum();
    for (x, y) in ca.zip(cb) {
        for ((s, p), q) in acc.iter_mut().zip(x).zip(y) {
            *s += p * q;
        }
    }
    acc.iter().sum::<f32>() + tail
}

fn load(path: &str) -> Result<Data, Box<dyn std::error::Error>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = db.prepare(
        "SELECT c.document_id, v.vector FROM chunks c JOIN vectors v ON v.embed_hash=c.embed_hash \
         ORDER BY c.document_id, c.ordinal",
    )?;
    let mut data = Data {
        dim: 0,
        n: 0,
        v: Vec::new(),
        doc: Vec::new(),
    };
    let rows = statement.query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
    for row in rows {
        let (doc, blob) = row?;
        let mut vector: Vec<f32> = blob
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        data.dim = vector.len();
        normalize(&mut vector);
        data.v.extend(vector);
        data.doc.push(doc);
        data.n += 1;
    }
    Ok(data)
}

/// Синтетика: документ — центр из смеси двух настоящих чанков, чанк — центр с шумом.
fn synth(pool: &Data, n: usize, seed: u64) -> Data {
    let mut rng = Rng(seed);
    let dim = pool.dim;
    let sigma = 0.5 / (dim as f32).sqrt();
    let mut data = Data {
        dim,
        n: 0,
        v: Vec::with_capacity(n * dim),
        doc: Vec::new(),
    };
    let mut doc = 0u32;
    while data.n < n {
        let (a, b) = (pool.row(rng.below(pool.n)), pool.row(rng.below(pool.n)));
        let w = 0.55 + 0.45 * rng.unit();
        let mut center: Vec<f32> = a.iter().zip(b).map(|(x, y)| w * x + (1.0 - w) * y).collect();
        normalize(&mut center);
        for _ in 0..3 + rng.below(7) {
            if data.n == n {
                break;
            }
            let mut chunk: Vec<f32> = center.iter().map(|x| x + sigma * rng.normal()).collect();
            normalize(&mut chunk);
            data.v.extend(chunk);
            data.doc.push(doc);
            data.n += 1;
        }
        doc += 1;
    }
    data
}

#[derive(Clone, Copy)]
struct Top {
    len: usize,
    sim: [f32; K],
    idx: [u32; K],
}

fn better(s: f32, i: u32, s2: f32, i2: u32) -> bool {
    s > s2 || (s == s2 && i < i2)
}

impl Top {
    fn new() -> Self {
        Top {
            len: 0,
            sim: [0.0; K],
            idx: [0; K],
        }
    }
    fn push(&mut self, s: f32, i: u32) {
        if self.len == K && !better(s, i, self.sim[K - 1], self.idx[K - 1]) {
            return;
        }
        let mut p = self.len.min(K - 1);
        while p > 0 && better(s, i, self.sim[p - 1], self.idx[p - 1]) {
            self.sim[p] = self.sim[p - 1];
            self.idx[p] = self.idx[p - 1];
            p -= 1;
        }
        self.sim[p] = s;
        self.idx[p] = i;
        self.len = (self.len + 1).min(K);
    }
    fn ids(&self) -> &[u32] {
        &self.idx[..self.len]
    }
}

fn row_top(d: &Data, i: usize) -> Top {
    let mut top = Top::new();
    let vi = d.row(i);
    for j in (0..d.n).filter(|&j| j != i) {
        let s = dot(vi, d.row(j));
        if s >= TAU {
            top.push(s, j as u32);
        }
    }
    top
}

fn triangle_block(d: &Data, bi: usize, lists: &mut [Top]) {
    let blocks = d.n.div_ceil(BLOCK);
    let (i0, i1) = (bi * BLOCK, ((bi + 1) * BLOCK).min(d.n));
    for bj in bi..blocks {
        let (j0, j1) = (bj * BLOCK, ((bj + 1) * BLOCK).min(d.n));
        for i in i0..i1 {
            let vi = d.row(i);
            for j in j0.max(i + 1)..j1 {
                let s = dot(vi, d.row(j));
                if s >= TAU {
                    lists[i].push(s, j as u32);
                    lists[j].push(s, i as u32);
                }
            }
        }
    }
}

/// Точный kNN: каждая пара считается один раз, строки собираются из частичных списков потоков.
fn triangle(d: &Data, threads: usize) -> Vec<Top> {
    let blocks = d.n.div_ceil(BLOCK);
    let next = AtomicUsize::new(0);
    let parts: Vec<Vec<Top>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut lists = vec![Top::new(); d.n];
                    loop {
                        let bi = next.fetch_add(1, Relaxed);
                        if bi >= blocks {
                            break lists;
                        }
                        triangle_block(d, bi, &mut lists);
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut parts = parts.into_iter();
    let mut merged = parts.next().unwrap();
    for part in parts {
        for (target, source) in merged.iter_mut().zip(&part) {
            for (s, i) in source.sim.iter().zip(&source.idx).take(source.len) {
                target.push(*s, *i);
            }
        }
    }
    merged
}

fn same_lists(a: &[Top], b: &[Top]) -> bool {
    a.iter().zip(b).all(|(x, y)| {
        x.len == y.len && x.ids() == y.ids() && x.sim[..x.len] == y.sim[..y.len]
    })
}

/// Правка документа: новые векторы изменённых чанков, обновление списков без полного расчёта.
/// Возвращает число строк чужих чанков, у которых в списке был изменённый чанк.
fn apply_edit(d: &mut Data, lists: &mut [Top], changed: &[usize], fresh: &[Vec<f32>]) -> usize {
    let dim = d.dim;
    let mut changed_mask = vec![false; d.n];
    for (&c, vector) in changed.iter().zip(fresh) {
        d.v[c * dim..(c + 1) * dim].copy_from_slice(vector);
        changed_mask[c] = true;
    }
    let affected: Vec<usize> = (0..d.n)
        .filter(|&j| {
            !changed_mask[j] && lists[j].ids().iter().any(|&x| changed_mask[x as usize])
        })
        .collect();
    let mut recompute = vec![false; d.n];
    for &a in &affected {
        recompute[a] = true;
    }
    for &c in changed {
        lists[c] = Top::new();
        let vc = d.row(c).to_vec();
        for j in (0..d.n).filter(|&j| j != c) {
            let s = dot(&vc, d.row(j));
            if s >= TAU {
                lists[c].push(s, j as u32);
                if !changed_mask[j] && !recompute[j] {
                    lists[j].push(s, c as u32);
                }
            }
        }
    }
    for &a in &affected {
        lists[a] = row_top(d, a);
    }
    affected.len()
}

fn edited_vector(d: &Data, c: usize, rng: &mut Rng) -> Vec<f32> {
    let other = d.row(rng.below(d.n));
    let mut v: Vec<f32> = d
        .row(c)
        .iter()
        .zip(other)
        .map(|(x, y)| 0.85 * x + 0.15 * y)
        .collect();
    normalize(&mut v);
    v
}

fn doc_range(d: &Data, c: usize) -> Vec<usize> {
    (0..d.n).filter(|&i| d.doc[i] == d.doc[c]).collect()
}

#[derive(Clone, Copy, PartialEq)]
struct Sim(f32, u32);
impl Eq for Sim {}
impl PartialOrd for Sim {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Sim {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0).then(other.1.cmp(&self.1))
    }
}

struct Scratch {
    stamp: Vec<u32>,
    cur: u32,
}

impl Scratch {
    fn new(n: usize) -> Self {
        Scratch {
            stamp: vec![0; n],
            cur: 0,
        }
    }
    fn begin(&mut self, n: usize) {
        if self.stamp.len() < n {
            self.stamp.resize(n, 0);
        }
        self.cur += 1;
    }
    fn seen(&mut self, i: usize) -> bool {
        let was = self.stamp[i] == self.cur;
        self.stamp[i] = self.cur;
        was
    }
}

struct Hnsw {
    dim: usize,
    m: usize,
    m0: usize,
    ef_c: usize,
    links: Vec<Vec<Vec<u32>>>,
    deleted: Vec<bool>,
    entry: usize,
    top: usize,
}

impl Hnsw {
    fn new(dim: usize, m: usize, ef_c: usize) -> Self {
        Hnsw {
            dim,
            m,
            m0: 2 * m,
            ef_c,
            links: Vec::new(),
            deleted: Vec::new(),
            entry: 0,
            top: 0,
        }
    }

    fn vec<'a>(&self, vecs: &'a [f32], i: usize) -> &'a [f32] {
        &vecs[i * self.dim..(i + 1) * self.dim]
    }

    fn search_layer(
        &self,
        vecs: &[f32],
        q: &[f32],
        eps: &[usize],
        ef: usize,
        level: usize,
        scratch: &mut Scratch,
    ) -> Vec<Sim> {
        scratch.begin(self.links.len());
        let mut cand = BinaryHeap::new();
        let mut res: BinaryHeap<Reverse<Sim>> = BinaryHeap::new();
        for &e in eps {
            scratch.seen(e);
            let s = Sim(dot(q, self.vec(vecs, e)), e as u32);
            cand.push(s);
            res.push(Reverse(s));
        }
        while let Some(c) = cand.pop() {
            if res.len() >= ef && c.0 < res.peek().unwrap().0.0 {
                break;
            }
            for &nb in &self.links[c.1 as usize][level] {
                if scratch.seen(nb as usize) {
                    continue;
                }
                let s = Sim(dot(q, self.vec(vecs, nb as usize)), nb);
                if res.len() < ef || s.0 > res.peek().unwrap().0.0 {
                    cand.push(s);
                    res.push(Reverse(s));
                    if res.len() > ef {
                        res.pop();
                    }
                }
            }
        }
        res.into_sorted_vec().into_iter().map(|r| r.0).collect()
    }

    fn select(&self, vecs: &[f32], cands: &[Sim], m: usize) -> Vec<u32> {
        let mut chosen: Vec<Sim> = Vec::new();
        let mut rest = Vec::new();
        for &c in cands {
            if chosen.len() >= m {
                break;
            }
            let vc = self.vec(vecs, c.1 as usize);
            if chosen
                .iter()
                .all(|s| dot(vc, self.vec(vecs, s.1 as usize)) < c.0)
            {
                chosen.push(c);
            } else {
                rest.push(c);
            }
        }
        for c in rest {
            if chosen.len() >= m {
                break;
            }
            chosen.push(c);
        }
        chosen.into_iter().map(|s| s.1).collect()
    }

    fn insert(&mut self, vecs: &[f32], id: usize, rng: &mut Rng, scratch: &mut Scratch) {
        let ml = 1.0 / (self.m as f64).ln();
        let level = (-(f64::from(rng.unit().max(1e-7))).ln() * ml) as usize;
        self.links.push(vec![Vec::new(); level + 1]);
        self.deleted.push(false);
        if id == 0 {
            self.entry = 0;
            self.top = level;
            return;
        }
        let q = self.vec(vecs, id);
        let mut eps = vec![self.entry];
        for l in (level + 1..=self.top).rev() {
            eps = vec![self.search_layer(vecs, q, &eps, 1, l, scratch)[0].1 as usize];
        }
        for l in (0..=level.min(self.top)).rev() {
            let cands = self.search_layer(vecs, q, &eps, self.ef_c, l, scratch);
            let cap = if l == 0 { self.m0 } else { self.m };
            let chosen = self.select(vecs, &cands, self.m);
            for &nb in &chosen {
                self.links[nb as usize][l].push(id as u32);
                if self.links[nb as usize][l].len() > cap {
                    let vn = self.vec(vecs, nb as usize);
                    let mut list: Vec<Sim> = self.links[nb as usize][l]
                        .iter()
                        .map(|&x| Sim(dot(vn, self.vec(vecs, x as usize)), x))
                        .collect();
                    list.sort_by(|a, b| b.cmp(a));
                    self.links[nb as usize][l] = self.select(vecs, &list, cap);
                }
            }
            self.links[id][l] = chosen;
            eps = cands.iter().map(|s| s.1 as usize).collect();
        }
        if level > self.top {
            self.entry = id;
            self.top = level;
        }
    }

    /// Соседи узла `q` среди живых узлов с cosine не ниже TAU.
    fn query(&self, vecs: &[f32], q: &[f32], skip: usize, ef: usize, scratch: &mut Scratch) -> Top {
        let mut eps = vec![self.entry];
        for l in (1..=self.top).rev() {
            eps = vec![self.search_layer(vecs, q, &eps, 1, l, scratch)[0].1 as usize];
        }
        let found = self.search_layer(vecs, q, &eps, ef.max(K + 1), 0, scratch);
        let mut top = Top::new();
        for s in found.into_iter().rev() {
            if s.1 as usize != skip && !self.deleted[s.1 as usize] && s.0 >= TAU {
                top.push(s.0, s.1);
            }
        }
        top
    }

    fn link_count(&self) -> usize {
        self.links.iter().flatten().map(Vec::len).sum()
    }
}

fn build_hnsw(vecs: &[f32], dim: usize, n: usize, m: usize, ef_c: usize) -> Hnsw {
    let mut index = Hnsw::new(dim, m, ef_c);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut scratch = Scratch::new(n);
    for id in 0..n {
        index.insert(vecs, id, &mut rng, &mut scratch);
    }
    index
}

fn query_all(index: &Hnsw, vecs: &[f32], n: usize, ef: usize, threads: usize) -> Vec<Top> {
    let next = AtomicUsize::new(0);
    let parts: Vec<Vec<(usize, Top)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut scratch = Scratch::new(index.links.len());
                    let mut out = Vec::new();
                    loop {
                        let start = next.fetch_add(256, Relaxed);
                        if start >= n {
                            break out;
                        }
                        for i in start..(start + 256).min(n) {
                            let q = &vecs[i * index.dim..(i + 1) * index.dim];
                            out.push((i, index.query(vecs, q, i, ef, &mut scratch)));
                        }
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut lists = vec![Top::new(); n];
    for (i, top) in parts.into_iter().flatten() {
        lists[i] = top;
    }
    lists
}

/// Доля точных соседей, найденных приближённым списком.
fn recall(exact: &[Top], approx: &[Top], rows: &[usize]) -> f64 {
    let (mut hit, mut all) = (0usize, 0usize);
    for &i in rows {
        all += exact[i].len;
        hit += exact[i]
            .ids()
            .iter()
            .filter(|x| approx[i].ids().contains(x))
            .count();
    }
    if all == 0 { 1.0 } else { hit as f64 / all as f64 }
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn stats(values: &mut [f64]) -> serde_json::Value {
    values.sort_by(|a, b| a.total_cmp(b));
    let n = values.len();
    let mean = values.iter().sum::<f64>() / n.max(1) as f64;
    serde_json::json!({
        "mean": mean, "median": values[n / 2],
        "p95": values[((n * 95).div_ceil(100)).min(n) - 1], "max": values[n - 1],
    })
}

fn env_list(name: &str, default: &str) -> Vec<usize> {
    std::env::var(name)
        .unwrap_or_else(|_| default.into())
        .split(',')
        .map(|x| x.trim().parse().unwrap())
        .collect()
}

fn describe(d: &Data, lists: &[Top]) -> serde_json::Value {
    let n = d.n as f64;
    let top1 = lists.iter().filter(|t| t.len > 0).map(|t| f64::from(t.sim[0]));
    let with_neighbors = lists.iter().filter(|t| t.len > 0).count();
    let full = lists.iter().filter(|t| t.len == K).count();
    let mut indegree = vec![0usize; d.n];
    for t in lists {
        for &i in t.ids() {
            indegree[i as usize] += 1;
        }
    }
    serde_json::json!({
        "mean_top1_cosine": top1.sum::<f64>() / with_neighbors.max(1) as f64,
        "rows_with_neighbors": with_neighbors as f64 / n,
        "rows_with_full_k": full as f64 / n,
        "mean_indegree": indegree.iter().sum::<usize>() as f64 / n,
        "max_indegree": indegree.iter().max(),
        "chunks_per_doc": d.n as f64 / (d.doc.iter().collect::<std::collections::BTreeSet<_>>().len() as f64),
    })
}

fn lookup_cost(n: usize) -> serde_json::Value {
    let mut rng = Rng(7);
    let ids: Vec<String> = (0..n).map(|_| format!("{:016x}{:016x}", rng.next(), rng.next())).collect();
    let edges = n * K;
    let sample = 1000.min(edges);
    let started = Instant::now();
    let mut found = 0usize;
    for _ in 0..sample {
        let target = &ids[rng.below(n)];
        found += ids.iter().position(|x| x == target).unwrap_or(0);
    }
    let per_find_ms = ms(started) / sample as f64;
    std::hint::black_box(found);
    let started = Instant::now();
    let map: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, x)| (x.as_str(), i)).collect();
    let mut hits = 0usize;
    for _ in 0..edges * 2 {
        hits += map[ids[rng.below(n)].as_str()];
    }
    std::hint::black_box(hits);
    serde_json::json!({
        "edges": edges,
        "linear_find_ms_per_lookup": per_find_ms,
        "linear_total_ms_estimate": per_find_ms * (edges * 2) as f64,
        "hashmap_total_ms": ms(started),
    })
}

fn run(name: &str, base: &Data, pool_for_edits: &Data, threads: &[usize], edits: usize) -> serde_json::Value {
    let _ = pool_for_edits;
    let n = base.n;
    println!("== {name}: {n} чанков, dim {}", base.dim);
    let mut result = serde_json::Map::new();
    result.insert("chunks".into(), n.into());
    result.insert("dim".into(), base.dim.into());

    let mut brute = serde_json::Map::new();
    let mut exact = Vec::new();
    for &t in threads {
        if t == 1 && n > 20_000 {
            continue;
        }
        let started = Instant::now();
        let lists = triangle(base, t);
        let took = ms(started);
        println!("  перебор (пара один раз), {t} пот.: {took:.0} мс");
        brute.insert(format!("triangle_threads_{t}_ms"), took.into());
        exact = lists;
    }
    result.insert("brute".into(), brute.into());
    result.insert("structure".into(), describe(base, &exact));

    if n <= 5000 {
        let nodes: Vec<similarity::VectorNode> = (0..n)
            .map(|i| similarity::VectorNode {
                id: format!("{i:06}"),
                vector: base.row(i).to_vec(),
            })
            .collect();
        let started = Instant::now();
        let graph = similarity::build_knn(
            &nodes,
            similarity::KnnOptions {
                k: K,
                min_cosine: TAU,
                max_comparisons: u64::MAX,
            },
            || false,
        )
        .unwrap();
        let took = ms(started);
        println!("  текущий build_knn (без предела): {took:.0} мс, сравнений {}", graph.comparisons);
        result.insert("current_build_knn_ms".into(), took.into());
        let agree = graph.neighbors.iter().all(|(id, list)| {
            let i: usize = id.parse().unwrap();
            let ids: Vec<u32> = list.iter().map(|x| x.id.parse::<u32>().unwrap()).collect();
            ids == exact[i].ids()
        });
        result.insert("current_build_knn_matches_triangle".into(), agree.into());
    }

    let tmax = *threads.iter().max().unwrap();
    let mut hn = Vec::new();
    for (m, ef_c) in [(16usize, 100usize)] {
        let started = Instant::now();
        let index = build_hnsw(&base.v, base.dim, n, m, ef_c);
        let build = ms(started);
        println!("  HNSW M={m} efC={ef_c}: построение {build:.0} мс");
        let links_mb = (index.link_count() * 4) as f64 / 1e6;
        for ef in [32usize, 64, 128] {
            let started = Instant::now();
            let lists = query_all(&index, &base.v, n, ef, tmax);
            let all_ms = ms(started);
            let sample = 500.min(n);
            let started = Instant::now();
            let one = query_all(&index, &base.v, sample, ef, 1);
            let per_query = ms(started) / sample as f64;
            std::hint::black_box(&one);
            let rows: Vec<usize> = (0..n).collect();
            let rec = recall(&exact, &lists, &rows);
            println!("    efS={ef}: все строки {tmax} пот. {all_ms:.0} мс, {per_query:.2} мс на запрос (1 пот.), полнота {rec:.4}");
            hn.push(serde_json::json!({
                "m": m, "ef_construction": ef_c, "ef_search": ef,
                "build_ms": build, "query_all_threads": tmax, "query_all_ms": all_ms,
                "query_ms_single_thread": per_query, "recall": rec, "links_mb": links_mb,
            }));
        }
    }
    result.insert("hnsw".into(), hn.into());
    result.insert(
        "memory_mb".into(),
        serde_json::json!({"vectors": (n * base.dim * 4) as f64 / 1e6}),
    );

    // Правки документов: точное обновление списков.
    let mut d = base.clone();
    let mut lists = exact.clone();
    let mut rng = Rng(42);
    let (mut times, mut affected_counts, mut sizes) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..edits {
        let range = doc_range(&d, rng.below(d.n));
        let fresh: Vec<Vec<f32>> = range.iter().map(|&c| edited_vector(&d, c, &mut rng)).collect();
        let started = Instant::now();
        let affected = apply_edit(&mut d, &mut lists, &range, &fresh);
        times.push(ms(started));
        affected_counts.push(affected as f64);
        sizes.push(range.len() as f64);
    }
    let check = if n <= 20_000 {
        let full = triangle(&d, tmax);
        Some(same_lists(&lists, &full))
    } else {
        None
    };
    println!(
        "  правка документа: {:.0} мс в среднем, затронуто строк {:.1} в среднем, совпадает с полным пересчётом: {check:?}",
        times.iter().sum::<f64>() / times.len() as f64,
        affected_counts.iter().sum::<f64>() / affected_counts.len() as f64
    );
    result.insert(
        "edit_exact".into(),
        serde_json::json!({
            "edits": edits, "ms": stats(&mut times), "affected_rows": stats(&mut affected_counts),
            "chunks_per_edit": stats(&mut sizes), "matches_full_recompute": check,
        }),
    );

    // Правки документов в HNSW: вставка новых и пометка старых удалёнными.
    let mut hv = base.v.clone();
    let mut index = build_hnsw(&hv, base.dim, n, 16, 100);
    let mut live: Vec<usize> = (0..n).collect();
    let mut scratch = Scratch::new(n + 1);
    let mut rng = Rng(43);
    let mut insert_ms = Vec::new();
    let mut hrng = Rng(99);
    let mut doc_of: Vec<u32> = base.doc.clone();
    for _ in 0..edits {
        let c = live[rng.below(live.len())];
        let old: Vec<usize> = live.iter().copied().filter(|&i| doc_of[i] == doc_of[c]).collect();
        let started = Instant::now();
        let mut ids = Vec::new();
        for &o in &old {
            let other = rng.below(hv.len() / base.dim);
            let mut v: Vec<f32> = hv[o * base.dim..(o + 1) * base.dim]
                .iter()
                .zip(&hv[other * base.dim..(other + 1) * base.dim])
                .map(|(x, y)| 0.85 * x + 0.15 * y)
                .collect();
            normalize(&mut v);
            hv.extend(v);
            let id = hv.len() / base.dim - 1;
            index.insert(&hv, id, &mut hrng, &mut scratch);
            doc_of.push(doc_of[o]);
            ids.push(id);
        }
        for &o in &old {
            index.deleted[o] = true;
        }
        live.retain(|i| !old.contains(i));
        live.extend(ids);
        insert_ms.push(ms(started));
    }
    let sample: Vec<usize> = (0..300).map(|_| live[rng.below(live.len())]).collect();
    let mut hits = 0usize;
    let mut total = 0usize;
    for &i in &sample {
        let q = &hv[i * base.dim..(i + 1) * base.dim];
        let mut exact_top = Top::new();
        for &j in live.iter().filter(|&&j| j != i) {
            let s = dot(q, &hv[j * base.dim..(j + 1) * base.dim]);
            if s >= TAU {
                exact_top.push(s, j as u32);
            }
        }
        let approx = index.query(&hv, q, i, 64, &mut scratch);
        total += exact_top.len;
        hits += exact_top.ids().iter().filter(|x| approx.ids().contains(x)).count();
    }
    let rec = if total == 0 { 1.0 } else { hits as f64 / total as f64 };
    println!(
        "  HNSW после {edits} правок: вставка документа {:.1} мс в среднем, полнота {rec:.4} (живых {} из {} узлов)",
        insert_ms.iter().sum::<f64>() / insert_ms.len() as f64,
        live.len(),
        hv.len() / base.dim
    );
    result.insert(
        "hnsw_edits".into(),
        serde_json::json!({
            "edits": edits, "insert_document_ms": stats(&mut insert_ms),
            "recall_after_edits": rec, "live": live.len(), "nodes_total": hv.len() / base.dim,
        }),
    );
    result.insert("lookup_cost".into(), lookup_cost(n));
    result.into()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("knn-probe <результат.json> <index.sqlite>...".into());
    }
    let scales = env_list("SCALES", "2000,5000,10000,20000,50000");
    let threads = env_list("THREADS", "1,4,8");
    let edits = env_list("EDITS", "20")[0];
    let mut report = serde_json::Map::new();
    let mut pool = Data {
        dim: 0,
        n: 0,
        v: Vec::new(),
        doc: Vec::new(),
    };
    for path in &args[2..] {
        let real = load(path)?;
        let label = format!("real:{}", path.replace('\\', "/"));
        report.insert(label.clone(), run(&label, &real, &real, &threads, edits));
        pool.dim = real.dim;
        pool.n += real.n;
        pool.v.extend(&real.v);
        pool.doc.extend(&real.doc);
    }
    for &n in &scales {
        let data = synth(&pool, n, 2026 + n as u64);
        report.insert(format!("synthetic:{n}"), run(&format!("synthetic:{n}"), &data, &pool, &threads, edits));
        std::fs::write(&args[1], serde_json::to_vec_pretty(&report)?)?;
    }
    std::fs::write(&args[1], serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
