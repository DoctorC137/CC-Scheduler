// config.rs
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Port HTTP (Clever Cloud injecte PORT=8080)
    pub port: u16,

    /// URL PostgreSQL (POSTGRESQL_ADDON_URI injectée par l'add-on)
    pub database_url: String,

    /// OAuth1 access token (depuis clever-tools config : token)
    pub cc_oauth_token: String,

    /// OAuth1 token secret (depuis clever-tools config : secret)
    pub cc_oauth_secret: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok(); // Charge .env en développement local

        Ok(Self {
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()?,
            database_url: std::env::var("POSTGRESQL_ADDON_URI")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .expect("POSTGRESQL_ADDON_URI or DATABASE_URL must be set"),
            cc_oauth_token: std::env::var("CC_OAUTH_TOKEN")
                .or_else(|_| std::env::var("CC_API_TOKEN"))
                .expect("CC_OAUTH_TOKEN must be set"),
            cc_oauth_secret: std::env::var("CC_OAUTH_SECRET")
                .expect("CC_OAUTH_SECRET must be set"),
        })
    }
}
