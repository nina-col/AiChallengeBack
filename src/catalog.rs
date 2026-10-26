//! Catálogo versionado de tareas, sensaciones, estímulos y ajustes (catalog/catalog.json).
//! Va incrustado en el binario: cambiarlo es un commit revisable, no un dato suelto en la base.

use std::{collections::HashSet, sync::LazyLock};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    Es,
    En,
}

impl Locale {
    pub fn parse(value: &str) -> Self {
        if value == "en" { Locale::En } else { Locale::Es }
    }
}

#[derive(Deserialize)]
pub struct Text {
    es: String,
    en: String,
}

impl Text {
    pub fn get(&self, locale: Locale) -> &str {
        match locale {
            Locale::Es => &self.es,
            Locale::En => &self.en,
        }
    }
}

#[derive(Deserialize)]
pub struct Labeled {
    pub id: String,
    pub label: Text,
}

#[derive(Deserialize)]
pub struct Domain {
    pub id: String,
    pub question: Text,
    pub stimuli: Vec<String>,
}

#[derive(Deserialize)]
pub struct Stimulus {
    pub id: String,
    pub domain: String,
    pub label: Text,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum AdjustmentKind {
    /// La persona lo puede hacer ahora.
    #[serde(rename = "self")]
    SelfServe,
    /// Hay que pedirlo a la empresa.
    #[serde(rename = "ask")]
    Ask,
    /// "Esta vez no cambies nada": la comparación que separa un ajuste del paso del tiempo.
    #[serde(rename = "baseline")]
    Baseline,
}

/// Id del registro de comparación sin ajuste.
pub const BASELINE_ID: &str = "no_change";

#[derive(Deserialize)]
pub struct Baseline {
    pub title: Text,
    pub steps: Text,
    pub why: Text,
}

#[derive(Deserialize)]
pub struct Adjustment {
    pub id: String,
    pub stimuli: Vec<String>,
    pub kind: AdjustmentKind,
    /// Probabilidad a priori de que ayude; se corrige con la experiencia de cada persona.
    pub prior: f64,
    #[serde(default)]
    pub not_for_tasks: Vec<String>,
    pub title: Text,
    pub steps: Text,
    pub why: Text,
}

impl Adjustment {
    pub fn fits(&self, stimulus: &str, task: &str) -> bool {
        self.stimuli.iter().any(|s| s == stimulus) && !self.not_for_tasks.iter().any(|t| t == task)
    }
}

#[derive(Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub tasks: Vec<Labeled>,
    pub feelings: Vec<Labeled>,
    pub domains: Vec<Domain>,
    pub stimuli: Vec<Stimulus>,
    pub adjustments: Vec<Adjustment>,
    pub baseline: Baseline,
}

pub static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../catalog/catalog.json")).expect("catalog.json inválido")
});

impl Catalog {
    pub fn task(&self, id: &str) -> Option<&Labeled> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn feeling(&self, id: &str) -> Option<&Labeled> {
        self.feelings.iter().find(|f| f.id == id)
    }

    pub fn stimulus(&self, id: &str) -> Option<&Stimulus> {
        self.stimuli.iter().find(|s| s.id == id)
    }

    pub fn adjustment(&self, id: &str) -> Option<&Adjustment> {
        self.adjustments.iter().find(|a| a.id == id)
    }

    /// Lo que se le muestra a la persona para un id guardado, incluida la comparación sin ajuste.
    pub fn adjustment_view(&self, id: &str, locale: Locale) -> Option<LocalizedAdjustment<'_>> {
        if id == BASELINE_ID {
            return Some(LocalizedAdjustment {
                id: BASELINE_ID,
                kind: AdjustmentKind::Baseline,
                stimuli: &[],
                title: self.baseline.title.get(locale),
                steps: self.baseline.steps.get(locale),
                why: self.baseline.why.get(locale),
            });
        }
        self.adjustment(id).map(|a| a.localized(locale))
    }

    pub fn localized(&self, locale: Locale) -> LocalizedCatalog<'_> {
        fn label(item: &Labeled, locale: Locale) -> LocalizedLabel<'_> {
            LocalizedLabel { id: &item.id, label: item.label.get(locale) }
        }
        LocalizedCatalog {
            version: self.version,
            tasks: self.tasks.iter().map(|t| label(t, locale)).collect(),
            feelings: self.feelings.iter().map(|f| label(f, locale)).collect(),
            domains: self
                .domains
                .iter()
                .map(|d| LocalizedDomain {
                    id: &d.id,
                    question: d.question.get(locale),
                    stimuli: self
                        .stimuli
                        .iter()
                        .filter(|s| s.domain == d.id)
                        .map(|s| LocalizedLabel { id: &s.id, label: s.label.get(locale) })
                        .collect(),
                })
                .collect(),
            adjustments: self.adjustments.iter().map(|a| a.localized(locale)).collect(),
        }
    }
}

impl Adjustment {
    pub fn localized(&self, locale: Locale) -> LocalizedAdjustment<'_> {
        LocalizedAdjustment {
            id: &self.id,
            kind: self.kind,
            stimuli: &self.stimuli,
            title: self.title.get(locale),
            steps: self.steps.get(locale),
            why: self.why.get(locale),
        }
    }
}

