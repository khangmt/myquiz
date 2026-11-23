-- Quiz retention (FR-QZ-18 to 20) and editor drafts.

-- Deletion date = last_used_at + RETENTION_DAYS. Set on create, import,
-- save, publish and when a game of the quiz ends.
ALTER TABLE quizzes ADD COLUMN last_used_at timestamptz NOT NULL DEFAULT now();
CREATE INDEX quizzes_last_used_idx ON quizzes (last_used_at) WHERE deleted_at IS NULL;

-- Reminder emails sent, so none goes out twice (week 3).
CREATE TABLE retention_notices (
    id      bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    quiz_id uuid NOT NULL REFERENCES quizzes (id) ON DELETE CASCADE,
    kind    text NOT NULL CHECK (kind IN ('7d', '1d')),
    sent_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (quiz_id, kind)
);

-- A draft may hold an option the creator has not typed yet; publish
-- requires 1 to 120 characters (the engine validator).
ALTER TABLE question_options DROP CONSTRAINT question_options_text_check;
ALTER TABLE question_options ADD CONSTRAINT question_options_text_check
    CHECK (char_length(text) <= 120);
