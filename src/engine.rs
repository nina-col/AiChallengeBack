//! Motor personal: elige UN ajuste con Thompson sampling sobre la evidencia de cada persona.
//!
//! Cada ajuste tiene una probabilidad de "me ayudó" modelada como Beta(α, β). Se arma con:
//! - un prior del catálogo (qué tan seguido suele ayudar ese ajuste para ese estímulo),
//! - lo que le pasó a esta persona en el mismo contexto (estímulo × tarea × espacio), con peso 1,
//! - lo que le pasó a esta persona con ese estímulo en otros contextos, con peso medio,
//! - lo que les pasó a todas las personas, con peso bajo y un tope, para arrancar sin datos.
//!
//! Se toma una muestra de cada Beta y gana la mayor: propone lo que ya funcionó, sin dejar de
//! explorar ajustes que todavía no conoce.

use rand::{Rng, RngExt};
use rand_distr::{Beta, Distribution};

/// Pruebas anteriores de un ajuste: cuántas veces bajó el esfuerzo y cuántas no.
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

/// Evidencia de un ajuste en tres niveles, del más específico al más general.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArmEvidence {
    /// Esta persona, mismo estímulo, misma tarea y mismo espacio.
    pub exact: Evidence,
    /// Esta persona, mismo estímulo, en cualquier tarea o espacio (incluye `exact`).
    pub person: Evidence,
    /// Todas las personas, mismo estímulo.
    pub global: Evidence,
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
/// Lo vivido por la persona en otros contextos cuenta la mitad que lo vivido aquí.
const PERSON_WEIGHT: f64 = 0.5;
/// Cada prueba de otras personas cuenta poco, y en total nunca más que 3 pruebas propias.
const GLOBAL_WEIGHT: f64 = 0.1;
const GLOBAL_CAP: f64 = 3.0;

pub fn posterior(arm: &Arm) -> (f64, f64) {
    let e = arm.evidence;
    // `person` incluye las pruebas de `exact`; se restan para no contarlas dos veces.
    let elsewhere_helped = f64::from(e.person.helped.saturating_sub(e.exact.helped));
    let elsewhere_not = f64::from(e.person.not_helped.saturating_sub(e.exact.not_helped));
    let global_trials = f64::from(e.global.trials());
    let global_scale = if global_trials > 0.0 {
        (GLOBAL_WEIGHT * global_trials).min(GLOBAL_CAP) / global_trials
    } else {
        0.0
    };
    let prior = arm.prior.clamp(0.01, 0.99);

    let alpha = PRIOR_STRENGTH * prior
        + f64::from(e.exact.helped)
        + PERSON_WEIGHT * elsewhere_helped
        + global_scale * f64::from(e.global.helped);
    let beta = PRIOR_STRENGTH * (1.0 - prior)
        + f64::from(e.exact.not_helped)
        + PERSON_WEIGHT * elsewhere_not
        + global_scale * f64::from(e.global.not_helped);
    (alpha, beta)
}

