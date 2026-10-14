//! Спайк поиска bars: нарезка, индекс SQLite (FTS5 + векторы BLOB), гибридный поиск, реранк.
//! Выбрасываемый код. Команды:
//!   index <docs_dir> <db>
//!   query <db> <queries.txt> [--rerank]
//!   bench <db> <N>[,<N>...]

use anyhow::{bail, Context, Result};
use fastembed::{
    EmbeddingModel, RerankInitOptions, RerankerModel, TextEmbedding, TextInitOptions, TextRerank,
};
use rusqlite::{params, Connection};
use rust_stemmers::{Algorithm, Stemmer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MIN_CHARS: usize = 250;
const MAX_CHARS: usize = 6000;
const CANDIDATES: usize = 30;

fn main() -> Result<()> {
    // HF_HOME перекрывает cache_dir fastembed (docs/knowledge/stack-research.md).
    std::env::remove_var("HF_HOME");
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("index") => index(Path::new(&args[2]), Path::new(&args[3])),
        Some("query") => query(Path::new(&args[2]), Path::new(&args[3]), args.iter().any(|a| a == "--rerank")),
        Some("bench") => bench(Path::new(&args[2]), &args[3]),
        Some("rerank-bench") => rerank_bench(Path::new(&args[2]), args[3].parse()?, args[4].parse()?, &args[5]),
        Some("embed-compare") => embed_compare(Path::new(&args[2])),
        Some("embed-bench") => embed_bench(Path::new(&args[2]), &args[3], args[4].parse()?, args[5].parse()?, &args[6]),
        _ => bail!("usage: index <docs> <db> | query <db> <queries.txt> [--rerank] | bench <db> <N,...>"),
    }
}

fn models_dir() -> PathBuf {
    dirs::home_dir().unwrap().join(".bars").join("models")
}

fn embedder() -> Result<TextEmbedding> {
    let t = Instant::now();
    let m = TextEmbedding::try_new(
        TextInitOptions::new(EmbeddingModel::BGEM3)
            .with_cache_dir(models_dir())
            .with_max_length(8192)
            .with_show_download_progress(true),
    )?;
    eprintln!("embed model loaded in {:.1}s", t.elapsed().as_secs_f64());
    Ok(m)
}

// ---------- нарезка (упрощённо по docs/indexing/chunking.md) ----------

struct Section {
    level: usize,
    title: String,
    line: usize,
    end: usize,
    body: Vec<String>,
    children: Vec<usize>,
    chain: String,
}

struct Chunk {
    path: String,
    module: String,
    title: String,
    chain: String,
    line_start: usize,
    line_end: usize,
    text: String,
}

fn heading(line: &str) -> Option<(usize, String)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &t[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim().to_string();
    Some((hashes, text))
}

fn fence(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|&x| x == c).count();
    (n >= 3).then_some((c, n))
}

