//! Search orchestration with deterministic parser and metadata fallbacks.

use std::{sync::Arc, time::Instant};

use super::{
    domain::{
        MIN_RELEVANCE_SCORE, RankedStation, RepositoryError, SearchConstraints, SearchOutcome,
        SearchQuery, Station, StationRepository,
    },
    embedding::{Embedding, EmbeddingProvider},
    query::{
        DeterministicQueryParser, QueryParser, QueryParserInput, has_explicit_country_request,
        tokenize, validate_intent,
    },
    semantic_filters::SemanticLanguageClassifier,
    taxonomy::{genre_ancestors, station_matches_requested_genre},
};

/// Search orchestration with deterministic parser and metadata fallbacks.
#[derive(Clone)]
pub struct SearchService {
    repository: Arc<dyn StationRepository + Send + Sync>,
    query_parser: Arc<dyn QueryParser>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    language_classifier: Option<Arc<SemanticLanguageClassifier>>,
}

impl SearchService {
    /// Creates metadata-only search using the deterministic query parser.
    pub fn new(repository: Arc<dyn StationRepository + Send + Sync>) -> Self {
        Self {
            repository,
            query_parser: Arc::new(DeterministicQueryParser),
            embedding_provider: None,
            language_classifier: None,
        }
    }

    /// Creates search with replaceable parser and optional embedding provider boundaries.
    ///
    /// Any parser or embedding failure degrades to deterministic metadata behavior.
    pub fn with_providers(
        repository: Arc<dyn StationRepository + Send + Sync>,
        query_parser: Arc<dyn QueryParser>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    ) -> Self {
        Self::with_providers_and_language_classifier(
            repository,
            query_parser,
            embedding_provider,
            None,
        )
    }

    /// Creates search with an optional confidence-gated semantic language classifier.
    ///
    /// The classifier is deliberately separate from the station ranking embedding so a
    /// deployment can disable hard language filters without disabling semantic ranking.
    pub fn with_providers_and_language_classifier(
        repository: Arc<dyn StationRepository + Send + Sync>,
        query_parser: Arc<dyn QueryParser>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
        language_classifier: Option<Arc<SemanticLanguageClassifier>>,
    ) -> Self {
        Self {
            repository,
            query_parser,
            embedding_provider,
            language_classifier,
        }
    }

    /// Returns a bounded public catalog page without exposing repository internals.
    pub async fn public_catalog(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        self.repository.list_public(after_id, limit).await
    }

    /// Returns a server-filtered administrator catalog page without exposing repository internals.
    pub async fn admin_catalog(
        &self,
        query: Option<&str>,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        self.repository.list_admin(query, after_id, limit).await
    }

    /// Returns one active public catalog station by stable identifier.
    pub async fn public_station(&self, id: &str) -> Result<Option<Station>, RepositoryError> {
        self.repository.get_public(id).await
    }

    /// Returns matching stations ordered by score descending and station ID ascending.
    ///
    /// When the exact genre filter produces no results, the search progressively
    /// relaxes the filter using the genre hierarchy (e.g. `"heavy metal"` falls
    /// back to stations tagged `"rock"`).  If even the broadest ancestor yields
    /// nothing, the genre constraint is dropped entirely while keeping the
    /// minimum relevance score gate.
    pub async fn search(
        &self,
        query: &SearchQuery,
        constraints: &SearchConstraints,
    ) -> Result<Vec<RankedStation>, RepositoryError> {
        let embedding_started_at = Instant::now();
        let embedding = self.query_embedding(&query.original).await;
        self.search_with_embedding(
            query,
            constraints,
            embedding.as_ref(),
            embedding_started_at.elapsed().as_millis(),
            true,
        )
        .await
    }

    /// Searches with an already computed request embedding to avoid duplicate local inference.
    async fn search_with_embedding(
        &self,
        query: &SearchQuery,
        constraints: &SearchConstraints,
        embedding: Option<&Embedding>,
        embedding_elapsed_ms: u128,
        log_input: bool,
    ) -> Result<Vec<RankedStation>, RepositoryError> {
        let repository_started_at = Instant::now();
        let mut stations = self
            .repository
            .search(query, constraints, embedding)
            .await?;
        tracing::debug!(
            embedding_elapsed_ms,
            repository_elapsed_ms = repository_started_at.elapsed().as_millis(),
            result_count = stations.len(),
            "station search stages completed"
        );
        stations.retain(|station| station.score >= MIN_RELEVANCE_SCORE);

        let with_genre: Vec<_> = stations
            .iter()
            .filter(|s| station_matches_requested_genre(&query.tags, &s.station.tags))
            .cloned()
            .collect();
        if !with_genre.is_empty() {
            return Ok(with_genre);
        }

        // Progressively broaden by walking up the genre hierarchy.
        let parent_tags = broaden_tags(&query.tags);
        if !parent_tags.is_empty() {
            let with_parents: Vec<_> = stations
                .iter()
                .filter(|s| station_matches_requested_genre(&parent_tags, &s.station.tags))
                .cloned()
                .collect();
            if !with_parents.is_empty() {
                if log_input {
                    tracing::info!(
                        original_tags = ?query.tags,
                        broadened_tags = ?parent_tags,
                        "genre fallback: broadened to parent tags"
                    );
                }
                return Ok(with_parents);
            }
        }

        // Last resort: drop genre filter, keep only MIN_RELEVANCE_SCORE gate.
        if !stations.is_empty() && log_input {
            tracing::info!(
                original_tags = ?query.tags,
                "genre fallback: dropped genre filter entirely"
            );
        }
        Ok(stations)
    }

