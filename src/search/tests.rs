//! Search-service regression tests kept beside the domain behavior they exercise.

use std::{
    collections::BTreeSet,
    io,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;

use super::{
    DeterministicQueryParser, Embedding, EmbeddingProvider, EmbeddingProviderError,
    InMemoryStationRepository, QueryIntent, QueryParser, QueryParserError, QueryParserInput,
    RankedStation, RepositoryError, SearchAction, SearchConstraints, SearchQuery, SearchService,
    SemanticLanguageClassifier, StationRepository, UnavailableStationRepository, normalize_query,
};

#[tokio::test]
async fn equal_scores_are_ordered_by_station_id() {
    let service = SearchService::new(Arc::new(
        InMemoryStationRepository::with_legacy_fixture_catalog(),
    ));
    let query = normalize_query("rock".to_owned(), "en-US".to_owned());
    let constraints = SearchConstraints {
        limit: 10,
        offset: 0,
        excluded_station_ids: BTreeSet::new(),
    };

    let ids = service
        .search(&query, &constraints)
        .await
        .unwrap()
        .into_iter()
        .map(|station| station.station.id)
        .collect::<Vec<_>>();

    // "Радио Рок" scores higher because transliteration adds "рок"
    // which substring-matches in its name, boosting its score.
    assert_eq!(
        ids,
        [
            "station-rock-ru-001",
            "station-rock-001",
            "station-rock-002",
        ]
    );
}

#[tokio::test]
async fn exclusions_are_applied_before_ranking() {
    let service = SearchService::new(Arc::new(
        InMemoryStationRepository::with_legacy_fixture_catalog(),
    ));
    let query = normalize_query("jazz".to_owned(), "en-US".to_owned());
    let constraints = SearchConstraints {
        limit: 10,
        offset: 0,
        excluded_station_ids: BTreeSet::from(["station-jazz-001".to_owned()]),
    };

    let ids = service
        .search(&query, &constraints)
        .await
        .unwrap()
        .into_iter()
        .map(|station| station.station.id)
        .collect::<Vec<_>>();

    assert_eq!(ids, ["station-jazz-002"]);
}

#[tokio::test]
async fn unavailable_catalog_is_explicitly_unready_and_never_serves_a_fixture() {
    let repository = UnavailableStationRepository::from_preflight_error(RepositoryError::new(
        "fixture preflight",
        io::Error::other("invalid catalog fixture"),
    ));
    let constraints = SearchConstraints {
        limit: 10,
        offset: 0,
        excluded_station_ids: BTreeSet::new(),
    };

    assert!(repository.check_readiness().await.is_err());
    assert!(
        repository
            .search(
                &normalize_query("rock".to_owned(), "en-US".to_owned()),
                &constraints,
                None,
            )
            .await
            .is_err()
    );
}

struct RecordingParser {
    input: Arc<Mutex<Option<QueryParserInput>>>,
    fail: bool,
}

#[async_trait]
impl QueryParser for RecordingParser {
    async fn parse(&self, input: &QueryParserInput) -> Result<QueryIntent, QueryParserError> {
        *self.input.lock().unwrap() = Some(input.clone());
        if self.fail {
            return Err(QueryParserError::safe("scripted parser failure"));
        }
        Ok(QueryIntent {
            action: SearchAction::Play,
            terms: vec!["rock".to_owned()],
            tags: vec!["rock".to_owned()],
            language: Some("en".to_owned()),
            country_code: None,
            core_term_count: 1,
            raw_query: "rock".to_owned(),
        })
    }
}

#[tokio::test]
async fn query_parser_receives_only_request_input_and_returns_structured_intent() {
    let input_seen = Arc::new(Mutex::new(None));
    let service = SearchService::with_providers(
        Arc::new(InMemoryStationRepository::with_legacy_fixture_catalog()),
        Arc::new(RecordingParser {
            input: input_seen.clone(),
            fail: false,
        }),
        None,
    );
    let input = QueryParserInput {
        query: "music for driving".to_owned(),
        locale: "en-US".to_owned(),
    };

    let outcome = service
        .interpret_and_search(
            input.clone(),
            &SearchConstraints {
                limit: 10,
                offset: 0,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .unwrap();

    assert_eq!(*input_seen.lock().unwrap(), Some(input));
    assert!(outcome.query.terms.contains(&"rock".to_owned()));
    assert!(outcome.query.terms.contains(&"рок".to_owned()));
    assert_eq!(outcome.stations.len(), 2);
}

struct FailingEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for FailingEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Embedding, EmbeddingProviderError> {
        Err(EmbeddingProviderError::safe("scripted embedding failure"))
    }
}

#[tokio::test]
async fn parser_and_embedding_failures_preserve_metadata_fallback() {
    let service = SearchService::with_providers(
        Arc::new(InMemoryStationRepository::with_legacy_fixture_catalog()),
        Arc::new(RecordingParser {
            input: Arc::new(Mutex::new(None)),
            fail: true,
        }),
        Some(Arc::new(FailingEmbeddingProvider)),
    );

    let outcome = service
        .interpret_and_search(
            QueryParserInput {
                query: "rock".to_owned(),
                locale: "en-US".to_owned(),
            },
            &SearchConstraints {
                limit: 10,
                offset: 0,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        outcome
            .stations
            .iter()
            .map(|station| station.station.id.as_str())
            .collect::<Vec<_>>(),
        [
            "station-rock-ru-001",
            "station-rock-001",
            "station-rock-002",
        ]
    );
}

struct InvalidIntentParser;

#[async_trait]
impl QueryParser for InvalidIntentParser {
    async fn parse(&self, _input: &QueryParserInput) -> Result<QueryIntent, QueryParserError> {
        Ok(QueryIntent {
            action: SearchAction::Play,
            terms: vec!["jazz".to_owned()],
            tags: Vec::new(),
            language: Some("english".to_owned()),
            country_code: None,
            core_term_count: 1,
            raw_query: "jazz".to_owned(),
        })
    }
}

#[tokio::test]
async fn invalid_hard_filter_from_parser_uses_deterministic_fallback() {
    let service = SearchService::with_providers(
        Arc::new(InMemoryStationRepository::with_builtin_catalog().unwrap()),
        Arc::new(InvalidIntentParser),
        None,
    );
    let outcome = service
        .interpret_and_search(
            QueryParserInput {
                query: "rock".to_owned(),
                locale: "en-US".to_owned(),
            },
            &SearchConstraints {
                limit: 10,
                offset: 0,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .unwrap();

    assert_eq!(outcome.query.language, None);
    assert!(outcome.query.terms.contains(&"rock".to_owned()));
}

struct FixedEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for FixedEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Embedding, EmbeddingProviderError> {
        Ok(Embedding::new("fake", "1", 2, vec![1.0, 0.0]).unwrap())
    }
}

#[derive(Default)]
struct RecordingRepository {
    embedding: Mutex<Option<Embedding>>,
}

#[async_trait]
impl StationRepository for RecordingRepository {
    async fn search(
        &self,
        _query: &SearchQuery,
        _constraints: &SearchConstraints,
        embedding: Option<&Embedding>,
    ) -> Result<Vec<RankedStation>, RepositoryError> {
        *self.embedding.lock().unwrap() = embedding.cloned();
        Ok(Vec::new())
    }

    async fn check_readiness(&self) -> Result<(), RepositoryError> {
        Ok(())
    }
}

#[tokio::test]
async fn heavy_metal_query_finds_rock_stations_via_genre_hierarchy() {
    let service = SearchService::new(Arc::new(
        InMemoryStationRepository::with_legacy_fixture_catalog(),
    ));
    let query = SearchQuery {
        action: SearchAction::Play,
        original: "heavy metal".to_owned(),
        locale: "en-US".to_owned(),
        terms: vec!["heavy".to_owned(), "metal".to_owned()],
        tags: vec!["heavy metal".to_owned()],
        language: None,
        country_code: None,
        core_term_count: 2,
        raw_query: "heavy metal".to_owned(),
        prefer_station_name: false,
        station_name_hint_queries: Vec::new(),
    };
    let constraints = SearchConstraints {
        limit: 10,
        offset: 0,
        excluded_station_ids: BTreeSet::new(),
    };

    let ids: Vec<_> = service
        .search(&query, &constraints)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.station.id)
        .collect();

    // metal-001 matches exactly; rock stations match via hierarchy fallback.
    assert!(ids.contains(&"station-metal-001".to_owned()));
    assert!(!ids.is_empty());
}

#[tokio::test]
async fn english_heavy_query_prefers_english_stations() {
    let service = SearchService::new(Arc::new(
        InMemoryStationRepository::with_legacy_fixture_catalog(),
    ));
    let query = SearchQuery {
        action: SearchAction::Play,
        original: "english heavy".to_owned(),
        locale: "en-US".to_owned(),
        terms: vec!["english".to_owned(), "heavy".to_owned()],
        tags: vec!["heavy metal".to_owned()],
        language: Some("en".to_owned()),
        country_code: None,
        core_term_count: 2,
        raw_query: "english heavy".to_owned(),
        prefer_station_name: false,
        station_name_hint_queries: Vec::new(),
    };
    let constraints = SearchConstraints {
        limit: 10,
        offset: 0,
        excluded_station_ids: BTreeSet::new(),
    };

    let results = service.search(&query, &constraints).await.unwrap();

    assert!(!results.is_empty());
    // All returned stations must be English-language (language hard filter).
    for station in &results {
        assert_eq!(station.station.language.as_deref(), Some("en"));
    }
    // The exact heavy-metal station must appear.
    assert!(results.iter().any(|s| s.station.id == "station-metal-001"));
}

#[tokio::test]
async fn genre_fallback_drops_filter_when_no_hierarchy_match() {
    let service = SearchService::new(Arc::new(
        InMemoryStationRepository::with_builtin_catalog().unwrap(),
    ));
    let query = SearchQuery {
        action: SearchAction::Play,
        original: "reggae".to_owned(),
        locale: "en-US".to_owned(),
        terms: vec!["reggae".to_owned()],
        tags: vec!["reggae".to_owned()],
        language: None,
        country_code: None,
        core_term_count: 1,
        raw_query: "reggae".to_owned(),
        prefer_station_name: false,
        station_name_hint_queries: Vec::new(),
    };
    let constraints = SearchConstraints {
        limit: 10,
        offset: 0,
        excluded_station_ids: BTreeSet::new(),
    };

    let results = service.search(&query, &constraints).await.unwrap();

    // No reggae station in catalog, but MIN_RELEVANCE_SCORE gate still
    // prevents random stations from leaking through. The builtin catalog
    // has no term "reggae" anywhere, so the result should be empty.
    assert!(results.is_empty());
}

#[tokio::test]
async fn deterministic_fake_embedding_crosses_only_the_repository_boundary() {
    let repository = Arc::new(RecordingRepository::default());
    let service = SearchService::with_providers(
        repository.clone(),
        Arc::new(DeterministicQueryParser),
        Some(Arc::new(FixedEmbeddingProvider)),
    );

    service
        .search(
            &normalize_query("anything".to_owned(), "en-US".to_owned()),
            &SearchConstraints {
                limit: 1,
                offset: 0,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        repository
            .embedding
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .provenance()
            .model,
        "fake"
    );
}

#[tokio::test]
async fn confident_semantic_language_filter_is_applied_before_search() {
    let classifier = SemanticLanguageClassifier::from_embeddings(vec![
        (
            "en",
            Embedding::new("fake", "1", 2, vec![1.0, 0.0]).unwrap(),
        ),
        (
            "es",
            Embedding::new("fake", "1", 2, vec![0.0, 1.0]).unwrap(),
        ),
    ]);
    let service = SearchService::with_providers_and_language_classifier(
        Arc::new(InMemoryStationRepository::with_builtin_catalog().unwrap()),
        Arc::new(DeterministicQueryParser),
        Some(Arc::new(FixedEmbeddingProvider)),
        Some(Arc::new(classifier)),
    );

    let outcome = service
        .interpret_and_search(
            QueryParserInput {
                query: "Включи английский рок".to_owned(),
                locale: "ru-RU".to_owned(),
            },
            &SearchConstraints {
                limit: 10,
                offset: 0,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .unwrap();

    assert_eq!(outcome.query.language.as_deref(), Some("en"));
    assert!(
        outcome
            .stations
            .iter()
            .all(|station| station.station.language.as_deref() == Some("en"))
    );
}