fn parse_doc(rel: &str, module: &str, src: &str) -> Vec<Chunk> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let lines: Vec<&str> = src.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let mut start = 0;
    let mut title = Path::new(rel).file_stem().unwrap().to_string_lossy().to_string();
    if lines.first() == Some(&"---") {
        if let Some(end) = lines.iter().skip(1).position(|l| *l == "---") {
            for l in &lines[1..=end] {
                if let Some(v) = l.strip_prefix("title:") {
                    title = v.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }
            start = end + 2;
        }
    }
    let mut secs: Vec<Section> = vec![Section {
        level: 0,
        title: title.clone(),
        line: start + 1,
        end: start + 1,
        body: vec![],
        children: vec![],
        chain: title.clone(),
    }];
    let mut stack: Vec<usize> = vec![0];
    let mut in_fence: Option<(char, usize)> = None;
    for (i, l) in lines.iter().enumerate().skip(start) {
        let ln = i + 1;
        if let Some((c, n)) = in_fence {
            if let Some((c2, n2)) = fence(l) {
                if c2 == c && n2 >= n {
                    in_fence = None;
                }
            }
        } else if let Some(f) = fence(l) {
            in_fence = Some(f);
        } else if let Some((lvl, text)) = heading(l) {
            while stack.len() > 1 && secs[*stack.last().unwrap()].level >= lvl {
                stack.pop();
            }
            let parent = *stack.last().unwrap();
            let chain = if parent == 0 { text.clone() } else { format!("{}/{}", secs[parent].chain, text) };
            let id = secs.len();
            secs.push(Section { level: lvl, title: text, line: ln, end: ln, body: vec![], children: vec![], chain });
            if parent != 0 {
                secs[parent].children.push(id);
            }
            stack.push(id);
            continue;
        }
        let cur = *stack.last().unwrap();
        secs[cur].body.push(l.to_string());
        secs[cur].end = ln;
    }
    // собственный текст раздела + other headers
    let mut raw: Vec<(String, usize, usize, String)> = vec![]; // chain, start, end, text
    for s in &secs {
        let mut text = s.body.join("\n").trim().to_string();
        if !s.children.is_empty() {
            text.push_str("\nother headers:");
            for &c in &s.children {
                text.push_str(&format!("\n{} {}", "#".repeat(secs[c].level), secs[c].title));
            }
        }
        if s.level == 0 && text.is_empty() {
            continue;
        }
        let text = if s.level > 0 && text.is_empty() { String::new() } else { text };
        raw.push((s.chain.clone(), s.line, s.end, text));
        let _ = s.title.len();
    }
    // склейка маленьких со следующим
    let mut merged: Vec<(String, usize, usize, String)> = vec![];
    let mut i = 0;
    while i < raw.len() {
        let mut acc = raw[i].clone();
        while acc.3.chars().count() < MIN_CHARS && i + 1 < raw.len() {
            let next = &raw[i + 1];
            let head = next.0.rsplit('/').next().unwrap_or("");
            let add = format!("\n## {}\n{}", head, next.3);
            if acc.3.chars().count() + add.chars().count() > MAX_CHARS {
                break;
            }
            acc.3.push_str(&add);
            acc.2 = next.2;
            i += 1;
        }
        merged.push(acc);
        i += 1;
    }
    if merged.len() > 1 && merged.last().unwrap().3.chars().count() < MIN_CHARS {
        let last = merged.pop().unwrap();
        let prev = merged.last_mut().unwrap();
        let head = last.0.rsplit('/').next().unwrap_or("");
        prev.3.push_str(&format!("\n## {}\n{}", head, last.3));
        prev.2 = last.2;
    }
    // дорезка больших по абзацам
    let mut out = vec![];
    for (chain, ls, le, text) in merged {
        for part in split_big(&text) {
            out.push(Chunk {
                path: rel.to_string(),
                module: module.to_string(),
                title: title.clone(),
                chain: chain.clone(),
                line_start: ls,
                line_end: le,
                text: part,
            });
        }
    }
    out
}

