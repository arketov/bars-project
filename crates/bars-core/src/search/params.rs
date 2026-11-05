//! Параметры поиска: настройки из `[search]` и вызов агента (`docs/features/settings/settings.md`, Т-18, Т-24).

/// Настройки поиска: таблица `[search]` из `settings.toml` и `.bars/config.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchSettings {
    /// Сколько кандидатов берёт каждый из двух поисков.
    pub candidates: usize,
    /// Сколько лучших после слияния уходит в реранк.
    pub rerank_candidates: usize,
    /// Результатов без реранка.
    pub results: usize,
    /// Результатов с реранком.
    pub results_reranked: usize,
    /// Единый порог по опоре оценки: по оценке rerank, а без rerank — по косинусу (ADR-0007).
    pub min_score: f32,
    /// Веса добавок к опоре и свёртка разделов (схема баллов).
    pub weights: ScoreWeights,
    /// Пределы, в которых агент может задать свой порог: от и до.
    pub agent_min_score: (f32, f32),
    pub max_results: usize,
    pub max_candidates: usize,
    pub max_rerank_candidates: usize,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            candidates: 30,
            rerank_candidates: 15,
            results: 5,
            results_reranked: 3,
            // Измерено шагом 2.3.7 (docs/knowledge/search-scores.md): с rerank 0,2 снижает долю
            // ложных ответов на запросах без ответа с 15 до 9 % ценой одного запроса в Hit@3;
            // косинус при этом пороге не режет ничего (до 0,4 почти ничего не отделяет: 87 % ложных ответов).
            min_score: 0.2,
            weights: ScoreWeights::default(),
            agent_min_score: (0.0, 1.0),
            max_results: 20,
            max_candidates: 100,
            max_rerank_candidates: 50,
        }
    }
}

/// Веса схемы баллов `S = a + w_c·c + w_f·f + w_p·p + w_l·l` (`docs/features/search/hybrid-search.md`,
/// ADR-0007). Значения проверены бенчем шага 2.3.7 (`docs/knowledge/search-scores.md`): сетки
/// весов и перекрёстная проверка не нашли схемы лучше этих на запросах, которых подбор не видел;
/// это константы продукта (OQ-278). В `settings.toml` не выставляются: бенч задаёт их кодом.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreWeights {
    /// `w_c`: вес косинуса, когда опора — rerank; без rerank косинус сам опора.
    pub cosine: f32,
    /// `w_f`: слова запроса по полям (составной `bm25`, нормирован на лучший балл списка).
    pub fulltext: f32,
    /// `w_p`: точное совпадение (идентификаторы, фраза, пары слов подряд).
    pub exact: f32,
    /// `w_l`: связи документов (сигнал `l`); 0 — сигнал не считается.
    pub links: f32,
    /// Бонус разделу за каждый найденный кусок сверх лучшего (`fold_bonus`).
    pub fold_bonus: f32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            cosine: 0.0,
            fulltext: 0.1,
            exact: 0.1,
            links: 0.0,
            fold_bonus: 0.0,
        }
    }
}

/// Вид оценки результата: у опоры есть абсолютный смысл, поэтому к ней применяется порог.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScoreKind {
    /// Оценка rerank-модели.
    Rerank,
    /// Косинусная близость вектора чанка к вектору запроса.
    #[default]
    Cosine,
    /// Опоры нет (ни rerank, ни embed): порядок по словам и точному совпадению, порога нет.
    None,
}

/// Запрос на поиск: то, что передают окно, агент или CLI. Пустые поля — значения из настроек.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchRequest {
    pub query: String,
    /// Модули (Т-14); корневой модуль — `.`. Пустой список вместе с пустым `files` — весь проект.
    pub modules: Vec<String>,
    /// Файлы относительно корня документации (Т-14).
    pub files: Vec<String>,
    pub results: Option<usize>,
    pub candidates: Option<usize>,
    pub rerank_candidates: Option<usize>,
    pub min_score: Option<f32>,
}

/// Параметры, с которыми поиск выполнен на самом деле. Они идут в ответ: агент видит, что его
/// значение прижато к пределу (Т-24).
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedParams {
    pub results: usize,
    pub candidates: usize,
    pub rerank_candidates: usize,
    pub min_score: f32,
    pub score_kind: ScoreKind,
    /// Хотя бы одно значение из вызова вышло за пределы и прижато.
    pub clamped: bool,
    /// Веса, с которыми сложены баллы.
    pub weights: ScoreWeights,
    /// Сигналы, посчитанные для ответа: `rerank`, `cosine`, `fulltext`, `exact`, `links`.
    pub signals: Vec<&'static str>,
}

