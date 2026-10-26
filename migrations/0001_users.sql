-- Cuentas anónimas por dispositivo (se usan desde la iteración 1 en POST /v1/auth/device).
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