fn split_big(text: &str) -> Vec<String> {
    if text.chars().count() <= MAX_CHARS {
        return vec![text.to_string()];
    }
    let mut parts = vec![];
    let mut cur = String::new();
    let pieces: Vec<String> = text
        .split("\n\n")
        .flat_map(|p| {
            if p.chars().count() <= MAX_CHARS {
                vec![p.to_string()]
            } else {
                p.split('\n')
                    .flat_map(|l| {
                        let cs: Vec<char> = l.chars().collect();
                        cs.chunks(MAX_CHARS).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>()
                    })
                    .collect()
            }
        })
        .collect();
    for p in pieces {
        if !cur.is_empty() && cur.chars().count() + p.chars().count() + 2 > MAX_CHARS {
            parts.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(&p);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

fn embed_text(c: &Chunk) -> String {
    format!("path: {}\n{}\n{}", c.path, c.chain, c.text)
}

// ---------- стемминг ----------

struct Stem {
    ru: Stemmer,
    en: Stemmer,
}

impl Stem {
    fn new() -> Self {
        Stem { ru: Stemmer::create(Algorithm::Russian), en: Stemmer::create(Algorithm::English) }
    }
    fn words(s: &str) -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect()
    }
    fn word(&self, w: &str) -> String {
        if w.chars().any(|c| ('а'..='я').contains(&c) || c == 'ё') {
            self.ru.stem(w).into_owned()
        } else {
            self.en.stem(w).into_owned()
        }
    }
    fn text(&self, s: &str) -> String {
        Self::words(s).iter().map(|w| self.word(w)).collect::<Vec<_>>().join(" ")
    }
}

// ---------- индекс ----------

fn open(db: &Path) -> Result<Connection> {
    let c = Connection::open(db)?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    Ok(c)
}

fn schema(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS chunks(id INTEGER PRIMARY KEY, path TEXT, module TEXT, title TEXT, chain TEXT,
            line_start INT, line_end INT, text TEXT, hash TEXT);
         CREATE TABLE IF NOT EXISTS vectors(hash TEXT PRIMARY KEY, v BLOB);
         CREATE VIRTUAL TABLE IF NOT EXISTS fts_stem USING fts5(chain, body, tokenize='unicode61 remove_diacritics 2');
         CREATE VIRTUAL TABLE IF NOT EXISTS fts_raw USING fts5(chain, body, tokenize='unicode61 remove_diacritics 2');",
    )?;
    Ok(())
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}
fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}
fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn index(docs: &Path, db: &Path) -> Result<()> {
    if db.exists() {
        std::fs::remove_file(db)?;
    }
    let t = Instant::now();
    let mut chunks = vec![];
    let mut files = 0;
    for e in walkdir::WalkDir::new(docs).follow_links(false).into_iter().filter_entry(|e| {
        e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')
    }) {
        let e = e?;
        if !e.file_type().is_file() || e.path().extension().map_or(true, |x| x != "md") {
            continue;
        }
        let rel = e.path().strip_prefix(docs)?.to_string_lossy().replace('\\', "/");
        if rel == "BARS_INSTRUCTION.md" {
            continue;
        }
        let module = if rel.contains('/') { rel.split('/').next().unwrap().to_string() } else { ".".into() };
        let src = std::fs::read_to_string(e.path()).with_context(|| rel.clone())?;
        chunks.extend(parse_doc(&rel, &module, &src));
        files += 1;
    }
    let chars: usize = chunks.iter().map(|c| c.text.chars().count()).sum();
    let maxc = chunks.iter().map(|c| c.text.chars().count()).max().unwrap_or(0);
    eprintln!("{files} files -> {} chunks, {chars} chars (max chunk {maxc}), parse {:.2}s", chunks.len(), t.elapsed().as_secs_f64());

    let mut model = embedder()?;
    let texts: Vec<String> = chunks.iter().map(embed_text).collect();
    let mut order: Vec<usize> = (0..texts.len()).collect();
    order.sort_by_key(|&i| texts[i].len());
    let mut vecs: Vec<Vec<f32>> = vec![vec![]; texts.len()];
    let t = Instant::now();
    for batch in order.chunks(16) {
        let b: Vec<&str> = batch.iter().map(|&i| texts[i].as_str()).collect();
        let out = model.embed(&b, Some(16))?;
        for (&i, mut v) in batch.iter().zip(out) {
            normalize(&mut v);
            vecs[i] = v;
        }
    }
    let et = t.elapsed().as_secs_f64();
    eprintln!("embedded {} chunks in {et:.1}s ({:.0} ms/chunk), dim {}", texts.len(), et * 1000.0 / texts.len() as f64, vecs[0].len());

    let stem = Stem::new();
    let mut c = open(db)?;
    schema(&c)?;
    let tx = c.transaction()?;
    for (i, ch) in chunks.iter().enumerate() {
        let hash = blake3::hash(texts[i].as_bytes()).to_hex().to_string();
        tx.execute(
            "INSERT INTO chunks(id,path,module,title,chain,line_start,line_end,text,hash) VALUES(?,?,?,?,?,?,?,?,?)",
            params![i as i64, ch.path, ch.module, ch.title, ch.chain, ch.line_start as i64, ch.line_end as i64, ch.text, hash],
        )?;
        tx.execute("INSERT OR IGNORE INTO vectors(hash,v) VALUES(?,?)", params![hash, to_blob(&vecs[i])])?;
        tx.execute("INSERT INTO fts_stem(rowid,chain,body) VALUES(?,?,?)", params![i as i64, stem.text(&ch.chain), stem.text(&ch.text)])?;
        tx.execute("INSERT INTO fts_raw(rowid,chain,body) VALUES(?,?,?)", params![i as i64, ch.chain, ch.text])?;
    }
    tx.commit()?;
    eprintln!("index written: {} ({} KB)", db.display(), std::fs::metadata(db)?.len() / 1024);
    Ok(())
}