#[derive(Serialize)]
pub struct LocalizedLabel<'a> {
    id: &'a str,
    label: &'a str,
}

#[derive(Serialize)]
pub struct LocalizedDomain<'a> {
    id: &'a str,
    question: &'a str,
    stimuli: Vec<LocalizedLabel<'a>>,
}

#[derive(Serialize)]
pub struct LocalizedAdjustment<'a> {
    pub id: &'a str,
    pub kind: AdjustmentKind,
    pub stimuli: &'a [String],
    pub title: &'a str,
    pub steps: &'a str,
    pub why: &'a str,
}

#[derive(Serialize)]
pub struct LocalizedCatalog<'a> {
    version: u32,
    tasks: Vec<LocalizedLabel<'a>>,
    feelings: Vec<LocalizedLabel<'a>>,
    domains: Vec<LocalizedDomain<'a>>,
    adjustments: Vec<LocalizedAdjustment<'a>>,
}

/// Comprueba la coherencia del catálogo; las pruebas fallan si alguien lo rompe.
pub fn problems(catalog: &Catalog) -> Vec<String> {
    let mut problems = Vec::new();
    let mut unique = |kind: &str, ids: Vec<&str>| {
        let mut seen = HashSet::new();
        for id in ids {
            if !seen.insert(id) {
                problems.push(format!("{kind} repetido: {id}"));
            }
        }
    };
    unique("tarea", catalog.tasks.iter().map(|t| t.id.as_str()).collect());
    unique("sensación", catalog.feelings.iter().map(|f| f.id.as_str()).collect());
    unique("dominio", catalog.domains.iter().map(|d| d.id.as_str()).collect());
    unique("estímulo", catalog.stimuli.iter().map(|s| s.id.as_str()).collect());
    unique("ajuste", catalog.adjustments.iter().map(|a| a.id.as_str()).collect());

    let mut texts: Vec<(&str, &Text)> = Vec::new();
    texts.extend(catalog.tasks.iter().map(|t| (t.id.as_str(), &t.label)));
    texts.extend(catalog.feelings.iter().map(|f| (f.id.as_str(), &f.label)));
    texts.extend(catalog.domains.iter().map(|d| (d.id.as_str(), &d.question)));
    texts.extend(catalog.stimuli.iter().map(|s| (s.id.as_str(), &s.label)));
    for a in &catalog.adjustments {
        texts.extend([(a.id.as_str(), &a.title), (a.id.as_str(), &a.steps), (a.id.as_str(), &a.why)]);
        if a.id == BASELINE_ID || a.kind == AdjustmentKind::Baseline {
            problems.push(format!("{}: la comparación sin ajuste va en \"baseline\", no en la lista", a.id));
        }
    }
    let b = &catalog.baseline;
    texts.extend([(BASELINE_ID, &b.title), (BASELINE_ID, &b.steps), (BASELINE_ID, &b.why)]);
    for (id, text) in texts {
        if text.es.trim().is_empty() || text.en.trim().is_empty() {
            problems.push(format!("texto vacío en {id}"));
        }
    }

    for d in &catalog.domains {
        for s in &d.stimuli {
            match catalog.stimulus(s) {
                None => problems.push(format!("el dominio {} nombra un estímulo inexistente: {s}", d.id)),
                Some(st) if st.domain != d.id => {
                    problems.push(format!("{s} está en el dominio {} pero dice {}", d.id, st.domain))
                }
                Some(_) => {}
            }
        }
    }
    for s in &catalog.stimuli {
        if !catalog.domains.iter().any(|d| d.stimuli.contains(&s.id)) {
            problems.push(format!("el estímulo {} no aparece en ningún dominio", s.id));
        }
        if !catalog
            .adjustments
            .iter()
            .any(|a| a.kind == AdjustmentKind::SelfServe && a.stimuli.contains(&s.id))
        {
            problems.push(format!("el estímulo {} no tiene ningún ajuste que la persona pueda hacer", s.id));
        }
    }
    for a in &catalog.adjustments {
        if !(0.05..=0.95).contains(&a.prior) {
            problems.push(format!("prior fuera de rango en {}", a.id));
        }
        if a.stimuli.is_empty() {
            problems.push(format!("{} no atiende ningún estímulo", a.id));
        }
        for s in &a.stimuli {
            if catalog.stimulus(s).is_none() {
                problems.push(format!("{} nombra un estímulo inexistente: {s}", a.id));
            }
        }
        for t in &a.not_for_tasks {
            if catalog.task(t).is_none() {
                problems.push(format!("{} excluye una tarea inexistente: {t}", a.id));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        let problems = problems(&CATALOG);
        assert!(problems.is_empty(), "problemas en catalog.json:\n{}", problems.join("\n"));
    }

    #[test]
    fn every_task_has_something_to_try_for_every_stimulus() {
        for task in &CATALOG.tasks {
            for stimulus in &CATALOG.stimuli {
                let any_self = CATALOG
                    .adjustments
                    .iter()
                    .any(|a| a.kind == AdjustmentKind::SelfServe && a.fits(&stimulus.id, &task.id));
                assert!(any_self, "sin ajuste propio para {} en {}", stimulus.id, task.id);
            }
        }
    }
}
