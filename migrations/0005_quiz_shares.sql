-- Sharing a quiz with specific people by email. The address need not have
-- an account yet: the quiz shows up once someone signs in with it. Only a
-- published quiz is visible to the people it is shared with. Additive, so an
-- older binary still works after a rollback.
CREATE TABLE quiz_shares (
    quiz_id    uuid NOT NULL REFERENCES quizzes (id) ON DELETE CASCADE,
    email      text NOT NULL CHECK (email = lower(email) AND char_length(email) BETWEEN 3 AND 254),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (quiz_id, email)
);
CREATE INDEX quiz_shares_email_idx ON quiz_shares (email);

-- The Public quizzes list: published, public, newest first.
CREATE INDEX quizzes_public_idx ON quizzes (updated_at DESC)
    WHERE deleted_at IS NULL AND visibility = 'public' AND status = 'published';
