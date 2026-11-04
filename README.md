<<<<<<< Updated upstream
# SpaceEase — API (Rust)
=======
# Space Ease — Backend / API
>>>>>>> Stashed changes

Space Ease acompaña principalmente a personas neurodivergentes para reconocer molestias del entorno y necesidades internas, probar ajustes y registrar si ayudan. Este repositorio contiene la API que guarda cuentas de dispositivo, lugares, check-ins y resultados, y propone ajustes a partir de un catálogo. La interfaz móvil y las actividades locales viven en el frontend. El producto no realiza diagnósticos.

<<<<<<< Updated upstream
> **Para el equipo:** aquí se puede trabajar. Este repo es el espejo de `apps/api` del monorepo de trabajo.
> - Antes de empezar, `git pull`: los commits `sync monorepo@…` traen lo que cambió en el monorepo.
> - Haz commits normales en `main` (o una rama con PR), nunca force-push.
> - Quique los trae al monorepo uno por uno con `scripts/import-team-repos.sh`, con tu nombre como autor. Ahí se revisan (tipos, lint y pruebas) y luego llegan a la app.
> - El siguiente sync no sobrescribe tu trabajo: se detiene si encuentra commits tuyos que todavía no se importaron.
> - Al mezclarse a `main` del monorepo, Railway despliega la API: revisa que `scripts/smoke-api.sh` pase.
=======
## Repositorios e integración

