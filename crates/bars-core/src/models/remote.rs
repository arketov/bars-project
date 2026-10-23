//! Внешние модели по HTTP: embed через OpenAI/Ollama API и rerank в формате Cohere
//! (`docs/features/models/external-models.md`, Т-17).
//!
//! Ollama и TEI используют нативные API; остальные — OpenAI/Cohere API.
//! Ключ читается из [`KeyRef`] при каждом запросе и вычищается из сообщений об
//! ошибках.

use std::time::Duration;

use serde_json::{Value, json};

use super::embedder::{EmbedError, Embedder, ModelFingerprint};
use super::reranker::{RerankError, Reranker};
use crate::secrets::KeyRef;

/// Повторов после первой неудачи при сетевой ошибке или ответе 5xx (`external-models.md`).
const RETRIES: u32 = 3;
/// Пауза перед первым повтором; перед каждым следующим она удваивается.
const RETRY_PAUSE: Duration = Duration::from_secs(1);
const EMBED_TIMEOUT: Duration = Duration::from_secs(120);
const RERANK_TIMEOUT: Duration = Duration::from_secs(60);
/// Сколько знаков ответа провайдера попадает в сообщение об ошибке.
const SNIPPET_CHARS: usize = 300;

/// Что нужно знать об адресе внешней модели.
#[derive(Clone)]
pub struct RemoteTarget {
    /// `remote`, `local-openai`, `ollama`, `lmstudio` или `tei`: идёт в отпечаток.
    pub source: &'static str,
    pub base_url: String,
    /// Имя модели у провайдера.
    pub model: String,
    pub key: KeyRef,
}

/// Ответ провайдера, который не удалось разобрать или который сообщает об отказе.
enum Failure {
    /// Сеть или 5xx: имеет смысл повторить.
    Transient(String),
    /// 4xx и непонятный ответ: повтор не поможет.
    Fatal(String),
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// `<base>/v1/<path>`; если адрес уже заканчивается на `/v1`, вторая `v1` не дописывается.
fn versioned_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}/{path}")
    } else {
        format!("{base}/v1/{path}")
    }
}

fn plain_url(base_url: &str, path: &str) -> String {
    format!("{}/{path}", base_url.trim_end_matches('/'))
}

struct Reply {
    status: u16,
    body: String,
}

fn send(agent: &ureq::Agent, url: &str, key: Option<&str>, body: &Value) -> Result<Reply, Failure> {
    let mut request = agent.post(url).header("Accept", "application/json");
    if let Some(key) = key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let transient = |error: ureq::Error| Failure::Transient(redact(&error.to_string(), key));
    let mut response = request.send_json(body).map_err(transient)?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_string()
        .map_err(transient)?;
    Ok(Reply { status, body: text })
}

/// Ошибка по статусу ответа; `None` для 2xx.
fn status_failure(reply: &Reply, key: Option<&str>) -> Option<Failure> {
    let status = reply.status;
    if (200..300).contains(&status) {
        return None;
    }
    let snippet: String = redact(&reply.body, key)
        .chars()
        .take(SNIPPET_CHARS)
        .collect();
    let message = match status {
        401 | 403 => format!("провайдер отклонил ключ (ответ {status}): {snippet}"),
        413 => format!("провайдер отказал: вход слишком длинный (ответ 413): {snippet}"),
        _ => format!("провайдер ответил {status}: {snippet}"),
    };
    Some(if status >= 500 || status == 429 {
        Failure::Transient(message)
    } else {
        Failure::Fatal(message)
    })
}

/// Убирает ключ из текста, чтобы он не попал в ошибку.
fn redact(text: &str, key: Option<&str>) -> String {
    match key {
        Some(key) if !key.is_empty() => text.replace(key, "***"),
        _ => text.to_string(),
    }
}

/// POST с повторами при сетевой ошибке и 5xx. Возвращает тело успешного ответа.
fn post_with_retries(
    agent: &ureq::Agent,
    url: &str,
    key: &KeyRef,
    body: &Value,
    retries: u32,
    pause: Duration,
) -> Result<String, EmbedError> {
    let key = key
        .resolve()
        .map_err(|error| EmbedError::Load(error.to_string()))?;
    let mut wait = pause;
    let mut attempt = 0;
    loop {
        let failure = match send(agent, url, key.as_deref(), body) {
            Ok(reply) => match status_failure(&reply, key.as_deref()) {
                None => return Ok(reply.body),
                Some(failure) => failure,
            },
            Err(failure) => failure,
        };
        match failure {
            Failure::Fatal(message) => return Err(EmbedError::Rejected(message)),
            Failure::Transient(message) if attempt >= retries => {
                return Err(EmbedError::Embed(message));
            }
            Failure::Transient(_) => {
                attempt += 1;
                std::thread::sleep(wait);
                wait *= 2;
            }
        }
    }
}

