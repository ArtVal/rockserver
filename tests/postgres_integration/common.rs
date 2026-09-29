//! Shared helper functions and test providers for PostgreSQL integration tests.

use async_trait::async_trait;
use rockserver::catalog::{
    CatalogImportError, CatalogImportProvider, ImportPage, ImportedStation, ImportedStream,
};
use rockserver::providers::radio_browser::SOURCE;
use rockserver::search::RankedStation;
use sqlx::PgPool;
use uuid::Uuid;

/// Connects an inspection pool to the target test database.
pub(crate) async fn repository_pool(database_url: &str) -> PgPool {
    PgPool::connect(database_url)
        .await
        .expect("inspection connection must succeed")
}

/// Extracts station IDs from ranked search results.
pub(crate) fn station_ids(results: &[RankedStation]) -> Vec<&str> {
    results
        .iter()
        .map(|ranked| ranked.station.id.as_str())
        .collect()
}

/// A deterministic one-page import provider for testing catalog import pipelines.
pub(crate) struct OnePageProvider {
    pub(crate) station: Option<ImportedStation>,
    pub(crate) fail: bool,
}

impl OnePageProvider {
    pub(crate) fn station(station: ImportedStation) -> Self {
        Self {
            station: Some(station),
            fail: false,
        }
    }

    pub(crate) fn failure() -> Self {
        Self {
            station: None,
            fail: true,
        }
    }
}

#[async_trait]
impl CatalogImportProvider for OnePageProvider {
    fn source(&self) -> &'static str {
        SOURCE
    }

    async fn fetch_page(
        &self,
        _offset: usize,
        _limit: usize,
    ) -> Result<ImportPage, CatalogImportError> {
        if self.fail {
            return Err(CatalogImportError::safe("mock provider unavailable"));
        }
        Ok(ImportPage {
            fetched: usize::from(self.station.is_some()),
            stations: self.station.clone().into_iter().collect(),
            skipped: 0,
        })
    }
}

/// Constructs a synthetic `ImportedStation` for provider tests.
pub(crate) fn imported_station(
    source_station_id: &str,
    name: &str,
    stream_url: &str,
    tags: &[&str],
) -> ImportedStation {
    ImportedStation {
        source: SOURCE,
        source_station_id: source_station_id.to_owned(),
        id: format!("rb-{source_station_id}"),
        name: name.to_owned(),
        homepage_url: Some("https://example.com/imported-radio".to_owned()),
        favicon_source_url: Some("https://icons.example.com/imported-radio.png".to_owned()),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        language: Some("en".to_owned()),
        country_code: Some("US".to_owned()),
        streams: vec![ImportedStream {
            source_stream_id: source_station_id.to_owned(),
            stream_url: stream_url.to_owned(),
            codec: Some("MP3".to_owned()),
            bitrate_kbps: Some(192),
            is_primary: true,
        }],
    }
}

/// Asserts the completion status, counts, and error summary of an import run.
pub(crate) async fn assert_run(
    pool: &PgPool,
    run_id: &str,
    expected_status: &str,
    expected_counts: (i64, i64, i64, i64),
    expects_error: bool,
) {
    let run_id = Uuid::parse_str(run_id).unwrap();
    let row = sqlx::query_as::<_, (String, i64, i64, i64, i64, Option<String>, bool)>(
        r#"
SELECT status, fetched_count, imported_count, skipped_count, failed_count, error_summary,
       completed_at IS NOT NULL
FROM import_runs
WHERE id = $1
"#,
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(row.0, expected_status);
    assert_eq!((row.1, row.2, row.3, row.4), expected_counts);
    assert_eq!(row.5.is_some(), expects_error);
    assert!(row.6, "terminal run must have completed_at");
}
