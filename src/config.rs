use anyhow::Context;

pub struct Config {
    pub port: u16,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let port = match std::env::var("PORT") {
            Ok(port) => port.parse().context("PORT inválido")?,
            Err(_) => 8080,
        };
        let database_url = std::env::var("DATABASE_URL").context("falta DATABASE_URL")?;
        Ok(Self { port, database_url })
    }
}
