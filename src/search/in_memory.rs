//! In-memory and unavailable station repository implementations.

use std::io;

use async_trait::async_trait;

use super::{
    domain::{
        RankedStation, RepositoryError, SearchConstraints, SearchQuery, Station, StationHealth,
        StationRepository,
    },
    embedding::Embedding,
    ranking::rank_stations,
};

/// A small, built-in station catalog for deterministic local search.
#[derive(Clone, Debug, Default)]
pub struct InMemoryStationRepository {
    stations: Vec<Station>,
}

impl InMemoryStationRepository {
    /// Creates the validated pinned shared catalog used when PostgreSQL is unavailable.
    ///
    /// A malformed, checksum-invalid, or semantically invalid production pin is returned as a
    /// startup/readiness error; it is never replaced with a development fixture.
    pub fn with_builtin_catalog() -> Result<Self, RepositoryError> {
        crate::catalog::PinnedSharedCatalog::load()
            .map(Self::from_pinned_catalog)
            .map_err(|error| RepositoryError::new("shared catalog preflight", error))
    }

    /// Builds the in-memory active view from an already preflighted immutable release.
    pub(crate) fn from_pinned_catalog(catalog: crate::catalog::PinnedSharedCatalog) -> Self {
        Self {
            stations: catalog
                .stations()
                .iter()
                .filter_map(|station| {
                    station
                        .streams
                        .iter()
                        .find(|stream| stream.is_primary)
                        .map(|stream| Station {
                            id: station.id.clone(),
                            name: station.name.clone(),
                            stream_url: stream.stream_url.clone(),
                            homepage_url: station.homepage_url.clone(),
                            favicon_url: None,
                            tags: station.tags.clone(),
                            language: station.language.clone(),
                            country_code: station.country_code.clone(),
                            codec: stream.codec.clone(),
                            bitrate_kbps: stream.bitrate_kbps,
                            health: StationHealth::Unknown,
                        })
                })
                .collect(),
        }
    }

    /// Provides a compact deterministic fixture for isolated unit tests.
    #[cfg(test)]
    fn legacy_fixture_catalog() -> Self {
        Self {
            stations: vec![
                station(
                    "station-ambient-001",
                    "Arctic Ambient",
                    "https://streams.example.com/arctic-ambient.mp3",
                    StationMetadata {
                        homepage_url: Some("https://example.com/arctic-ambient"),
                        tags: &["ambient", "calm", "electronic", "instrumental"],
                        language: Some("en"),
                        country_code: Some("IS"),
                        codec: Some("MP3"),
                        bitrate_kbps: Some(160),
                    },
                ),
                station(
                    "station-jazz-001",
                    "Quiet Jazz Radio",
                    "https://streams.example.com/quiet-jazz.mp3",
                    StationMetadata {
                        homepage_url: Some("https://example.com/quiet-jazz"),
                        tags: &["calm", "instrumental", "jazz"],
                        language: Some("en"),
                        country_code: Some("US"),
                        codec: Some("MP3"),
                        bitrate_kbps: Some(192),
                    },
                ),
                station(
                    "station-jazz-002",
                    "Midnight Jazz Lounge",
                    "https://streams.example.com/midnight-jazz.aac",
                    StationMetadata {
                        homepage_url: None,
                        tags: &["jazz", "smooth"],
                        language: Some("en"),
                        country_code: Some("GB"),
                        codec: Some("AAC"),
                        bitrate_kbps: Some(128),
                    },
                ),
                station(
                    "station-rock-001",
                    "Highway Rock",
                    "https://streams.example.com/highway-rock.aac",
                    StationMetadata {
                        homepage_url: None,
                        tags: &["classic rock", "rock", "upbeat"],
                        language: Some("en"),
                        country_code: Some("GB"),
                        codec: Some("AAC"),
                        bitrate_kbps: Some(128),
                    },
                ),
                station(
                    "station-rock-002",
                    "Heritage Rock",
                    "https://streams.example.com/heritage-rock.mp3",
                    StationMetadata {
                        homepage_url: Some("https://example.com/heritage-rock"),
                        tags: &["classic rock", "rock"],
                        language: Some("en"),
                        country_code: Some("US"),
                        codec: Some("MP3"),
                        bitrate_kbps: Some(192),
                    },
                ),
                station(
                    "station-rock-ru-001",
                    "Радио Рок",
                    "https://streams.example.com/radio-rock-ru.mp3",
                    StationMetadata {
                        homepage_url: Some("https://example.com/radio-rock-ru"),
                        tags: &["classic rock", "rock"],
                        language: Some("ru"),
                        country_code: Some("RU"),
                        codec: Some("MP3"),
                        bitrate_kbps: Some(128),
                    },
                ),
                station(
                    "station-metal-001",
                    "Iron Forge Radio",
                    "https://streams.example.com/iron-forge.mp3",
                    StationMetadata {
                        homepage_url: None,
                        tags: &["heavy metal", "metal"],
                        language: Some("en"),
                        country_code: Some("US"),
                        codec: Some("MP3"),
                        bitrate_kbps: Some(192),
                    },
                ),
            ],
        }
    }

