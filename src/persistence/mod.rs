//! Persistent catalog implementations and environment-driven backend selection.

mod account_postgres;
mod admin_postgres;
mod device_control_postgres;
mod embedding_postgres;
mod import_postgres;
mod personal_data_postgres;
mod postgres;

use std::{env, io, sync::Arc};

pub use crate::account_cleanup::{
    CleanupAccount, CleanupActionResult, CleanupCounts, CleanupDependency, CleanupError,
    CleanupPreview,
};
pub use account_postgres::{AdminDeviceReadModel, EncryptedYandexHomeToken, PostgresAccountStore};
pub use admin_postgres::PostgresAdminStore;
pub use device_control_postgres::PostgresDeviceControlStore;
pub use embedding_postgres::PostgresEmbeddingStore;
pub use import_postgres::{OwnedCatalogReplacement, PostgresImportStore};
pub use personal_data_postgres::PostgresPersonalDataStore;
pub use postgres::PostgresStationRepository;

use crate::search::StationRepository;
use crate::search::taxonomy::{GenreRow, GenreTaxonomy};

/// Environment variable that enables the PostgreSQL catalog backend.
pub const DATABASE_URL_ENV: &str = "DATABASE_URL";

/// Selects and initializes the catalog backend from the process environment.
///
/// PostgreSQL migrations and the pinned-catalog activation are applied before the backend is returned.
/// A missing database URL is a startup configuration error; the in-memory catalog is reserved for
/// isolated unit tests and is never selected by the service process.
pub async fn repository_from_env()
-> Result<Arc<dyn StationRepository + Send + Sync>, crate::search::RepositoryError> {
    match env::var(DATABASE_URL_ENV) {
        Ok(database_url) if !database_url.trim().is_empty() => {
            let repository = PostgresStationRepository::connect(&database_url).await?;
            tracing::info!(backend = "postgresql", "station repository selected");
            Ok(Arc::new(repository))
        }
        Ok(_) | Err(env::VarError::NotPresent) => Err(crate::search::RepositoryError::new(
            "configuration",
            io::Error::other("DATABASE_URL is required"),
        )),
        Err(error) => Err(crate::search::RepositoryError::new("configuration", error)),
    }
}

/// Loads the genre taxonomy from PostgreSQL, falling back to the compiled-in defaults on error.
pub async fn taxonomy_from_pool(pool: &sqlx::PgPool) -> GenreTaxonomy {
    match load_genre_hierarchy(pool).await {
        Ok(taxonomy) => {
            tracing::info!(
                tags = taxonomy.canonical_tags().len(),
                "genre taxonomy loaded from database"
            );
            taxonomy
        }
        Err(error) => {
            tracing::warn!(%error, "genre taxonomy query failed; using builtin fallback");
            GenreTaxonomy::builtin()
        }
    }
}

async fn load_genre_hierarchy(pool: &sqlx::PgPool) -> Result<GenreTaxonomy, sqlx::Error> {
    let rows = sqlx::query_as::<_, GenreHierarchyRow>(
        "SELECT tag, parent_tag, is_canonical FROM genre_hierarchy",
    )
    .fetch_all(pool)
    .await?;

    let genre_rows: Vec<GenreRow> = rows
        .into_iter()
        .map(|r| GenreRow {
            tag: r.tag,
            parent_tag: r.parent_tag,
            is_canonical: r.is_canonical,
        })
        .collect();

    Ok(GenreTaxonomy::from_rows(genre_rows))
}

#[derive(sqlx::FromRow)]
struct GenreHierarchyRow {
    tag: String,
    parent_tag: Option<String>,
    is_canonical: bool,
}

/// Updates stream health for a batch of station streams after probe results.
pub async fn update_stream_health(
    pool: &sqlx::PgPool,
    stream_url: &str,
    healthy: bool,
    error_message: Option<&str>,
) -> Result<(), sqlx::Error> {
    let health = if healthy { "healthy" } else { "degraded" };
    sqlx::query(
        r#"
UPDATE station_streams
SET health = $1,
    last_probe_at = now(),
    last_probe_error = $2,
    updated_at = now()
WHERE stream_url = $3
"#,
    )
    .bind(health)
    .bind(error_message)
    .bind(stream_url)
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns stream URLs that need probing, ordered by oldest probe first.
pub async fn streams_to_probe(pool: &sqlx::PgPool, limit: i64) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT stream_url FROM station_streams ORDER BY last_probe_at NULLS FIRST, id LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
