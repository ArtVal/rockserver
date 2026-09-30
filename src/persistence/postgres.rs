//! PostgreSQL implementation of the station repository boundary.

use async_trait::async_trait;
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions};
use std::time::Instant;

use crate::search::{
    Embedding, METADATA_WEIGHT, RankedStation, RepositoryError, SEMANTIC_WEIGHT, SearchConstraints,
    SearchQuery, Station, StationHealth, StationRepository,
};

use super::embedding_postgres::vector_literal;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

// Hard filters and exclusions are applied in `candidates`, before scoring and the final limit.
// Metadata fallback remains exact; compatible embeddings add an exact cosine-derived score.
/// Two-phase search: independent index-backed tag, FTS, and trigram branches
/// first produce the candidate IDs. Full scoring (token match, substring,
/// cosine) then runs only on the same bounded candidate set. If no branch
/// produces candidates, the query returns quickly instead of running
/// semantic-only fallback across the whole embedding corpus.
const SEARCH_SQL: &str = r#"
WITH query_terms AS (
    SELECT
        term,
        CASE
            WHEN term = '' THEN NULL
            ELSE plainto_tsquery('simple', term)
        END AS fts_query
    FROM UNNEST($1::text[]) AS t(term)
),
query_tags AS (
    SELECT tag FROM UNNEST($2::text[]) AS t(tag)
),
search_input AS MATERIALIZED (
    SELECT CASE
        WHEN $14::text = '' THEN NULL
        ELSE plainto_tsquery('simple', $14)
    END AS fts_query
),
tag_candidate_ids AS (
    SELECT s.id
    FROM stations AS s
    JOIN query_tags AS qt ON s.tags @> ARRAY[qt.tag]::text[]
    WHERE s.retired_at IS NULL
      AND ($3::text IS NULL OR s.language = $3)
      AND ($4::text IS NULL OR s.country_code = $4)
      AND NOT (s.id = ANY($5::text[]))
),
fts_candidate_ids AS (
    SELECT s.id
    FROM stations AS s
    CROSS JOIN search_input AS input
    WHERE s.retired_at IS NULL
      AND input.fts_query IS NOT NULL
      AND s.searchable_tsv @@ input.fts_query
      AND ($3::text IS NULL OR s.language = $3)
      AND ($4::text IS NULL OR s.country_code = $4)
      AND NOT (s.id = ANY($5::text[]))
),
trigram_candidate_ids AS (
    SELECT s.id
    FROM stations AS s
    JOIN query_terms AS qt
      ON length(qt.term) >= 3
     AND lower(s.name) % qt.term
    WHERE s.retired_at IS NULL
      AND ($3::text IS NULL OR s.language = $3)
      AND ($4::text IS NULL OR s.country_code = $4)
      AND NOT (s.id = ANY($5::text[]))
),
candidate_match_ids AS (
    SELECT id FROM tag_candidate_ids
    UNION
    SELECT id FROM fts_candidate_ids
    UNION
    SELECT id FROM trigram_candidate_ids
),
prefiltered AS MATERIALIZED (
    SELECT
        s.id,
        (
            (
                SELECT COUNT(DISTINCT qt.tag)
                FROM query_tags qt
                WHERE s.tags @> ARRAY[qt.tag]::text[]
            )::float8
            + CASE
                -- `ts_rank_cd` is only non-zero for an FTS match, so avoid
                -- calculating it for the broad tag/trigram candidates.
                WHEN input.fts_query IS NULL OR NOT s.searchable_tsv @@ input.fts_query THEN 0.0
                ELSE ts_rank_cd(s.searchable_tsv, input.fts_query)
            END
        ) AS prefilter_score
    FROM stations AS s
    JOIN candidate_match_ids AS match_ids ON match_ids.id = s.id
    CROSS JOIN search_input AS input
    WHERE s.retired_at IS NULL
    ORDER BY prefilter_score DESC, s.id ASC
    LIMIT GREATEST(($6 + $17) * 20, 200)
),
candidate_ids AS (
    SELECT id FROM prefiltered
),
candidates AS MATERIALIZED (
    SELECT
        s.id,
        s.name,
        primary_stream.stream_url,
        s.homepage_url,
        COALESCE(icon.status = 'ready', false) AS favicon_ready,
        s.tags,
        s.language,
        s.country_code,
        primary_stream.codec,
        primary_stream.bitrate_kbps,
        primary_stream.health,
        station_embedding.embedding,
        (
            SELECT COUNT(DISTINCT qt.term)
            FROM query_terms qt
            WHERE qt.fts_query IS NOT NULL
              AND s.searchable_tsv @@ qt.fts_query
        ) + (
            SELECT COUNT(DISTINCT qt.tag)
            FROM query_tags qt
            WHERE s.tags @> ARRAY[qt.tag]::text[]
        ) AS matched_count,
        (
            SELECT COUNT(DISTINCT qt.term)
            FROM query_terms qt
            WHERE length(qt.term) >= 3
              AND station_name.normalized LIKE '%' || qt.term || '%'
        ) AS substring_match_count,
        (
            SELECT COUNT(DISTINCT qt.term)
            FROM query_terms qt
            WHERE EXISTS (
                SELECT 1 FROM UNNEST(name_tokens.tokens) AS token
                WHERE token = qt.term
            )
        ) AS name_token_match_count,
        cardinality(name_tokens.tokens) AS name_token_count,
        CASE
            WHEN $15::bool THEN EXISTS (
                SELECT 1
                FROM UNNEST($16::text[]) AS hinted(query)
                WHERE hinted.query <> ''
                  AND regexp_replace(lower(s.name), '[^[:alnum:]]+', ' ', 'g') LIKE '%' || hinted.query || '%'
            )
            ELSE FALSE
        END AS ordered_name_match,
        COALESCE((
            SELECT MAX(similarity(station_name.normalized, qt.term))
            FROM query_terms qt
            WHERE length(qt.term) >= 2
        ), 0.0) AS trgm_score
    FROM stations AS s
    JOIN candidate_ids ci ON ci.id = s.id
    CROSS JOIN LATERAL (
        SELECT lower(s.name) AS normalized
    ) AS station_name
    CROSS JOIN LATERAL (
        SELECT ARRAY(
            SELECT token
            FROM regexp_split_to_table(station_name.normalized, '[^[:alnum:]]+') AS token
            WHERE token <> ''
        ) AS tokens
    ) AS name_tokens
    JOIN LATERAL (
        SELECT stream_url, codec, bitrate_kbps, health
        FROM station_streams
        WHERE station_id = s.id AND retired_at IS NULL
        ORDER BY is_primary DESC, id ASC
        LIMIT 1
    ) AS primary_stream ON true
    LEFT JOIN station_icons AS icon ON icon.station_id = s.id
    LEFT JOIN station_embeddings AS station_embedding
      ON station_embedding.station_id = s.id
     AND station_embedding.model = $8
     AND station_embedding.version = $9
     AND station_embedding.dimension = $10
     AND $7::text IS NOT NULL
    WHERE primary_stream.health <> 'degraded'
), scored AS MATERIALIZED (
    SELECT
        *,
        (
            matched_count::float8
            + substring_match_count::float8 * 0.5
            + trgm_score * 0.3
            + CASE
                -- Prefer exact station-name hits for short "play station X" style queries.
                WHEN $13::int > 0 AND name_token_match_count >= $13::int
                    THEN 1.0 / GREATEST(name_token_count, 1)::float8
                ELSE 0.0
              END
            + CASE
                WHEN ordered_name_match THEN 2.0
                ELSE 0.0
              END
        )
            / GREATEST($13::int + cardinality($2::text[]), 1)
            AS metadata_score,
        CASE
            WHEN embedding IS NULL OR $7::text IS NULL THEN NULL
            ELSE LEAST(1.0, GREATEST(0.0, 1.0 - (embedding <=> ($7::text)::vector) / 2.0))
        END AS semantic_score
    FROM candidates
)
SELECT
    id,
    name,
    stream_url,
    homepage_url,
    favicon_ready,
    tags,
    language,
    country_code,
    codec,
    bitrate_kbps,
    health,
    metadata_score,
    semantic_score,
    CASE
        WHEN semantic_score IS NULL THEN metadata_score
        ELSE $11::float8 * metadata_score + $12::float8 * semantic_score
    END AS score,
    COUNT(*) OVER() AS total_matches
