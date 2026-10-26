-- Llave del dispositivo (solo su SHA-256) e idioma. La tabla users todavía no tiene filas en producción.
ALTER TABLE users
    ADD COLUMN key_hash BYTEA NOT NULL UNIQUE,
    ADD COLUMN locale TEXT NOT NULL DEFAULT 'es' CHECK (locale IN ('es', 'en'));

-- Lugares donde trabaja la persona ("Mi escritorio", "Sala 3", "Casa").
CREATE TABLE spaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 60),
    kind TEXT NOT NULL CHECK (kind IN ('office', 'home', 'other')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX spaces_user_idx ON spaces (user_id, created_at);
