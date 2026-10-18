use bars_core::models::{Embedder, EmbedError, ModelFingerprint, QWEN3_QUERY_PREFIX};
use llama_cpp_2::{context::LlamaContext, model::LlamaModel, llama_batch::LlamaBatch};

pub struct Gguf<'a> {
    pub model: &'a LlamaModel,
    pub ctx: LlamaContext<'a>,
    pub fp: ModelFingerprint,
}
impl Embedder for Gguf<'_> {
    fn fingerprint(&self) -> &ModelFingerprint { &self.fp }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut batch = LlamaBatch::new(8192, 1);
        texts.iter().map(|text| {
            let tokens = self.model.vocab().tokenize(text.as_bytes(), true, true);
            if tokens.is_empty() || tokens.len() > 8192 {
                return Err(EmbedError::Rejected(format!("длина {} токенов", tokens.len())));
            }
            self.ctx.clear_kv_cache(); batch.clear();
            batch.add_sequence(&tokens, 0, false).map_err(|e| EmbedError::Embed(e.to_string()))?;
            self.ctx.decode(&mut batch).map_err(|e| EmbedError::Embed(e.to_string()))?;
            let v = self.ctx.embeddings_seq_ith(0).map_err(|e| EmbedError::Embed(e.to_string()))?;
            if v.len() != self.fp.dim { return Err(EmbedError::Dimension { expected: self.fp.dim, got:v.len() }); }
            let norm = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
            if norm <= 0.0 || !norm.is_finite() { return Err(EmbedError::Embed("некорректный вектор".into())); }
            Ok(v.iter().map(|x| (*x as f64/norm) as f32).collect())
        }).collect()
    }
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        Ok(self.embed(&[&format!("{QWEN3_QUERY_PREFIX}{text}")])?.remove(0))
    }
}
