-- Favoritos de cada aluno ou grupo: ilhas e trilhos, sincronizados entre telemóveis.
CREATE TABLE favorites (
    student_id  UUID NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('island', 'trail')),
    item_id     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (student_id, kind, item_id)
);