/// Un ajuste que ya falló varias veces aquí, sin ningún éxito. Thompson sampling lo seguiría
/// proponiendo de vez en cuando, y a la persona eso le parece que la app no la escucha.
pub fn is_exhausted(arm: &Arm) -> bool {
    arm.evidence.exact.helped == 0 && arm.evidence.exact.not_helped >= 2
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
    let eligible: Vec<&Arm> = arms.iter().filter(|arm| all_exhausted || !is_exhausted(arm)).collect();
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

/// Por qué se propone un ajuste, para mostrárselo a la persona.
#[derive(Debug, PartialEq, Eq)]
pub enum Reason {
    /// Ya lo probó en esta tarea y este espacio.
    TriedHere(Evidence),
    /// Lo probó con este estímulo, pero en otra tarea o espacio.
    TriedElsewhere(Evidence),
    /// Nunca lo ha probado con este estímulo: es un primer intento.
    FirstTry,
    /// Nunca lo ha probado, aunque otro ajuste ya le ayudó: se prueba algo nuevo para aprender.
    Exploring,
}

pub fn reason(arm: &Arm, all: &[Arm]) -> Reason {
    if arm.evidence.exact.trials() > 0 {
        Reason::TriedHere(arm.evidence.exact)
    } else if arm.evidence.person.trials() > 0 {
        Reason::TriedElsewhere(arm.evidence.person)
    } else if all.iter().any(|other| other.evidence.person.helped > 0) {
        Reason::Exploring
    } else {
        Reason::FirstTry
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    fn arm(id: &str, prior: f64, exact: (u32, u32), person: (u32, u32), global: (u32, u32)) -> Arm<'_> {
        let ev = |(helped, not_helped)| Evidence { helped, not_helped };
        Arm {
            id,
            prior,
            evidence: ArmEvidence { exact: ev(exact), person: ev(person), global: ev(global) },
        }
    }

    fn wins(arms: &[Arm], id: &str, draws: usize) -> usize {
        let mut rng = StdRng::seed_from_u64(7);
        (0..draws).filter(|_| choose(arms, 0.0, &mut rng).unwrap().id == id).count()
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
    fn exact_evidence_is_not_double_counted() {
        let only_exact = posterior(&arm("x", 0.5, (3, 1), (3, 1), (0, 0)));
        let exact_alone = posterior(&arm("x", 0.5, (3, 1), (0, 0), (0, 0)));
        assert_eq!(only_exact, exact_alone);
    }

    #[test]
    fn global_evidence_is_capped() {
        let (a, b) = posterior(&arm("x", 0.5, (0, 0), (0, 0), (900, 100)));
        assert!((a + b - (PRIOR_STRENGTH + GLOBAL_CAP)).abs() < 1e-9);
    }

    #[test]
    fn what_worked_here_usually_wins() {
        let arms = [
            arm("funciono", 0.5, (4, 0), (4, 0), (0, 0)),
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        assert!(wins(&arms, "funciono", 1000) > 800);
    }

    #[test]
    fn what_failed_here_twice_is_not_proposed_while_there_are_alternatives() {
        let arms = [
            arm("fallo", 0.6, (0, 2), (0, 2), (0, 0)),
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        assert_eq!(wins(&arms, "fallo", 1000), 0);
        // Si todo está agotado, se sigue proponiendo algo en vez de dejar a la persona sin opción.
        let only_failed = [arm("fallo", 0.6, (0, 3), (0, 3), (0, 0))];
        assert_eq!(wins(&only_failed, "fallo", 10), 10);
    }

    #[test]
    fn one_failure_lowers_the_odds_without_ruling_it_out() {
        let arms = [
            arm("fallo_una", 0.5, (0, 1), (0, 1), (0, 0)),
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        let failed = wins(&arms, "fallo_una", 1000);
        assert!((150..450).contains(&failed), "propuso el que falló una vez {failed} veces");
    }

    #[test]
    fn untried_options_keep_being_explored() {
        let arms = [
            arm("conocido", 0.5, (2, 1), (2, 1), (0, 0)),
            arm("nuevo", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        let explored = wins(&arms, "nuevo", 1000);
        assert!(explored > 150, "solo exploró {explored} veces");
    }

    #[test]
    fn reasons_follow_the_most_specific_evidence() {
        let here = arm("x", 0.5, (2, 1), (5, 1), (0, 0));
        assert_eq!(reason(&here, &[]), Reason::TriedHere(Evidence { helped: 2, not_helped: 1 }));
        let elsewhere = arm("x", 0.5, (0, 0), (1, 1), (0, 0));
        assert_eq!(reason(&elsewhere, &[]), Reason::TriedElsewhere(Evidence { helped: 1, not_helped: 1 }));
        let new = arm("x", 0.5, (0, 0), (0, 0), (9, 1));
        assert_eq!(reason(&new, &[]), Reason::FirstTry);
        let known_good = arm("y", 0.5, (2, 0), (2, 0), (0, 0));
        assert_eq!(reason(&new, &[known_good]), Reason::Exploring);
    }

    #[test]
    fn exploration_floor_reaches_every_option() {
        let arms = [
            arm("favorito", 0.5, (9, 0), (9, 0), (0, 0)),
            arm("otro", 0.5, (0, 0), (0, 0), (0, 0)),
        ];
        let mut rng = StdRng::seed_from_u64(3);
        let explored = (0..2000).filter(|_| choose(&arms, 1.0, &mut rng).unwrap().id == "otro").count();
        assert!((800..1200).contains(&explored), "con exploración total, mitad y mitad: {explored}");
    }
}
