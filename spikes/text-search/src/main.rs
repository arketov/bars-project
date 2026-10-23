// Выбрасываемый спайк 3.6.1: поиск вхождений по файлам .md на диске, замер против 300 мс (НФТ-6).
use rayon::prelude::*;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

const INSTRUCTION: &str = "BARS_INSTRUCTION.md";
const PER_DOC: usize = 5;
const LIMIT_DOCS: usize = 100;

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let p = e.path();
        if ft.is_dir() {
            walk(&p, out);
        } else if ft.is_file()
            && name.len() > 3
            && name[name.len() - 3..].eq_ignore_ascii_case(".md")
            && !name.eq_ignore_ascii_case(INSTRUCTION)
        {
            out.push(p);
        }
    }
}

fn read_utf8(p: &Path) -> Option<String> {
    let b = fs::read(p).ok()?;
    let s = String::from_utf8(b).ok()?;
    Some(s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s))
}

#[derive(Clone)]
enum Matcher {
    Re(Regex),                              // регэксп или (?i)-литерал
    Sub(memchr::memmem::Finder<'static>),   // с учётом регистра
    Lower(memchr::memmem::Finder<'static>), // без регистра: to_lowercase + memmem
}

#[derive(Default)]
struct Res {
    docs: usize,
    hits: usize,
    kept: Vec<(usize, String)>, // (строка, текст) — до PER_DOC на документ, только для первых LIMIT_DOCS
}

fn line_of(text: &str, off: usize) -> (usize, &str) {
    let off = (0..=off).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
    let s = text[..off].rfind('\n').map_or(0, |i| i + 1);
    let e = text[off..].find('\n').map_or(text.len(), |i| off + i);
    let mut end = e.min(s + 240).max(off);
    while !text.is_char_boundary(end) {
        end += 1;
    }
    (text[..off].matches('\n').count() + 1, &text[s..end])
}

fn search_one(m: &Matcher, text: &str, res: &mut Res) {
    let mut n = 0usize;
    let mut kept: Vec<(usize, String)> = Vec::new();
    let push = |hay: &str, off: usize, kept: &mut Vec<(usize, String)>| {
        if kept.len() < PER_DOC {
            let (l, t) = line_of(hay, off);
            kept.push((l, t.to_string()));
        }
    };
    match m {
        Matcher::Re(re) => {
            for x in re.find_iter(text) {
                n += 1;
                push(text, x.start(), &mut kept);
            }
        }
        Matcher::Sub(f) => {
            for off in f.find_iter(text.as_bytes()) {
                n += 1;
                push(text, off, &mut kept);
            }
        }
        Matcher::Lower(f) => {
            let low = text.to_lowercase();
            for off in f.find_iter(low.as_bytes()) {
                n += 1;
                push(&low, off, &mut kept);
            }
        }
    }
    if n > 0 {
        res.docs += 1;
        res.hits += n;
        if res.docs <= LIMIT_DOCS {
            res.kept.extend(kept);
        }
    }
}

fn merge(mut a: Res, b: Res) -> Res {
    a.docs += b.docs;
    a.hits += b.hits;
    if a.kept.len() < LIMIT_DOCS * PER_DOC {
        a.kept.extend(b.kept);
    }
    a
}

#[derive(Clone, Copy, PartialEq)]
enum Variant {
    Single,
    Pool,
    CachedSingle,
    CachedPool,
}

struct Corpus {
    root: PathBuf,
    cache: Vec<String>,
}

fn run(c: &Corpus, m: &Matcher, v: Variant) -> (Res, f64, f64) {
    // (итог, мс всего, мс до первого документа с вхождением — только 1 поток с диска)
    let t = Instant::now();
    let mut first = f64::NAN;
    let res = match v {
        Variant::Single => {
            let mut files = Vec::new();
            walk(&c.root, &mut files);
            let mut r = Res::default();
            for p in &files {
                if let Some(s) = read_utf8(p) {
                    search_one(m, &s, &mut r);
                    if r.docs > 0 && first.is_nan() {
                        first = t.elapsed().as_secs_f64() * 1e3;
                    }
                }
            }
            r
        }
        Variant::Pool => {
            let mut files = Vec::new();
            walk(&c.root, &mut files);
            files
                .par_iter()
                .fold(Res::default, |mut r, p| {
                    if let Some(s) = read_utf8(p) {
                        search_one(m, &s, &mut r);
                    }
                    r
                })
                .reduce(Res::default, merge)
        }
        Variant::CachedSingle => {
            let mut r = Res::default();
            for s in &c.cache {
                search_one(m, s, &mut r);
            }
            r
        }
        Variant::CachedPool => c
            .cache
            .par_iter()
            .fold(Res::default, |mut r, s| {
                search_one(m, s, &mut r);
                r
            })
            .reduce(Res::default, merge),
    };
    (res, t.elapsed().as_secs_f64() * 1e3, first)
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

#[repr(C)]
struct Pmc {
    cb: u32,
    pf: u32,
    peak_ws: usize,
    ws: usize,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    e: usize,
    f: usize,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(h: isize, p: *mut Pmc, cb: u32) -> i32;
}
fn peak_mb() -> f64 {
    unsafe {
        let mut p = std::mem::zeroed::<Pmc>();
        p.cb = std::mem::size_of::<Pmc>() as u32;
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut p, p.cb);
        p.peak_ws as f64 / 1048576.0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // spike <имя> <корень> <прогонов> <редкое> <частое>
    let (name, root, runs) = (&args[1], PathBuf::from(&args[2]), args[3].parse::<usize>().unwrap());
    let (rare, freq) = (args[4].clone(), args[5].clone());
    let mut files = Vec::new();
    let t = Instant::now();
    walk(&root, &mut files);
    let walk_ms = t.elapsed().as_secs_f64() * 1e3;
    let (mut bytes, mut skipped, mut cache) = (0usize, 0usize, Vec::new());
    let t = Instant::now();
    for p in &files {
        match read_utf8(p) {
            Some(s) => {
                bytes += s.len();
                cache.push(s)
            }
            None => skipped += 1,
        }
    }
    let first_read_ms = t.elapsed().as_secs_f64() * 1e3;
    println!(
        "## {name}: {} файлов, {:.1} МБ, не прочитано {skipped}; обход {walk_ms:.0} мс; первое чтение файлов {first_read_ms:.0} мс; потоков {}",
        files.len(),
        bytes as f64 / 1048576.0,
        rayon::current_num_threads()
    );
    let c = Corpus { root, cache };
    type Mk = Box<dyn Fn(&str) -> Matcher>;
    let modes: Vec<(&str, Mk)> = vec![
        ("без регистра, regex (?i)", Box::new(|q| Matcher::Re(Regex::new(&format!("(?i){}", regex::escape(q))).unwrap()))),
        ("без регистра, lowercase+memmem", Box::new(|q| Matcher::Lower(memchr::memmem::Finder::new(&q.to_lowercase()).into_owned()))),
        ("с регистром, memmem", Box::new(|q| Matcher::Sub(memchr::memmem::Finder::new(q).into_owned()))),
        ("регулярное выражение", Box::new(|q| Matcher::Re(Regex::new(&format!("(?m){}", q)).unwrap()))),
    ];
    let queries = [("редкое", rare.clone()), ("частое", freq.clone()), ("нет в корпусе", "ъъъщщщ_нет_такого".to_string())];
    println!("| режим | запрос | вариант | вхождений / документов | p50 мс | p95 мс | max мс | до первого док. мс (p50) |");
    println!("|---|---|---|---|---|---|---|---|");
    for (mname, mk) in &modes {
        for (qname, q) in &queries {
            let q = if *mname == "регулярное выражение" {
                match *qname {
                    "редкое" => format!("{}\\w*", regex::escape(&rare)),
                    "частое" => format!("\\b{}\\w*", regex::escape(&freq)),
                    _ => "ъъъ\\d{3}щщщ".to_string(),
                }
            } else {
                q.clone()
            };
            let m = mk(&q);
            for (vname, v) in [("1 поток, диск", Variant::Single), ("пул, диск", Variant::Pool), ("1 поток, кэш", Variant::CachedSingle), ("пул, кэш", Variant::CachedPool)] {
                let _ = run(&c, &m, v); // прогрев
                let (mut t, mut f) = (Vec::new(), Vec::new());
                let mut last = Res::default();
                for _ in 0..runs {
                    let (r, ms, first) = run(&c, &m, v);
                    t.push(ms);
                    if !first.is_nan() {
                        f.push(first)
                    }
                    last = r;
                }
                let first = if f.is_empty() { "—".to_string() } else { format!("{:.1}", pct(&mut f, 0.5)) };
                println!(
                    "| {mname} | {qname} `{}` | {vname} | {} / {} | {:.1} | {:.1} | {:.1} | {first} |",
                    q.chars().take(24).collect::<String>(),
                    last.hits,
                    last.docs,
                    pct(&mut t.clone(), 0.5),
                    pct(&mut t.clone(), 0.95),
                    pct(&mut t, 1.0)
                );
            }
        }
    }
    println!("\nПиковый рабочий набор процесса: {:.0} МБ (включает кэш текстов {:.0} МБ)\n", peak_mb(), bytes as f64 / 1048576.0);
}
