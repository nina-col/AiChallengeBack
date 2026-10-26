-- Un registro = una dificultad en una tarea y un lugar, el ajuste que se propuso y si ayudó.
CREATE TABLE checkins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    space_id UUID REFERENCES spaces (id) ON DELETE SET NULL,
    task TEXT NOT NULL,
    feeling TEXT NOT NULL,
    -- Esfuerzo percibido, 1 (muy poco) a 5 (muchísimo).
    effort_before SMALLINT NOT NULL CHECK (effort_before BETWEEN 1 AND 5),
    -- El estímulo en el que se trabaja; NULL si la persona no identificó ninguno.
    focus_stimulus TEXT,
    adjustment_id TEXT,
    -- Ajustes descartados en este registro ("otra idea" o "no puedo aquí").
    declined TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'proposed'
        CHECK (status IN ('proposed', 'trying', 'done', 'abandoned')),
    remind_at TIMESTAMPTZ,
    effort_after SMALLINT CHECK (effort_after BETWEEN 1 AND 5),
    -- Lo que la persona cree: ¿ayudó el ajuste?
    attribution TEXT CHECK (attribution IN ('yes', 'no', 'unsure')),
    -- Lo que aprende el motor; 'unclear' no cuenta como evidencia.
    outcome TEXT CHECK (outcome IN ('helped', 'not_helped', 'unclear')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE INDEX checkins_user_idx ON checkins (user_id, created_at DESC);
CREATE INDEX checkins_evidence_idx ON checkins (focus_stimulus, adjustment_id)
    WHERE outcome IN ('helped', 'not_helped');

-- Lo que la persona contestó sobre su entorno. Desde la iteración 2 también llega de la foto y el audio.
CREATE TABLE observations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    checkin_id UUID NOT NULL REFERENCES checkins (id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (source IN ('question', 'photo', 'audio')),
    stimulus TEXT NOT NULL,
    -- yes = está y le afecta · no = no está o no le afecta · unsure = "no sé"
    response TEXT NOT NULL CHECK (response IN ('yes', 'no', 'unsure')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX observations_checkin_idx ON observations (checkin_id);

-- "No puedo hacer esto en este lugar": no se vuelve a proponer ahí.
CREATE TABLE space_blocks (
    space_id UUID NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    adjustment_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (space_id, adjustment_id)
);
