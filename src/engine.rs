//! Motor personal: elige UN ajuste con Thompson sampling sobre la evidencia de cada persona.
//!
//! Cada ajuste tiene una probabilidad de "me ayudó" modelada como Beta(α, β). Se arma con:
//! - un prior del catálogo (qué tan seguido suele ayudar ese ajuste para ese estímulo),
//! - lo que le pasó a esta persona en el mismo contexto (estímulo × tarea × lugar), con peso 1,
//! - lo que le pasó en este mismo lugar con ese estímulo, en otra tarea, con peso 0.75,
//! - lo que le pasó con ese estímulo en otros lugares, con peso medio,
//! - lo que les pasó a otras personas, con peso bajo y un tope, para arrancar sin datos.
//!
//! Cada prueba cuenta una sola vez, en su nivel más específico. Lo propio pesa más si es reciente
//! (vale la mitad cada 60 días), por si el lugar cambió. "Este lugar cambió" manda lo anterior de
//! ese lugar al nivel general de la persona: sigue siendo cierto de ella, pero ya no de ese lugar.
//!
//! Se toma una muestra de cada Beta y gana la mayor: propone lo que ya funcionó, sin dejar de
//! explorar ajustes que todavía no conoce.

use std::ops::Add;

use chrono::{DateTime, Utc};
use rand::{Rng, RngExt};
use rand_distr::{Beta, Distribution};
use uuid::Uuid;

/// Pruebas anteriores de un ajuste: cuántas veces bajó la molestia y cuántas no. Son conteos
/// reales: los que se le muestran a la persona y los que usan las reglas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    pub helped: u32,
    pub not_helped: u32,
}

impl Evidence {
    pub fn trials(self) -> u32 {
        self.helped + self.not_helped
    }
}

impl Add for Evidence {
    type Output = Evidence;

    fn add(self, other: Evidence) -> Evidence {
        Evidence {
            helped: self.helped + other.helped,
            not_helped: self.not_helped + other.not_helped,
        }
    }
}

/// Pruebas propias en un nivel: los conteos y su peso por antigüedad.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    pub count: Evidence,
    /// Las mismas pruebas pesadas por lo recientes que son; solo las usa el posterior.
    pub helped_weight: f64,
    pub not_helped_weight: f64,
}

impl Tally {
    pub fn add(&mut self, helped: bool, weight: f64) {
        if helped {
            self.count.helped += 1;
            self.helped_weight += weight;
        } else {
            self.count.not_helped += 1;
            self.not_helped_weight += weight;
        }
    }
}

/// Evidencia de un ajuste en cuatro niveles que no se pisan, del más específico al más general.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArmEvidence {
    /// Esta persona, mismo estímulo, misma tarea y mismo lugar.
    pub exact: Tally,
    /// Esta persona, mismo estímulo y mismo lugar, en otra tarea.
    pub space: Tally,
    /// Esta persona, mismo estímulo, en otros lugares (o aquí antes de "Este lugar cambió").
    pub person: Tally,
    /// Otras personas, mismo estímulo (solo las de lugares del mismo tipo, si son bastantes).
    pub global: Evidence,
}

impl ArmEvidence {
    /// Todo lo que la persona vivió con este ajuste y este estímulo.
    pub fn personal(&self) -> Evidence {
        self.exact.count + self.space.count + self.person.count
    }

    /// Lo que vivió fuera de esta tarea en este lugar.
    pub fn elsewhere(&self) -> Evidence {
        self.space.count + self.person.count
    }

    /// Suma una prueba propia en su nivel más específico, pesada por su antigüedad.
    pub fn add_own(&mut self, here: &Here, trial: &OwnTrial, now: DateTime<Utc>) {
        let weight = recency(trial.completed_at, now);
        let tally = match level(here, trial) {
            Level::Exact => &mut self.exact,
            Level::Space => &mut self.space,
            Level::Person => &mut self.person,
        };
        tally.add(trial.helped, weight);
    }
}

/// El registro para el que se elige.
pub struct Here<'a> {
    pub task: &'a str,
    pub space_id: Option<Uuid>,
    /// "Este lugar cambió": lo de este lugar anterior a esta fecha ya no cuenta como de aquí.
    pub reset_at: Option<DateTime<Utc>>,
}

