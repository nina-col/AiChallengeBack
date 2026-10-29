use std::{env, fs, path::Path};

fn main() {
    // sqlx::migrate!() incrusta las migraciones al compilar; esto fuerza a recompilar cuando cambian.
    println!("cargo:rerun-if-changed=migrations");
    // El catálogo del cuarto (catalog/room.json) lo genera docs/brand/draw_room.py. Se incrusta si
    // existe y, si no, queda vacío y la API arranca igual. Se vigila la carpeta y no el archivo:
    // con un archivo que no existe, Cargo correría este script en cada build.
    println!("cargo:rerun-if-changed=catalog");
    let room = fs::read_to_string("catalog/room.json").unwrap_or_default();
    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("room.json");
    fs::write(out, room).expect("no se pudo escribir room.json en OUT_DIR");
}
