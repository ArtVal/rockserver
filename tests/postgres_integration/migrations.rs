//! Integration tests for database migrations, initial seed, full-text & semantic search,
//! and live readiness probes against PostgreSQL.

use std::collections::BTreeSet;
use std::env;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use rockserver::catalog::{CatalogImporter, ImportLimits};
use rockserver::http::{
    HealthResponse, HealthStatus, router_with_repository, router_with_search_service,
};
use rockserver::persistence::{
    PostgresEmbeddingStore, PostgresImportStore, PostgresStationRepository,
};
use rockserver::providers::radio_browser::SOURCE;
use rockserver::search::{
    DeterministicQueryParser, Embedding, EmbeddingProvider, EmbeddingProviderError, EmbeddingStore,
    SearchAction, SearchConstraints, SearchQuery, SearchService, normalize_query,
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{OnePageProvider, assert_run, imported_station, repository_pool, station_ids};

struct FixedEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for FixedEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Embedding, EmbeddingProviderError> {
        Ok(Embedding::new("integration-model", "1", 3, vec![1.0, 0.0, 0.0]).unwrap())
    }
}

struct FailingEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for FailingEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Embedding, EmbeddingProviderError> {
        Err(EmbeddingProviderError::safe(
            "scripted integration provider failure",
        ))
    }
}