Trabajamos en dos repositorios: `AiChallengeFront` para la aplicación móvil y `AiChallengeBack` para el servidor. Integramos ambos en el [monorepo AiChallenge](https://github.com/Quique2/AiChallenge.git) para coordinar el producto y preparar la versión de iOS distribuida por TestFlight:

- `apps/mobile`: frontend Expo/React Native.
- `apps/api`: backend Rust.

TestFlight distribuye la app; la API sigue ejecutándose por separado en Railway con PostgreSQL. Los commits `sync monorepo@…` señalan sincronizaciones con el repositorio de integración. Coordinar los cambios del backend con el responsable del monorepo antes de sincronizar o desplegar, conservar autoría y evitar force-push.

## Stack y estructura

Rust, Axum, Tokio, SQLx y PostgreSQL. El análisis opcional de fotos usa la API de Claude (Anthropic) mediante HTTP. El backend actual no incluye ONNX Runtime. Las versiones exactas están en `Cargo.toml` y `Cargo.lock`.

| Ruta | Responsabilidad |
|---|---|
| `src/main.rs` | Inicio, conexión a PostgreSQL y migraciones |
| `src/config.rs` | Variables de entorno |
| `src/routes/` | Endpoints de cuenta, lugares, check-ins y foto |
| `src/auth.rs` | Autenticación por llave de dispositivo |
| `src/catalog.rs` y `catalog/catalog.json` | Catálogo y validación de ajustes |
| `src/engine.rs` | Selección de propuestas |
| `src/claude.rs` | Cliente del proveedor de visión |
| `migrations/` | Evolución del esquema SQL |
| `static/privacy.html` | Aviso servido en `/privacy` |

## Requisitos

- Rust con soporte para edición 2024 y Cargo; usar un toolchain estable que resuelva `Cargo.lock`.
- PostgreSQL; el ejemplo local utiliza la imagen `postgres:17`.
- Docker para iniciar la base de datos con el comando siguiente, o una instancia PostgreSQL existente.
- Acceso a Anthropic y una llave propia solo si se probará el análisis de fotos.

## Ejecución local

Desde la raíz de este repositorio, iniciar PostgreSQL:

```bash
docker run --name sensa-postgres \
  -e POSTGRES_USER=sensa \
  -e POSTGRES_PASSWORD=sensa \
  -e POSTGRES_DB=sensa \
  -p 127.0.0.1:5432:5432 \
  -v space-ease-postgres:/var/lib/postgresql/data \
  -d postgres:17
```

El nombre `sensa-postgres` se conserva como identificador de desarrollo; la app se llama **Space Ease**. Las credenciales del ejemplo son exclusivamente locales. Si el contenedor ya existe y está detenido, usar `docker start sensa-postgres`. El volumen conserva los datos al recrearlo; el puerto 5432 debe estar libre.

Crear la configuración local:

```bash
cp .env.example .env
```

Editar `.env` para usar:

```dotenv
DATABASE_URL=postgres://sensa:sensa@localhost:5432/sensa
PORT=8080
RUST_LOG=spaceease_api=info,tower_http=info
```

Iniciar la API:

```bash
cargo run --locked
```

Las migraciones de `migrations/` se aplican automáticamente al arrancar. La base debe estar disponible antes de iniciar el servidor. Comprobar la conexión:

```bash
curl http://localhost:8080/health
```

La respuesta debe indicar `status: "ok"` y `database: "ok"`. Para conectar el teléfono, configurar `EXPO_PUBLIC_API_URL` en el front con la IP local de esta computadora y el puerto 8080; consultar su README.

### Variables de entorno

| Variable | Uso |
|---|---|
| `DATABASE_URL` | Obligatoria. Conexión a PostgreSQL. |
| `PORT` | Puerto HTTP; por defecto `8080`. |
| `RUST_LOG` | Nivel de registro de diagnóstico. |
| `ANTHROPIC_API_KEY` | Opcional. Habilita el análisis de fotos; debe permanecer en el servidor. |
| `CLAUDE_VISION_MODEL` | Modelo de visión utilizado por `src/config.rs`; configurar uno disponible para la cuenta del equipo. |
| `CLAUDE_VISION_EFFORT` | Esfuerzo de inferencia; por defecto `medium`. Debe ser compatible con el modelo elegido. |

Sin `ANTHROPIC_API_KEY`, el servidor puede iniciar y las demás funciones siguen disponibles; el endpoint de foto responde «no disponible» (503). Con una llave, confirmar también la disponibilidad del modelo configurado antes de la demo. No subir `.env` ni llaves al repositorio.

## API principal

| Endpoint | Función |
|---|---|
| `GET /health` | Estado de la API y conexión a base de datos |
| `GET /privacy` | Aviso de privacidad |
| `POST /v1/auth/device` | Crear o recuperar sesión por llave de dispositivo |
| `GET/PATCH/DELETE /v1/me` | Consultar, actualizar o borrar la cuenta |
| `GET/POST /v1/spaces` | Consultar o crear lugares |
| `DELETE /v1/spaces/{id}` | Borrar un lugar |
| `GET /v1/catalog` | Catálogo de ajustes |
| `GET/POST /v1/checkins` | Consultar o crear check-ins |
| `GET /v1/checkins/{id}` | Consultar un check-in |
| `POST /v1/checkins/{id}/accept`, `/decline`, `/outcome`, `/abandon` | Gestionar una prueba y su resultado |
| `GET /v1/playbook`, `/v1/progress` | Consultar estrategias y progreso |
| `POST /v1/scan/photo` | Analizar opcionalmente una foto con Claude |

Las rutas privadas requieren `Authorization: Bearer <llave-del-dispositivo>`. Los esquemas y validaciones están en los módulos de `src/routes/`.

**Límite de este snapshot:** el router actual no expone las rutas de memoria, reinicio y cuarto de un lugar que declara el cliente móvil (`/v1/spaces/:id/memory`, `/reset`, `/room`). Coordinar una versión compatible desde el monorepo antes de demostrar esas funciones. Las pausas guiadas y la personalización de la interfaz se gestionan en el front.

## Pruebas y compilación

```bash
cargo fmt --check
cargo test --locked
cargo build --release --locked
```

Hay pruebas de autenticación, catálogo, motor de propuestas y validación de respuestas de visión. Para validar el recorrido completo, iniciar PostgreSQL y ejecutar desde la app la creación de cuenta, lugar, check-in, aceptación y registro del resultado. Estos comandos documentan cómo verificar una versión; no afirman que se hayan ejecutado en cada revisión.

El `Dockerfile` incluye una compilación por etapas. Para construir la imagen desde este repositorio:

```bash
docker build -t space-ease-api .
```

En Railway, configurar las variables del servicio y una base PostgreSQL accesible. En el monorepo, el contexto del backend es `apps/api`. El backend no se publica mediante TestFlight: la app iOS se compila desde `apps/mobile` y se conecta a esta API.

## Decisiones técnicas y de producto

| Decisión | Motivo y alcance |
|---|---|
| Rust, Axum y Tokio | Usar tipos explícitos y rutas HTTP separadas para modelar validaciones y procesar solicitudes de forma asíncrona. |
| PostgreSQL y SQLx | Mantener relaciones entre cuenta, lugares, pruebas y resultados; usar consultas parametrizadas y migraciones versionadas. |
| Separar API y aplicación móvil | Evolucionar persistencia y recomendaciones sin incorporar secretos de servidor al cliente; requiere coordinar versiones de ambos repositorios. |
| Motor de propuestas basado en catálogo | Seleccionar ajustes definidos por el proyecto y apoyarse en el historial de pruebas, en lugar de pedir a un modelo generativo que invente todas las recomendaciones. |
| Claude solo para exploración opcional de fotos | Complementar lo que observa la persona con hipótesis sobre el entorno. La persona puede revisar los hallazgos; la imagen no constituye un diagnóstico. |
| Llave aleatoria por dispositivo y hash en servidor | Reducir la fricción de registro sin solicitar correo o contraseña. La posesión de la llave da acceso y no existe un flujo de recuperación entre dispositivos en este snapshot. |
| Registrar si el ajuste ayudó | Reconocer que una misma intervención no sirve igual a todas las personas y apoyar decisiones con resultados de la propia persona. |
| Mantener pausas y preferencias en el front | Permitir actividades locales sin enviar al servidor cada interacción o elección visual. El historial remoto sí requiere conexión. |

El servidor limita el tamaño de las solicitudes y autentica las rutas privadas. El router actual usa CORS permisivo; debe revisarse según los clientes y el entorno del despliegue. Estas medidas no sustituyen una auditoría de seguridad ni validación del producto con usuarios.

## Nombres internos

Algunos identificadores conservan nombres anteriores, como `spaceease-api`, `spaceease_api` y `sensa-postgres`. Se documentan tal como aparecen en los comandos y el código para que la instalación sea reproducible; el nombre del producto es **Space Ease**.
>>>>>>> Stashed changes