impl SearchSettings {
    /// Складывает настройки и вызов. Значения вызова прижимаются к пределам; настройки
    /// пользователя не трогаются.
    pub fn resolve(&self, request: &SearchRequest, reranked: bool) -> AppliedParams {
        let mut clamped = false;
        let mut limit = |value: usize, max: usize| {
            let applied = value.clamp(1, max.max(1));
            clamped |= applied != value;
            applied
        };
        let results = request.results.map_or(
            if reranked {
                self.results_reranked
            } else {
                self.results
            },
            |value| limit(value, self.max_results),
        );
        let candidates = request
            .candidates
            .map_or(self.candidates, |value| limit(value, self.max_candidates));
        let rerank_candidates = request
            .rerank_candidates
            .map_or(self.rerank_candidates, |value| {
                limit(value, self.max_rerank_candidates)
            });
        let min_score = request.min_score.map_or(self.min_score, |value| {
            let (low, high) = self.agent_min_score;
            let applied = value.clamp(low, high);
            clamped |= applied != value;
            applied
        });
        AppliedParams {
            results,
            candidates,
            rerank_candidates,
            min_score,
            score_kind: if reranked {
                ScoreKind::Rerank
            } else {
                ScoreKind::Cosine
            },
            clamped,
            weights: self.weights,
            signals: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_settings_without_a_rerank() {
        let applied = SearchSettings::default().resolve(&SearchRequest::default(), false);
        assert_eq!(applied.results, 5);
        assert_eq!(applied.candidates, 30);
        assert_eq!(applied.min_score, 0.2);
        assert_eq!(applied.score_kind, ScoreKind::Cosine);
        assert!(!applied.clamped);
    }

    #[test]
    fn rerank_changes_result_count_and_score_kind_but_not_the_single_threshold() {
        let applied = SearchSettings::default().resolve(&SearchRequest::default(), true);
        assert_eq!(applied.results, 3);
        assert_eq!(applied.min_score, 0.2);
        assert_eq!(applied.score_kind, ScoreKind::Rerank);
    }

    #[test]
    fn explicit_values_inside_the_limits_are_applied() {
        let request = SearchRequest {
            results: Some(7),
            candidates: Some(40),
            rerank_candidates: Some(20),
            min_score: Some(0.4),
            ..SearchRequest::default()
        };
        let applied = SearchSettings::default().resolve(&request, true);
        assert_eq!(
            (
                applied.results,
                applied.candidates,
                applied.rerank_candidates
            ),
            (7, 40, 20)
        );
        assert_eq!(applied.min_score, 0.4);
        assert!(!applied.clamped);
    }

    #[test]
    fn values_over_the_limits_are_clamped_and_reported() {
        let request = SearchRequest {
            results: Some(500),
            candidates: Some(1000),
            rerank_candidates: Some(1000),
            ..SearchRequest::default()
        };
        let applied = SearchSettings::default().resolve(&request, true);
        assert_eq!(
            (
                applied.results,
                applied.candidates,
                applied.rerank_candidates
            ),
            (20, 100, 50)
        );
        assert!(applied.clamped);
    }

    #[test]
    fn zero_results_is_raised_to_one() {
        let request = SearchRequest {
            results: Some(0),
            ..SearchRequest::default()
        };
        let applied = SearchSettings::default().resolve(&request, false);
        assert_eq!(applied.results, 1);
        assert!(applied.clamped);
    }

    #[test]
    fn agent_threshold_is_clamped_to_the_users_range() {
        let settings = SearchSettings {
            agent_min_score: (0.3, 0.8),
            ..SearchSettings::default()
        };
        let low = SearchRequest {
            min_score: Some(0.0),
            ..SearchRequest::default()
        };
        let high = SearchRequest {
            min_score: Some(0.95),
            ..SearchRequest::default()
        };
        let applied_low = settings.resolve(&low, false);
        let applied_high = settings.resolve(&high, false);
        assert_eq!(applied_low.min_score, 0.3);
        assert_eq!(applied_high.min_score, 0.8);
        assert!(applied_low.clamped && applied_high.clamped);
    }

    #[test]
    fn users_own_threshold_is_not_clamped() {
        let settings = SearchSettings {
            min_score: 0.9,
            agent_min_score: (0.0, 0.5),
            ..SearchSettings::default()
        };
        let applied = settings.resolve(&SearchRequest::default(), false);
        assert_eq!(applied.min_score, 0.9);
        assert!(!applied.clamped);
    }
}
