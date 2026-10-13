-- Alunos: cada um tem um client_id e um segredo (guardado só como hash argon2).
CREATE TABLE students (
    id          UUID PRIMARY KEY,
    client_id   TEXT NOT NULL UNIQUE,
    secret_hash TEXT NOT NULL,
    name        TEXT NOT NULL,
    github      TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen   TIMESTAMPTZ
);

-- Relatos: texto e, opcionalmente, uma fotografia sobre o tempo numa ilha.
CREATE TABLE reports (
    id          UUID PRIMARY KEY,
    student_id  UUID NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    island_id   TEXT NOT NULL,
    text        TEXT NOT NULL,
    photo       TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX reports_island_created ON reports (island_id, created_at DESC);
CREATE INDEX reports_created ON reports (created_at DESC);
