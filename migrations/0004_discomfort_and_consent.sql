-- Medida principal: cuánto molesta el entorno (1 casi nada – 5 muchísimo). El esfuerzo queda como
-- medida secundaria. Ver docs/research/reports (decisiones 7 y 8).
ALTER TABLE checkins
    ADD COLUMN discomfort_before SMALLINT CHECK (discomfort_before BETWEEN 1 AND 5),
    ADD COLUMN discomfort_after SMALLINT CHECK (discomfort_after BETWEEN 1 AND 5),
    -- discomfort_before − discomfort_after: positivo es mejor.
    ADD COLUMN improvement REAL,
    -- Mejora típica de la persona en ese momento; el resultado se decide contra ella.
    ADD COLUMN typical_improvement REAL;

-- Consentimiento expreso para guardar los registros (LFPDPPP art. 8: datos sensibles).
ALTER TABLE users
    ADD COLUMN consent_version SMALLINT,
    ADD COLUMN consented_at TIMESTAMPTZ;
