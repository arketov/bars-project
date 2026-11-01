//! Cross-encoder yes/no over generation APIs (ADR-0006).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{RemoteTarget, RerankError, Reranker};

const SYSTEM: &str = "Judge whether the Document meets the requirements based on the Query and the Instruct provided. Note that the answer can only be \"yes\" or \"no\".";
const INSTRUCT: &str = "Given a web search query, retrieve relevant passages that answer the query";
const SUFFIX: &str = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n";

pub struct LogprobReranker {
    target: RemoteTarget,
    agent: ureq::Agent,
    parallel: usize,
    context_tokens: usize,
    timeout: Duration,
}

impl LogprobReranker {
    pub fn new(target: RemoteTarget, parallel: usize, context_tokens: usize) -> Self {
        Self {
            target,
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
            parallel: parallel.clamp(1, 16),
            context_tokens,
            timeout: Duration::from_secs(60),
        }
    }

    fn context(&self, deadline: Instant) -> Result<usize, String> {
        if self.target.source == "ollama" {
            return Ok(self.context_tokens);
        }
        let base = self.target.base_url.trim_end_matches('/');
        let base = base.strip_suffix("/v1").unwrap_or(base);
        let value = self.request(&format!("{base}/api/v0/models"), None, deadline)?;
        let model = value["data"]
            .as_array()
            .and_then(|models| models.iter().find(|m| m["id"] == self.target.model))
            .ok_or("LM Studio не нашёл модель; загрузите её и укажите точный идентификатор")?;
        let actual = model["loaded_context_length"]
            .as_u64()
            .filter(|n| *n > 0)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("LM Studio не подтвердил загруженный контекст; загрузите модель перед rerank")?;
        Ok(self.context_tokens.min(actual))
    }

    fn pair(
        &self,
        query: &str,
        document: &str,
        context_tokens: usize,
        deadline: Instant,
    ) -> Result<f32, String> {
        let user = format!("<Instruct>: {INSTRUCT}\n<Query>: {query}\n<Document>: {document}");
        let ollama = self.target.source == "ollama";
        let base = self.target.base_url.trim_end_matches('/');
        let (url, body) = if ollama {
            let prompt =
                format!("<|im_start|>system\n{SYSTEM}<|im_end|>\n<|im_start|>user\n{user}{SUFFIX}");
            // Byte-level tokenizers cannot require more tokens than input bytes.
            // Reserve space for BOS and the single generated token; never truncate.
            if prompt.len().saturating_add(16) > context_tokens {
                return Err("пара длиннее безопасного контекста Ollama; увеличьте rerank.context_tokens или уменьшите чанк".into());
            }
            (
                format!("{base}/api/generate"),
                json!({
                    "model": self.target.model, "prompt": prompt, "raw": true,
                    "stream": false, "logprobs": true, "top_logprobs": 20,
                    "options": {"num_predict": 1, "temperature": 0, "repeat_penalty": 1,
                        "num_ctx": context_tokens}
                }),
            )
        } else {
            // LM Studio can ignore truncation:disabled. Use its loaded context,
            // with a conservative byte bound and allowance for the server template.
            if user.len().saturating_add(SYSTEM.len()).saturating_add(1024) > context_tokens {
                return Err("пара длиннее безопасного контекста LM Studio; увеличьте контекст модели и rerank.context_tokens или уменьшите чанк".into());
            }
            let url = if base.ends_with("/v1") {
                format!("{base}/responses")
            } else {
                format!("{base}/v1/responses")
            };
            (
                url,
                json!({
                    "model": self.target.model, "stream": false, "store": false,
                    "input": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": user}],
                    "max_output_tokens": 1, "temperature": 0,
                    "frequency_penalty": 0, "presence_penalty": 0,
                    "reasoning": {"effort": "none"}, "truncation": "disabled",
                    "include": ["message.output_text.logprobs"], "top_logprobs": 20
                }),
            )
        };
        let value = self.request(&url, Some(&body), deadline)?;
        let token = if ollama {
            if value["done"] != true
                || value["eval_count"].as_u64() != Some(1)
                || value["prompt_eval_count"]
                    .as_u64()
                    .is_none_or(|n| n == 0 || n >= context_tokens as u64)
                || value
                    .get("thinking")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
            {
                return Err("Ollama не подтвердил один текстовый токен и полный контекст".into());
            }
            first_token(&value["logprobs"])?
        } else {
            if value["usage"]["output_tokens"].as_u64() != Some(1)
                || value["usage"]["input_tokens"]
                    .as_u64()
                    .is_none_or(|n| n == 0 || n >= context_tokens as u64)
            {
                return Err("LM Studio не подтвердил один выходной токен и длину входа".into());
            }
            let output = value["output"]
                .as_array()
                .ok_or("нет output в ответе LM Studio")?;
            let message = output.first().ok_or("пустой output LM Studio")?;
            if message["type"] != "message" || output.len() != 1 {
                return Err(
                    "LM Studio вернул reasoning или несколько сообщений вместо оценки".into(),
                );
            }
            let content = message["content"]
                .as_array()
                .ok_or("нет content в ответе LM Studio")?;
            if content.len() != 1 || content[0]["type"] != "output_text" {
                return Err("LM Studio не вернул единственный текстовый токен".into());
            }
            first_token(&content[0]["logprobs"])?
        };
        score(
            token,
            self.target
                .model
                .to_ascii_lowercase()
                .contains("qwen3-reranker"),
        )
    }