    /// Interprets request-only input and searches without exposing catalog data to providers.
    pub async fn interpret_and_search(
        &self,
        input: QueryParserInput,
        constraints: &SearchConstraints,
    ) -> Result<SearchOutcome, RepositoryError> {
        self.interpret_and_search_with_input_logs(input, constraints, true)
            .await
    }

    /// Interprets and searches without recording user-supplied text or derived terms.
    pub async fn interpret_and_search_private(
        &self,
        input: QueryParserInput,
        constraints: &SearchConstraints,
    ) -> Result<SearchOutcome, RepositoryError> {
        self.interpret_and_search_with_input_logs(input, constraints, false)
            .await
    }

    async fn interpret_and_search_with_input_logs(
        &self,
        input: QueryParserInput,
        constraints: &SearchConstraints,
        log_input: bool,
    ) -> Result<SearchOutcome, RepositoryError> {
        let parser_started_at = Instant::now();
        // Query parsing (which may call external LLM) and local text embedding generation
        // are independent: the embedding only needs the raw query text. Run them concurrently
        // via `tokio::join!` so local embedding generation finishes during the network round-trip.
        let (parse_result, (embedding, embedding_elapsed_ms)) =
            tokio::join!(self.query_parser.parse(&input), async {
                let start = Instant::now();
                let embedding = self.query_embedding(&input.query).await;
                (embedding, start.elapsed().as_millis())
            },);
        let parser_elapsed_ms = parser_started_at.elapsed().as_millis();

        let intent = match parse_result.and_then(validate_intent) {
            Ok(intent) => intent,
            Err(error) => {
                tracing::warn!(%error, "query parser failed; using deterministic metadata fallback");
                DeterministicQueryParser
                    .parse(&input)
                    .await
                    .expect("deterministic query parser cannot fail")
            }
        };
        // Providers (LLMs) may occasionally return:
        // - both empty `terms` and `tags`
        // - only `tags` but no `terms` (hurts station name matching)
        //
        // For station-name matching deterministic tokenization is more reliable.
        let mut intent = intent;
        if intent.terms.is_empty() {
            let deterministic = DeterministicQueryParser
                .parse(&input)
                .await
                .expect("deterministic query parser cannot fail");
            let deterministic = validate_intent(deterministic.clone()).unwrap_or(deterministic);

            if intent.tags.is_empty() {
                // Full fallback: provider returned nothing actionable.
                intent = deterministic;
            } else {
                // Partial fallback: keep provider's hard genre tags, but use deterministic
                // `terms` for token/sub-token and trigram name matching.
                intent.terms = deterministic.terms;
                intent.raw_query = deterministic.raw_query;
                intent.core_term_count = deterministic.core_term_count;
            }
        }

        let request_terms = tokenize(&input.query);
        if intent.language.is_none()
            && !has_explicit_country_request(&request_terms)
            && let (Some(classifier), Some(embedding)) = (&self.language_classifier, &embedding)
        {
            if let Some(language) = classifier.classify(embedding) {
                tracing::debug!(
                    language = %language.code,
                    score = language.score,
                    margin = language.margin,
                    "semantic language filter accepted"
                );
                intent.language = Some(language.code);
            } else {
                tracing::debug!("semantic language filter rejected as low confidence");
            }
        }

        let query = SearchQuery::from_intent(input.query, input.locale, intent);
        if log_input {
            tracing::debug!(
                parser_elapsed_ms,
                original = %query.original,
                terms = ?query.terms,
                tags = ?query.tags,
                core_term_count = query.core_term_count,
                language = ?query.language,
                country_code = ?query.country_code,
                "search query parsed"
            );
        }
        let stations = self
            .search_with_embedding(
                &query,
                constraints,
                embedding.as_ref(),
                embedding_elapsed_ms,
                log_input,
            )
            .await?;
        if stations.is_empty() {
            if log_input {
                tracing::debug!(original = %query.original, "search returned zero results");
            }
        } else if log_input {
            for (i, s) in stations.iter().take(5).enumerate() {
                tracing::debug!(
                    rank = i + 1,
                    id = %s.station.id,
                    name = %s.station.name,
                    score = s.score,
                    reason = %s.reason,
                    "search result"
                );
            }
        }
        Ok(SearchOutcome { query, stations })
    }

    /// Checks whether the configured catalog backend is currently available.
    pub async fn check_readiness(&self) -> Result<(), RepositoryError> {
        self.repository.check_readiness().await
    }

    async fn query_embedding(&self, text: &str) -> Option<Embedding> {
        let provider = self.embedding_provider.as_ref()?;
        match provider.embed(text).await {
            Ok(embedding) => Some(embedding),
            Err(error) => {
                tracing::warn!(%error, "embedding provider failed; using metadata fallback");
                None
            }
        }
    }
}

/// Replaces each genre tag with its nearest parent from the hierarchy.
///
/// Mood tags pass through unchanged. Tags without a parent are dropped
/// because broadening is only meaningful for hierarchical genres.
fn broaden_tags(tags: &[String]) -> Vec<String> {
    let mut broadened = std::collections::BTreeSet::new();
    for tag in tags {
        for ancestor in genre_ancestors(tag) {
            broadened.insert(ancestor.to_owned());
        }
    }
    broadened.into_iter().collect()
}