/// Exercises migrations, seed data, search semantics, and live database readiness.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_migrations_seed_search_and_readiness() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let repository = PostgresStationRepository::connect(&database_url)
        .await
        .expect("migrations and seed must succeed");
    let service = SearchService::new(Arc::new(repository.clone()));
    let extension_pool = repository_pool(&database_url).await;
    let extension_version = sqlx::query_scalar::<_, String>(
        "SELECT extversion FROM pg_extension WHERE extname = 'vector'",
    )
    .fetch_one(&extension_pool)
    .await
    .expect("pgvector migration must enable the extension");
    assert!(!extension_version.is_empty());
    extension_pool.close().await;

    let rock_query = normalize_query("rock".to_owned(), "en-US".to_owned());
    let rock_results = service
        .search(
            &rock_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("seeded search must succeed");
    assert!(
        rock_results
            .iter()
            .any(|ranked| ranked.station.id == "rock-antenne")
    );

    let limited_results = service
        .search(
            &rock_query,
            &SearchConstraints {
                limit: 1,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("limited search must succeed");
    assert_eq!(
        station_ids(&limited_results),
        station_ids(&rock_results)[..1]
    );

    let metal_query = normalize_query("metal".to_owned(), "en-US".to_owned());
    let exclusion_results = service
        .search(
            &metal_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::from(["somafm-metal-detector".to_owned()]),
            },
        )
        .await
        .expect("excluded search must succeed");
    assert!(!station_ids(&exclusion_results).contains(&"somafm-metal-detector"));

    let embedding_store = PostgresEmbeddingStore::connect(&database_url)
        .await
        .expect("embedding store migrations must succeed");
    embedding_store
        .upsert_embedding(
            "rock-antenne",
            &Embedding::new("integration-model", "1", 3, vec![0.0, 1.0, 0.0]).unwrap(),
        )
        .await
        .expect("first embedding insert must succeed");
    embedding_store
        .upsert_embedding(
            "rock-antenne",
            &Embedding::new("integration-model", "1", 3, vec![1.0, 0.0, 0.0]).unwrap(),
        )
        .await
        .expect("repeat embedding update must succeed");
    for (station_id, values) in [
        ("radio-record-rock", vec![0.0, 1.0, 0.0]),
        ("gotradio-punk-rock", vec![0.0, 0.0, 1.0]),
        ("rock-antenne-alternative", vec![0.0, 0.0, 1.0]),
    ] {
        embedding_store
            .upsert_embedding(
                station_id,
                &Embedding::new("integration-model", "1", 3, values).unwrap(),
            )
            .await
            .expect("station embedding insert must succeed");
    }

    let semantic_service = SearchService::with_providers(
        Arc::new(repository.clone()),
        Arc::new(DeterministicQueryParser),
        Some(Arc::new(FixedEmbeddingProvider)),
    );
    let semantic_query = SearchQuery {
        action: SearchAction::Play,
        original: "rock".to_owned(),
        locale: "en-US".to_owned(),
        terms: vec!["rock".to_owned()],
        tags: Vec::new(),
        language: None,
        country_code: None,
        core_term_count: 1,
        raw_query: "rock".to_owned(),
        prefer_station_name: false,
        station_name_hint_queries: Vec::new(),
    };
    let semantic_results = semantic_service
        .search(
            &semantic_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("semantic search must succeed");
    assert!(!semantic_results.is_empty());

    let british_semantic_results = semantic_service
        .search(
            &SearchQuery {
                country_code: Some("GB".to_owned()),
                ..semantic_query.clone()
            },
            &SearchConstraints {
                limit: 1,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("hard-filtered semantic search must succeed");
    assert!(
        british_semantic_results
            .iter()
            .all(|station| station.station.country_code.as_deref() == Some("GB"))
    );

    let tie_results = semantic_service
        .search(
            &semantic_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::from([
                    "rock-antenne".to_owned(),
                    "radio-record-rock".to_owned(),
                ]),
            },
        )
        .await
        .expect("semantic tie search must succeed");
    assert!(!tie_results.is_empty());

    let fallback_service = SearchService::with_providers(
        Arc::new(repository.clone()),
        Arc::new(DeterministicQueryParser),
        Some(Arc::new(FailingEmbeddingProvider)),
    );
    let fallback_results = fallback_service
        .search(
            &rock_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("provider failure must retain metadata search");
    assert!(!station_ids(&fallback_results).is_empty());

    let embedding_pool = repository_pool(&database_url).await;
    let persisted_count = sqlx::query_scalar::<_, i64>(
        r#"
SELECT COUNT(*)
FROM station_embeddings
WHERE station_id = 'rock-antenne'
  AND model = 'integration-model'
  AND version = '1'
"#,
    )
    .fetch_one(&embedding_pool)
    .await
    .expect("embedding count must be inspectable");
    let persisted = sqlx::query_as::<_, (i32, f64, bool)>(
        r#"
SELECT dimension, embedding <=> '[1,0,0]'::vector, updated_at >= created_at
FROM station_embeddings
WHERE station_id = 'rock-antenne'
  AND model = 'integration-model'
  AND version = '1'
"#,
    )
    .fetch_one(&embedding_pool)
    .await
    .expect("embedding persistence must be inspectable");
    assert_eq!(persisted_count, 1);
    assert_eq!(persisted.0, 3);
    assert!(persisted.1.abs() < 0.000_001);
    assert!(persisted.2);
    embedding_pool.close().await;
    embedding_store.close().await;

    let import_store = PostgresImportStore::connect(&database_url)
        .await
        .expect("import store migrations must succeed");
    let source_station_id = "01234567-89ab-cdef-0123-456789abcdef";
    let first_import = CatalogImporter::new(
        OnePageProvider::station(imported_station(
            source_station_id,
            "Integration Rock Radio",
            "https://streams.example.com/imported-first.mp3",
            &["rock"],
        )),
        import_store.clone(),
        ImportLimits {
            page_size: 10,
            max_pages: 2,
        },
    )
    .run()
    .await
    .expect("first import must succeed");
    let repeat_import = CatalogImporter::new(
        OnePageProvider::station(imported_station(
            source_station_id,
            "Integration Rock Radio Updated",
            "https://streams.example.com/imported-updated.mp3",
            &["rock", "upbeat"],
        )),
        import_store.clone(),
        ImportLimits {
            page_size: 10,
            max_pages: 2,
        },
    )
    .run()
    .await
    .expect("repeat import must update in place");
    let failed_import = CatalogImporter::new(
        OnePageProvider::failure(),
        import_store.clone(),
        ImportLimits {
            page_size: 10,
            max_pages: 2,
        },
    )
    .run()
    .await
    .expect_err("provider failure must fail the run");
    assert_eq!(failed_import.safe_summary(), "mock provider unavailable");

    let inspection_pool = PgPool::connect(&database_url)
        .await
        .expect("inspection connection must succeed");
    let shared_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM stations WHERE source = 'rockcatalog'")
            .fetch_one(&inspection_pool)
            .await
            .unwrap();
    assert_eq!(
        shared_count, 41,
        "shared release must remain active beside imports"
    );
    let provider_station_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM stations WHERE source = $1 AND source_station_id = $2",
    )
    .bind(SOURCE)
    .bind(source_station_id)
    .fetch_one(&inspection_pool)
    .await
    .unwrap();
    assert_eq!(
        provider_station_count, 1,
        "repeat import must not duplicate"
    );
    let provider_stream = sqlx::query_as::<_, (String, String)>(
        r#"
SELECT s.name, ss.stream_url
FROM stations AS s
JOIN station_streams AS ss ON ss.station_id = s.id
WHERE s.source = $1 AND s.source_station_id = $2
"#,
    )
    .bind(SOURCE)
    .bind(source_station_id)
    .fetch_one(&inspection_pool)
    .await
    .unwrap();
    assert_eq!(provider_stream.0, "Integration Rock Radio Updated");
    assert_eq!(
        provider_stream.1,
        "https://streams.example.com/imported-updated.mp3"
    );

    assert_run(
        &inspection_pool,
        &first_import.run_id,
        "completed",
        (1, 1, 0, 0),
        false,
    )
    .await;
    assert_run(
        &inspection_pool,
        &repeat_import.run_id,
        "completed",
        (1, 1, 0, 0),
        false,
    )
    .await;
    let failed_run_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM import_runs WHERE source = $1 AND status = 'failed' ORDER BY started_at DESC LIMIT 1",
    )
    .bind(SOURCE)
    .fetch_one(&inspection_pool)
    .await
    .unwrap()
    .to_string();
    assert_run(
        &inspection_pool,
        &failed_run_id,
        "failed",
        (0, 0, 0, 0),
        true,
    )
    .await;

    let imported_query = normalize_query("upbeat".to_owned(), "en-US".to_owned());
    let imported_results = service
        .search(
            &imported_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("search over imported metadata must succeed");
    assert!(
        imported_results
            .iter()
            .any(|ranked| ranked.station.id == format!("rb-{source_station_id}")),
        "imported station must be searchable"
    );
    inspection_pool.close().await;
    import_store.close().await;

    let app = router_with_repository(Arc::new(repository.clone()));
    let ready = app
        .clone()
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(ready.status(), axum::http::StatusCode::OK);

    let provider_independent_ready = router_with_search_service(fallback_service)
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        provider_independent_ready.status(),
        axum::http::StatusCode::OK
    );

    repository.close().await;

    let unavailable = app
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        unavailable.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    let body = unavailable.into_body().collect().await.unwrap().to_bytes();
    let payload: HealthResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.status, HealthStatus::NotReady);

    let response = router_with_repository(Arc::new(
        PostgresStationRepository::connect(&database_url)
            .await
            .expect("repository should reconnect"),
    ))
    .oneshot(
        Request::post("/api/v1/search")
            .header("content-type", "application/json")
            .header(
                "authorization",
                format!("Bearer {}", rockserver::http::TEST_API_BEARER_TOKEN),
            )
            .body(Body::from(r#"{"query":"rock"}"#))
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        payload["stations"][0]["id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
}
