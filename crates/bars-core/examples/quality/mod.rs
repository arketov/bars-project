use bars_core::search::SearchHit;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Debug, Deserialize)]
pub struct Gold {
    pub path: String,
    pub heading: String,
    pub evidence: String,
    pub reason: String,
}
#[derive(Debug, Deserialize)]
pub struct Case {
    pub id: String,
    pub split: String,
    pub category: String,
    pub query: String,
    pub gold: Vec<Gold>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Hit {
    pub id: i64,
    pub path: String,
    pub heading: String,
    pub first: usize,
    pub last: usize,
    pub score: f32,
    pub relevant: Vec<usize>,
    pub document_relevant: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub candidates: usize,
    pub k: usize,
    pub repeat: usize,
    pub mode: String,
    pub total: f64,
    pub embed: f64,
    pub rerank: f64,
    pub hits: Vec<Hit>,
}
impl Row {
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.id, self.candidates, self.k, self.repeat, self.mode
        )
    }
}
pub fn digest(b: &[u8]) -> String {
    blake3::hash(b).to_hex().to_string()
}
pub fn file_digest(p: &Path) -> Result<String> {
    let mut f = File::open(p)?;
    let mut h = blake3::Hasher::new();
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}
pub fn load_cases(p: &Path) -> Result<Vec<Case>> {
    let s = fs::read_to_string(p)?;
    Ok(serde_json::from_str(s.trim_start_matches('\u{feff}'))?)
}
pub fn validate_cases(root: &Path, cases: &[Case]) -> Result<()> {
    let mut ids = HashSet::new();
    let mut questions = HashSet::new();
    for c in cases {
        if !ids.insert(&c.id) || !questions.insert(c.query.trim()) {
            return Err("повтор id/вопроса".into());
        }
        if !["tune", "test", "regression"].contains(&c.split.as_str()) || c.query.trim().is_empty()
        {
            return Err(format!("выборка/вопрос: {}", c.id).into());
        }
        if (c.category == "negative") != c.gold.is_empty() {
            return Err(format!("negative/gold: {}", c.id).into());
        }
        for g in &c.gold {
            if g.path.contains('\\')
                || Path::new(&g.path).is_absolute()
                || g.path.split('/').any(|s| s == "..")
            {
                return Err("эталон выходит за корпус".into());
            }
            let source = fs::read_to_string(root.join(&g.path))?;
            let lines: Vec<_> = source.lines().collect();
            let heading = format!("## {}", g.heading);
            let starts: Vec<_> = lines
                .iter()
                .enumerate()
                .filter_map(|(i, l)| (*l == heading).then_some(i))
                .collect();
            if starts.len() != 1 || g.evidence.chars().count() < 20 || g.reason.is_empty() {
                return Err(format!("заголовок/эталон: {} {}", c.id, g.heading).into());
            }
            let start = starts[0] + 1;
            let end = lines[start..]
                .iter()
                .position(|l| l.starts_with("## "))
                .map_or(lines.len(), |i| start + i);
            if !lines[start..end].join("\n").contains(&g.evidence) {
                return Err(format!("нет опоры: {} {}", c.id, g.heading).into());
            }
        }
    }
    Ok(())
}
pub fn pack_hits(hits: &[SearchHit], case: &Case) -> Vec<Hit> {
    hits.iter()
        .map(|h| Hit {
            id: h.chunk_id,
            path: h.path.clone(),
            heading: h.heading_chain.clone(),
            first: h.first_line,
            last: h.last_line,
            score: h.score,
            relevant: case
                .gold
                .iter()
                .enumerate()
                .filter_map(|(i, g)| {
                    (h.path == g.path && h.text.contains(&g.evidence)).then_some(i)
                })
                .collect(),
            document_relevant: case.gold.iter().any(|g| g.path == h.path),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrong_section_of_correct_file() {
        let c = Case {
            id: "a".into(),
            split: "tune".into(),
            category: "semantic".into(),
            query: "q".into(),
            gold: vec![Gold {
                path: "a.md".into(),
                heading: "x".into(),
                evidence: "нужная опора".into(),
                reason: "r".into(),
            }],
        };
        let h = SearchHit {
            chunk_id: 1,
            path: "a.md".into(),
            module: ".".into(),
            title: "a".into(),
            heading_chain: "y".into(),
            first_line: 1,
            last_line: 3,
            text: "другой раздел".into(),
            score: 0.9,
            score_kind: bars_core::search::ScoreKind::Cosine,
        };
        let p = pack_hits(&[h], &c);
        assert!(p[0].document_relevant);
        assert!(p[0].relevant.is_empty());
    }
    #[test]
    fn missing_or_changed_gold() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("a.md"),
            "## x\nСовершенно другой текст раздела",
        )
        .unwrap();
        let mut cases:Vec<Case>=serde_json::from_str(r#"[{"id":"x","split":"test","category":"semantic","query":"q","gold":[{"path":"a.md","heading":"x","evidence":"опорный текст длиной больше двадцати символов","reason":"r"}]}]"#).unwrap();
        assert!(validate_cases(d.path(), &cases).is_err());
        cases[0].gold[0].path = "missing.md".into();
        assert!(validate_cases(d.path(), &cases).is_err());
    }
}
