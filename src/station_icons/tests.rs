//! Unit tests for station icons storage, normalization, candidate discovery, and planning.

use std::{io::Cursor, sync::Mutex};

use async_trait::async_trait;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use url::Url;

use super::{
    coordinator::{IconJobItem, ItemPlan, resolve_item_plan},
    domain::{
        IconSourceFetcher, IconStorage, IconStorageKey, IconValidationError, PreparedIcon,
        prepare_icon,
    },
    fetcher::{homepage_icon_candidates, icon_links},
    storage::FilesystemIconStorage,
};

/// Deterministic offline fetcher used to verify job outcome classification.
///
/// `fetched` records the candidate URLs the plan actually tried, in order.
struct FakeFetcher {
    discovered: Result<Vec<String>, IconValidationError>,
    icon: Mutex<Vec<Result<PreparedIcon, IconValidationError>>>,
    fetched: Mutex<Vec<String>>,
}

impl FakeFetcher {
    /// Builds a fetcher that answers every candidate fetch with the same result.
    fn uniform(
        discovered: Result<Vec<String>, IconValidationError>,
        icon: Result<PreparedIcon, IconValidationError>,
    ) -> Self {
        Self {
            discovered,
            icon: Mutex::new(Vec::new()),
            fetched: Mutex::new(Vec::new()),
        }
        .with_icon_results(icon)
    }

    /// Queues per-candidate fetch results; the last one repeats.
    fn with_icon_results(self, first: Result<PreparedIcon, IconValidationError>) -> Self {
        *self.icon.lock().unwrap() = vec![first];
        self
    }

    /// Queues per-candidate fetch results; the last one repeats.
    fn with_results(self, results: Vec<Result<PreparedIcon, IconValidationError>>) -> Self {
        assert!(!results.is_empty());
        *self.icon.lock().unwrap() = results;
        self
    }

    /// URLs this fetcher was asked to fetch, in order.
    fn fetched(&self) -> Vec<String> {
        self.fetched.lock().unwrap().clone()
    }
}

#[async_trait]
impl IconSourceFetcher for FakeFetcher {
    async fn fetch_icon(&self, source: &str) -> Result<PreparedIcon, IconValidationError> {
        self.fetched.lock().unwrap().push(source.to_owned());
        let mut results = self.icon.lock().unwrap();
        if results.len() > 1 {
            results.remove(0)
        } else {
            results[0].clone()
        }
    }

    async fn discover_homepage_icons(
        &self,
        _homepage: &str,
    ) -> Result<Vec<String>, IconValidationError> {
        self.discovered.clone()
    }
}

/// Builds one claimable item without touching persistence.
fn item(source_url: Option<&str>, homepage_url: Option<&str>) -> IconJobItem {
    IconJobItem {
        station_id: "station-1".to_owned(),
        source_url: source_url.map(str::to_owned),
        homepage_url: homepage_url.map(str::to_owned),
    }
}

/// Builds a small valid prepared icon for ready-path assertions.
fn sample_prepared() -> PreparedIcon {
    let mut source = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(32, 16, Rgba([9, 8, 7, 255])))
        .write_to(&mut source, ImageFormat::Png)
        .unwrap();
    prepare_icon(source.get_ref()).unwrap()
}

