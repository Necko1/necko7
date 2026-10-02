ALTER TABLE script_projects
    ADD COLUMN execution_timeout_secs INTEGER NOT NULL DEFAULT 30,
    ADD COLUMN host_timeout_secs INTEGER NOT NULL DEFAULT 10,
    ADD CONSTRAINT script_project_execution_timeout CHECK (execution_timeout_secs BETWEEN 1 AND 120),
    ADD CONSTRAINT script_project_host_timeout CHECK (host_timeout_secs BETWEEN 1 AND 60 AND host_timeout_secs <= execution_timeout_secs);