FROM scored
WHERE metadata_score > 0 OR semantic_score > 0
ORDER BY score DESC, id ASC
LIMIT $6 OFFSET $17
"#;

/// PostgreSQL-backed station catalog with startup migrations and seeded development data.
#[derive(Clone, Debug)]
pub struct PostgresStationRepository {
    pool: PgPool,
}

impl PostgresStationRepository {
    /// Connects to PostgreSQL and applies all pending versioned migrations.
    pub async fn connect(database_url: &str) -> Result<Self, RepositoryError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .map_err(|error| RepositoryError::new("connection", error))?;

        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(RepositoryError::new("migration", error));
        }

        let has_active_stations = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM stations WHERE retired_at IS NULL)",
        )
        .fetch_one(&pool)
        .await
        .map_err(|error| RepositoryError::new("catalog presence check", error))?;
        if !has_active_stations {
            let catalog = crate::catalog::PinnedSharedCatalog::load()
                .map_err(|error| RepositoryError::new("shared catalog preflight", error))?;
            super::PostgresImportStore::from_pool(pool.clone())
                .activate_shared_catalog(&catalog)
                .await
                .map_err(|error| RepositoryError::new("shared catalog activation", error))?;
        }

        Ok(Self { pool })
    }

    /// Closes this repository's shared connection pool after in-flight work completes.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[async_trait]
