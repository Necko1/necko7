use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct ExecutionLimits {
    pub execution_timeout_secs: i32,
    pub host_timeout_secs: i32,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self { execution_timeout_secs: 30, host_timeout_secs: 10 }
    }
}

impl ExecutionLimits {
    pub fn validate(self) -> Result<(), &'static str> {
        if !(1..=120).contains(&self.execution_timeout_secs) {
            return Err("Execution timeout must be a whole number from 1 to 120 seconds");
        }
        if !(1..=60).contains(&self.host_timeout_secs) {
            return Err("Host timeout must be a whole number from 1 to 60 seconds");
        }
        if self.host_timeout_secs > self.execution_timeout_secs {
            return Err("Host timeout cannot exceed the execution timeout");
        }
        Ok(())
    }

    pub fn deadline(self, start: Instant) -> Instant {
        start + Duration::from_secs(self.execution_timeout_secs as u64)
    }

    pub fn call_budget(self, deadline: Instant) -> Duration {
        deadline.saturating_duration_since(Instant::now())
            .min(Duration::from_secs(self.host_timeout_secs as u64))
    }
}

pub async fn load(pool: &sqlx::PgPool, project: Uuid, channel: &str) -> Result<ExecutionLimits, sqlx::Error> {
    sqlx::query_as("SELECT execution_timeout_secs,host_timeout_secs FROM script_projects WHERE id=$1 AND channel_id=$2 AND deleted_at IS NULL")
        .bind(project).bind(channel).fetch_one(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_and_remaining_budget_are_independent() {
        let defaults = ExecutionLimits::default();
        defaults.validate().unwrap();
        assert_eq!(defaults.execution_timeout_secs, 30);
        assert_eq!(defaults.host_timeout_secs, 10);
        for (execution_timeout_secs, host_timeout_secs) in [(0, 1), (121, 1), (30, 0), (120, 61), (5, 6)] {
            assert!(ExecutionLimits { execution_timeout_secs, host_timeout_secs }.validate().is_err());
        }
        let budget = defaults.call_budget(Instant::now() + Duration::from_millis(80));
        assert!(budget > Duration::ZERO && budget <= Duration::from_millis(80));
        assert_eq!(defaults.call_budget(Instant::now() - Duration::from_millis(1)), Duration::ZERO);
        assert_eq!(defaults.call_budget(defaults.deadline(Instant::now())), Duration::from_secs(10));
    }
}