    /// Supplies the former compact fixture solely for unit tests that isolate ranking mechanics.
    #[cfg(test)]
    pub(crate) fn with_legacy_fixture_catalog() -> Self {
        Self::legacy_fixture_catalog()
    }
}

#[async_trait]
impl StationRepository for InMemoryStationRepository {
    async fn search(
        &self,
        query: &SearchQuery,
        constraints: &SearchConstraints,
        _embedding: Option<&Embedding>,
    ) -> Result<Vec<RankedStation>, RepositoryError> {
        Ok(rank_stations(&self.stations, query, constraints))
    }

    async fn check_readiness(&self) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn list_public(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        // The stable-ID cursor contract requires ascending station-ID order; the PostgreSQL
        // repository enforces the same ORDER BY, but the pinned catalog is not pre-sorted.
        let mut page: Vec<&Station> = self
            .stations
            .iter()
            .filter(|station| after_id.is_none_or(|after| station.id.as_str() > after))
            .collect();
        page.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(page.into_iter().take(limit).cloned().collect())
    }

    async fn list_admin(
        &self,
        query: Option<&str>,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        let query = query.map(str::to_lowercase);
        // Same ascending station-ID contract as the public listing and the PostgreSQL backend.
        let mut page: Vec<&Station> = self
            .stations
            .iter()
            .filter(|station| {
                query.as_ref().is_none_or(|needle| {
                    station.name.to_lowercase().contains(needle)
                        || station
                            .tags
                            .iter()
                            .any(|tag| tag.to_lowercase().contains(needle))
                })
            })
            .filter(|station| after_id.is_none_or(|after| station.id.as_str() > after))
            .collect();
        page.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(page.into_iter().take(limit).cloned().collect())
    }

    async fn get_public(&self, id: &str) -> Result<Option<Station>, RepositoryError> {
        Ok(self
            .stations
            .iter()
            .find(|station| station.id == id)
            .cloned())
    }
}

/// Repository used only to surface an unavailable pinned catalog through readiness and request errors.
#[derive(Clone, Debug)]
pub struct UnavailableStationRepository {
    reason: String,
}

impl UnavailableStationRepository {
    /// Preserves a sanitized catalog-preflight failure without substituting unrelated station data.
    pub fn from_preflight_error(error: RepositoryError) -> Self {
        Self {
            reason: error.to_string(),
        }
    }

    fn failure(&self) -> RepositoryError {
        RepositoryError::new(
            "shared catalog unavailable",
            io::Error::other(self.reason.clone()),
        )
    }
}

#[async_trait]
impl StationRepository for UnavailableStationRepository {
    async fn search(
        &self,
        _query: &SearchQuery,
        _constraints: &SearchConstraints,
        _embedding: Option<&Embedding>,
    ) -> Result<Vec<RankedStation>, RepositoryError> {
        Err(self.failure())
    }

    async fn check_readiness(&self) -> Result<(), RepositoryError> {
        Err(self.failure())
    }
}

#[cfg(test)]
struct StationMetadata<'a> {
    homepage_url: Option<&'a str>,
    tags: &'a [&'a str],
    language: Option<&'a str>,
    country_code: Option<&'a str>,
    codec: Option<&'a str>,
    bitrate_kbps: Option<u32>,
}

#[cfg(test)]
fn station(id: &str, name: &str, stream_url: &str, metadata: StationMetadata<'_>) -> Station {
    Station {
        id: id.to_owned(),
        name: name.to_owned(),
        stream_url: stream_url.to_owned(),
        homepage_url: metadata.homepage_url.map(str::to_owned),
        favicon_url: None,
        tags: metadata.tags.iter().map(|tag| (*tag).to_owned()).collect(),
        language: metadata.language.map(str::to_owned),
        country_code: metadata.country_code.map(str::to_owned),
        codec: metadata.codec.map(str::to_owned),
        bitrate_kbps: metadata.bitrate_kbps,
        health: StationHealth::Healthy,
    }
}