impl StationRepository for PostgresStationRepository {
    async fn search(
        &self,
        query: &SearchQuery,
        constraints: &SearchConstraints,
        embedding: Option<&Embedding>,
    ) -> Result<Vec<RankedStation>, RepositoryError> {
        let parameters = PostgresSearchParameters::from_domain(query, constraints, embedding)
            .map_err(|error| RepositoryError::new("parameter conversion", error))?;
        let started_at = Instant::now();
        let rows = sqlx::query_as::<_, StationRow>(SEARCH_SQL)
            .bind(&parameters.terms)
            .bind(&parameters.tags)
            .bind(&parameters.language)
            .bind(&parameters.country_code)
            .bind(&parameters.excluded_station_ids)
            .bind(parameters.limit)
            .bind(&parameters.embedding)
            .bind(&parameters.embedding_model)
            .bind(&parameters.embedding_version)
            .bind(parameters.embedding_dimension)
            .bind(parameters.metadata_weight)
            .bind(parameters.semantic_weight)
            .bind(parameters.core_term_count)
            .bind(&parameters.raw_query)
            .bind(parameters.prefer_station_name)
            .bind(&parameters.station_name_hint_queries)
            .bind(parameters.offset)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| RepositoryError::new("search", error))?;
        tracing::debug!(
            elapsed_ms = started_at.elapsed().as_millis(),
            result_count = rows.len(),
            "PostgreSQL station search completed"
        );