/// Embed через OpenAI `/v1/embeddings` либо Ollama `/api/embed`.
pub struct HttpEmbedder {
    agent: ureq::Agent,
    url: String,
    target: RemoteTarget,
    batch_size: usize,
    /// До первого ответа размерность неизвестна и равна нулю.
    fingerprint: ModelFingerprint,
    pause: Duration,
}

/// Отпечаток внешней модели по настройкам: размерность узнаётся из первого ответа, до него — 0.
pub fn remote_fingerprint(source: &str, model: &str) -> ModelFingerprint {
    ModelFingerprint {
        source: source.to_string(),
        model: model.to_string(),
        dim: 0,
        doc_prefix: String::new(),
    }
}

impl HttpEmbedder {
    pub fn new(target: RemoteTarget, batch_size: usize) -> Self {
        Self {
            agent: agent(EMBED_TIMEOUT),
            url: if target.source == "tei" {
                plain_url(&target.base_url, "embed")
            } else if target.source == "ollama" {
                plain_url(&target.base_url, "api/embed")
            } else {
                versioned_url(&target.base_url, "embeddings")
            },
            fingerprint: remote_fingerprint(target.source, &target.model),
            target,
            batch_size: batch_size.max(1),
            pause: RETRY_PAUSE,
        }
    }

    /// Пауза перед первым повтором; тесты ставят нулевую.
    pub fn with_retry_pause(mut self, pause: Duration) -> Self {
        self.pause = pause;
        self
    }

    fn request(&self, batch: &[&str], retries: u32) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut body = json!({ "model": self.target.model, "input": batch });
        if self.target.source == "ollama" {
            body["truncate"] = json!(false);
        } else if self.target.source == "tei" {
            body = json!({"inputs": batch, "truncate": false, "normalize": true});
        }
        let text = post_with_retries(
            &self.agent,
            &self.url,
            &self.target.key,
            &body,
            retries,
            self.pause,
        )?;
        let vectors = if self.target.source == "tei" {
            parse_tei_embeddings(&text)
        } else if self.target.source == "ollama" {
            parse_ollama_embeddings(&text)
        } else {
            parse_embeddings(&text)
        }
        .map_err(EmbedError::Embed)?;
        if vectors.len() != batch.len() {
            return Err(EmbedError::Count {
                expected: batch.len(),
                got: vectors.len(),
            });
        }
        Ok(vectors)
    }
}

fn parse_tei_embeddings(text: &str) -> Result<Vec<Vec<f32>>, String> {
    // Типизированный serde-error способен повторить строку провайдера (в том числе ключ).
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("ответ TEI не JSON: {error}"))?;
    let vectors: Vec<Vec<f32>> = value
        .as_array()
        .ok_or("ответ embed TEI не массив")?
        .iter()
        .map(|item| {
            item.as_array()
                .ok_or("вектор TEI не массив")?
                .iter()
                .map(|n| {
                    n.as_f64()
                        .map(|n| n as f32)
                        .ok_or("координата TEI не число".to_string())
                })
                .collect()
        })
        .collect::<Result<_, String>>()?;
    if vectors.iter().any(|vector| {
        vector.is_empty()
            || vector.iter().any(|n| !n.is_finite())
            || vector.iter().all(|n| *n == 0.0)
    }) {
        return Err("TEI вернул пустой, нулевой или неконечный вектор".into());
    }
    Ok(vectors)
}

fn parse_ollama_embeddings(text: &str) -> Result<Vec<Vec<f32>>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("ответ Ollama не JSON: {error}"))?;
    let data = value
        .get("embeddings")
        .and_then(Value::as_array)
        .ok_or("в ответе Ollama нет списка `embeddings`")?;
    data.iter()
        .map(|item| {
            let vector: Vec<f32> = item
                .as_array()
                .ok_or("вектор Ollama не массив")?
                .iter()
                .map(|number| number.as_f64().map(|n| n as f32))
                .collect::<Option<_>>()
                .ok_or("вектор Ollama содержит нечисловую координату")?;
            if vector.is_empty() || vector.iter().any(|n| !n.is_finite()) {
                return Err("Ollama вернула пустой вектор или неконечные координаты".into());
            }
            Ok(vector)
        })
        .collect()
}

