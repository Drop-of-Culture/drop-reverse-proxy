-- Admins of the back-office. Identity comes from oauth2-proxy (GitHub login),
-- this table only decides who is allowed in and with which role.
CREATE TABLE IF NOT EXISTS "admin_user" (
    github_login TEXT PRIMARY KEY,
    role         TEXT NOT NULL CHECK (role IN ('owner', 'editor')),
    active       BOOLEAN NOT NULL DEFAULT TRUE,
    create_date  TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- One row per change made through the back-office, written in the same
-- transaction as the change itself.
CREATE TABLE IF NOT EXISTS "audit_log" (
    id           BIGSERIAL PRIMARY KEY,
    github_login TEXT NOT NULL,
    entity       TEXT NOT NULL,
    entity_id    TEXT NOT NULL,
    action       TEXT NOT NULL CHECK (action IN ('create', 'update', 'delete')),
    before       JSONB,
    after        JSONB,
    create_date  TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS audit_log_entity_idx ON "audit_log" (entity, entity_id);