// ---------- поиск ----------

fn fts(c: &Connection, table: &str, expr: &str) -> Result<Vec<i64>> {
    if expr.is_empty() {
        return Ok(vec![]);
    }
    let sql = format!("SELECT rowid FROM {table} WHERE {table} MATCH ?1 ORDER BY bm25({table}) LIMIT {CANDIDATES}");
    let mut st = c.prepare(&sql)?;
    let r = st.query_map([expr], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
    Ok(r)
}

fn load_vectors(c: &Connection) -> Result<Vec<(i64, Vec<f32>)>> {
    let mut st = c.prepare("SELECT chunks.id, vectors.v FROM chunks JOIN vectors ON vectors.hash = chunks.hash")?;
    let r = st
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, from_blob(&r.get::<_, Vec<u8>>(1)?))))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(r)
}

fn top_k(scored: impl Iterator<Item = (i64, f32)>, k: usize) -> Vec<(i64, f32)> {
    let mut v: Vec<(i64, f32)> = scored.collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    v.truncate(k);
    v
}

fn label(c: &Connection, id: i64) -> Result<String> {
    Ok(c.query_row("SELECT path || ':' || line_start || '  ' || chain FROM chunks WHERE id=?", [id], |r| r.get(0))?)
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn query(db: &Path, qfile: &Path, rerank: bool) -> Result<()> {
    let c = open(db)?;
    let stem = Stem::new();
    let mut model = embedder()?;
    let mut rr = if rerank {
        let t = Instant::now();
        let m = TextRerank::try_new(
            RerankInitOptions::new(RerankerModel::BGERerankerV2M3)
                .with_cache_dir(models_dir())
                .with_max_length(1024)
                .with_show_download_progress(true),
        )?;
        eprintln!("rerank model loaded in {:.1}s", t.elapsed().as_secs_f64());
        Some(m)
    } else {
        None
    };
    let vectors = load_vectors(&c)?;
    let queries = std::fs::read_to_string(qfile)?;
    for q in queries.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        println!("\n=== {q}");
        let t = Instant::now();
        let mut qv = model.embed(&[q], None)?.remove(0);
        normalize(&mut qv);
        let t_embed = t.elapsed();

        let t = Instant::now();
        let vec_hits = top_k(vectors.iter().map(|(id, v)| (*id, dot(&qv, v))), CANDIDATES);
        let t_vec = t.elapsed();
        let cos: HashMap<i64, f32> = vectors.iter().map(|(id, v)| (*id, dot(&qv, v))).collect();

        let words = Stem::words(q);
        let stem_expr = words.iter().map(|w| format!("\"{}\"", stem.word(w))).collect::<Vec<_>>().join(" OR ");
        let raw_expr = words.iter().map(|w| format!("\"{w}\"*")).collect::<Vec<_>>().join(" OR ");
        let t = Instant::now();
        let stem_hits = fts(&c, "fts_stem", &stem_expr)?;
        let t_fts = t.elapsed();
        let raw_hits = fts(&c, "fts_raw", &raw_expr)?;

        let mut rrf: HashMap<i64, f32> = HashMap::new();
        for (rank, id) in stem_hits.iter().enumerate() {
            *rrf.entry(*id).or_default() += 1.0 / (60.0 + rank as f32 + 1.0);
        }
        for (rank, (id, _)) in vec_hits.iter().enumerate() {
            *rrf.entry(*id).or_default() += 1.0 / (60.0 + rank as f32 + 1.0);
        }
        let hybrid = top_k(rrf.into_iter(), CANDIDATES);
        println!(
            "time: embed {:.0} ms, vector scan {:.1} ms ({} vectors), fts {:.1} ms",
            t_embed.as_secs_f64() * 1000.0,
            t_vec.as_secs_f64() * 1000.0,
            vectors.len(),
            t_fts.as_secs_f64() * 1000.0
        );
        println!("-- fts prefix (raw):");
        for id in raw_hits.iter().take(5) {
            println!("   {}", label(&c, *id)?);
        }
        println!("-- fts stemmed:");
        for id in stem_hits.iter().take(5) {
            println!("   {}", label(&c, *id)?);
        }
        println!("-- vector:");
        for (id, s) in vec_hits.iter().take(5) {
            println!("   {s:.3}  {}", label(&c, *id)?);
        }
        println!("-- hybrid (RRF), cosine shown:");
        for (id, _) in hybrid.iter().take(5) {
            println!("   {:.3}  {}", cos[id], label(&c, *id)?);
        }
        if let Some(m) = rr.as_mut() {
            let docs: Vec<String> = hybrid
                .iter()
                .map(|(id, _)| {
                    c.query_row("SELECT 'path: ' || path || char(10) || chain || char(10) || text FROM chunks WHERE id=?", [id], |r| r.get(0))
                })
                .collect::<Result<_, _>>()?;
            let t = Instant::now();
            let res = m.rerank(q.to_string(), docs.clone(), false, Some(8))?;
            let t_rr = t.elapsed();
            println!("-- rerank of {} candidates in {:.0} ms (sigmoid score):", docs.len(), t_rr.as_secs_f64() * 1000.0);
            for r in res.iter().take(5) {
                println!("   {:.3}  {}", sigmoid(r.score), label(&c, hybrid[r.index].0)?);
            }
            let tail: Vec<String> = res.iter().skip(5).take(5).map(|r| format!("{:.3}", sigmoid(r.score))).collect();
            println!("   next scores: {}", tail.join(" "));
        }
    }
    Ok(())
}

