-- Account-owned favourites and playback history personal data (RM-012-A).
-- Records are identified by client-generated record_id UUIDs and merged last-writer-wins
-- on updated_at; one shared monotonic sequence provides the per-account delta cursor and
-- tombstones carry deletions to devices that were offline when the deletion happened.
CREATE SEQUENCE personal_data_sync_revision_seq AS bigint;

CREATE TABLE favourite_records (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    record_id uuid NOT NULL,
    station_id varchar(256),
    added_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    deleted_at timestamptz,
    sync_revision bigint NOT NULL DEFAULT nextval('personal_data_sync_revision_seq'),
    PRIMARY KEY (user_id, record_id),
    CHECK (deleted_at IS NOT NULL OR station_id IS NOT NULL),
    CHECK (deleted_at IS NULL OR deleted_at <= updated_at)
);

CREATE TABLE history_records (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    record_id uuid NOT NULL,
    station_id varchar(256),
    started_at timestamptz NOT NULL,
    last_played_at timestamptz NOT NULL,
    ended_at timestamptz,
    play_duration_ms bigint CHECK (play_duration_ms >= 0),
    metadata jsonb CHECK (jsonb_typeof(metadata) = 'object'),
    updated_at timestamptz NOT NULL,
    deleted_at timestamptz,
    sync_revision bigint NOT NULL DEFAULT nextval('personal_data_sync_revision_seq'),
    PRIMARY KEY (user_id, record_id),
    CHECK (deleted_at IS NOT NULL OR station_id IS NOT NULL),
    CHECK (last_played_at >= started_at),
    CHECK (ended_at IS NULL OR ended_at >= started_at),
    CHECK (deleted_at IS NULL OR deleted_at <= updated_at)
);

CREATE INDEX favourite_records_sync_idx ON favourite_records (user_id, sync_revision);
CREATE INDEX history_records_sync_idx ON history_records (user_id, sync_revision);
CREATE INDEX history_records_retention_idx ON history_records (started_at) WHERE deleted_at IS NULL;
