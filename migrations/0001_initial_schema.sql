-- Initial schema: the ten MVP tables from design section 5.
-- Login sessions and live game state live in Redis, not here.

CREATE EXTENSION IF NOT EXISTS pgcrypto;  -- gen_random_uuid()
CREATE EXTENSION IF NOT EXISTS pg_trgm;   -- trigram search on quiz titles

CREATE TYPE user_role       AS ENUM ('user', 'admin');
CREATE TYPE user_status     AS ENUM ('active', 'suspended');
CREATE TYPE quiz_visibility AS ENUM ('private', 'public');
CREATE TYPE quiz_status     AS ENUM ('draft', 'published');
CREATE TYPE question_type   AS ENUM ('single', 'multi', 'true_false', 'fill_blank');
CREATE TYPE session_status  AS ENUM ('lobby', 'running', 'ended');

CREATE TABLE users (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email             text NOT NULL,
    display_name      text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 60),
    password_hash     text,                         -- null for Google-only accounts
    role              user_role   NOT NULL DEFAULT 'user',
    status            user_status NOT NULL DEFAULT 'active',
    locale            text NOT NULL DEFAULT 'vi' CHECK (locale IN ('vi', 'en')),
    email_verified_at timestamptz,
    age_confirmed_at  timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    deleted_at        timestamptz
);
-- Emails compare case-insensitively.
CREATE UNIQUE INDEX users_email_key ON users (lower(email));

CREATE TABLE auth_identities (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider   text NOT NULL CHECK (provider IN ('google')),
    subject    text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider, subject)
);
CREATE INDEX auth_identities_user_idx ON auth_identities (user_id);

CREATE TABLE media (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    storage_key text NOT NULL UNIQUE,               -- random file name on the media volume (SEC-11)
    mime        text NOT NULL,
    bytes       integer NOT NULL CHECK (bytes > 0),
    width       integer NOT NULL CHECK (width > 0),
    height      integer NOT NULL CHECK (height > 0),
    alt_text    text CHECK (char_length(alt_text) <= 300),
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX media_owner_idx ON media (owner_id);

CREATE TABLE quizzes (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title          text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),   -- FR-QZ-01
    description    text NOT NULL DEFAULT '' CHECK (char_length(description) <= 1000),
    cover_media_id uuid REFERENCES media (id) ON DELETE SET NULL,
    visibility     quiz_visibility NOT NULL DEFAULT 'private',                    -- FR-QZ-10
    status         quiz_status     NOT NULL DEFAULT 'draft',
    version        integer NOT NULL DEFAULT 1,   -- If-Match for autosave
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    deleted_at     timestamptz                   -- soft delete, purged after 30 days (FR-QZ-11)
);
CREATE INDEX quizzes_owner_idx ON quizzes (owner_id, updated_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX quizzes_title_trgm_idx ON quizzes USING gin (title gin_trgm_ops);

CREATE TABLE questions (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    quiz_id           uuid NOT NULL REFERENCES quizzes (id) ON DELETE CASCADE,
    position          integer NOT NULL CHECK (position >= 0),
    type              question_type NOT NULL,
    text              text NOT NULL CHECK (char_length(text) <= 300),           -- FR-QZ-02
    media_id          uuid REFERENCES media (id) ON DELETE SET NULL,
    time_limit_s      integer NOT NULL DEFAULT 20 CHECK (time_limit_s BETWEEN 5 AND 240),  -- FR-QZ-06
    points            integer NOT NULL DEFAULT 1000 CHECK (points IN (0, 1000, 2000)),
    accepted_answers  text[] NOT NULL DEFAULT '{}'                               -- fill_blank only, up to 5 (FR-QZ-04)
                      CHECK (cardinality(accepted_answers) <= 5),
    ignore_diacritics boolean NOT NULL DEFAULT false,
    UNIQUE (quiz_id, position) DEFERRABLE INITIALLY DEFERRED                     -- reorder in one transaction
);

CREATE TABLE question_options (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    question_id uuid NOT NULL REFERENCES questions (id) ON DELETE CASCADE,
    position    integer NOT NULL CHECK (position BETWEEN 0 AND 5),               -- 2 to 6 options
    text        text NOT NULL CHECK (char_length(text) BETWEEN 1 AND 120),
    is_correct  boolean NOT NULL DEFAULT false,
    UNIQUE (question_id, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE game_sessions (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    quiz_id       uuid REFERENCES quizzes (id) ON DELETE SET NULL,  -- the snapshot keeps the report intact
    host_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    pin           char(6),                                          -- cleared at game end (SEC-17)
    quiz_snapshot jsonb NOT NULL,
    options       jsonb NOT NULL DEFAULT '{}',
    status        session_status NOT NULL DEFAULT 'lobby',
    created_at    timestamptz NOT NULL DEFAULT now(),
    started_at    timestamptz,
    ended_at      timestamptz,
    player_count  integer NOT NULL DEFAULT 0
);
CREATE INDEX game_sessions_host_idx ON game_sessions (host_id, created_at DESC);
-- PINs are unique among active sessions (FR-GM-01); Redis enforces this first.
CREATE UNIQUE INDEX game_sessions_active_pin_key ON game_sessions (pin) WHERE pin IS NOT NULL;

CREATE TABLE session_players (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id    uuid NOT NULL REFERENCES game_sessions (id) ON DELETE CASCADE,
    user_id       uuid REFERENCES users (id) ON DELETE SET NULL,  -- null for guests
    nickname      text NOT NULL CHECK (char_length(nickname) BETWEEN 2 AND 20),  -- FR-GM-02
    team_id       uuid,                                            -- team mode, week 3 (FR-GM-13)
    join_order    integer NOT NULL,
    final_score   integer,
    final_rank    integer,
    total_time_ms bigint NOT NULL DEFAULT 0,
    kicked        boolean NOT NULL DEFAULT false,
    UNIQUE (session_id, join_order)
);
CREATE INDEX session_players_session_idx ON session_players (session_id);
CREATE INDEX session_players_user_idx ON session_players (user_id) WHERE user_id IS NOT NULL;  -- FR-RP-08

CREATE TABLE answers (
    id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session_id     uuid NOT NULL REFERENCES game_sessions (id) ON DELETE CASCADE,
    player_id      uuid NOT NULL REFERENCES session_players (id) ON DELETE CASCADE,
    question_index integer NOT NULL CHECK (question_index >= 0),
    option_ids     integer[],
    text_answer    text CHECK (char_length(text_answer) <= 50),
    is_correct     boolean NOT NULL,
    response_ms    integer NOT NULL CHECK (response_ms >= 0),
    points         integer NOT NULL CHECK (points >= 0),
    received_at    timestamptz NOT NULL,
    UNIQUE (player_id, question_index)                              -- SEC-16 backstop after Redis
);
CREATE INDEX answers_session_question_idx ON answers (session_id, question_index);

CREATE TABLE audit_log (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at            timestamptz NOT NULL DEFAULT now(),
    actor_user_id uuid,                -- no FK: the log outlives deleted users
    action        text NOT NULL,
    target_type   text,
    target_id     text,
    ip_hash       text,                -- hashed, never the raw IP (SEC-12)
    details       jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX audit_log_actor_idx ON audit_log (actor_user_id, at DESC);
CREATE INDEX audit_log_at_idx ON audit_log (at DESC);

-- The audit log is append only (SEC-12).
CREATE FUNCTION audit_log_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_log is append only';
END;
$$;
CREATE TRIGGER audit_log_no_update BEFORE UPDATE OR DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_append_only();