fn parse_embeddings(text: &str) -> Result<Vec<Vec<f32>>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("ответ не JSON: {error}"))?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or("в ответе нет списка `data`")?;
    let mut items: Vec<(usize, Vec<f32>)> = Vec::with_capacity(data.len());
    for (position, item) in data.iter().enumerate() {
        let embedding = item
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or("в элементе ответа нет `embedding`")?;
        let vector = embedding
            .iter()
            .map(|number| number.as_f64().map(|n| n as f32))
            .collect::<Option<Vec<f32>>>()
            .ok_or("в `embedding` не только числа")?;
        if vector.is_empty() || vector.iter().any(|n| !n.is_finite()) {
            return Err("провайдер вернул пустой вектор или неконечные координаты".into());
        }
        let index = match item.get("index") {
            Some(value) => value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("неверный `index` в ответе embeddings")?,
            None => position,
        };
        if index >= data.len() || items.iter().any(|(seen, _)| *seen == index) {
            return Err(
                "повторяющийся или выходящий за границы `index` в ответе embeddings".into(),
            );
        }
        items.push((index, vector));
    }
    items.sort_by_key(|(index, _)| *index);
    Ok(items.into_iter().map(|(_, vector)| vector).collect())
}

impl Embedder for HttpEmbedder {
    fn execution(&self) -> super::ExecutionInfo {
        super::ExecutionInfo {
            backend: "external",
            ..Default::default()
        }
    }
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let instructed;
        let model = self.target.model.to_ascii_lowercase();
        let lm_qwen = self.target.source == "lmstudio"
            && (model.starts_with("text-embedding-qwen3-embedding-")
                || model.starts_with("qwen3-embedding-")
                || model.starts_with("qwen/qwen3-embedding-"));
        let tei_qwen = matches!(self.target.source, "tei" | "local-openai")
            && (model.starts_with("qwen/qwen3-embedding-")
                || model.starts_with("qwen3-embedding-"));
        let text = if lm_qwen
            || tei_qwen
            || (self.target.source == "ollama"
                && self.target.model.split(':').next() == Some("qwen3-embedding"))
        {
            instructed = format!("{}{text}", super::catalog::QWEN3_QUERY_PREFIX);
            &instructed
        } else {
            text
        };
        let mut vectors = self.request(&[text], RETRIES)?;
        let vector = vectors.remove(0);
        if self.fingerprint.dim == 0 {
            self.fingerprint.dim = vector.len();
        }
        if vector.len() != self.fingerprint.dim {
            return Err(EmbedError::Dimension {
                expected: self.fingerprint.dim,
                got: vector.len(),
            });
        }
        Ok(vector)
    }
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn prepare(&mut self) -> Result<(), EmbedError> {
        if self.fingerprint.dim == 0 {
            self.embed_query("bars")?;
        }
        Ok(())
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut result = Vec::with_capacity(texts.len());
        for batch in texts.chunks(self.batch_size) {
            result.extend(self.request(batch, 0)?);
        }
        if let Some(first) = result.first() {
            if self.fingerprint.dim == 0 {
                self.fingerprint.dim = first.len();
            }
            if let Some(bad) = result.iter().find(|v| v.len() != self.fingerprint.dim) {
                return Err(EmbedError::Dimension {
                    expected: self.fingerprint.dim,
                    got: bad.len(),
                });
            }
        }
        Ok(result)
    }
}

/// Rerank через `POST <base_url>/rerank`, а если там 404 — через `<base_url>/v1/rerank`.
pub struct HttpReranker {
    agent: ureq::Agent,
    target: RemoteTarget,
    /// Адреса по порядку проб; найденный рабочий ставится первым.
    urls: Vec<String>,
}

impl HttpReranker {
    pub fn new(target: RemoteTarget) -> Self {
        let mut urls = vec![plain_url(&target.base_url, "rerank")];
        let versioned = versioned_url(&target.base_url, "rerank");
        if target.source != "tei" && !urls.contains(&versioned) {
            urls.push(versioned);
        }
        Self {
            agent: agent(RERANK_TIMEOUT),
            target,
            urls,
        }
    }