/// Una prueba terminada de esta persona con este estímulo y este ajuste.
pub struct OwnTrial<'a> {
    pub task: &'a str,
    pub space_id: Option<Uuid>,
    pub completed_at: DateTime<Utc>,
    pub helped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Exact,
    Space,
    Person,
}

/// Nivel más específico de una prueba propia. Sin lugar no hay nivel de lugar: "sin lugar" no es
/// un lugar, así que solo cuenta la misma tarea sin lugar.
pub fn level(here: &Here, trial: &OwnTrial) -> Level {
    let current = here.reset_at.is_none_or(|reset| trial.completed_at > reset);
    let same_place = trial.space_id == here.space_id && current;
    match (same_place, trial.task == here.task) {
        (true, true) => Level::Exact,
        (true, false) if here.space_id.is_some() => Level::Space,
        _ => Level::Person,
    }
}

/// Días para que una prueba propia valga la mitad.
const HALF_LIFE_DAYS: f64 = 60.0;

/// Peso de una prueba propia por su antigüedad: 1 hoy, 0.5 a los 60 días, 0.25 a los 120.
pub fn recency(completed_at: DateTime<Utc>, now: DateTime<Utc>) -> f64 {
    let age_days = (now - completed_at).num_seconds().max(0) as f64 / 86_400.0;
    0.5_f64.powf(age_days / HALF_LIFE_DAYS)
}

#[derive(Clone, Debug)]
pub struct Arm<'a> {
    pub id: &'a str,
    /// Probabilidad a priori de que ayude, del catálogo (0–1).
    pub prior: f64,
    pub evidence: ArmEvidence,
}

/// Fuerza del prior del catálogo, en "pruebas equivalentes".
const PRIOR_STRENGTH: f64 = 2.0;
/// Lo vivido aquí en otra tarea cuenta tres cuartos; en otros lugares, la mitad.
const SPACE_WEIGHT: f64 = 0.75;
const PERSON_WEIGHT: f64 = 0.5;
/// Cada prueba de otras personas cuenta poco, y en total nunca más que 3 pruebas propias.
const GLOBAL_WEIGHT: f64 = 0.1;
const GLOBAL_CAP: f64 = 3.0;

pub fn posterior(arm: &Arm) -> (f64, f64) {
    let e = &arm.evidence;
    let own = |weight: fn(&Tally) -> f64| {
        weight(&e.exact) + SPACE_WEIGHT * weight(&e.space) + PERSON_WEIGHT * weight(&e.person)
    };
    let global_trials = f64::from(e.global.trials());
    let global_scale = if global_trials > 0.0 {
        (GLOBAL_WEIGHT * global_trials).min(GLOBAL_CAP) / global_trials
    } else {
        0.0
    };
    let prior = arm.prior.clamp(0.01, 0.99);

    let alpha = PRIOR_STRENGTH * prior
        + own(|t| t.helped_weight)
        + global_scale * f64::from(e.global.helped);
    let beta = PRIOR_STRENGTH * (1.0 - prior)
        + own(|t| t.not_helped_weight)
        + global_scale * f64::from(e.global.not_helped);
    (alpha, beta)
}

/// Un ajuste que ya falló varias veces aquí, sin ningún éxito. Thompson sampling lo seguiría
/// proponiendo de vez en cuando, y a la persona eso le parece que la app no la escucha.
pub fn is_exhausted(arm: &Arm) -> bool {
    let exact = arm.evidence.exact.count;
    exact.helped == 0 && exact.not_helped >= 2
}

/// Probabilidad mínima de explorar al azar (HeartSteps acota igual): aunque un ajuste parezca
/// el mejor, de vez en cuando se prueba otro para no quedarse con una primera impresión.
pub const EXPLORATION: f64 = 0.1;

