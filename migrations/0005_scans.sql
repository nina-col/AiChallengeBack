-- Cada análisis de foto con IA, sin la foto: solo para el límite diario y para medir el costo.
CREATE TABLE scans (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    model TEXT NOT NULL,
    input_tokens INT NOT NULL DEFAULT 0,
    output_tokens INT NOT NULL DEFAULT 0,
    hypotheses SMALLINT NOT NULL DEFAULT 0,
    ok BOOLEAN NOT NULL
);

CREATE INDEX scans_user_day_idx ON scans (user_id, created_at);
