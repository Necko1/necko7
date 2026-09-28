CREATE TABLE script_editor_reports (
 id UUID PRIMARY KEY, project_id UUID NOT NULL REFERENCES script_projects(id),
 source TEXT NOT NULL CHECK(source IN ('validate','publish','dry_run')),
 status TEXT NOT NULL CHECK(status IN ('success','failed')), report JSONB NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX script_editor_reports_history ON script_editor_reports(project_id,created_at DESC);