/// Elige un ajuste: casi siempre muestreando su Beta y, con probabilidad `explore`, al azar.
/// Descarta los agotados mientras haya alternativas. `None` solo si no hay candidatos.
pub fn choose<'a, R: Rng + ?Sized>(
    arms: &'a [Arm<'a>],
    explore: f64,
    rng: &mut R,
) -> Option<&'a Arm<'a>> {
    let all_exhausted = arms.iter().all(is_exhausted);
    let eligible: Vec<&Arm> = arms
        .iter()
        .filter(|arm| all_exhausted || !is_exhausted(arm))
        .collect();
    if eligible.is_empty() {
        return None;
    }
    if rng.random_bool(explore.clamp(0.0, 1.0)) {
        return Some(eligible[rng.random_range(0..eligible.len())]);
    }
    eligible
        .into_iter()
        .map(|arm| {
            let (alpha, beta) = posterior(arm);
            let draw = Beta::new(alpha, beta).map_or(arm.prior, |dist| dist.sample(rng));
            (arm, draw)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(arm, _)| arm)
}

/// Por qué se propone un ajuste, para mostrárselo a la persona. Siempre con conteos reales.
#[derive(Debug, PartialEq, Eq)]
pub enum Reason {
    /// Ya lo probó en esta tarea y este lugar.
    TriedHere(Evidence),
    /// Lo probó con este estímulo, pero en otra tarea o lugar.
    TriedElsewhere(Evidence),
    /// Nunca lo ha probado con este estímulo: es un primer intento.
    FirstTry,
    /// Nunca lo ha probado, aunque otro ajuste ya le ayudó: se prueba algo nuevo para aprender.
    Exploring,
}

pub fn reason(arm: &Arm, all: &[Arm]) -> Reason {
    let e = &arm.evidence;
    if e.exact.count.trials() > 0 {
        Reason::TriedHere(e.exact.count)
    } else if e.elsewhere().trials() > 0 {
        Reason::TriedElsewhere(e.elsewhere())
    } else if all.iter().any(|other| other.evidence.personal().helped > 0) {
        Reason::Exploring
    } else {
        Reason::FirstTry
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    /// Pruebas propias de hoy: cada una pesa 1.
    fn own((helped, not_helped): (u32, u32)) -> Tally {
        Tally {
            count: Evidence { helped, not_helped },
            helped_weight: f64::from(helped),
            not_helped_weight: f64::from(not_helped),
        }
    }

    /// `elsewhere` va en el nivel de persona (otros lugares).
    fn arm(
        id: &str,
        prior: f64,
        exact: (u32, u32),
        elsewhere: (u32, u32),
        global: (u32, u32),
    ) -> Arm<'_> {
        Arm {
            id,
            prior,
            evidence: ArmEvidence {
                exact: own(exact),
                space: Tally::default(),
                person: own(elsewhere),
                global: Evidence {
                    helped: global.0,
                    not_helped: global.1,
                },
            },
        }
    }

    fn wins(arms: &[Arm], id: &str, draws: usize) -> usize {
        let mut rng = StdRng::seed_from_u64(7);
        (0..draws)
            .filter(|_| choose(arms, 0.0, &mut rng).unwrap().id == id)
            .count()
    }

    fn place(n: u128) -> Option<Uuid> {
        Some(Uuid::from_u128(n))
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn trial(task: &str, space_id: Option<Uuid>, days_ago: i64, helped: bool) -> OwnTrial<'_> {
        OwnTrial {
            task,
            space_id,
            completed_at: now() - Duration::days(days_ago),
            helped,
        }
    }

    fn evidence_of(here: &Here, trials: &[OwnTrial]) -> ArmEvidence {
        let mut evidence = ArmEvidence::default();
        for t in trials {
            evidence.add_own(here, t, now());
        }
        evidence
    }

    #[test]
    fn no_candidates_means_no_choice() {
        let mut rng = StdRng::seed_from_u64(1);
        assert!(choose(&[], EXPLORATION, &mut rng).is_none());
    }

    #[test]
    fn prior_alone_gives_its_mean() {
        let (a, b) = posterior(&arm("x", 0.6, (0, 0), (0, 0), (0, 0)));
        assert!((a / (a + b) - 0.6).abs() < 1e-9);
        assert!((a + b - PRIOR_STRENGTH).abs() < 1e-9);
    }

    #[test]
    fn each_trial_counts_once_at_its_most_specific_level() {
        let here = Here {
            task: "reading",
            space_id: place(1),
            reset_at: None,
        };
        let e = evidence_of(
            &here,
            &[
                trial("reading", place(1), 0, true),
                trial("writing", place(1), 0, false),
                trial("reading", place(2), 0, true),
                trial("reading", None, 0, true),
            ],
        );
        let ev = |helped, not_helped| Evidence { helped, not_helped };
        assert_eq!(e.exact.count, ev(1, 0));
        assert_eq!(e.space.count, ev(0, 1));
        assert_eq!(e.person.count, ev(2, 0));
        assert_eq!(e.personal().trials(), 4);
        let (a, b) = posterior(&Arm {
            id: "x",
            prior: 0.5,
            evidence: e,
        });
        assert!((a - (1.0 + 1.0 + PERSON_WEIGHT * 2.0)).abs() < 1e-9);
        assert!((b - (1.0 + SPACE_WEIGHT)).abs() < 1e-9);
    }

    #[test]
    fn levels_weigh_one_three_quarters_half_and_a_capped_global() {
        let mut evidence = arm("x", 0.5, (1, 0), (1, 0), (900, 100)).evidence;
        evidence.space = own((1, 0));
        let (a, b) = posterior(&Arm {
            id: "x",
            prior: 0.5,
            evidence,
        });
        assert!((a + b - (PRIOR_STRENGTH + 1.0 + 0.75 + 0.5 + GLOBAL_CAP)).abs() < 1e-9);
        assert!((a - (1.0 + 2.25 + GLOBAL_CAP * 0.9)).abs() < 1e-9);
    }

    #[test]
    fn without_a_place_there_is_no_place_level() {
        let here = Here {
            task: "reading",
            space_id: None,
            reset_at: None,
        };
        assert_eq!(level(&here, &trial("reading", None, 0, true)), Level::Exact);
        assert_eq!(
            level(&here, &trial("writing", None, 0, true)),
            Level::Person
        );
        assert_eq!(
            level(&here, &trial("reading", place(1), 0, true)),
            Level::Person
        );
    }

    #[test]
    fn recent_trials_weigh_more_but_counts_stay_whole() {
        assert!((recency(now(), now()) - 1.0).abs() < 1e-9);
        assert!((recency(now() - Duration::days(60), now()) - 0.5).abs() < 1e-9);
        assert!((recency(now() - Duration::days(120), now()) - 0.25).abs() < 1e-9);
        // Un reloj adelantado no da pesos mayores que 1.
        assert!((recency(now() + Duration::days(3), now()) - 1.0).abs() < 1e-9);

        let here = Here {
            task: "reading",
            space_id: place(1),
            reset_at: None,
        };
        let e = evidence_of(&here, &[trial("reading", place(1), 60, true)]);
        assert!((e.exact.helped_weight - 0.5).abs() < 1e-9);
        let old = Arm {
            id: "x",
            prior: 0.5,
            evidence: e,
        };
        assert_eq!(
            reason(&old, &[]),
            Reason::TriedHere(Evidence {
                helped: 1,
                not_helped: 0
            })
        );
    }

    #[test]
    fn recent_failures_outweigh_old_successes() {
        let here = Here {
            task: "reading",
            space_id: place(1),
            reset_at: None,
        };
        let mut history: Vec<OwnTrial> = (0..3)
            .map(|_| trial("reading", place(1), 180, true))
            .collect();
        history.extend((0..2).map(|_| trial("reading", place(1), 1, false)));
        let arms = [
            Arm {
                id: "antes_servia",
                prior: 0.5,
                evidence: evidence_of(&here, &history),
            },
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        // Sin pesos por antigüedad irían 4 a 3 a favor del viejo y ganaría más de la mitad.
        let new = wins(&arms, "nuevo", 1000);
        assert!(new > 600, "el nuevo ganó solo {new} veces");
    }

    #[test]
    fn after_this_place_changed_old_trials_here_count_for_the_person_only() {
        let here = Here {
            task: "reading",
            space_id: place(1),
            reset_at: Some(now() - Duration::days(10)),
        };
        let at = |task, space, days| level(&here, &trial(task, space, days, false));
        assert_eq!(at("reading", place(1), 20), Level::Person);
        assert_eq!(at("writing", place(1), 20), Level::Person);
        assert_eq!(at("reading", place(1), 5), Level::Exact);
        assert_eq!(at("writing", place(1), 5), Level::Space);
        // El reinicio de un lugar no toca lo de otros lugares.
        assert_eq!(at("reading", place(2), 5), Level::Person);

        // Lo que falló dos veces antes del cambio vuelve a tener oportunidad aquí.
        let failed_before = Arm {
            id: "fallo",
            prior: 0.5,
            evidence: evidence_of(
                &here,
                &[
                    trial("reading", place(1), 30, false),
                    trial("reading", place(1), 20, false),
                ],
            ),
        };
        assert!(!is_exhausted(&failed_before));
        assert_eq!(
            reason(&failed_before, &[]),
            Reason::TriedElsewhere(Evidence {
                helped: 0,
                not_helped: 2
            })
        );
    }

    #[test]
    fn what_worked_here_usually_wins() {
        let arms = [
            arm("funciono", 0.5, (4, 0), (0, 0), (0, 0)),
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        assert!(wins(&arms, "funciono", 1000) > 800);
    }

    #[test]
    fn what_failed_here_twice_is_not_proposed_while_there_are_alternatives() {
        let arms = [
            arm("fallo", 0.6, (0, 2), (0, 0), (0, 0)),
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        assert_eq!(wins(&arms, "fallo", 1000), 0);
        // Si todo está agotado, se sigue proponiendo algo en vez de dejar a la persona sin opción.
        let only_failed = [arm("fallo", 0.6, (0, 3), (0, 0), (0, 0))];
        assert_eq!(wins(&only_failed, "fallo", 10), 10);
    }

    #[test]
    fn one_failure_lowers_the_odds_without_ruling_it_out() {
        let arms = [
            arm("fallo_una", 0.5, (0, 1), (0, 0), (0, 0)),
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        let failed = wins(&arms, "fallo_una", 1000);
        assert!(
            (150..450).contains(&failed),
            "propuso el que falló una vez {failed} veces"
        );
    }

    #[test]
    fn untried_options_keep_being_explored() {
        let arms = [
            arm("conocido", 0.5, (2, 1), (0, 0), (0, 0)),
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        let explored = wins(&arms, "nuevo", 1000);
        assert!(explored > 150, "solo exploró {explored} veces");
    }

    #[test]
    fn reasons_follow_the_most_specific_evidence() {
        let here = arm("x", 0.5, (2, 1), (3, 0), (0, 0));
        assert_eq!(
            reason(&here, &[]),
            Reason::TriedHere(Evidence {
                helped: 2,
                not_helped: 1
            })
        );
        // Otra tarea en este lugar y otros lugares se muestran juntos: "en otra tarea o lugar".
        let mut elsewhere = arm("x", 0.5, (0, 0), (1, 0), (0, 0));
        elsewhere.evidence.space = own((0, 1));
        assert_eq!(
            reason(&elsewhere, &[]),
            Reason::TriedElsewhere(Evidence {
                helped: 1,
                not_helped: 1
            })
        );
        let new = arm("x", 0.5, (0, 0), (0, 0), (9, 1));
        assert_eq!(reason(&new, &[]), Reason::FirstTry);
        let known_good = arm("y", 0.5, (2, 0), (0, 0), (0, 0));
        assert_eq!(reason(&new, &[known_good]), Reason::Exploring);
    }

    #[test]
    fn exploration_floor_reaches_every_option() {
        let arms = [
            arm("favorito", 0.5, (9, 0), (0, 0), (0, 0)),
            arm("otro", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        let mut rng = StdRng::seed_from_u64(3);
        let explored = (0..2000)
            .filter(|_| choose(&arms, 1.0, &mut rng).unwrap().id == "otro")
            .count();
        assert!(
            (800..1200).contains(&explored),
            "con exploración total, mitad y mitad: {explored}"
        );
    }
}
