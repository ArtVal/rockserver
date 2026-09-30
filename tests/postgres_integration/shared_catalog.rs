//! Integration tests for shared catalog activation, tombstones, and rollback safety.

use std::collections::BTreeSet;
use std::env;
use std::sync::Arc;

use rockserver::catalog::{CatalogImporter, ImportLimits, PinnedSharedCatalog};
use rockserver::persistence::{
    OwnedCatalogReplacement, PostgresImportStore, PostgresStationRepository,
};
use rockserver::search::{SearchConstraints, SearchService, normalize_query};
use sha2::Digest;

use crate::common::{OnePageProvider, imported_station, repository_pool};

/// Exercises canonical retirement metadata through transactional activation, search, rollback, and
/// provider coexistence against a disposable database.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn shared_catalog_tombstones_are_active_idempotent_and_rollback_safe() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let repository = PostgresStationRepository::connect(&database_url)
        .await
        .expect("baseline activation must succeed");
    let store = PostgresImportStore::connect(&database_url)
        .await
        .expect("catalog store must connect");
    let before = lifecycle_catalog("test-before", false);
    let after = lifecycle_catalog("test-after", true);

    store
        .activate_shared_catalog(&before)
        .await
        .expect("initial lifecycle release must activate");
    let radio_browser_import = CatalogImporter::new(
        OnePageProvider::station(imported_station(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "Provider ownership sentinel",
            "https://streams.example.com/ownership-sentinel.mp3",
            &["rock"],
        )),
        store.clone(),
        ImportLimits {
            page_size: 10,
            max_pages: 1,
        },
    );
    radio_browser_import
        .run()
        .await
        .expect("Radio Browser sentinel must import");

    store
        .activate_shared_catalog(&after)
        .await
        .expect("retirement release must activate");
    store
        .activate_shared_catalog(&after)
        .await
        .expect("re-import of the same release must be idempotent");

    assert_eq!(
        store
            .lookup_shared_catalog_replacement("merge-old")
            .await
            .unwrap(),
        OwnedCatalogReplacement::Redirect("merge-target".to_owned())
    );
    assert_eq!(
        store
            .lookup_shared_catalog_replacement("split-old")
            .await
            .unwrap(),
        OwnedCatalogReplacement::Ambiguous(vec!["split-one".to_owned(), "split-two".to_owned()])
    );
    assert_eq!(
        store
            .lookup_shared_catalog_replacement("removed-old")
            .await
            .unwrap(),
        OwnedCatalogReplacement::Removed
    );

    let service = SearchService::new(Arc::new(repository.clone()));
    let retired_results = service
        .search(
            &normalize_query("legacy retired marker".to_owned(), "en-US".to_owned()),
            &SearchConstraints {
                limit: 10,
                offset: 0,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("retired rows must be filtered rather than breaking search");
    assert!(retired_results.iter().all(|station| !matches!(
        station.station.id.as_str(),
        "removed-old" | "merge-old" | "split-old"
    )));

    let pool = repository_pool(&database_url).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM catalog_tombstones WHERE source = 'rockcatalog'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        3
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM stations WHERE source = 'radio_browser' AND source_station_id = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1,
        "RockCatalog activation must not change Radio Browser ownership"
    );

    store
        .activate_shared_catalog(&before)
        .await
        .expect("previous release rollback must reactivate its rows");
    assert_eq!(
        store
            .lookup_shared_catalog_replacement("merge-old")
            .await
            .unwrap(),
        OwnedCatalogReplacement::Unknown
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM stations WHERE source = 'rockcatalog' AND source_station_id = 'merge-old' AND retired_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    pool.close().await;
    store.close().await;
    repository.close().await;
}

/// Builds two immutable fixtures that model a prior release and a replacement release.
fn lifecycle_catalog(version: &str, with_tombstones: bool) -> PinnedSharedCatalog {
    let retired = if with_tombstones {
        ""
    } else {
        r#",
    {"id":"removed-old","name":"Legacy retired marker","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{"id":"main","url":"https://example.com/removed-old","codec":"mp3","bitrateKbps":128,"primary":true}]},
    {"id":"merge-old","name":"Legacy merge marker","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{"id":"main","url":"https://example.com/merge-old","codec":"mp3","bitrateKbps":128,"primary":true}]},
    {"id":"split-old","name":"Legacy split marker","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{"id":"main","url":"https://example.com/split-old","codec":"mp3","bitrateKbps":128,"primary":true}]}"#
    };
    let tombstones = if with_tombstones {
        r#"[{"id":"removed-old","reason":"removed","replacementIds":[]},{"id":"merge-old","reason":"merged","replacementIds":["merge-target"]},{"id":"split-old","reason":"split","replacementIds":["split-one","split-two"]}]"#
    } else {
        "[]"
    };
    let document = format!(
        r#"{{"schemaVersion":1,"catalogVersion":"{version}","stations":[
    {{"id":"merge-target","name":"Merge target","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{{"id":"main","url":"https://example.com/merge-target","codec":"mp3","bitrateKbps":128,"primary":true}}]}},
    {{"id":"split-one","name":"Split one","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{{"id":"main","url":"https://example.com/split-one","codec":"mp3","bitrateKbps":128,"primary":true}}]}},
    {{"id":"split-two","name":"Split two","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{{"id":"main","url":"https://example.com/split-two","codec":"mp3","bitrateKbps":128,"primary":true}}]}}{retired}],"tombstones":{tombstones}}}"#,
    );
    let digest = format!("{:x}", sha2::Sha256::digest(document.as_bytes()));
    PinnedSharedCatalog::from_bytes(document.as_bytes(), version, &digest).unwrap()
}
