-- Drafts are never executable. Revisions are immutable snapshots.
CREATE TABLE script_projects (
 id UUID PRIMARY KEY, channel_id VARCHAR(255) NOT NULL REFERENCES broadcasters(channel_id),
 name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80), enabled BOOLEAN NOT NULL DEFAULT false,
 draft JSONB NOT NULL DEFAULT '{"main.rhai":"fn on_event(ctx) {\n    log.info(ctx.event.kind);\n}\n"}',
 draft_version BIGINT NOT NULL DEFAULT 1, active_revision BIGINT,
 deleted_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX script_projects_channel ON script_projects(channel_id);
CREATE TABLE script_revisions (
 project_id UUID NOT NULL REFERENCES script_projects(id), revision BIGINT NOT NULL,
 files JSONB NOT NULL, has_on_event BOOLEAN NOT NULL, has_on_timer BOOLEAN NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY(project_id,revision)
);
ALTER TABLE script_projects ADD FOREIGN KEY(id,active_revision) REFERENCES script_revisions(project_id,revision);
CREATE TABLE script_storage (
 project_id UUID NOT NULL REFERENCES script_projects(id), key TEXT NOT NULL CHECK(length(key) BETWEEN 1 AND 128),
 value JSONB NOT NULL CHECK(octet_length(value::text)<=65536), updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 PRIMARY KEY(project_id,key)
);
CREATE TABLE script_jobs (
 id UUID PRIMARY KEY, project_id UUID NOT NULL, revision BIGINT NOT NULL, job_key TEXT NOT NULL CHECK(length(job_key) BETWEEN 1 AND 128),
 payload JSONB NOT NULL CHECK(octet_length(payload::text)<=65536), scheduled_for TIMESTAMPTZ NOT NULL,
 status TEXT NOT NULL DEFAULT 'scheduled' CHECK(status IN ('scheduled','blocked','queued','completed','cancelled','failed')),
 reason TEXT, host_action TEXT CHECK(host_action IS NULL OR host_action='hide_reward'), created_at TIMESTAMPTZ NOT NULL DEFAULT now(), completed_at TIMESTAMPTZ,
 FOREIGN KEY(project_id,revision) REFERENCES script_revisions(project_id,revision)
);
CREATE UNIQUE INDEX script_jobs_live_key ON script_jobs(project_id,job_key) WHERE status IN ('scheduled','blocked','queued');
CREATE INDEX script_jobs_due ON script_jobs(scheduled_for) WHERE status='scheduled';
CREATE TABLE script_matches (
 id UUID PRIMARY KEY, channel_id VARCHAR(255) NOT NULL REFERENCES broadcasters(channel_id),
 device_id UUID NOT NULL, session_id UUID NOT NULL, map TEXT NOT NULL,
 data JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), completed_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX script_matches_active ON script_matches(channel_id) WHERE completed_at IS NULL;
CREATE INDEX script_matches_channel ON script_matches(channel_id,created_at DESC);
-- One normalized snapshot per source payload, shared by every project/event.
CREATE TABLE script_snapshots (
 id BIGSERIAL PRIMARY KEY, channel_id VARCHAR(255) NOT NULL REFERENCES broadcasters(channel_id),
 device_id UUID NOT NULL, session_id UUID NOT NULL, seq BIGINT NOT NULL,
 context JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), UNIQUE(device_id,session_id,seq)
);
CREATE TABLE script_executions (
 sequence BIGSERIAL UNIQUE, id UUID PRIMARY KEY, project_id UUID NOT NULL, revision BIGINT NOT NULL,
 source TEXT NOT NULL CHECK(source IN ('cs2','timer')), snapshot_id BIGINT REFERENCES script_snapshots(id),
 event_index INTEGER, job_id UUID UNIQUE REFERENCES script_jobs(id),
 status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','success','failed','skipped','interrupted')),
 actor_type TEXT NOT NULL DEFAULT 'script', actor_user_id TEXT,
 report JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), finished_at TIMESTAMPTZ,
 FOREIGN KEY(project_id,revision) REFERENCES script_revisions(project_id,revision)
);
CREATE INDEX script_executions_queue ON script_executions(project_id,sequence) WHERE status IN ('queued','running');
CREATE INDEX script_executions_history ON script_executions(project_id,created_at DESC);
