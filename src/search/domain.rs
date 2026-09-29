//! Domain models, constraints, and repository traits for catalog search.

use std::{collections::BTreeSet, error::Error, fmt, io};

use async_trait::async_trait;

use super::{
    embedding::Embedding,
    query::{QueryIntent, SearchAction, station_name_hint_queries},
};

/// Results below this score can be produced by semantic similarity alone and
/// are not reliable enough to claim that a station matches the requested genre.
pub const MIN_RELEVANCE_SCORE: f64 = 0.35;

/// A normalized station-search query understood by the deterministic search service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchQuery {
    pub action: SearchAction,
    /// Original request text after trimming leading and trailing whitespace.
    pub original: String,
    /// Locale used while interpreting the request.
    pub locale: String,
    /// Lowercase terms extracted from the original request.
    pub terms: Vec<String>,
    /// Recognized catalog tags extracted from the request.
    pub tags: Vec<String>,
    /// Language constraint inferred from the locale, when available.
    pub language: Option<String>,
    /// Country constraint inferred from request terms, when available.
    pub country_code: Option<String>,
    /// Number of original query terms before transliteration expansion.
    /// Used as the denominator for score calculation so that alias terms
    /// don't dilute match quality.
    pub core_term_count: usize,
    /// Cleaned query string (stop-words removed) for full-text search.
    pub raw_query: String,
    /// When true, station-name matching should outrank generic tag similarity.
    pub prefer_station_name: bool,
    /// Ordered station-name phrases derived from the original command.
    pub station_name_hint_queries: Vec<String>,
}

impl SearchQuery {
    pub(crate) fn from_intent(original: String, locale: String, intent: QueryIntent) -> Self {
        let station_name_hint_queries = station_name_hint_queries(&original);
        Self {
            action: intent.action,
            original,
            locale,
            core_term_count: intent.core_term_count,
            raw_query: intent.raw_query,
            prefer_station_name: !station_name_hint_queries.is_empty(),
            station_name_hint_queries,
            terms: intent.terms,
            tags: intent.tags,
            language: intent.language,
            country_code: intent.country_code,
        }
    }
}

/// Constraints that affect which and how many stations are returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchConstraints {
    /// Maximum number of ranked stations to return.
    pub limit: usize,
    /// Station identifiers that must not appear in the result.
    pub excluded_station_ids: BTreeSet<String>,
}

/// A station record exposed by a catalog repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Station {
    /// Stable RockServer station identifier.
    pub id: String,
    /// Human-readable station name.
    pub name: String,
    /// Direct, playable stream URL.
    pub stream_url: String,
    /// Optional public home page for the station.
    pub homepage_url: Option<String>,
    /// Server-owned prepared icon URL, present only when a ready artifact exists.
    pub favicon_url: Option<String>,
    /// Normalized searchable station tags.
    pub tags: Vec<String>,
    /// ISO 639 language code, when known.
    pub language: Option<String>,
    /// ISO 3166-1 alpha-2 country code, when known.
    pub country_code: Option<String>,
    /// Audio codec, when known.
    pub codec: Option<String>,
    /// Stream bitrate in kilobits per second, when known.
    pub bitrate_kbps: Option<u32>,
    /// Current catalog health classification.
    pub health: StationHealth,
}

/// Health information stored with a station record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationHealth {
    /// The catalog considers the station healthy.
    Healthy,
    /// The catalog considers the station degraded.
    Degraded,
    /// The catalog has no health information for the station.
    Unknown,
}

/// A station selected by the search service with its deterministic score.
#[derive(Clone, Debug, PartialEq)]
pub struct RankedStation {
    /// The matching catalog record.
    pub station: Station,
    /// Match score in the inclusive range from zero to one.
    pub score: f64,
    /// Short explanation of the metadata that matched the request.
    pub reason: String,
}

/// Catalog access boundary used by the search domain.
#[async_trait]
pub trait StationRepository {
    /// Searches the catalog using domain-normalized input and deterministic ordering rules.
    async fn search(
        &self,
        query: &SearchQuery,
        constraints: &SearchConstraints,
        embedding: Option<&Embedding>,
    ) -> Result<Vec<RankedStation>, RepositoryError>;

    /// Verifies that the repository dependency can currently serve requests.
    async fn check_readiness(&self) -> Result<(), RepositoryError>;

    /// Lists a bounded, deterministic public view of active playable stations.
    ///
    /// Implementations that do not own a browsable catalog may leave the default safe failure.
    async fn list_public(
        &self,
        _after_id: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        Err(RepositoryError::new(
            "public catalog listing",
            io::Error::other("public catalog listing is unavailable"),
        ))
    }

    /// Lists a bounded administrator catalog page in stable identifier order.
    ///
    /// The optional query matches a station name or a tag in the repository, so callers never
    /// need to load the catalog to filter it in a browser.
    async fn list_admin(
        &self,
        _query: Option<&str>,
        _after_id: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        Err(RepositoryError::new(
            "administrator catalog listing",
            io::Error::other("administrator catalog listing is unavailable"),
        ))
    }

    /// Resolves one active playable station by its stable public identifier.
    async fn get_public(&self, id: &str) -> Result<Option<Station>, RepositoryError> {
        Ok(self
            .list_public(None, usize::MAX)
            .await?
            .into_iter()
            .find(|station| station.id == id))
    }
}

/// An operational repository failure safe to map at service boundaries.
#[derive(Debug)]
pub struct RepositoryError {
    operation: &'static str,
    source: Box<dyn Error + Send + Sync>,
}

impl RepositoryError {
    /// Wraps an implementation error without exposing provider details to HTTP clients.
    pub(crate) fn new(operation: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            operation,
            source: Box::new(source),
        }
    }
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "station repository {} failed", self.operation)
    }
}

impl Error for RepositoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Interpreted search result returned to the HTTP transport layer.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchOutcome {
    /// Normalized structured interpretation returned to the caller.
    pub query: SearchQuery,
    /// Ranked stations returned by the repository.
    pub stations: Vec<RankedStation>,
}