    fn request(&self, url: &str, body: Option<&Value>, deadline: Instant) -> Result<Value, String> {
        let key = self.target.key.resolve().map_err(|e| e.to_string())?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or("истёк общий дедлайн rerank")?;
        let result = if let Some(body) = body {
            let mut request = self
                .agent
                .post(url)
                .config()
                .timeout_global(Some(remaining))
                .build();
            if let Some(key) = &key {
                request = request.header("Authorization", format!("Bearer {key}"));
            }
            request.send_json(body)
        } else {
            let mut request = self
                .agent
                .get(url)
                .config()
                .timeout_global(Some(remaining))
                .build();
            if let Some(key) = &key {
                request = request.header("Authorization", format!("Bearer {key}"));
            }
            request.call()
        };
        let redact = |message: String| match key.as_deref() {
            Some(key) if !key.is_empty() => message.replace(key, "***"),
            _ => message,
        };
        let mut response = result.map_err(|e| redact(e.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map_err(|e| redact(e.to_string()))?;
        if Instant::now() >= deadline {
            return Err("истёк общий дедлайн rerank".into());
        }
        if !(200..300).contains(&status) {
            return Err(format!(
                "провайдер ответил {status}: {}",
                redact(text).chars().take(300).collect::<String>()
            ));
        }
        let value: Value = serde_json::from_str(&text).map_err(|_| "неверный JSON провайдера")?;
        if value.get("error").is_some_and(|e| !e.is_null()) {
            return Err("провайдер сообщил об ошибке генерации".into());
        }
        Ok(value)
    }
}

fn first_token(value: &Value) -> Result<&Value, String> {
    let tokens = value.as_array().ok_or("нет logprobs первого токена")?;
    if tokens.len() != 1 {
        return Err("нужен ровно один токен logprobs".into());
    }
    Ok(&tokens[0])
}

fn score(token: &Value, exact: bool) -> Result<f32, String> {
    let top = token["top_logprobs"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or("пустые top_logprobs")?;
    let mut classes: [Option<f64>; 2] = [None, None];
    let mut tail = -10.0_f64;
    let mut seen = std::collections::HashSet::new();
    for candidate in top {
        let text = candidate["token"].as_str().ok_or("нет имени токена")?;
        if !seen.insert(text) {
            return Err("повтор токена в top_logprobs".into());
        }
        let lp = candidate["logprob"]
            .as_f64()
            .filter(|n| n.is_finite() && *n <= 0.0)
            .ok_or("неверный logprob")?;
        tail = tail.min(lp);
        let normalized = if exact {
            text.to_string()
        } else {
            text.trim().to_ascii_lowercase()
        };
        let index = match normalized.as_str() {
            "yes" => 0,
            "no" => 1,
            _ => continue,
        };
        classes[index] = Some(match classes[index] {
            None => lp,
            Some(previous) => {
                let max = previous.max(lp);
                max + ((previous - max).exp() + (lp - max).exp()).ln()
            }
        });
    }
    if classes == [None, None] {
        return Err("нет yes/no в logprobs; проверьте модель и шаблон".into());
    }
    let yes = classes[0].unwrap_or(tail);
    let no = classes[1].unwrap_or(tail);
    let max = yes.max(no);
    Ok(((yes - max).exp() / ((yes - max).exp() + (no - max).exp())) as f32)
}

impl Reranker for LogprobReranker {
    fn rerank(&mut self, query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let deadline = Instant::now() + self.timeout;
        let context = self.context(deadline).map_err(RerankError::Rerank)?;
        let next = AtomicUsize::new(0);
        let failed = AtomicBool::new(false);
        let results = Mutex::new(vec![None; documents.len()]);
        let error = Mutex::new(None);
        std::thread::scope(|scope| {
            for _ in 0..self.parallel.min(documents.len()) {
                scope.spawn(|| {
                    loop {
                        if failed.load(Ordering::Acquire) {
                            break;
                        }
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(document) = documents.get(i) else {
                            break;
                        };
                        match self.pair(query, document, context, deadline) {
                            Ok(score) => results.lock().expect("results lock")[i] = Some(score),
                            Err(message) => {
                                failed.store(true, Ordering::Release);
                                error.lock().expect("error lock").get_or_insert(message);
                                break;
                            }
                        }
                    }
                });
            }
        });
        if let Some(error) = error.into_inner().expect("error lock") {
            return Err(RerankError::Rerank(error));
        }
        if Instant::now() >= deadline {
            return Err(RerankError::Rerank("истёк общий дедлайн rerank".into()));
        }
        results
            .into_inner()
            .expect("results lock")
            .into_iter()
            .map(|score| {
                score.ok_or_else(|| RerankError::Rerank("неполная оценка кандидатов".into()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
