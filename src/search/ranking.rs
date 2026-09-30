//! Shared deterministic metadata and hybrid ranking rules.

use std::collections::{BTreeSet, HashSet};

use super::{RankedStation, SearchConstraints, SearchQuery, Station, query::tokenize};

/// Metadata contribution used when a compatible semantic score exists.
pub const METADATA_WEIGHT: f64 = 0.70;
/// Semantic contribution used when a compatible semantic score exists.
pub const SEMANTIC_WEIGHT: f64 = 0.30;

/// Applies metadata fallback semantics to an already-loaded catalog.
pub(super) fn rank_stations(
    stations: &[Station],
    query: &SearchQuery,
    constraints: &SearchConstraints,
) -> Vec<RankedStation> {
    let mut results = stations
        .iter()
        .filter(|station| !constraints.excluded_station_ids.contains(&station.id))
        .filter(|station| language_matches(station, query))
        .filter(|station| country_matches(station, query))
        .filter_map(|station| rank_station(station, query))
        .collect::<Vec<_>>();

    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.station.id.cmp(&right.station.id))
    });
    let total = results.len();
    results
        .into_iter()
        .skip(constraints.offset)
        .take(constraints.limit)
        .map(|mut r| {
            r.total_matches = Some(total);
            r
        })
        .collect()
}

/// Combines normalized metadata and cosine-derived semantic scores.
///
/// A missing compatible station embedding preserves the complete metadata score.
pub fn hybrid_score(metadata_score: f64, semantic_score: Option<f64>) -> f64 {
    semantic_score.map_or(metadata_score, |semantic| {
        (METADATA_WEIGHT * metadata_score + SEMANTIC_WEIGHT * semantic).clamp(0.0, 1.0)
    })
}

fn language_matches(station: &Station, query: &SearchQuery) -> bool {
    query
        .language
        .as_deref()
        .is_none_or(|language| station.language.as_deref() == Some(language))
}

fn country_matches(station: &Station, query: &SearchQuery) -> bool {
    query
        .country_code
        .as_deref()
        .is_none_or(|country_code| station.country_code.as_deref() == Some(country_code))
}

fn rank_station(station: &Station, query: &SearchQuery) -> Option<RankedStation> {
    let searchable_terms = station_searchable_terms(station);
    let station_name_lower = station.name.to_lowercase();
    let matched_terms = query
        .terms
        .iter()
        .filter(|term| searchable_terms.contains(term.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();
    let matched_tags = query
        .tags
        .iter()
        .filter(|tag| station.tags.iter().any(|station_tag| station_tag == *tag))
        .cloned()
        .collect::<BTreeSet<_>>();

    // Substring matches for terms ≥ 3 chars not already exactly matched.
    let substring_matches: usize = query
        .terms
        .iter()
        .filter(|t| t.len() >= 3 && !matched_terms.contains(t.as_str()))
        .filter(|t| station_name_lower.contains(t.as_str()))
        .count();

    if matched_terms.is_empty() && matched_tags.is_empty() && substring_matches == 0 {
        return None;
    }

    let exact = matched_terms.len() + matched_tags.len();
    let query_count = query.core_term_count + query.tags.len();
    let score = (exact as f64 + substring_matches as f64 * 0.5) / query_count.max(1) as f64;
    let reason_terms = matched_tags
        .into_iter()
        .chain(matched_terms)
        .collect::<Vec<_>>();

    Some(RankedStation {
        station: station.clone(),
        score,
        reason: format!("Matched catalog metadata: {}.", reason_terms.join(", ")),
        total_matches: None,
    })
}

fn station_searchable_terms(station: &Station) -> HashSet<String> {
    tokenize(&station.name)
        .into_iter()
        .chain(station.tags.iter().flat_map(|tag| tokenize(tag)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::hybrid_score;

    #[test]
    fn hybrid_score_uses_fixed_weights_and_metadata_when_embedding_is_absent() {
        assert_eq!(hybrid_score(0.8, None), 0.8);
        assert!((hybrid_score(0.8, Some(0.5)) - 0.71).abs() < f64::EPSILON);
    }
}