// ---------- замер скорости на синтетике ----------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 10_000) as f32 / 10_000.0 - 0.5
    }
}

fn bench(db: &Path, ns: &str) -> Result<()> {
    let src = open(db)?;
    let base = load_vectors(&src)?;
    let texts: Vec<(String, String)> = {
        let mut st = src.prepare("SELECT chain, text FROM chunks ORDER BY id")?;
        let r = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        r
    };
    let stem = Stem::new();
    let queries: Vec<Vec<f32>> = base.iter().take(20).map(|(_, v)| v.clone()).collect();
    let fts_q = ["\"индекс\" OR \"модел\"", "\"настройк\" OR \"порог\"", "\"воркер\" OR \"канал\""];
    for n in ns.split(',') {
        let n: usize = n.parse()?;
        let path = db.with_extension(format!("bench{n}.sqlite"));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let mut c = open(&path)?;
        c.execute_batch(
            "CREATE TABLE vec(id INTEGER PRIMARY KEY, module TEXT, v BLOB);
             CREATE VIRTUAL TABLE fts USING fts5(chain, body, tokenize='unicode61 remove_diacritics 2');",
        )?;
        let mut rng = Rng(0x9E3779B97F4A7C15);
        let tx = c.transaction()?;
        for i in 0..n {
            let (_, b) = &base[i % base.len()];
            let mut v: Vec<f32> = b.iter().map(|x| x + rng.next() * 0.05).collect();
            normalize(&mut v);
            tx.execute("INSERT INTO vec(id,module,v) VALUES(?,?,?)", params![i as i64, format!("m{}", i % 8), to_blob(&v)])?;
            let (ch, tx_) = &texts[i % texts.len()];
            tx.execute("INSERT INTO fts(rowid,chain,body) VALUES(?,?,?)", params![i as i64, stem.text(ch), stem.text(tx_)])?;
        }
        tx.commit()?;

        // перебор с чтением векторов из SQLite на каждый запрос
        let t = Instant::now();
        for q in &queries {
            let mut st = c.prepare_cached("SELECT id, v FROM vec")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
            let mut best: Vec<(i64, f32)> = Vec::with_capacity(n);
            for r in rows {
                let (id, b) = r?;
                best.push((id, dot(q, &from_blob(&b))));
            }
            let _ = top_k(best.into_iter(), CANDIDATES);
        }
        let sql_ms = t.elapsed().as_secs_f64() * 1000.0 / queries.len() as f64;

        // перебор по векторам в памяти
        let mem: Vec<(i64, Vec<f32>)> = {
            let mut st = c.prepare("SELECT id, v FROM vec")?;
            let r = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, from_blob(&r.get::<_, Vec<u8>>(1)?))))?.collect::<Result<Vec<_>, _>>()?;
            r
        };
        let t = Instant::now();
        for q in &queries {
            let _ = top_k(mem.iter().map(|(id, v)| (*id, dot(q, v))), CANDIDATES);
        }
        let mem_ms = t.elapsed().as_secs_f64() * 1000.0 / queries.len() as f64;

        let t = Instant::now();
        for q in fts_q.iter().cycle().take(30) {
            let _ = fts(&c, "fts", q)?;
        }
        let fts_ms = t.elapsed().as_secs_f64() * 1000.0 / 30.0;
        println!(
            "N={n}: vector scan from SQLite {sql_ms:.1} ms, in memory {mem_ms:.1} ms, fts5 {fts_ms:.1} ms; db {} MB",
            std::fs::metadata(&path)?.len() / 1_048_576
        );
    }
    Ok(())
}

