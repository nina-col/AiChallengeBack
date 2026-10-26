use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use rand::RngExt;
use uuid::Uuid;

use crate::{
    AppState,
    auth::AuthUser,
    catalog::{AdjustmentKind, BASELINE_ID, CATALOG, Locale, LocalizedAdjustment, LocalizedCatalog},
    engine::{self, Arm, ArmEvidence, EXPLORATION, Evidence, Reason},
    error::{AppError, AppResult},
};

#[derive(Deserialize)]
pub struct CatalogQuery {
    locale: Option<String>,
}

pub async fn get_catalog(Query(query): Query<CatalogQuery>) -> Json<LocalizedCatalog<'static>> {
    Json(CATALOG.localized(Locale::parse(query.locale.as_deref().unwrap_or("es"))))
}

#[derive(Clone, sqlx::FromRow)]
pub struct CheckinRow {
    id: Uuid,
    space_id: Option<Uuid>,
    space_name: Option<String>,
    task: String,
    feeling: String,
    effort_before: i16,
    discomfort_before: Option<i16>,
    focus_stimulus: Option<String>,
    adjustment_id: Option<String>,
    declined: Vec<String>,
    status: String,
    remind_at: Option<DateTime<Utc>>,
    effort_after: Option<i16>,
    discomfort_after: Option<i16>,
    improvement: Option<f32>,
    typical_improvement: Option<f32>,
    attribution: Option<String>,
    outcome: Option<String>,
    created_at: DateTime<Utc>,
}

/// Arma la consulta de registros en tiempo de compilación (un `&'static str`, sin SQL dinámico).
macro_rules! select_checkin {
    ($tail:literal) => {
        concat!(
            "SELECT c.id, c.space_id, s.name AS space_name, c.task, c.feeling, c.effort_before,
                    c.discomfort_before, c.focus_stimulus, c.adjustment_id, c.declined, c.status,
                    c.remind_at, c.effort_after, c.discomfort_after, c.improvement,
                    c.typical_improvement, c.attribution, c.outcome, c.created_at
             FROM checkins c LEFT JOIN spaces s ON s.id = c.space_id ",
            $tail
        )
    };
}

#[derive(Serialize)]
pub struct CheckinView {
    id: Uuid,
    space: Option<SpaceRef>,
    task: String,
    feeling: String,
    effort_before: i16,
    discomfort_before: Option<i16>,
    focus_stimulus: Option<String>,
    status: String,
    remind_at: Option<DateTime<Utc>>,
    effort_after: Option<i16>,
    discomfort_after: Option<i16>,
    improvement: Option<f32>,
    typical_improvement: Option<f32>,
    attribution: Option<String>,
    outcome: Option<String>,
    created_at: DateTime<Utc>,
    adjustment: Option<LocalizedAdjustment<'static>>,
}

#[derive(Serialize)]
struct SpaceRef {
    id: Uuid,
    name: String,
}

fn view(row: CheckinRow, locale: Locale) -> CheckinView {
    CheckinView {
        id: row.id,
        space: row.space_id.zip(row.space_name).map(|(id, name)| SpaceRef { id, name }),
        task: row.task,
        feeling: row.feeling,
        effort_before: row.effort_before,
        discomfort_before: row.discomfort_before,
        focus_stimulus: row.focus_stimulus,
        status: row.status,
        remind_at: row.remind_at,
        effort_after: row.effort_after,
        discomfort_after: row.discomfort_after,
        improvement: row.improvement,
        typical_improvement: row.typical_improvement,
        attribution: row.attribution,
        outcome: row.outcome,
        created_at: row.created_at,
        adjustment: row.adjustment_id.as_deref().and_then(|id| CATALOG.adjustment_view(id, locale)),
    }
}

