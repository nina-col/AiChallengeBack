use anyhow::Context;

pub struct Config {
    pub port: u16,
    pub database_url: String,
    /// Sin llave, la app funciona igual y el escaneo con foto responde "no disponible".
    pub anthropic_api_key: Option<String>,
    pub vision_model: String,
    pub vision_effort: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let port = match std::env::var("PORT") {
            Ok(port) => port.parse().context("PORT inválido")?,
            Err(_) => 8080,
        };
        let database_url = std::env::var("DATABASE_URL").context("falta DATABASE_URL")?;
        let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty());
        let vision_model =
            std::env::var("CLAUDE_VISION_MODEL").unwrap_or_else(|_| "claude-opus-5-5".into());
        let vision_effort =
            std::env::var("CLAUDE_VISION_EFFORT").unwrap_or_else(|_| "medium".into());
        Ok(Self {
            port,
            database_url,
            anthropic_api_key,
            vision_model,
            vision_effort,
        })
    }
}
