//! Escaneo de foto: Claude propone hasta 3 hipótesis sobre el entorno; la persona confirma cada una.
//! La foto llega con las caras ya pixeladas en el teléfono, vive solo en memoria y nunca se guarda.

use axum::{Json, extract::State};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AppState,
    auth::AuthUser,
    catalog::{CATALOG, Locale},
    claude::ClaudeError,
    error::{AppError, AppResult},
};

const MAX_IMAGE_BYTES: usize = 1_500_000;
const DAILY_SCANS: i64 = 30;
const MAX_HYPOTHESES: usize = 3;
const MAX_TOKENS: u32 = 8000;

/// Lo que una foto puede sugerir. Voces, olores o parpadeo no se ven: los cubren el audio y las preguntas.
const PHOTO_STIMULI: [&str; 7] = [
    "glare",
    "light_bright",
    "light_dim",
    "visual_clutter",
    "movement",
    "noise_sudden",
    "too_warm",
];
/// Lo que el análisis de audio en el teléfono puede sugerir.
const AUDIO_STIMULI: [&str; 3] = ["voices", "noise_constant", "noise_sudden"];

const SYSTEM: &str = "You help a person find what in their workspace might be making a task harder, \
from one photo they took. You look at the place, never at people.

Return at most 3 hypotheses about visible sources of sensory discomfort for the task. Use only these \
stimulus ids:
- glare: reflections on a screen or a glossy surface
- light_bright: very bright or direct light (window, lamp, patch of sun) in the line of sight
- light_dim: not enough light on the work area
- visual_clutter: many objects, papers, posters or screens in view
- movement: a walkway, door or busy area in the line of sight
- noise_sudden: visible equipment that makes sudden noise (printer, desk phone, coffee machine, door)
- too_warm: direct sun on the desk or seat

Rules:
- Report only what is clearly visible. Vision models tend to imagine objects that usually appear \
together; do not. Use confidence \"low\" when unsure. If nothing plausible is visible, return an \
empty list.
- Never describe people: no appearance, identity, emotions, health, behaviour or body. Pixelated \
areas hide people for privacy; ignore them.
- Point x and y at the source in the image, normalized 0 to 1 from the top-left corner.
- Write each question in the requested language, in plain words, at most 20 words. Start with a \
hedge (\"Parece haber…\" or \"Posible…\" in Spanish; \"There seems to be…\" or \"Possibly…\" in \
English), then ask whether it bothers them for this task. Talk about the place, never about the \
person. No medical or diagnostic words.
- Audio hints only help you prioritise; never present them as something seen.";

#[derive(Deserialize)]
pub struct PhotoScan {
    /// JPEG en base64, ya reducido (~1024 px) y con caras pixeladas en el teléfono.
    image_base64: String,
    task: String,
    /// Estímulos que sugirió el audio procesado en el teléfono (nunca el audio en sí).
    #[serde(default)]
    audio: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Hypothesis {
    stimulus: String,
    x: f64,
    y: f64,
    question: String,
    confidence: String,
}

#[derive(Serialize)]
pub struct PhotoScanResult {
    hypotheses: Vec<Hypothesis>,
    model: String,
}

pub async fn scan_photo(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<PhotoScan>,
) -> AppResult<Json<PhotoScanResult>> {
    let Some(claude) = state.claude.as_ref() else {
        return Err(AppError::Unavailable);
    };
    let task = CATALOG
        .task(&body.task)
        .ok_or_else(|| AppError::BadRequest(format!("tarea desconocida: {}", body.task)))?;
    if body
        .audio
        .iter()
        .any(|s| !AUDIO_STIMULI.contains(&s.as_str()))
    {
        return Err(AppError::BadRequest(
            "audio solo admite voices, noise_constant y noise_sudden".into(),
        ));
    }
    let image = base64::engine::general_purpose::STANDARD
        .decode(body.image_base64.as_bytes())
        .map_err(|_| AppError::BadRequest("image_base64 no es base64 válido".into()))?;
    if image.len() > MAX_IMAGE_BYTES || !image.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Err(AppError::BadRequest(
            "la imagen debe ser un JPEG de 1.5 MB o menos".into(),
        ));
    }
    drop(image);

    let today: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scans WHERE user_id = $1 AND created_at > now() - interval '1 day'",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if today >= DAILY_SCANS {
        return Err(AppError::TooManyRequests);
    }

    let language = match user.locale {
        Locale::Es => "Spanish (Mexico)",
        Locale::En => "English",
    };
    let audio_hint = if body.audio.is_empty() {
        "nothing in particular".to_owned()
    } else {
        body.audio.join(", ")
    };
    let content = json!([
        {
            "type": "image",
            "source": { "type": "base64", "media_type": "image/jpeg", "data": body.image_base64 },
        },
        {
            "type": "text",
            "text": format!(
                "Task: {}. Audio analysed on the phone suggested: {audio_hint}. Language for questions: {language}.",
                task.label.get(Locale::En)
            ),
        },
    ]);

    let reply = claude
        .structured(SYSTEM, content, schema(), MAX_TOKENS)
        .await;
    let (hypotheses, model, input, output, ok) = match reply {
        Ok(reply) => (
            sanitize(&reply.json),
            reply.model,
            reply.input_tokens,
            reply.output_tokens,
            true,
        ),
        // Si la IA declina, la persona sigue con las preguntas: no es un error para ella.
        Err(ClaudeError::Refusal) => (Vec::new(), claude.model.clone(), 0, 0, false),
        Err(err) => {
            tracing::warn!(error = %err, "falló el análisis de foto");
            record(&state, &user, &claude.model, 0, 0, 0, false).await?;
            return Err(AppError::Unavailable);
        }
    };
    record(&state, &user, &model, input, output, hypotheses.len(), ok).await?;
    Ok(Json(PhotoScanResult { hypotheses, model }))
}