#[derive(Serialize)]
pub struct Proposal {
    adjustment: LocalizedAdjustment<'static>,
    reason: ReasonView,
    /// Cuántas otras ideas quedan para este estímulo si esta no sirve.
    alternatives: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReasonView {
    FirstTry,
    Exploring,
    TriedHere { helped: u32, trials: u32 },
    TriedElsewhere { helped: u32, trials: u32 },
    /// Registro de comparación sin ajuste.
    Comparison,
}

#[derive(Serialize)]
pub struct CheckinWithProposal {
    checkin: CheckinView,
    proposal: Option<Proposal>,
}

#[derive(Deserialize)]
pub struct NewObservation {
    stimulus: String,
    response: String,
    /// De dónde salió: una pregunta, una hipótesis de la foto o del audio que la persona confirmó.
    #[serde(default = "question_source")]
    source: String,
}

fn question_source() -> String {
    "question".to_owned()
}

#[derive(Deserialize)]
pub struct NewCheckin {
    space_id: Option<Uuid>,
    task: String,
    feeling: String,
    effort_before: i16,
    discomfort_before: i16,
    #[serde(default)]
    observations: Vec<NewObservation>,
    focus_stimulus: Option<String>,
}

pub async fn create_checkin(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<NewCheckin>,
) -> AppResult<Json<CheckinWithProposal>> {
    if CATALOG.task(&body.task).is_none() {
        return Err(AppError::BadRequest(format!("tarea desconocida: {}", body.task)));
    }
    if CATALOG.feeling(&body.feeling).is_none() {
        return Err(AppError::BadRequest(format!("sensación desconocida: {}", body.feeling)));
    }
    check_scale(body.effort_before)?;
    check_scale(body.discomfort_before)?;
    for obs in &body.observations {
        if CATALOG.stimulus(&obs.stimulus).is_none() {
            return Err(AppError::BadRequest(format!("estímulo desconocido: {}", obs.stimulus)));
        }
        if !matches!(obs.response.as_str(), "yes" | "no" | "unsure") {
            return Err(AppError::BadRequest("response debe ser yes, no o unsure".into()));
        }
        if !matches!(obs.source.as_str(), "question" | "photo" | "audio") {
            return Err(AppError::BadRequest("source debe ser question, photo o audio".into()));
        }
    }
    if let Some(space_id) = body.space_id {
        let owned: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM spaces WHERE id = $1 AND user_id = $2)")
                .bind(space_id)
                .bind(user.id)
                .fetch_one(&state.db)
                .await?;
        if !owned {
            return Err(AppError::NotFound);
        }
    }

    // El foco es lo que la persona eligió; si solo marcó una cosa que le afecta, es esa.
    let affecting: Vec<&str> = body
        .observations
        .iter()
        .filter(|o| o.response == "yes")
        .map(|o| o.stimulus.as_str())
        .collect();
    let focus = match body.focus_stimulus.as_deref() {
        Some(focus) if affecting.contains(&focus) => Some(focus.to_owned()),
        Some(focus) => {
            return Err(AppError::BadRequest(format!(
                "focus_stimulus debe ser algo que la persona marcó: {focus}"
            )));
        }
        None if affecting.len() == 1 => Some(affecting[0].to_owned()),
        None if affecting.is_empty() => None,
        None => return Err(AppError::BadRequest("hay varios estímulos: falta focus_stimulus".into())),
    };