#[tokio::test]
async fn filesystem_storage_publishes_and_removes_one_safe_key() {
    let root = std::env::temp_dir().join(format!("rockserver-icons-{}", uuid::Uuid::new_v4()));
    let storage = FilesystemIconStorage::open(&root).unwrap();
    let key = IconStorageKey::from_hash(&[7; 32]);
    storage.put_atomic(&key, b"webp").await.unwrap();
    assert!(storage.exists(&key).await.unwrap());
    assert_eq!(storage.get(&key).await.unwrap(), Some(b"webp".to_vec()));
    storage.delete(&key).await.unwrap();
    assert_eq!(storage.get(&key).await.unwrap(), None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn storage_keys_are_content_addressed_and_never_paths() {
    assert!(IconStorageKey::parse("a".repeat(64).as_str()).is_err());
    assert!(IconStorageKey::parse("../icon.webp").is_err());
    assert_eq!(IconStorageKey::from_hash(&[0; 32]).as_str().len(), 69);
}

#[test]
fn raster_input_becomes_a_square_webp() {
    let mut source = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(32, 16, Rgba([1, 2, 3, 255])))
        .write_to(&mut source, ImageFormat::Png)
        .unwrap();
    let prepared = prepare_icon(source.get_ref()).unwrap();
    assert_eq!((prepared.width, prepared.height), (256, 256));
    assert_eq!(
        image::guess_format(&prepared.bytes).unwrap(),
        ImageFormat::WebP
    );
}

#[test]
fn invalid_or_oversized_input_is_never_prepared() {
    assert_eq!(prepare_icon(b"<svg />"), Err(IconValidationError::Format));
    assert_eq!(
        prepare_icon(&vec![0; 2 * 1024 * 1024 + 1]),
        Err(IconValidationError::Size)
    );
}

#[test]
fn homepage_icon_link_is_extracted_and_resolved() {
    let base = Url::parse("https://radio.example/en/index.html").unwrap();
    let html = r#"<html><head><LINK REL="shortcut icon" HREF='/static/img/favicon.png'><link rel="stylesheet" href="styles.css"></head><body/></html>"#;
    assert_eq!(
        icon_links(html, &base, 4),
        vec!["https://radio.example/static/img/favicon.png"]
    );
}

#[test]
fn apple_touch_icon_and_absolute_hrefs_are_accepted() {
    let base = Url::parse("https://radio.example/deep/page").unwrap();
    let html = r#"<head><link rel="apple-touch-icon" href="https://cdn.example/touch.png"></head>"#;
    assert_eq!(
        icon_links(html, &base, 4),
        vec!["https://cdn.example/touch.png"]
    );
}

#[test]
fn mask_icon_data_and_entity_hrefs_are_handled() {
    let base = Url::parse("https://radio.example/deep/page").unwrap();
    let html = concat!(
        r#"<head><link rel="mask-icon" href="/icon.svg">"#,
        r#"<link rel=icon href="data:image/png;base64,AAA">"#,
        r#"<link rel="icon" type="image/png" href="favicon.png?v=2&amp;size=64"></head>"#,
    );
    assert_eq!(
        icon_links(html, &base, 4),
        vec!["https://radio.example/deep/favicon.png?v=2&size=64"]
    );
}

#[test]
fn homepage_candidates_end_with_the_root_favicon_and_dedupe() {
    let base = Url::parse("https://radio.example/deep/page?from=nav#top").unwrap();
    assert_eq!(
        icon_links("<html><head><title>x</title></head></html>", &base, 4),
        Vec::<String>::new()
    );
    assert_eq!(
        homepage_icon_candidates("<html><head><title>x</title></head></html>", &base),
        vec!["https://radio.example/favicon.ico"]
    );
    let declared = r#"<head><link rel="icon" href="/a.png"><link rel="apple-touch-icon" href="/b.png"><link rel="icon" href="/a.png"></head>"#;
    assert_eq!(
        homepage_icon_candidates(declared, &base),
        vec![
            "https://radio.example/a.png",
            "https://radio.example/b.png",
            "https://radio.example/favicon.ico",
        ]
    );
}

#[tokio::test]
async fn explicit_catalog_source_wins_with_top_priority() {
    // A wrongly consulted homepage discovery with this result would end the item
    // as Permanent instead of Ready, so this assertion also proves it is skipped.
    let fetcher = FakeFetcher::uniform(Err(IconValidationError::Format), Ok(sample_prepared()));
    let plan = resolve_item_plan(
        &fetcher,
        &item(
            Some("https://icons.example/explicit.png"),
            Some("https://radio.example"),
        ),
    )
    .await;
    assert!(matches!(
        plan,
        ItemPlan::Ready {
            source_priority: 2,
            ..
        }
    ));
}

#[tokio::test]
async fn homepage_discovery_is_the_fallback_source() {
    let fetcher = FakeFetcher::uniform(
        Ok(vec!["https://radio.example/favicon.ico".to_owned()]),
        Ok(sample_prepared()),
    );
    let plan = resolve_item_plan(&fetcher, &item(None, Some("https://radio.example"))).await;
    assert!(matches!(
        plan,
        ItemPlan::Ready {
            source_priority: 1,
            source_url: ref source,
            ..
        } if source == "https://radio.example/favicon.ico"
    ));
}

#[tokio::test]
async fn candidates_are_tried_in_order_until_one_is_ready() {
    // First declared link is unusable, second is transport-flaky, root fallback works:
    // production showed both patterns, and the tried URLs must be recorded in order.
    let fetcher = FakeFetcher::uniform(
        Ok(vec![
            "https://radio.example/broken.png".to_owned(),
            "https://cdn.example/touch.png".to_owned(),
            "https://radio.example/favicon.ico".to_owned(),
        ]),
        Ok(sample_prepared()),
    )
    .with_results(vec![
        Err(IconValidationError::Format),
        Err(IconValidationError::Decode),
        Ok(sample_prepared()),
    ]);
    let plan = resolve_item_plan(&fetcher, &item(None, Some("https://radio.example"))).await;
    assert!(matches!(
        plan,
        ItemPlan::Ready {
            source_priority: 1,
            ref source_url,
            ..
        } if source_url == "https://radio.example/favicon.ico"
    ));
    assert_eq!(
        fetcher.fetched(),
        vec![
            "https://radio.example/broken.png".to_owned(),
            "https://cdn.example/touch.png".to_owned(),
            "https://radio.example/favicon.ico".to_owned(),
        ]
    );
}

#[tokio::test]
async fn all_dead_candidates_stay_permanent_but_flaky_ones_retry() {
    let all_dead = FakeFetcher::uniform(
        Ok(vec!["https://radio.example/a.png".to_owned()]),
        Err(IconValidationError::Format),
    );
    assert_eq!(
        resolve_item_plan(&all_dead, &item(None, Some("https://radio.example"))).await,
        ItemPlan::Permanent
    );
    let flaky = FakeFetcher::uniform(
        Ok(vec![
            "https://radio.example/a.png".to_owned(),
            "https://radio.example/favicon.ico".to_owned(),
        ]),
        Err(IconValidationError::Decode),
    );
    assert_eq!(
        resolve_item_plan(&flaky, &item(None, Some("https://radio.example"))).await,
        ItemPlan::Retryable
    );
}

#[tokio::test]
async fn station_without_any_source_is_missing() {
    let fetcher = FakeFetcher::uniform(
        Ok(vec!["https://radio.example/favicon.ico".to_owned()]),
        Ok(sample_prepared()),
    );
    assert_eq!(
        resolve_item_plan(&fetcher, &item(None, None)).await,
        ItemPlan::Missing
    );
}

#[tokio::test]
async fn transient_homepage_failures_are_retryable_and_dead_ones_permanent() {
    let transient = FakeFetcher::uniform(Err(IconValidationError::Decode), Ok(sample_prepared()));
    assert_eq!(
        resolve_item_plan(&transient, &item(None, Some("https://radio.example"))).await,
        ItemPlan::Retryable
    );
    let dead = FakeFetcher::uniform(Err(IconValidationError::Format), Ok(sample_prepared()));
    assert_eq!(
        resolve_item_plan(&dead, &item(None, Some("https://radio.example"))).await,
        ItemPlan::Permanent
    );
    let oversized = FakeFetcher::uniform(Err(IconValidationError::Size), Ok(sample_prepared()));
    assert_eq!(
        resolve_item_plan(&oversized, &item(None, Some("https://radio.example"))).await,
        ItemPlan::Permanent
    );
}

#[tokio::test]
async fn icon_fetch_failures_keep_the_boundary_classification() {
    let retryable = FakeFetcher::uniform(
        Ok(vec!["https://radio.example/favicon.ico".to_owned()]),
        Err(IconValidationError::Decode),
    );
    assert_eq!(
        resolve_item_plan(&retryable, &item(None, Some("https://radio.example"))).await,
        ItemPlan::Retryable
    );
    let permanent = FakeFetcher::uniform(
        Ok(vec!["https://radio.example/favicon.ico".to_owned()]),
        Err(IconValidationError::Format),
    );
    assert_eq!(
        resolve_item_plan(&permanent, &item(None, Some("https://radio.example"))).await,
        ItemPlan::Permanent
    );
    let oversized = FakeFetcher::uniform(
        Ok(vec!["https://radio.example/favicon.ico".to_owned()]),
        Err(IconValidationError::Size),
    );
    assert_eq!(
        resolve_item_plan(&oversized, &item(None, Some("https://radio.example"))).await,
        ItemPlan::Permanent
    );
}
