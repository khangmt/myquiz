-- A question's media area can show Markdown (usually a fenced code block)
-- instead of an image. Nullable and additive, so an older binary still works
-- after a rollback.
ALTER TABLE questions ADD COLUMN markdown text
    CHECK (markdown IS NULL OR char_length(markdown) <= 4000);
