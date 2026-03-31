use anyhow::Result;

#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Port HTTP (Clever Cloud injecte PORT=8080)
    pub port: u16,

    /// URL PostgreSQL
    pub database_url: String,

    /// Secret pour signer les cookies de session (générer avec: openssl rand -hex 32)
    pub session_secret: String,

    /// URL publique de l'app (ex: https://app-xxx.cleverapps.io) — utilisée pour le callback OAuth1
    pub base_url: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()?,
            database_url: std::env::var("POSTGRESQL_ADDON_URI")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .expect("POSTGRESQL_ADDON_URI or DATABASE_URL must be set"),
            session_secret: std::env::var("SESSION_SECRET")
                .expect("SESSION_SECRET must be set (generate with: openssl rand -hex 32)"),
            base_url: std::env::var("BASE_URL")
                .unwrap_or_else(|_| "http://localhost:8080".into()),
        })
    }
}
