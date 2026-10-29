//! Cuarto isométrico de cada lugar: el acomodo (layout v1) y sus reglas.
//!
//! Espeja `apps/mobile/src/components/room/types.ts`. La app valida antes de guardar y el servidor
//! vuelve a validar con las mismas reglas y los mismos códigos de problema que `validateLayout`
//! (`layout.ts` y `occupancy.ts`), así los dos lados corren los mismos casos de prueba
//! (`catalog/room-fixtures`). El catálogo de objetos (`catalog/room.json`) lo genera
//! `docs/brand/draw_room.py`; si todavía no existe, solo se revisa la forma del acomodo.
//! Diseño: docs/research/research_notes/Escritorio isometrico/tecnica.md (§6–§8).

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::LazyLock,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::{CATALOG, Locale, Text};

pub const LAYOUT_VERSION: i64 = 1;
pub const MAX_ITEMS: usize = 60;
pub const MAX_BYTES: usize = 16384;
pub const MAX_SIZE: i64 = 8;
/// Filas de 16 px de arte en cada pared (48 px de alto).
pub const WALL_ROWS: i64 = 3;
/// Problema de un acomodo guardado por una app más nueva: la API pide actualizar la app.
pub const UNSUPPORTED: &str = "v: unsupported";

// ── Layout v1 (se guarda normalizado en `rooms.layout`) ──────────────────────────────────────

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Wall {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Size {
    pub w: i64,
    pub d: i64,
}

impl Size {
    /// Largo de una pared en celdas: la izquierda (plano x = 0) corre a lo largo de `y`, la
    /// derecha (plano y = 0) a lo largo de `x`.
    fn wall_length(self, wall: Wall) -> i64 {
        match wall {
            Wall::Left => self.d,
            Wall::Right => self.w,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Window {
    pub wall: Wall,
    pub u: i64,
}

#[derive(Debug, Serialize)]
pub struct Shell {
    pub floor: String,
    pub wall: String,
    /// La ventana ocupa una celda de una pared, en todas sus filas.
    pub window: Window,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "on", rename_all = "lowercase", deny_unknown_fields)]
pub enum ItemPlace {
    Floor { x: i64, y: i64, rot: i64 },
    Surface { host: String, slot: i64 },
    Wall { wall: Wall, u: i64, row: i64 },
}

impl ItemPlace {
    fn kind(&self) -> PlaceKind {
        match self {
            ItemPlace::Floor { .. } => PlaceKind::Floor,
            ItemPlace::Surface { .. } => PlaceKind::Surface,
            ItemPlace::Wall { .. } => PlaceKind::Wall,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RoomItem {
    /// 8 caracteres `[a-z0-9]`, únicos en el cuarto.
    pub uid: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub variant: String,
    pub place: ItemPlace,
}

/// Lo que se guarda, ya validado. Se vuelve a serializar desde aquí: los campos que no son del
/// formato se descartan.
#[derive(Debug, Serialize)]
pub struct RoomLayout {
    pub v: i64,
    pub size: Size,
    pub shell: Shell,
    pub items: Vec<RoomItem>,
    /// Ajustes fijados en el corcho "Lo que me sirve aquí". Solo los fija la persona.
    pub pinned: Vec<String>,
}

// ── Catálogo de objetos (`catalog/room.json`) ────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlaceKind {
    Floor,
    Surface,
    Wall,
}

#[derive(Deserialize)]
pub struct ShellOptions {
    pub floors: Vec<String>,
    pub walls: Vec<String>,
}

#[derive(Deserialize)]
pub struct Surface {
    /// Celdas relativas con `rot` 0 donde puede ir un objeto de superficie.
    pub slots: Vec<[i64; 2]>,
}

/// Lo que la API necesita de cada tipo; el resto del manifiesto (alto, animaciones) se ignora.
#[derive(Deserialize)]
pub struct TypeSpec {
    pub place: Vec<PlaceKind>,
    /// Piso: `[ancho en x, fondo en y]` con `rot` 0. Pared: `[largo, filas]`.
    pub footprint: [i64; 2],
    pub facings: Vec<i64>,
    #[serde(default)]
    pub surface: Option<Surface>,
    /// Alfombra: su propia capa; puede ir debajo de un mueble, no de otra alfombra.
    #[serde(default)]
    pub flat: bool,
    #[serde(default)]
    pub base: bool,
    #[serde(default)]
    pub decor: bool,
    /// Ajustes cuya prueba terminada da este objeto.
    #[serde(default)]
    pub adjustments: Vec<String>,
    /// No se coloca: cambia otra pieza (`window` o `monitor`).
    #[serde(default)]
    pub modifies: Option<String>,
    pub variants: Vec<String>,
    pub label: Text,
}

impl TypeSpec {
    /// Huella en el piso con el giro aplicado: los giros impares cambian ancho por fondo.
    fn floor_footprint(&self, rot: i64) -> (i64, i64) {
        let [w, d] = self.footprint;
        if rot % 2 == 0 { (w, d) } else { (d, w) }
    }

    /// La base y la decoración se tienen desde el primer día; lo demás, al probar uno de sus ajustes.
    fn earned(&self, owned: &[String]) -> bool {
        self.base || self.decor || self.adjustments.iter().any(|a| owned.contains(a))
    }
}

#[derive(Deserialize)]
pub struct RoomCatalog {
    pub version: u32,
    pub size: Size,
    pub shell: ShellOptions,
    pub types: BTreeMap<String, TypeSpec>,
}

/// `catalog/room.json` incrustado al compilar (ver build.rs); vacío si todavía no existe.
const ROOM_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/room.json"));

pub static ROOM_CATALOG: LazyLock<Option<RoomCatalog>> = LazyLock::new(|| {
    (!ROOM_JSON.trim().is_empty())
        .then(|| serde_json::from_str(ROOM_JSON).expect("room.json inválido"))
});

/// Coherencia del catálogo de objetos; la API no arranca si falla.
pub fn catalog_problems(catalog: &RoomCatalog) -> Vec<String> {
    let mut problems = Vec::new();
    if catalog.version != 1 {
        problems.push(format!("versión desconocida: {}", catalog.version));
    }
    let size = catalog.size;
    if !(1..=MAX_SIZE).contains(&size.w) || !(1..=MAX_SIZE).contains(&size.d) {
        problems.push(format!("size debe estar entre 1 y {MAX_SIZE}"));
    }
    if catalog.shell.floors.is_empty() || catalog.shell.walls.is_empty() {
        problems.push("faltan pisos o paredes".into());
    }
    for (id, spec) in &catalog.types {
        let [w, d] = spec.footprint;
        if w < 1 || d < 1 {
            problems.push(format!("{id}: huella vacía"));
        }
        if spec.place.contains(&PlaceKind::Wall) && d > WALL_ROWS {
            problems.push(format!("{id}: más de {WALL_ROWS} filas de pared"));
        }
        if spec.facings.is_empty() || spec.facings.iter().any(|f| !(0..=3).contains(f)) {
            problems.push(format!("{id}: facings deben ir de 0 a 3"));
        }
        if spec.variants.is_empty() {
            problems.push(format!("{id}: sin variantes"));
        }
        if spec.label.get(Locale::Es).trim().is_empty()
            || spec.label.get(Locale::En).trim().is_empty()
        {
            problems.push(format!("{id}: etiqueta vacía"));
        }
        match spec.modifies.as_deref() {
            None if spec.place.is_empty() => problems.push(format!("{id}: no dice dónde va")),
            None => {}
            Some("window" | "monitor") if spec.place.is_empty() => {}
            Some("window" | "monitor") => {
                problems.push(format!("{id}: un modificador no se coloca"));
            }
            Some(other) => problems.push(format!("{id}: modifica algo desconocido: {other}")),
        }
        for a in &spec.adjustments {
            if CATALOG.adjustment(a).is_none() {
                problems.push(format!("{id}: ajuste inexistente: {a}"));
            }
        }
        if !spec.base && !spec.decor && spec.adjustments.is_empty() {
            problems.push(format!(
                "{id}: no es base ni decoración y ningún ajuste lo da"
            ));
        }
        if let Some(surface) = &spec.surface {
            if surface.slots.is_empty() {
                problems.push(format!("{id}: superficie sin slots"));
            }
            if surface
                .slots
                .iter()
                .any(|&[x, y]| !(0..w).contains(&x) || !(0..d).contains(&y))
            {
                problems.push(format!("{id}: slot fuera de la huella"));
            }
            if !spec.place.contains(&PlaceKind::Floor) {
                problems.push(format!("{id}: una superficie va en el piso"));
            }
        }
    }
    problems
}

// ── Validación ───────────────────────────────────────────────────────────────────────────────
//
// Problemas `<ruta>: <código>`, primero los generales y luego por objeto en el orden de la lista:
// - `layout: invalid` · `v: invalid` · `v: unsupported` · `size: invalid` · `shell: invalid` (se detiene ahí)
// - `shell.floor: unknown-variant` · `shell.wall: unknown-variant` · `shell.window: outside`
// - `items: invalid` (se detiene) · `items: too-many` · `pinned: invalid` · `layout: too-large` (se detiene)
// - `items[i]: invalid` · `items[i].uid: invalid|duplicate` · `items[i].variant: unknown-variant`
//   · `items[i].type: not-owned` · `items[i].place: invalid|outside|occupied|facing|no-slot|window|not-placeable`
// Los tipos desconocidos se conservan y no se revisan (salvo uid y forma del lugar), ni lo que
// tienen encima. Lo que no cabe no ocupa nada.

/// Ids de ajustes en el corcho.
fn valid_pinned(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn valid_uid(uid: &str) -> bool {
    uid.len() == 8
        && uid
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Entero de JSON, también si viene como `2.0` (para JS es el mismo número).
fn int(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
            .map(|f| f as i64)
    })
}

fn parse_size(value: &Value) -> Option<Size> {
    let dim = |key| int(value.get(key)?).filter(|n| (1..=MAX_SIZE).contains(n));
    Some(Size {
        w: dim("w")?,
        d: dim("d")?,
    })
}

fn parse_wall(value: &Value) -> Option<Wall> {
    match value.as_str()? {
        "left" => Some(Wall::Left),
        "right" => Some(Wall::Right),
        _ => None,
    }
}

fn parse_shell(value: &Value) -> Option<Shell> {
    let window = value.get("window")?;
    Some(Shell {
        floor: value.get("floor")?.as_str()?.to_owned(),
        wall: value.get("wall")?.as_str()?.to_owned(),
        window: Window {
            wall: parse_wall(window.get("wall")?)?,
            u: int(window.get("u")?)?,
        },
    })
}

fn parse_pinned(value: &Value) -> Option<Vec<String>> {
    let ids = value.as_array()?;
    let mut seen = HashSet::new();
    (ids.len() <= MAX_ITEMS).then_some(())?;
    ids.iter()
        .map(|id| {
            let id = id.as_str().filter(|id| valid_pinned(id))?;
            seen.insert(id).then(|| id.to_owned())
        })
        .collect()
}

/// El lugar con `serde` (sin campos de más) y con `rot` de 0 a 3.
fn parse_place(value: &Value) -> Option<ItemPlace> {
    let place: ItemPlace = serde_json::from_value(value.clone()).ok()?;
    match place {
        ItemPlace::Floor { rot, .. } if !(0..=3).contains(&rot) => None,
        place => Some(place),
    }
}

fn spec_of<'c>(catalog: Option<&'c RoomCatalog>, kind: &str) -> Option<&'c TypeSpec> {
    catalog?.types.get(kind)
}

/// Revisa un acomodo tal como llega (JSON sin tipar) y lo devuelve normalizado, o todos sus
/// problemas. `owned`: ajustes que la persona ya probó (lo mismo que `GET /v1/progress`).
/// Sin catálogo de objetos solo se revisa la forma.
pub fn validate(
    raw: &Value,
    catalog: Option<&RoomCatalog>,
    owned: &[String],
) -> Result<RoomLayout, Vec<String>> {
    let fail = |code: &str| Err(vec![code.to_owned()]);
    if !raw.is_object() {
        return fail("layout: invalid");
    }
    match raw.get("v").and_then(Value::as_f64) {
        Some(v) if v == LAYOUT_VERSION as f64 => {}
        Some(v) if v > LAYOUT_VERSION as f64 => return fail(UNSUPPORTED),
        _ => return fail("v: invalid"),
    }
    // El tamaño es el del catálogo: el arte de las paredes y el piso se hizo para él.
    let Some(size) = raw
        .get("size")
        .and_then(parse_size)
        .filter(|size| catalog.is_none_or(|c| c.size == *size))
    else {
        return fail("size: invalid");
    };
    let Some(shell) = raw.get("shell").and_then(parse_shell) else {
        return fail("shell: invalid");
    };

    let mut problems: Vec<String> = Vec::new();
    if let Some(c) = catalog {
        if !c.shell.floors.contains(&shell.floor) {
            problems.push("shell.floor: unknown-variant".into());
        }
        if !c.shell.walls.contains(&shell.wall) {
            problems.push("shell.wall: unknown-variant".into());
        }
    }
    let window = &shell.window;
    let window_ok = (0..size.wall_length(window.wall)).contains(&window.u);
    if !window_ok {
        problems.push("shell.window: outside".into());
    }
    let Some(raw_items) = raw.get("items").and_then(Value::as_array) else {
        problems.push("items: invalid".into());
        return Err(problems);
    };
    if raw_items.len() > MAX_ITEMS {
        problems.push("items: too-many".into());
    }
    let pinned = raw.get("pinned").and_then(parse_pinned);
    if pinned.is_none() {
        problems.push("pinned: invalid".into());
    }
    // Un acomodo enorme no se revisa objeto por objeto.
    if serde_json::to_string(raw).map_or(true, |json| json.len() > MAX_BYTES) {
        problems.push("layout: too-large".into());
        return Err(problems);
    }

    let mut per_item: Vec<Vec<String>> = vec![Vec::new(); raw_items.len()];
    let mut items: Vec<RoomItem> = Vec::new();
    let mut index: Vec<usize> = Vec::new();
    let mut seen = HashSet::new();
    for (i, it) in raw_items.iter().enumerate() {
        let out = &mut per_item[i];
        let text = |key| it.get(key).and_then(Value::as_str);
        let (Some(uid), Some(kind), Some(variant)) = (text("uid"), text("type"), text("variant"))
        else {
            out.push(format!("items[{i}]: invalid"));
            continue;
        };
        if !valid_uid(uid) {
            out.push(format!("items[{i}].uid: invalid"));
        } else if seen.contains(uid) {
            out.push(format!("items[{i}].uid: duplicate"));
        }
        seen.insert(uid);
        if let Some(spec) = spec_of(catalog, kind) {
            if !spec.variants.iter().any(|v| v == variant) {
                out.push(format!("items[{i}].variant: unknown-variant"));
            }
            if !spec.earned(owned) {
                out.push(format!("items[{i}].type: not-owned"));
            }
        }
        let Some(place) = it.get("place").and_then(parse_place) else {
            out.push(format!("items[{i}].place: invalid"));
            continue;
        };
        items.push(RoomItem {
            uid: uid.to_owned(),
            kind: kind.to_owned(),
            variant: variant.to_owned(),
            place,
        });
        index.push(i);
    }

    let window = window_ok.then_some(window);
    for (k, reason) in settle(&items, size, window, catalog)
        .into_iter()
        .enumerate()
    {
        if let Some(reason) = reason {
            let i = index[k];
            per_item[i].push(format!("items[{i}].place: {reason}"));
        }
    }
    problems.extend(per_item.into_iter().flatten());
    match pinned {
        Some(pinned) if problems.is_empty() => Ok(RoomLayout {
            v: LAYOUT_VERSION,
            size,
            shell,
            items,
            pinned,
        }),
        _ => Err(problems),
    }
}

/// Quién ocupa una celda.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    Window,
    Item(usize),
}

/// Rejillas de ocupación (tecnica.md §6.3). Piso y alfombras: celda `y * w + x`. Paredes:
/// `fila * largo + u`, con la ventana en todas las filas de su celda.
struct Occupancy<'a> {
    size: Size,
    floor: Vec<Cell>,
    flat: Vec<Cell>,
    left: Vec<Cell>,
    right: Vec<Cell>,
    /// `(anfitrión, slot)` → objeto que está encima.
    slots: HashMap<(&'a str, i64), usize>,
    /// uid → anfitriones colocados en el piso; solo ellos aceptan objetos encima.
    hosts: HashMap<&'a str, usize>,
}

/// Índice de una celda; quien llama ya comprobó que está dentro de la rejilla.
fn cell(row: i64, len: i64, col: i64) -> usize {
    usize::try_from(row * len + col).expect("celda dentro de la rejilla")
}

impl<'a> Occupancy<'a> {
    fn new(size: Size, window: Option<&Window>) -> Self {
        let cells = |n: i64| vec![Cell::Empty; usize::try_from(n).unwrap_or(0)];
        let mut occ = Occupancy {
            size,
            floor: cells(size.w * size.d),
            flat: cells(size.w * size.d),
            left: cells(size.d * WALL_ROWS),
            right: cells(size.w * WALL_ROWS),
            slots: HashMap::new(),
            hosts: HashMap::new(),
        };
        if let Some(window) = window {
            let len = size.wall_length(window.wall);
            let grid = occ.wall_mut(window.wall);
            for row in 0..WALL_ROWS {
                grid[cell(row, len, window.u)] = Cell::Window;
            }
        }
        occ
    }

    fn wall_mut(&mut self, wall: Wall) -> &mut Vec<Cell> {
        match wall {
            Wall::Left => &mut self.left,
            Wall::Right => &mut self.right,
        }
    }

    /// Por qué no cabe `items[me]`, o `None` si cabe.
    fn check(
        &self,
        items: &[RoomItem],
        catalog: Option<&RoomCatalog>,
        spec: &TypeSpec,
        me: usize,
    ) -> Option<&'static str> {
        let item = &items[me];
        if spec.modifies.is_some() || !spec.place.contains(&item.place.kind()) {
            return Some("not-placeable");
        }
        let taken = |c: Cell| c != Cell::Empty && c != Cell::Item(me);
        match &item.place {
            ItemPlace::Floor { x, y, rot } => {
                if !spec.facings.contains(rot) {
                    return Some("facing");
                }
                let (fw, fd) = spec.floor_footprint(*rot);
                let Size { w, d } = self.size;
                if *x < 0 || *y < 0 || *x > w - fw || *y > d - fd {
                    return Some("outside");
                }
                let grid = if spec.flat { &self.flat } else { &self.floor };
                let busy = (0..fd)
                    .flat_map(|j| (0..fw).map(move |i| cell(y + j, w, x + i)))
                    .any(|c| taken(grid[c]));
                busy.then_some("occupied")
            }
            ItemPlace::Wall { wall, u, row } => {
                let len = self.size.wall_length(*wall);
                let [span, tall] = spec.footprint;
                if *u < 0 || *row < 0 || *u > len - span || *row > WALL_ROWS - tall {
                    return Some("outside");
                }
                let grid = match wall {
                    Wall::Left => &self.left,
                    Wall::Right => &self.right,
                };
                let cells: Vec<Cell> = (0..tall)
                    .flat_map(|r| (0..span).map(move |i| cell(row + r, len, u + i)))
                    .map(|c| grid[c])
                    .collect();
                if cells.contains(&Cell::Window) {
                    Some("window")
                } else {
                    cells.into_iter().any(taken).then_some("occupied")
                }
            }
            // Superficie: un anfitrión colocado, un slot que exista y que nadie más use.
            ItemPlace::Surface { host, slot } => {
                let host = match self.hosts.get(host.as_str()) {
                    Some(&host) if host != me => host,
                    _ => return Some("no-slot"),
                };
                let slots = spec_of(catalog, &items[host].kind)
                    .and_then(|s| s.surface.as_ref())
                    .map_or(0, |s| s.slots.len());
                if usize::try_from(*slot).map_or(true, |s| s >= slots) {
                    return Some("no-slot");
                }
                let on_it = self.slots.get(&(items[host].uid.as_str(), *slot));
                on_it.is_some_and(|&k| k != me).then_some("occupied")
            }
        }
    }

    fn occupy(&mut self, items: &'a [RoomItem], spec: &TypeSpec, me: usize) {
        let item = &items[me];
        match &item.place {
            ItemPlace::Floor { x, y, rot } => {
                let (fw, fd) = spec.floor_footprint(*rot);
                let w = self.size.w;
                let grid = if spec.flat {
                    &mut self.flat
                } else {
                    &mut self.floor
                };
                for j in 0..fd {
                    for i in 0..fw {
                        grid[cell(y + j, w, x + i)] = Cell::Item(me);
                    }
                }
                if spec.surface.is_some() {
                    self.hosts.entry(&item.uid).or_insert(me);
                }
            }
            ItemPlace::Wall { wall, u, row } => {
                let len = self.size.wall_length(*wall);
                let [span, tall] = spec.footprint;
                let grid = self.wall_mut(*wall);
                for r in 0..tall {
                    for i in 0..span {
                        grid[cell(row + r, len, u + i)] = Cell::Item(me);
                    }
                }
            }
            ItemPlace::Surface { host, slot } => {
                self.slots.insert((host, *slot), me);
            }
        }
    }
}

/// Coloca los objetos en orden (primero piso y pared, luego superficies, para que el anfitrión
/// pueda ir después en la lista) y devuelve el problema de cada uno. Se saltan los tipos
/// desconocidos y lo que está encima de un anfitrión desconocido.
fn settle(
    items: &[RoomItem],
    size: Size,
    window: Option<&Window>,
    catalog: Option<&RoomCatalog>,
) -> Vec<Option<&'static str>> {
    let mut occ = Occupancy::new(size, window);
    let mut reasons = vec![None; items.len()];
    for surfaces in [false, true] {
        for (me, item) in items.iter().enumerate() {
            if matches!(item.place, ItemPlace::Surface { .. }) != surfaces {
                continue;
            }
            let Some(spec) = spec_of(catalog, &item.kind) else {
                continue;
            };
            if let ItemPlace::Surface { host, .. } = &item.place {
                let host_type = items.iter().find(|it| &it.uid == host).map(|h| &h.kind);
                if host_type.is_some_and(|kind| spec_of(catalog, kind).is_none()) {
                    continue;
                }
            }
            match occ.check(items, catalog, spec, me) {
                Some(reason) => reasons[me] = Some(reason),
                None => occ.occupy(items, spec, me),
            }
        }
    }
    reasons
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use serde_json::json;

    use super::*;

    fn fixtures() -> &'static Path {
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/catalog/room-fixtures"
        ))
    }

    fn fixture_catalog() -> RoomCatalog {
        let json = fs::read_to_string(fixtures().join("catalog.json")).unwrap();
        serde_json::from_str(&json).expect("catalog.json de prueba")
    }

    fn problems(layout: &Value, catalog: Option<&RoomCatalog>, owned: &[&str]) -> Vec<String> {
        let owned: Vec<String> = owned.iter().map(|s| (*s).to_owned()).collect();
        validate(layout, catalog, &owned).err().unwrap_or_default()
    }

    fn base_layout() -> Value {
        json!({
            "v": 1,
            "size": { "w": 5, "d": 5 },
            "shell": { "floor": "oak", "wall": "teal", "window": { "wall": "right", "u": 2 } },
            "items": [],
            "pinned": []
        })
    }

    fn chair(uid: &str, x: i64, y: i64) -> Value {
        json!({ "uid": uid, "type": "chair", "variant": "teal",
                "place": { "on": "floor", "x": x, "y": y, "rot": 0 } })
    }

    #[test]
    fn fixture_catalog_is_consistent() {
        let problems = catalog_problems(&fixture_catalog());
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn room_catalog_is_consistent_when_present() {
        if let Some(catalog) = ROOM_CATALOG.as_ref() {
            let problems = catalog_problems(catalog);
            assert!(
                problems.is_empty(),
                "problemas en room.json:\n{}",
                problems.join("\n")
            );
        }
    }

    /// Cada caso de `catalog/room-fixtures` trae el acomodo, lo que la persona ya probó y los
    /// problemas esperados, en el orden en que salen (los mismos que da la app).
    #[test]
    fn fixture_cases() {
        let catalog = fixture_catalog();
        let mut cases = 0;
        for entry in fs::read_dir(fixtures()).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".json") || name == "catalog.json" {
                continue;
            }
            let case: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            let owned: Vec<String> = serde_json::from_value(case["owned"].clone()).unwrap();
            let expected: Vec<String> = serde_json::from_value(case["problems"].clone()).unwrap();
            let got = validate(&case["layout"], Some(&catalog), &owned)
                .err()
                .unwrap_or_default();
            assert_eq!(got, expected, "caso {name}");
            cases += 1;
        }
        assert!(cases >= 5, "solo hubo {cases} casos");
    }

    #[test]
    fn valid_layouts_come_back_normalized() {
        let mut layout = base_layout();
        layout["extra"] = json!(true);
        layout["items"] = json!([{ "uid": "aaaa0001", "type": "frame", "variant": "home", "note": "x",
            "place": { "on": "wall", "wall": "left", "u": 1, "row": 0 } }]);
        let saved = validate(&layout, None, &[]).expect("válido");
        let normalized = serde_json::to_value(&saved).unwrap();
        layout.as_object_mut().unwrap().remove("extra");
        layout["items"][0].as_object_mut().unwrap().remove("note");
        assert_eq!(normalized, layout);
    }

    #[test]
    fn places_reject_unknown_fields() {
        let mut layout = base_layout();
        layout["items"] = json!([{ "uid": "aaaa0001", "type": "desk", "variant": "oak",
            "place": { "on": "floor", "x": 0, "y": 0, "rot": 0, "z": 3 } }]);
        assert_eq!(problems(&layout, None, &[]), ["items[0].place: invalid"]);
    }

    #[test]
    fn versions_and_shapes() {
        let mut newer = base_layout();
        newer["v"] = json!(2);
        assert_eq!(problems(&newer, None, &[]), [UNSUPPORTED]);
        let mut older = base_layout();
        older["v"] = json!(0);
        assert_eq!(problems(&older, None, &[]), ["v: invalid"]);
        assert_eq!(problems(&json!([1]), None, &[]), ["layout: invalid"]);

        let mut no_pinned = base_layout();
        no_pinned.as_object_mut().unwrap().remove("pinned");
        no_pinned["items"] = json!("nada");
        assert_eq!(problems(&no_pinned, None, &[]), ["items: invalid"]);
    }

    #[test]
    fn limits() {
        let mut layout = base_layout();
        let items: Vec<Value> = (0..61)
            .map(|i| {
                json!({ "uid": format!("zz{i:06}"), "type": "futuro", "variant": "x",
                "place": { "on": "floor", "x": 0, "y": 0, "rot": 0 } })
            })
            .collect();
        layout["items"] = json!(items);
        assert_eq!(problems(&layout, None, &[]), ["items: too-many"]);

        let mut huge = base_layout();
        huge["items"] = json!([chair("aaaa0001", 0, 0), chair("aaaa0001", 0, 0)]);
        huge["shell"]["floor"] = json!("x".repeat(MAX_BYTES));
        assert_eq!(
            problems(&huge, Some(&fixture_catalog()), &[]),
            ["shell.floor: unknown-variant", "layout: too-large"]
        );

        let mut sized = base_layout();
        sized["size"] = json!({ "w": 9, "d": 5 });
        assert_eq!(problems(&sized, None, &[]), ["size: invalid"]);
        sized["size"] = json!({ "w": 6, "d": 5 });
        assert!(problems(&sized, None, &[]).is_empty());
        assert_eq!(
            problems(&sized, Some(&fixture_catalog()), &[]),
            ["size: invalid"]
        );
    }

    #[test]
    fn without_a_catalog_only_the_shape_is_checked() {
        let mut layout = base_layout();
        layout["shell"]["floor"] = json!("marmol");
        layout["items"] = json!([
            chair("aaaa0001", 4, 4),
            { "uid": "aaaa0001", "type": "lamp", "variant": "x",
              "place": { "on": "surface", "host": "nadie123", "slot": 9 } }
        ]);
        assert_eq!(problems(&layout, None, &[]), ["items[1].uid: duplicate"]);
    }

    #[test]
    fn huge_coordinates_do_not_overflow() {
        let mut layout = base_layout();
        layout["items"] = json!([
            chair("aaaa0001", i64::MAX, 0),
            chair("aaaa0002", 0, i64::MIN),
            { "uid": "aaaa0003", "type": "shelf", "variant": "oak",
              "place": { "on": "wall", "wall": "left", "u": i64::MAX, "row": i64::MAX } },
            { "uid": "aaaa0004", "type": "mug", "variant": "cream",
              "place": { "on": "surface", "host": "aaaa0001", "slot": i64::MAX } }
        ]);
        assert_eq!(
            problems(&layout, Some(&fixture_catalog()), &[]),
            [
                "items[0].place: outside",
                "items[1].place: outside",
                "items[2].place: outside",
                "items[3].place: no-slot",
            ]
        );
    }

    #[test]
    fn what_does_not_fit_takes_no_space() {
        // La primera silla se sale; la segunda queda en (0, 0) sin chocar con nada.
        let mut layout = base_layout();
        layout["items"] = json!([
            { "uid": "aaaa0001", "type": "desk", "variant": "oak",
              "place": { "on": "floor", "x": 0, "y": 0, "rot": 2 } },
            chair("aaaa0002", 0, 0)
        ]);
        assert_eq!(
            problems(&layout, Some(&fixture_catalog()), &[]),
            ["items[0].place: facing"]
        );
    }
}
