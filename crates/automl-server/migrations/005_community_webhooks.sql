-- Iteration 6: forum, feedback, real outbound webhooks (outbox + deliveries).

CREATE TABLE forum_categories (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed categories (stable slugs; display fields may be edited later).
INSERT INTO forum_categories (slug, name, description) VALUES
    ('general',      'General',      'Anything about the platform'),
    ('papers',       'Papers',       'Discussion of published papers'),
    ('reproduction', 'Reproduction', 'Reproduce runs, evidence, environments'),
    ('agents',       'Agents',       'Silicon scholars and automation');

CREATE TABLE forum_threads (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    category_id  UUID NOT NULL REFERENCES forum_categories (id),
    title        TEXT NOT NULL,
    created_by   UUID NOT NULL REFERENCES scholars (id),
    deleted_at   TIMESTAMPTZ NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT forum_threads_title_nonempty CHECK (char_length(title) > 0)
);

CREATE INDEX forum_threads_list_idx ON forum_threads (category_id, created_at DESC, id DESC);

CREATE TABLE forum_posts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    thread_id   UUID NOT NULL REFERENCES forum_threads (id) ON DELETE CASCADE,
    -- Markdown source; rendering happens client-side only.
    body        TEXT NOT NULL,
    created_by  UUID NOT NULL REFERENCES scholars (id),
    deleted_at  TIMESTAMPTZ NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT forum_posts_body_nonempty CHECK (char_length(body) > 0)
);

CREATE INDEX forum_posts_list_idx ON forum_posts (thread_id, created_at ASC, id ASC);

CREATE TABLE feedback (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    category    TEXT NOT NULL CHECK (category IN ('bug', 'feature', 'question', 'other')),
    body        TEXT NOT NULL,
    contact     TEXT NULL,
    status      TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'triaged', 'closed')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT feedback_body_nonempty CHECK (char_length(body) > 0)
);

CREATE INDEX feedback_list_idx ON feedback (created_at DESC, id DESC);

-- Outbox: written INSIDE the business transaction that produced the event;
-- a periodic worker fans it out to subscriptions. Wire contract of payloads
-- is only-add (automl-core::event).
CREATE TABLE outbox_events (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type    TEXT NOT NULL,
    payload       JSONB NOT NULL,
    occurred_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at  TIMESTAMPTZ NULL
);

CREATE INDEX outbox_unprocessed_idx ON outbox_events (occurred_at) WHERE processed_at IS NULL;

CREATE TABLE webhook_subscriptions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url         TEXT NOT NULL,
    secret      TEXT NOT NULL,
    events      TEXT[] NOT NULL,
    active      BOOLEAN NOT NULL DEFAULT true,
    created_by  UUID NOT NULL REFERENCES scholars (id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE webhook_deliveries (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id          UUID NOT NULL,
    subscription_id   UUID NOT NULL REFERENCES webhook_subscriptions (id) ON DELETE CASCADE,
    event_type        TEXT NOT NULL,
    payload           JSONB NOT NULL,
    status            TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'delivered', 'dead')),
    attempt           INT NOT NULL DEFAULT 0,
    last_status_code  INT NULL,
    next_retry_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    occurred_at       TIMESTAMPTZ NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX webhook_deliveries_due_idx ON webhook_deliveries (next_retry_at) WHERE status = 'pending';
CREATE INDEX webhook_deliveries_list_idx ON webhook_deliveries (created_at DESC, id DESC);
