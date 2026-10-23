use super::*;
use serde_json::{Value, json};

pub struct LinkPage {
    pub known: bool,
    pub items: Vec<Value>,
    pub has_more: bool,
    pub next_offset: usize,
    pub fields_truncated: bool,
    pub bytes_limited: bool,
}

fn bounded(value: String, truncated: &mut bool) -> String {
    if value.chars().count() > 1024 {
        *truncated = true;
        value.chars().take(1024).collect()
    } else {
        value
    }
}

impl LinkIndex {
    pub fn snapshot(&self) -> Result<String, LinkIndexError> {
        let mut digest = blake3::Hasher::new();
        for value in [
            LINKS_PARSER_VERSION.to_string(),
            self.meta("docs_prefix")?.unwrap_or_default(),
        ] {
            digest.update(&(value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
        let mut statement = self
            .db
            .prepare("SELECT path, hash FROM docs ORDER BY path")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            for column in 0..2 {
                let value: String = row.get(column)?;
                digest.update(&(value.len() as u64).to_le_bytes());
                digest.update(value.as_bytes());
            }
        }
        Ok(digest.finalize().to_hex().to_string())
    }

    /// Поток записей: никогда не собирает incoming.lines или полный links_of.
    /// Бюджет включает запас на внешнюю оболочку и курсор, добавляемые воркером.
    pub fn page(
        &self,
        path: &str,
        incoming: bool,
        offset: usize,
        limit: usize,
    ) -> Result<LinkPage, LinkIndexError> {
        let prefix = self.meta("docs_prefix")?.unwrap_or_default();
        let mut titles = HashMap::<String, Option<String>>::new();
        let mut title_cut = HashSet::<String>::new();
        let mut statement = self
            .db
            .prepare("SELECT path, substr(title,1,1024), length(title)>1024 FROM docs")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            if row.get::<_, Option<bool>>(2)?.unwrap_or(false) {
                title_cut.insert(name.clone());
            }
            titles.insert(name, row.get(1)?);
        }
        let mut page = LinkPage {
            known: titles.contains_key(path),
            items: vec![],
            has_more: false,
            next_offset: offset,
            fields_truncated: false,
            bytes_limited: false,
        };
        if !page.known {
            return Ok(page);
        }
        let by_docs: HashMap<_, _> = titles
            .keys()
            .map(|p| (relative_to_docs(p, &prefix).to_owned(), p.clone()))
            .collect();
        let set = DocSet::new(by_docs.keys().cloned());
        let own = relative_to_docs(path, &prefix);
        let sql = if incoming {
            "SELECT src,line,start,end,kind,raw_target,text,fragment FROM links ORDER BY src,line,start,id"
        } else {
            "SELECT src,line,start,end,kind,raw_target,text,fragment FROM links WHERE src=?1 ORDER BY line,start,id"
        };
        let mut statement = self.db.prepare(sql)?;
        let mut rows = if incoming {
            statement.query([])?
        } else {
            statement.query([path])?
        };
        let mut matched = 0;
        let mut bytes = 4096usize;
        while let Some(row) = rows.next()? {
            let src: String = row.get(0)?;
            let link = ParsedLink {
                line: row.get(1)?,
                start: row.get::<_, i64>(2)? as usize,
                end: row.get::<_, i64>(3)? as usize,
                kind: LinkKind::parse(&row.get::<_, String>(4)?).unwrap_or(LinkKind::Inline),
                raw_target: row.get(5)?,
                text: row.get(6)?,
                fragment: row.get(7)?,
            };
            let resolved = set.resolve(relative_to_docs(&src, &prefix), &link);
            if incoming && (src == path || resolved != Resolved::Found(own))
                || resolved == Resolved::NotALink
            {
                continue;
            }
            if matched < offset {
                matched += 1;
                continue;
            }
            if page.items.len() == limit {
                page.has_more = true;
                break;
            }
            let mut cut = false;
            let item = if incoming {
                cut |= title_cut.contains(&src);
                json!({"source":src,"title":titles.get(&src).cloned().flatten(),"line":link.line,"fields_truncated":cut})
            } else {
                let target = match resolved {
                    Resolved::Found(p) => by_docs.get(p).cloned(),
                    _ => None,
                };
                cut |= target.as_ref().is_some_and(|t| title_cut.contains(t));
                let title = target
                    .as_ref()
                    .and_then(|t| titles.get(t).cloned().flatten());
                let raw = bounded(link.raw_target, &mut cut);
                let text = bounded(link.text, &mut cut);
                json!({"line":link.line,"kind":link.kind.as_str(),"raw":raw,"text":text,
                    "target":target,"target_title":title,"broken":resolved == Resolved::Broken,"fields_truncated":cut})
            };
            let size = serde_json::to_vec(&item).expect("link JSON").len() + 1;
            if bytes + size > 512 * 1024 {
                if page.items.is_empty() {
                    return Err(LinkIndexError::Refresh(
                        "запись ссылки превышает бюджет ответа".into(),
                    ));
                }
                page.has_more = true;
                page.bytes_limited = true;
                break;
            }
            bytes += size;
            page.fields_truncated |= cut;
            page.items.push(item);
            page.next_offset += 1;
        }
        if matched < offset {
            return Err(LinkIndexError::Refresh(
                "смещение страницы вне снимка".into(),
            ));
        }
        Ok(page)
    }
}
