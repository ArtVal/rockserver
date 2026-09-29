//! Search domain, provider boundaries, fallback semantics, and catalog boundary.

mod domain;
mod embedding;
mod in_memory;
mod llm;
mod query;
mod ranking;
mod semantic_filters;
mod service;
pub mod taxonomy;

#[cfg(test)]
mod tests;

pub use domain::{
    MIN_RELEVANCE_SCORE, RankedStation, RepositoryError, SearchConstraints, SearchOutcome,
    SearchQuery, Station, StationHealth, StationRepository,
};
pub use embedding::{
    Embedding, EmbeddingBackfill, EmbeddingBackfillResult, EmbeddingProvenance, EmbeddingProvider,
    EmbeddingProviderError, EmbeddingStore, EmbeddingStoreError, EmbeddingValidationError,
    MAX_EMBEDDING_DIMENSION, StationEmbeddingDocument,
};
pub use in_memory::{InMemoryStationRepository, UnavailableStationRepository};
pub use llm::{
    LlmProvider, LlmProviderError, LlmQueryParser, LlmRequest, MAX_LLM_INTENT_JSON_BYTES,
};
pub use query::{
    DeterministicQueryParser, QueryIntent, QueryParser, QueryParserError, QueryParserInput,
    SearchAction, normalize_query, tokenize,
};
pub use ranking::{METADATA_WEIGHT, SEMANTIC_WEIGHT, hybrid_score};
pub use semantic_filters::{
    SEMANTIC_LANGUAGE_FILTERS_ENV, SemanticLanguageClassifier, semantic_language_filters_enabled,
};
pub use service::SearchService;
