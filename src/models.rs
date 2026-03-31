use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Utilisateur authentifié via Clever Cloud OAuth1
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub cc_user_id: String,
    pub cc_email: Option<String>,
    pub access_token: String,
    pub access_secret: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Session HTTP (liée à un User)
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Schedule d'extinction/allumage pour une application Clever Cloud
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Schedule {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub org_id: String,
    pub app_id: String,
    pub name: Option<String>,
    pub cron_stop: Option<String>,
    pub cron_start: Option<String>,
    pub timezone: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_action: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSchedule {
    pub org_id: String,
    pub app_id: String,
    pub name: Option<String>,
    pub cron_stop: Option<String>,
    pub cron_start: Option<String>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSchedule {
    pub name: Option<String>,
    pub cron_stop: Option<Option<String>>,
    pub cron_start: Option<Option<String>>,
    pub timezone: Option<String>,
    pub enabled: Option<bool>,
}

fn default_timezone() -> String {
    "Europe/Paris".to_string()
}

fn default_true() -> bool {
    true
}