// ---------- замер эмбеддинга: модель, потоки, пачка, CPU или DirectML ----------

fn embed_bench(db: &Path, model: &str, threads: usize, batch: usize, ep: &str) -> Result<()> {
    let c = open(db)?;
    let texts: Vec<String> = {
        let mut st = c.prepare("SELECT 'path: ' || path || char(10) || chain || char(10) || text FROM chunks ORDER BY id")?;
        let r = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        r
    };
    let (m, max_len) = match model {
        "bge-m3" => (EmbeddingModel::BGEM3, 8192),
        "e5-base" => (EmbeddingModel::MultilingualE5Base, 512),
        "e5-small" => (EmbeddingModel::MultilingualE5Small, 512),
        "e5-large" => (EmbeddingModel::MultilingualE5Large, 512),
        _ => bail!("model: bge-m3 | e5-small | e5-base | e5-large"),
    };
    let mut opts = TextInitOptions::new(m)
        .with_cache_dir(models_dir())
        .with_max_length(max_len)
        .with_show_download_progress(true);
    if threads > 0 {
        opts = opts.with_intra_threads(threads);
    }
    let eps = ep_list(ep);
    if !eps.is_empty() {
        opts = opts.with_execution_providers(eps);
    }
    let t = Instant::now();
    let mut emb = TextEmbedding::try_new(opts)?;
    let load = t.elapsed().as_secs_f64();
    let mut order: Vec<usize> = (0..texts.len()).collect();
    order.sort_by_key(|&i| texts[i].len());
    let t = Instant::now();
    for b in order.chunks(batch) {
        let bt: Vec<&str> = b.iter().map(|&i| texts[i].as_str()).collect();
        emb.embed(&bt, Some(batch))?;
    }
    let total = t.elapsed().as_secs_f64();
    let t = Instant::now();
    for _ in 0..5 {
        emb.embed(&["что делает клиент, если воркер не запущен"], None)?;
    }
    let q = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
    println!(
        "model={model} ep={ep} threads={threads} batch={batch}: load {load:.1}s, {} chunks in {total:.1}s = {:.0} ms/chunk; query {q:.0} ms",
        texts.len(),
        total * 1000.0 / texts.len() as f64
    );
    Ok(())
}

