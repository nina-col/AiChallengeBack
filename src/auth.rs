use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{AppState, catalog::Locale, error::AppError};

/// La app genera una llave aleatoria por dispositivo y la manda como `Authorization: Bearer <llave>`.
/// El servidor solo guarda su SHA-256: sin correo ni contraseña, y borrar la cuenta borra todo.
pub struct AuthUser {
    pub id: Uuid,
    pub locale: Locale,
}

pub const MIN_KEY_LEN: usize = 32;
pub const MAX_KEY_LEN: usize = 128;

pub fn hash_device_key(key: &str) -> Vec<u8> {
    Sha256::digest(key.as_bytes()).to_vec()
}

pub fn valid_device_key(key: &str) -> bool {
    (MIN_KEY_LEN..=MAX_KEY_LEN).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|key| valid_device_key(key))
            .ok_or(AppError::Unauthorized)?;

        let user: Option<(Uuid, String)> =
            sqlx::query_as("SELECT id, locale FROM users WHERE key_hash = $1")
                .bind(hash_device_key(key))
                .fetch_optional(&state.db)
                .await?;
        user.map(|(id, locale)| AuthUser {
            id,
            locale: Locale::parse(&locale),
        })
        .ok_or(AppError::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_or_strange_keys() {
        assert!(!valid_device_key("corta"));
        assert!(!valid_device_key(&"a".repeat(MAX_KEY_LEN + 1)));
        assert!(!valid_device_key(&format!(
            "{}{}",
            "a".repeat(40),
            " espacio"
        )));
        assert!(valid_device_key(&"aZ09-_".repeat(8)));
    }

    #[test]
    fn hash_is_stable_and_not_the_key() {
        let key = "k".repeat(43);
        assert_eq!(hash_device_key(&key), hash_device_key(&key));
        assert_eq!(hash_device_key(&key).len(), 32);
        assert_ne!(hash_device_key(&key), key.as_bytes());
    }
}