    let mut tx = state.db.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO checkins
            (user_id, space_id, task, feeling, effort_before, discomfort_before, focus_stimulus)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(user.id)
    .bind(body.space_id)
    .bind(&body.task)
    .bind(&body.feeling)
    .bind(body.effort_before)
    .bind(body.discomfort_before)
    .bind(&focus)
    .fetch_one(&mut *tx)
    .await?;
    for obs in &body.observations {
        sqlx::query(
            "INSERT INTO observations (checkin_id, source, stimulus, response) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(&obs.source)
        .bind(&obs.stimulus)
        .bind(&obs.response)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let row = load(&state, user.id, id).await?;
    let proposal = propose(&state, user.id, user.locale, &row).await?;
    let row = load(&state, user.id, id).await?;
    Ok(Json(CheckinWithProposal { checkin: view(row, user.locale), proposal }))
}

#[derive(Deserialize)]
pub struct Decline {
    /// `other_idea`: prefiere otra cosa ahora · `cannot_here`: no se puede en este lugar.
    reason: String,
}

pub async fn decline(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<Decline>,
) -> AppResult<Json<CheckinWithProposal>> {
    let row = load(&state, user.id, id).await?;
    if row.status != "proposed" {
        return Err(AppError::BadRequest("este registro ya no está esperando una propuesta".into()));
    }
    let Some(current) = row.adjustment_id.clone() else {
        return Err(AppError::BadRequest("no hay ningún ajuste propuesto".into()));
    };
    match body.reason.as_str() {
        "other_idea" => {}
        "cannot_here" => {
            if let Some(space_id) = row.space_id {
                sqlx::query(
                    "INSERT INTO space_blocks (space_id, adjustment_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(space_id)
                .bind(&current)
                .execute(&state.db)
                .await?;
            }
        }
        _ => return Err(AppError::BadRequest("reason debe ser other_idea o cannot_here".into())),
    }
    sqlx::query(
        "UPDATE checkins SET declined = array_append(declined, $2), adjustment_id = NULL WHERE id = $1",
    )
    .bind(id)
    .bind(&current)
    .execute(&state.db)
    .await?;

    let row = load(&state, user.id, id).await?;
    let proposal = propose(&state, user.id, user.locale, &row).await?;
    let row = load(&state, user.id, id).await?;
    Ok(Json(CheckinWithProposal { checkin: view(row, user.locale), proposal }))
}

#[derive(Deserialize)]
pub struct Accept {
    /// Minutos hasta el recordatorio "¿cómo te fue?"; sin valor, la persona vuelve cuando quiera.
    remind_in_minutes: Option<u16>,
}

pub async fn accept(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<Accept>,
) -> AppResult<Json<CheckinView>> {
    let row = load(&state, user.id, id).await?;
    if row.status != "proposed" || row.adjustment_id.is_none() {
        return Err(AppError::BadRequest("no hay un ajuste propuesto para aceptar".into()));
    }
    let remind_at = match body.remind_in_minutes {
        Some(minutes @ 5..=240) => Some(Utc::now() + Duration::minutes(i64::from(minutes))),
        Some(_) => return Err(AppError::BadRequest("remind_in_minutes debe estar entre 5 y 240".into())),
        None => None,
    };
    sqlx::query("UPDATE checkins SET status = 'trying', started_at = now(), remind_at = $2 WHERE id = $1")
        .bind(id)
        .bind(remind_at)
        .execute(&state.db)
        .await?;
    Ok(Json(view(load(&state, user.id, id).await?, user.locale)))
}

#[derive(Deserialize)]
pub struct NewOutcome {
    effort_after: i16,
    discomfort_after: i16,
    attribution: String,
}

#[derive(Serialize)]
pub struct OutcomeView {
    checkin: CheckinView,
    /// Veces que este ajuste ha ayudado / probado con este estímulo, en esta tarea y lugar.
    here: EvidenceView,
}

#[derive(Serialize)]
struct EvidenceView {
    helped: u32,
    trials: u32,
}

pub async fn record_outcome(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<NewOutcome>,
) -> AppResult<Json<OutcomeView>> {
    check_scale(body.effort_after)?;
    check_scale(body.discomfort_after)?;
    if !matches!(body.attribution.as_str(), "yes" | "no" | "unsure") {
        return Err(AppError::BadRequest("attribution debe ser yes, no o unsure".into()));
    }
    let row = load(&state, user.id, id).await?;
    if row.status != "trying" {
        return Err(AppError::BadRequest("este registro no tiene un ajuste en prueba".into()));
    }
    // Registros anteriores a la medida de molestia: el esfuerzo hace de respaldo.
    let before = row.discomfort_before.unwrap_or(row.effort_before);
    let improvement = f64::from(before - body.discomfort_after);
    let typical = typical_improvement(&state, user.id).await?;
    // La comparación sin ajuste alimenta la línea base, no al motor.
    let is_baseline = row.adjustment_id.as_deref() == Some(BASELINE_ID);
    let outcome = (!is_baseline).then(|| outcome_of(improvement, typical, &body.attribution));
    sqlx::query(
        "UPDATE checkins SET status = 'done', completed_at = now(), effort_after = $2,
                discomfort_after = $3, attribution = $4, outcome = $5, improvement = $6,
                typical_improvement = $7
         WHERE id = $1",
    )
    .bind(id)
    .bind(body.effort_after)
    .bind(body.discomfort_after)
    .bind(&body.attribution)
    .bind(outcome)
    .bind(improvement as f32)
    .bind(typical as f32)
    .execute(&state.db)
    .await?;

    let row = load(&state, user.id, id).await?;
    let (helped, trials): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE outcome = 'helped'),
                count(*) FILTER (WHERE outcome IN ('helped', 'not_helped'))
         FROM checkins
         WHERE user_id = $1 AND adjustment_id = $2 AND focus_stimulus IS NOT DISTINCT FROM $3
           AND task = $4 AND space_id IS NOT DISTINCT FROM $5",
    )
    .bind(user.id)
    .bind(&row.adjustment_id)
    .bind(&row.focus_stimulus)
    .bind(&row.task)
    .bind(row.space_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(OutcomeView {
        checkin: view(row, user.locale),
        here: EvidenceView { helped: count(helped), trials: count(trials) },
    }))
}

pub async fn abandon(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<CheckinView>> {
    let updated = sqlx::query(
        "UPDATE checkins SET status = 'abandoned', completed_at = now()
         WHERE id = $1 AND user_id = $2 AND status IN ('proposed', 'trying')",
    )
    .bind(id)
    .bind(user.id)
    .execute(&state.db)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(view(load(&state, user.id, id).await?, user.locale)))
}

#[derive(Deserialize)]
pub struct ListQuery {
    status: Option<String>,
    limit: Option<i64>,
}

pub async fn list_checkins(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<Vec<CheckinView>>> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let rows: Vec<CheckinRow> = sqlx::query_as(select_checkin!(
        "WHERE c.user_id = $1 AND ($2::text IS NULL OR c.status = $2)
         ORDER BY c.created_at DESC LIMIT $3"
    ))
    .bind(user.id)
    .bind(&query.status)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| view(r, user.locale)).collect()))
}