// ---------- замер реранка: число кандидатов, длина входа, устройство ----------

fn ep_list(ep: &str) -> Vec<fastembed::ExecutionProviderDispatch> {
    match ep.strip_prefix("dml") {
        Some(rest) => {
            let id: i32 = rest.trim_start_matches(':').parse().unwrap_or(0);
            vec![ort::ep::DirectML::default().with_device_id(id).build().error_on_failure()]
        }
        None => vec![],
    }
}

fn rerank_bench(db: &Path, n: usize, max_len: usize, ep: &str) -> Result<()> {
    let c = open(db)?;
    let docs: Vec<String> = {
        let mut st = c.prepare("SELECT 'path: ' || path || char(10) || chain || char(10) || text FROM chunks ORDER BY length(text) DESC LIMIT ?")?;
        let r = st.query_map([n as i64], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        r
    };
    let mut opts = RerankInitOptions::new(RerankerModel::BGERerankerV2M3).with_cache_dir(models_dir()).with_max_length(max_len);
    let eps = ep_list(ep);
    if !eps.is_empty() {
        opts = opts.with_execution_providers(eps);
    }
    let t = Instant::now();
    let mut m = TextRerank::try_new(opts)?;
    let load = t.elapsed().as_secs_f64();
    m.rerank("прогрев".to_string(), docs[..2].to_vec(), false, Some(2))?;
    let t = Instant::now();
    m.rerank("что делает клиент, если воркер не запущен".to_string(), docs.clone(), false, Some(8))?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("rerank ep={ep} candidates={n} max_len={max_len}: load {load:.1}s, rerank {ms:.0} ms (longest chunks)");
    Ok(())
}

// ---------- совпадают ли векторы CPU и DirectML ----------

fn embed_compare(db: &Path) -> Result<()> {
    let c = open(db)?;
    let texts: Vec<String> = {
        let mut st = c.prepare("SELECT 'path: ' || path || char(10) || chain || char(10) || text FROM chunks ORDER BY id LIMIT 24")?;
        let r = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        r
    };
    let mk = |ep: &str| -> Result<Vec<Vec<f32>>> {
        let mut o = TextInitOptions::new(EmbeddingModel::BGEM3).with_cache_dir(models_dir()).with_max_length(8192);
        let eps = ep_list(ep);
        if !eps.is_empty() {
            o = o.with_execution_providers(eps);
        }
        let mut m = TextEmbedding::try_new(o)?;
        let mut v = m.embed(&texts, Some(4))?;
        v.iter_mut().for_each(|x| normalize(x));
        Ok(v)
    };
    let a = mk("cpu")?;
    let b = mk("dml:0")?;
    let sims: Vec<f32> = a.iter().zip(&b).map(|(x, y)| dot(x, y)).collect();
    let min = sims.iter().cloned().fold(1.0f32, f32::min);
    println!("cpu vs dml cosine over {} chunks: min {min:.6}", sims.len());
    Ok(())
}