        rows.into_iter()
            .map(RankedStation::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RepositoryError::new("row conversion", error))
    }

    async fn check_readiness(&self) -> Result<(), RepositoryError> {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| RepositoryError::new("readiness check", error))
    }

    async fn list_public(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        sqlx::query_as::<_, PublicStationRow>("SELECT s.id, s.name, ss.stream_url, s.homepage_url, COALESCE(i.status = 'ready', false) AS favicon_ready, s.tags, s.language, s.country_code, ss.codec, ss.bitrate_kbps, ss.health FROM stations s JOIN station_streams ss ON ss.station_id = s.id AND ss.is_primary LEFT JOIN station_icons i ON i.station_id = s.id WHERE ($1::text IS NULL OR s.id > $1) ORDER BY s.id LIMIT $2")
            .bind(after_id).bind(i64::try_from(limit).unwrap_or(50)).fetch_all(&self.pool).await
            .map_err(|error| RepositoryError::new("public catalog listing", error))?
            .into_iter().map(Station::try_from).collect::<Result<Vec<_>, _>>().map_err(|error| RepositoryError::new("public catalog row conversion", error))
    }

    async fn list_admin(
        &self,
        query: Option<&str>,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Station>, RepositoryError> {
        sqlx::query_as::<_, PublicStationRow>(
            "SELECT s.id, s.name, ss.stream_url, s.homepage_url, COALESCE(i.status = 'ready', false) AS favicon_ready, s.tags, s.language, s.country_code, ss.codec, ss.bitrate_kbps, ss.health \
             FROM stations s JOIN station_streams ss ON ss.station_id = s.id AND ss.is_primary \
             LEFT JOIN station_icons i ON i.station_id = s.id \
             WHERE s.retired_at IS NULL AND ($1::text IS NULL OR lower(s.name) LIKE '%' || lower($1) || '%' OR EXISTS (SELECT 1 FROM unnest(s.tags) AS tag WHERE lower(tag) LIKE '%' || lower($1) || '%')) \
             AND ($2::text IS NULL OR s.id > $2) ORDER BY s.id LIMIT $3",
        )
        .bind(query)
        .bind(after_id)
        .bind(i64::try_from(limit).unwrap_or(50))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| RepositoryError::new("administrator catalog listing", error))?
        .into_iter()
        .map(Station::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| RepositoryError::new("administrator catalog row conversion", error))
    }

    async fn get_public(&self, id: &str) -> Result<Option<Station>, RepositoryError> {
        sqlx::query_as::<_, PublicStationRow>("SELECT s.id, s.name, ss.stream_url, s.homepage_url, COALESCE(i.status = 'ready', false) AS favicon_ready, s.tags, s.language, s.country_code, ss.codec, ss.bitrate_kbps, ss.health FROM stations s JOIN station_streams ss ON ss.station_id = s.id AND ss.is_primary LEFT JOIN station_icons i ON i.station_id = s.id WHERE s.id = $1")
            .bind(id).fetch_optional(&self.pool).await.map_err(|error| RepositoryError::new("public catalog get", error))?
            .map(Station::try_from).transpose().map_err(|error| RepositoryError::new("public catalog row conversion", error))
    }
}

#[derive(Debug, FromRow)]
struct PublicStationRow {
    id: String,
    name: String,
    stream_url: String,
    homepage_url: Option<String>,
    favicon_ready: bool,
    tags: Vec<String>,
    language: Option<String>,
    country_code: Option<String>,
    codec: Option<String>,
    bitrate_kbps: Option<i32>,
    health: String,
}

impl TryFrom<PublicStationRow> for Station {
    type Error = RowConversionError;
    fn try_from(row: PublicStationRow) -> Result<Self, Self::Error> {
        let health = match row.health.as_str() {
            "healthy" => StationHealth::Healthy,
            "degraded" => StationHealth::Degraded,
            "unknown" => StationHealth::Unknown,
            _ => return Err(RowConversionError::InvalidHealth(row.health)),
        };
        let favicon_url = row
            .favicon_ready
            .then(|| format!("/api/v1/stations/{}/icon", row.id));
        Ok(Station {
            id: row.id,
            name: row.name,
            stream_url: row.stream_url,
            homepage_url: row.homepage_url,
            favicon_url,
            tags: row.tags,
            language: row.language,
            country_code: row.country_code,
            codec: row.codec,
            bitrate_kbps: row
                .bitrate_kbps
                .map(|value| u32::try_from(value).map_err(|_| RowConversionError::InvalidBitrate))
                .transpose()?,
            health,
        })
    }
}

#[derive(Debug, FromRow)]
struct StationRow {
    id: String,
    name: String,
    stream_url: String,
    homepage_url: Option<String>,
    favicon_ready: bool,
    tags: Vec<String>,
    language: Option<String>,
    country_code: Option<String>,
    codec: Option<String>,
    bitrate_kbps: Option<i32>,
    health: String,
    metadata_score: f64,
    semantic_score: Option<f64>,
    score: f64,
    total_matches: Option<i64>,
}

impl TryFrom<StationRow> for RankedStation {
    type Error = RowConversionError;