pub async fn get_checkin(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<CheckinView>> {
    Ok(Json(view(load(&state, user.id, id).await?, user.locale)))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct PlaybookRow {
    task: String,
    space_id: Option<Uuid>,
    space_name: Option<String>,
    stimulus: String,
    adjustment_id: String,
    helped: i64,
    trials: i64,
    last_tried: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct PlaybookEntry {
    task: String,
    space: Option<SpaceRef>,
    stimulus: String,
    adjustment: LocalizedAdjustment<'static>,
    helped: u32,
    trials: u32,
    last_tried: Option<DateTime<Utc>>,
}

/// Mis ajustes: lo que ha ayudado al menos una vez, por tarea, lugar y estímulo.
pub async fn playbook(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<PlaybookEntry>>> {
    let rows: Vec<PlaybookRow> = sqlx::query_as(
        "SELECT c.task, c.space_id, s.name AS space_name, c.focus_stimulus AS stimulus, c.adjustment_id,
                count(*) FILTER (WHERE c.outcome = 'helped') AS helped,
                count(*) FILTER (WHERE c.outcome IN ('helped', 'not_helped')) AS trials,
                max(c.completed_at) AS last_tried
         FROM checkins c LEFT JOIN spaces s ON s.id = c.space_id
         WHERE c.user_id = $1 AND c.status = 'done' AND c.adjustment_id <> 'no_change'
           AND c.adjustment_id IS NOT NULL AND c.focus_stimulus IS NOT NULL
         GROUP BY c.task, c.space_id, s.name, c.focus_stimulus, c.adjustment_id
         HAVING count(*) FILTER (WHERE c.outcome = 'helped') > 0
         ORDER BY count(*) FILTER (WHERE c.outcome = 'helped') DESC, max(c.completed_at) DESC",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let entries = rows
        .into_iter()
        .filter_map(|r| {
            let adjustment = CATALOG.adjustment(&r.adjustment_id)?.localized(user.locale);
            Some(PlaybookEntry {
                task: r.task,
                space: r.space_id.zip(r.space_name).map(|(id, name)| SpaceRef { id, name }),
                stimulus: r.stimulus,
                adjustment,
                helped: count(r.helped),
                trials: count(r.trials),
                last_tried: r.last_tried,
            })
        })
        .collect();
    Ok(Json(entries))
}

async fn load(state: &AppState, user_id: Uuid, id: Uuid) -> AppResult<CheckinRow> {
    sqlx::query_as(select_checkin!("WHERE c.id = $1 AND c.user_id = $2"))
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

#[derive(sqlx::FromRow)]
struct EvidenceRow {
    adjustment_id: String,
    exact_helped: i64,
    exact_not: i64,
    person_helped: i64,
    person_not: i64,
    global_helped: i64,
    global_not: i64,
}

/// Elige el siguiente ajuste para el registro y lo guarda. `None` si no hay estímulo o ya no quedan ideas.
async fn propose(
    state: &AppState,
    user_id: Uuid,
    locale: Locale,
    row: &CheckinRow,
) -> AppResult<Option<Proposal>> {
    let Some(stimulus) = row.focus_stimulus.as_deref() else {
        return Ok(None);
    };
    if wants_comparison(state, user_id, row).await? {
        sqlx::query("UPDATE checkins SET adjustment_id = $2 WHERE id = $1")
            .bind(row.id)
            .bind(BASELINE_ID)
            .execute(&state.db)
            .await?;
        let adjustment = CATALOG.adjustment_view(BASELINE_ID, locale).expect("baseline");
        return Ok(Some(Proposal { adjustment, reason: ReasonView::Comparison, alternatives: 1 }));
    }
    let blocked: Vec<String> = match row.space_id {
        Some(space_id) => sqlx::query_scalar("SELECT adjustment_id FROM space_blocks WHERE space_id = $1")
            .bind(space_id)
            .fetch_all(&state.db)
            .await?,
        None => Vec::new(),
    };
    let available: Vec<_> = CATALOG
        .adjustments
        .iter()
        .filter(|a| a.fits(stimulus, &row.task))
        .filter(|a| !row.declined.contains(&a.id) && !blocked.contains(&a.id))
        .collect();
    // En el momento se proponen cosas que la persona puede hacer ya; pedir a la empresa queda
    // para cuando no queda nada propio que probar.
    let has_self = available.iter().any(|a| a.kind == AdjustmentKind::SelfServe);
    let candidates: Vec<_> = available
        .into_iter()
        .filter(|a| !has_self || a.kind == AdjustmentKind::SelfServe)
        .collect();
    if candidates.is_empty() {
        return Ok(None);
    }

    let evidence: HashMap<String, ArmEvidence> = sqlx::query_as::<_, EvidenceRow>(
        "SELECT adjustment_id,
           count(*) FILTER (WHERE user_id = $1 AND task = $3 AND space_id IS NOT DISTINCT FROM $4 AND outcome = 'helped') AS exact_helped,
           count(*) FILTER (WHERE user_id = $1 AND task = $3 AND space_id IS NOT DISTINCT FROM $4 AND outcome = 'not_helped') AS exact_not,
           count(*) FILTER (WHERE user_id = $1 AND outcome = 'helped') AS person_helped,
           count(*) FILTER (WHERE user_id = $1 AND outcome = 'not_helped') AS person_not,
           count(*) FILTER (WHERE outcome = 'helped') AS global_helped,
           count(*) FILTER (WHERE outcome = 'not_helped') AS global_not
         FROM checkins
         WHERE focus_stimulus = $2 AND adjustment_id IS NOT NULL AND outcome IN ('helped', 'not_helped')
         GROUP BY adjustment_id",
    )
    .bind(user_id)
    .bind(stimulus)
    .bind(&row.task)
    .bind(row.space_id)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|e| {
        let ev = |helped, not_helped| Evidence { helped: count(helped), not_helped: count(not_helped) };
        (
            e.adjustment_id,
            ArmEvidence {
                exact: ev(e.exact_helped, e.exact_not),
                person: ev(e.person_helped, e.person_not),
                global: ev(e.global_helped, e.global_not),
            },
        )
    })
    .collect();

    let arms: Vec<Arm> = candidates
        .iter()
        .map(|a| Arm { id: &a.id, prior: a.prior, evidence: evidence.get(&a.id).copied().unwrap_or_default() })
        .collect();
    let chosen = engine::choose(&arms, EXPLORATION, &mut rand::rng()).expect("hay candidatos");
    let adjustment = CATALOG.adjustment(chosen.id).expect("el candidato viene del catálogo");

    sqlx::query("UPDATE checkins SET adjustment_id = $2 WHERE id = $1")
        .bind(row.id)
        .bind(&adjustment.id)
        .execute(&state.db)
        .await?;

    let reason = match engine::reason(chosen, &arms) {
        Reason::FirstTry => ReasonView::FirstTry,
        Reason::Exploring => ReasonView::Exploring,
        Reason::TriedHere(e) => ReasonView::TriedHere { helped: e.helped, trials: e.trials() },
        Reason::TriedElsewhere(e) => ReasonView::TriedElsewhere { helped: e.helped, trials: e.trials() },
    };
    Ok(Some(Proposal {
        adjustment: adjustment.localized(locale),
        reason,
        alternatives: candidates.len() - 1,
    }))
}

/// Cada cuánto se propone la comparación sin ajuste, una vez que la persona ya tiene historia.
const COMPARISON_RATE: f64 = 0.1;
const COMPARISON_MIN_HISTORY: i64 = 3;

/// ¿Toca proponer "esta vez no cambies nada"? Nunca dos veces seguidas ni si ya la descartó aquí.
async fn wants_comparison(state: &AppState, user_id: Uuid, row: &CheckinRow) -> AppResult<bool> {
    if row.declined.iter().any(|d| d == BASELINE_ID) || !rand::rng().random_bool(COMPARISON_RATE) {
        return Ok(false);
    }
    let (done, last_was_comparison): (i64, Option<bool>) = sqlx::query_as(
        "SELECT count(*),
                (array_agg(adjustment_id = 'no_change' ORDER BY completed_at DESC))[1]
         FROM checkins WHERE user_id = $1 AND status = 'done'",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(done >= COMPARISON_MIN_HISTORY && last_was_comparison != Some(true))
}

/// Mejora típica de la persona (molestia antes − después) en sus últimos 20 registros.
/// Si hay al menos dos comparaciones sin ajuste, se usan solo esas. Con pocos datos se acerca a
/// un prior de 0.5: quien registra suele hacerlo en un mal momento, y algo mejora solo.
async fn typical_improvement(state: &AppState, user_id: Uuid) -> AppResult<f64> {
    let (baseline_n, baseline_avg, all_n, all_avg): (i64, Option<f64>, i64, Option<f64>) =
        sqlx::query_as(
            "SELECT count(*) FILTER (WHERE adjustment_id = 'no_change'),
                    (avg(improvement) FILTER (WHERE adjustment_id = 'no_change'))::float8,
                    count(*), avg(improvement)::float8
             FROM (SELECT adjustment_id, improvement FROM checkins
                   WHERE user_id = $1 AND status = 'done' AND improvement IS NOT NULL
                   ORDER BY completed_at DESC LIMIT 20) recent",
        )
        .bind(user_id)
        .fetch_one(&state.db)
        .await?;
    Ok(if baseline_n >= 2 {
        shrink(baseline_avg.unwrap_or(0.0), baseline_n)
    } else {
        shrink(all_avg.unwrap_or(0.0), all_n)
    })
}

const TYPICAL_PRIOR: f64 = 0.5;
const TYPICAL_PRIOR_WEIGHT: f64 = 2.0;

fn shrink(average: f64, n: i64) -> f64 {
    let n = n as f64;
    (TYPICAL_PRIOR * TYPICAL_PRIOR_WEIGHT + average * n) / (TYPICAL_PRIOR_WEIGHT + n)
}

/// Qué aprende el motor. Se compara la mejora contra la típica de la persona, no contra cero,
/// para no premiar el simple paso del tiempo. Si la persona dice que no ayudó, no ayudó.
pub fn outcome_of(improvement: f64, typical: f64, attribution: &str) -> &'static str {
    let margin = improvement - typical;
    match attribution {
        "no" => "not_helped",
        "yes" if margin >= 0.0 => "helped",
        "yes" => "unclear",
        _ if margin >= 1.0 => "helped",
        _ if margin <= -1.0 => "not_helped",
        _ => "unclear",
    }
}

fn check_scale(value: i16) -> AppResult<()> {
    if (1..=5).contains(&value) {
        Ok(())
    } else {
        Err(AppError::BadRequest("las escalas van de 1 a 5".into()))
    }
}

fn count(n: i64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{outcome_of, shrink};

    #[test]
    fn a_no_is_always_a_no() {
        assert_eq!(outcome_of(3.0, 0.5, "no"), "not_helped");
    }

    #[test]
    fn helping_means_better_than_usual_not_just_better() {
        // Mejoró 1, pero lo típico para esta persona es mejorar 1.5: no cuenta como evidencia.
        assert_eq!(outcome_of(1.0, 1.5, "yes"), "unclear");
        assert_eq!(outcome_of(2.0, 1.5, "yes"), "helped");
        assert_eq!(outcome_of(3.0, 1.0, "unsure"), "helped");
        assert_eq!(outcome_of(0.0, 1.5, "unsure"), "not_helped");
        assert_eq!(outcome_of(1.0, 0.5, "unsure"), "unclear");
    }

    #[test]
    fn typical_improvement_starts_near_the_prior() {
        assert!((shrink(0.0, 0) - 0.5).abs() < 1e-9);
        assert!((shrink(2.0, 2) - 1.25).abs() < 1e-9);
        assert!((shrink(1.0, 100) - 1.0).abs() < 0.02);
    }
}