async fn record(
    state: &AppState,
    user: &AuthUser,
    model: &str,
    input_tokens: i32,
    output_tokens: i32,
    hypotheses: usize,
    ok: bool,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO scans (user_id, model, input_tokens, output_tokens, hypotheses, ok)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(user.id)
    .bind(model)
    .bind(input_tokens)
    .bind(output_tokens)
    .bind(hypotheses as i16)
    .bind(ok)
    .execute(&state.db)
    .await?;
    Ok(())
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "hypotheses": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "stimulus": { "type": "string", "enum": PHOTO_STIMULI },
                        "x": { "type": "number" },
                        "y": { "type": "number" },
                        "question": { "type": "string" },
                        "confidence": { "type": "string", "enum": ["low", "medium", "high"] }
                    },
                    "required": ["stimulus", "x", "y", "question", "confidence"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["hypotheses"],
        "additionalProperties": false
    })
}

/// El esquema garantiza la forma; aquí se aplican los límites que el esquema no puede expresar.
fn sanitize(json: &Value) -> Vec<Hypothesis> {
    let mut seen = Vec::new();
    json["hypotheses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| serde_json::from_value::<Hypothesis>(item.clone()).ok())
        .filter(|h| PHOTO_STIMULI.contains(&h.stimulus.as_str()) && !h.question.trim().is_empty())
        // Un marcador por estímulo: la persona confirma cada cosa una sola vez.
        .filter(|h| {
            let new = !seen.contains(&h.stimulus);
            seen.push(h.stimulus.clone());
            new
        })
        .map(|h| Hypothesis {
            x: h.x.clamp(0.0, 1.0),
            y: h.y.clamp(0.0, 1.0),
            question: h.question.trim().chars().take(200).collect(),
            ..h
        })
        .take(MAX_HYPOTHESES)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn keeps_three_valid_distinct_hypotheses_inside_the_photo() {
        let raw = json!({ "hypotheses": [
            { "stimulus": "glare", "x": 1.4, "y": -0.2, "question": "  ¿Reflejo?  ", "confidence": "high" },
            { "stimulus": "glare", "x": 0.5, "y": 0.5, "question": "Otra vez", "confidence": "low" },
            { "stimulus": "voices", "x": 0.5, "y": 0.5, "question": "No se ve", "confidence": "low" },
            { "stimulus": "light_bright", "x": 0.2, "y": 0.1, "question": "¿Luz?", "confidence": "medium" },
            { "stimulus": "movement", "x": 0.9, "y": 0.9, "question": "", "confidence": "low" },
            { "stimulus": "visual_clutter", "x": 0.3, "y": 0.6, "question": "¿Cosas?", "confidence": "low" },
            { "stimulus": "too_warm", "x": 0.3, "y": 0.6, "question": "¿Sol?", "confidence": "low" }
        ]});
        let kept = sanitize(&raw);
        let ids: Vec<_> = kept.iter().map(|h| h.stimulus.as_str()).collect();
        assert_eq!(ids, ["glare", "light_bright", "visual_clutter"]);
        assert_eq!((kept[0].x, kept[0].y), (1.0, 0.0));
        assert_eq!(kept[0].question, "¿Reflejo?");
    }

    #[test]
    fn schema_stimuli_exist_in_the_catalog() {
        for id in PHOTO_STIMULI.iter().chain(AUDIO_STIMULI.iter()) {
            assert!(
                CATALOG.stimulus(id).is_some(),
                "{id} no está en el catálogo"
            );
        }
    }
}