    fn try_from(row: StationRow) -> Result<Self, Self::Error> {
        let total_matches = row.total_matches.and_then(|t| usize::try_from(t).ok());
        let bitrate_kbps = row
            .bitrate_kbps
            .map(u32::try_from)
            .transpose()
            .map_err(|_| RowConversionError::InvalidBitrate)?;
        let health = match row.health.as_str() {
            "healthy" => StationHealth::Healthy,
            "degraded" => StationHealth::Degraded,
            "unknown" => StationHealth::Unknown,
            _ => return Err(RowConversionError::InvalidHealth(row.health)),
        };

        let reason = row.semantic_score.map_or_else(
            || {
                format!(
                    "Matched catalog metadata with score {:.3}.",
                    row.metadata_score
                )
            },
            |semantic_score| {
                format!(
                    "Hybrid match: metadata {:.3}, semantic {:.3}.",
                    row.metadata_score, semantic_score
                )
            },
        );
        let favicon_url = row
            .favicon_ready
            .then(|| format!("/api/v1/stations/{}/icon", row.id));

        Ok(Self {
            reason,
            score: row.score,
            total_matches,
            station: Station {
                id: row.id,
                name: row.name,
                stream_url: row.stream_url,
                homepage_url: row.homepage_url,
                favicon_url,
                tags: row.tags,
                language: row.language,
                country_code: row.country_code,
                codec: row.codec,
                bitrate_kbps,
                health,
            },
        })
    }
}

#[derive(Debug)]
enum RowConversionError {
    InvalidBitrate,
    InvalidHealth(String),
}

impl std::fmt::Display for RowConversionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBitrate => formatter.write_str("database bitrate cannot fit in u32"),
            Self::InvalidHealth(value) => {
                write!(formatter, "unknown database health value {value:?}")
            }
        }
    }
}

impl std::error::Error for RowConversionError {}

#[derive(Debug, PartialEq)]
struct PostgresSearchParameters {
    terms: Vec<String>,
    tags: Vec<String>,
    language: Option<String>,
    country_code: Option<String>,
    excluded_station_ids: Vec<String>,
    limit: i64,
    offset: i64,
    embedding: Option<String>,
    embedding_model: Option<String>,
    embedding_version: Option<String>,
    embedding_dimension: Option<i32>,
    metadata_weight: f64,
    semantic_weight: f64,
    core_term_count: i32,
    /// Cleaned query for plainto_tsquery FTS matching.
    raw_query: String,
    prefer_station_name: bool,
    station_name_hint_queries: Vec<String>,
}

impl PostgresSearchParameters {
    /// Copies normalized domain values into SQL-safe owned parameters without changing meaning.
    fn from_domain(
        query: &SearchQuery,
        constraints: &SearchConstraints,
        embedding: Option<&Embedding>,
    ) -> Result<Self, ParameterConversionError> {
        let embedding_dimension = embedding
            .map(|value| i32::try_from(value.provenance().dimension))
            .transpose()
            .map_err(|_| ParameterConversionError::EmbeddingDimensionTooLarge)?;
        Ok(Self {
            terms: query.terms.clone(),
            tags: query.tags.clone(),
            language: query.language.clone(),
            country_code: query.country_code.clone(),
            excluded_station_ids: constraints.excluded_station_ids.iter().cloned().collect(),
            limit: i64::try_from(constraints.limit)
                .map_err(|_| ParameterConversionError::LimitTooLarge)?,
            offset: i64::try_from(constraints.offset)
                .map_err(|_| ParameterConversionError::OffsetTooLarge)?,
            embedding: embedding.map(vector_literal),
            embedding_model: embedding.map(|value| value.provenance().model.clone()),
            embedding_version: embedding.map(|value| value.provenance().version.clone()),
            embedding_dimension,
            metadata_weight: METADATA_WEIGHT,
            semantic_weight: SEMANTIC_WEIGHT,
            core_term_count: query.core_term_count as i32,
            raw_query: query.raw_query.clone(),
            prefer_station_name: query.prefer_station_name,
            station_name_hint_queries: query.station_name_hint_queries.clone(),
        })
    }
}

