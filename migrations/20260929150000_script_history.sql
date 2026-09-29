-- Keep skipped queue admissions attached to their immutable job. A blocked job can
-- still be manually admitted; a job that actually ran cannot be executed twice.
ALTER TABLE script_executions DROP CONSTRAINT script_executions_job_id_key;
CREATE UNIQUE INDEX script_executions_job_attempt ON script_executions(job_id) WHERE status <> 'skipped';
CREATE INDEX script_executions_job_history ON script_executions(job_id,sequence DESC) WHERE job_id IS NOT NULL;
CREATE INDEX script_executions_history_cursor ON script_executions(project_id,created_at DESC,id DESC);
CREATE INDEX script_editor_reports_history_cursor ON script_editor_reports(project_id,created_at DESC,id DESC);
