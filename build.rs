// sqlx::migrate!() incrusta las migraciones al compilar; esto fuerza a recompilar cuando cambian.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