#[derive(Debug)]
enum ParameterConversionError {
    LimitTooLarge,
    OffsetTooLarge,
    EmbeddingDimensionTooLarge,
}

impl std::fmt::Display for ParameterConversionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitTooLarge => {
                formatter.write_str("search limit cannot fit in PostgreSQL bigint")
            }
            Self::OffsetTooLarge => {
                formatter.write_str("search offset cannot fit in PostgreSQL bigint")
            }
            Self::EmbeddingDimensionTooLarge => {
                formatter.write_str("embedding dimension cannot fit in PostgreSQL integer")
            }
        }
    }
}

impl std::error::Error for ParameterConversionError {}

#[cfg(test)]
mod tests {
    use super::{PostgresSearchParameters, SEARCH_SQL, StationRow};
    use crate::search::{
        METADATA_WEIGHT, RankedStation, SEMANTIC_WEIGHT, SearchConstraints, StationHealth,
        normalize_query,
    };
    use std::collections::BTreeSet;

    #[test]
    fn database_row_conversion_maps_stream_fields_and_health() {
        let ranked = RankedStation::try_from(StationRow {
            id: "station-test".to_owned(),
            name: "Test Station".to_owned(),
            stream_url: "https://streams.example.com/test.mp3".to_owned(),
            homepage_url: None,
            favicon_ready: true,
            tags: vec!["rock".to_owned()],
            language: Some("en".to_owned()),
            country_code: Some("US".to_owned()),
            codec: Some("MP3".to_owned()),
            bitrate_kbps: Some(192),
            health: "degraded".to_owned(),
            metadata_score: 0.5,
            semantic_score: None,
            score: 0.5,
            total_matches: None,
        })
        .unwrap();

        assert_eq!(ranked.station.health, StationHealth::Degraded);
        assert_eq!(ranked.station.bitrate_kbps, Some(192));
        assert_eq!(
            ranked.station.favicon_url.as_deref(),
            Some("/api/v1/stations/station-test/icon")
        );
        assert_eq!(ranked.score, 0.5);
    }

    #[test]
    fn domain_search_values_convert_to_stable_sql_parameters() {
        let query = normalize_query("british classic rock".to_owned(), "en-GB".to_owned());
        let constraints = SearchConstraints {
            limit: 7,
            offset: 0,
            excluded_station_ids: BTreeSet::from([
                "station-rock-002".to_owned(),
                "station-rock-001".to_owned(),
            ]),
        };

        let parameters = PostgresSearchParameters::from_domain(&query, &constraints, None).unwrap();

        assert_eq!(parameters.country_code.as_deref(), Some("GB"));
        assert_eq!(parameters.language, None);
        assert_eq!(parameters.limit, 7);
        assert_eq!(parameters.metadata_weight, METADATA_WEIGHT);
        assert_eq!(parameters.semantic_weight, SEMANTIC_WEIGHT);
        assert_eq!(
            parameters.excluded_station_ids,
            ["station-rock-001", "station-rock-002"]
        );
    }

    #[test]
    fn search_sql_unions_index_backed_candidate_branches_before_scoring() {
        for branch in [
            "tag_candidate_ids AS",
            "fts_candidate_ids AS",
            "trigram_candidate_ids AS",
        ] {
            assert!(SEARCH_SQL.contains(branch), "missing {branch}");
        }
        assert!(SEARCH_SQL.contains("candidate_match_ids AS"));
        assert!(SEARCH_SQL.contains("JOIN candidate_match_ids AS match_ids"));
        assert_eq!(SEARCH_SQL.matches("UNION\n    SELECT id FROM").count(), 2);
        assert!(SEARCH_SQL.contains("search_input AS MATERIALIZED"));
        assert!(SEARCH_SQL.contains("END AS fts_query"));
        assert!(SEARCH_SQL.contains("CROSS JOIN LATERAL"));
        assert!(SEARCH_SQL.contains("station_name.normalized"));
        assert!(SEARCH_SQL.contains("UNNEST(name_tokens.tokens)"));
    }
}
