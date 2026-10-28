# SpaceEase — API (Rust)

API en Rust (Axum + sqlx + ONNX Runtime) desplegada en Railway con Postgres. El proyecto se crea en la iteración 0.

> **Para el equipo:** aquí se puede trabajar. Este repo es el espejo de `apps/api` del monorepo de trabajo.
> - Antes de empezar, `git pull`: los commits `sync monorepo@…` traen lo que cambió en el monorepo.
> - Haz commits normales en `main` (o una rama con PR), nunca force-push.
> - Quique los trae al monorepo uno por uno con `scripts/import-team-repos.sh`, con tu nombre como autor. Ahí se revisan (tipos, lint y pruebas) y luego llegan a la app.
> - El siguiente sync no sobrescribe tu trabajo: se detiene si encuentra commits tuyos que todavía no se importaron.
> - Al mezclarse a `main` del monorepo, Railway despliega la API: revisa que `scripts/smoke-api.sh` pase.