    fn post(&mut self, body: &Value) -> Result<String, String> {
        let key = self
            .target
            .key
            .resolve()
            .map_err(|error| error.to_string())?;
        let mut last_missing = String::new();
        for position in 0..self.urls.len() {
            let url = self.urls[position].clone();
            let reply =
                send(&self.agent, &url, key.as_deref(), body).map_err(|failure| match failure {
                    Failure::Transient(message) | Failure::Fatal(message) => message,
                })?;
            if reply.status == 404 && position + 1 < self.urls.len() {
                last_missing = url;
                continue;
            }
            return match status_failure(&reply, key.as_deref()) {
                None => {
                    // Рабочий адрес пробуется первым в следующий раз.
                    self.urls.swap(0, position);
                    Ok(reply.body)
                }
                Some(Failure::Transient(message) | Failure::Fatal(message)) => Err(message),
            };
        }
        Err(format!("адрес {last_missing} не найден (ответ 404)"))
    }
}

impl Reranker for HttpReranker {
    fn execution(&self) -> super::ExecutionInfo {
        super::ExecutionInfo {
            backend: "external",
            ..Default::default()
        }
    }
    fn rerank(&mut self, query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let body = if self.target.source == "tei" {
            json!({"query": query, "texts": documents, "truncate": false,
                "raw_scores": false, "return_text": false})
        } else {
            json!({
                "model": self.target.model,
                "query": query,
                "documents": documents,
                "top_n": documents.len(),
            })
        };
        let text = self.post(&body).map_err(RerankError::Rerank)?;
        let scores = if self.target.source == "tei" {
            parse_tei_rerank(&text, documents.len())
        } else {
            parse_rerank(&text, documents.len())
        }
        .map_err(RerankError::Rerank)?;
        Ok(unit_scores(scores))
    }
}

fn parse_rerank(text: &str, expected: usize) -> Result<Vec<f32>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("ответ не JSON: {error}"))?;
    let results = value
        .get("results")
        .or_else(|| value.get("data"))
        .and_then(Value::as_array)
        .ok_or("в ответе нет списка `results`")?;
    parse_rerank_items(results, expected)
}

fn parse_tei_rerank(text: &str, expected: usize) -> Result<Vec<f32>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("ответ TEI не JSON: {error}"))?;
    let results = value.as_array().ok_or("ответ rerank TEI не массив")?;
    let scores = parse_rerank_items(results, expected)?;
    if scores.iter().any(|score| !(0.0..=1.0).contains(score)) {
        return Err("TEI вернул оценку вне диапазона 0…1 при raw_scores=false".into());
    }
    Ok(scores)
}

fn parse_rerank_items(results: &[Value], expected: usize) -> Result<Vec<f32>, String> {
    let mut scores: Vec<Option<f32>> = vec![None; expected];
    for item in results {
        let index = item
            .get("index")
            .and_then(Value::as_u64)
            .ok_or("в элементе ответа нет `index`")? as usize;
        let score = item
            .get("relevance_score")
            .or_else(|| item.get("score"))
            .and_then(Value::as_f64)
            .ok_or("в элементе ответа нет `relevance_score`")? as f32;
        let slot = scores.get_mut(index).ok_or_else(|| {
            format!("в ответе индекс {index} вне списка из {expected} документов")
        })?;
        if !score.is_finite() || slot.is_some() {
            return Err("провайдер вернул неконечную оценку или повторный индекс".into());
        }
        *slot = Some(score);
    }
    let got = scores.iter().flatten().count();
    if got != expected {
        return Err(format!(
            "провайдер вернул {got} оценок на {expected} документов"
        ));
    }
    Ok(scores.into_iter().flatten().collect())
}

/// Приводит оценки к диапазону 0…1. Cohere и совместимые провайдеры уже отдают его; сервер,
/// который отдаёт сырой логит (например, llama-server), получает сигмоиду (решение агента).
fn unit_scores(scores: Vec<f32>) -> Vec<f32> {
    if scores.iter().all(|score| (0.0..=1.0).contains(score)) {
        scores
    } else {
        scores
            .into_iter()
            .map(super::builtin_rerank::sigmoid)
            .collect()
    }
}

#[cfg(test)]
mod tests;
