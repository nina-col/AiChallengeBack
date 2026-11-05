-- "Este lugar cambió": desde esta fecha el motor solo toma como de aquí lo que se probó después.
-- Lo anterior sigue contando como de la persona; el historial no se borra.
ALTER TABLE spaces ADD COLUMN reset_at TIMESTAMPTZ;

-- El cuarto isométrico de cada lugar: el acomodo completo como un solo JSON versionado.
-- `revision` sube en cada guardado para que dos pantallas no se pisen.
CREATE TABLE rooms (
    space_id UUID PRIMARY KEY REFERENCES spaces (id) ON DELETE CASCADE,
    schema_version SMALLINT NOT NULL,
    layout JSONB NOT NULL
        CHECK (jsonb_typeof(layout) = 'object' AND octet_length(layout::text) <= 16384),
    revision INT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- La memoria de un lugar recorre sus registros.
CREATE INDEX checkins_space_idx ON checkins (space_id, completed_at) WHERE space_id IS NOT NULL;
