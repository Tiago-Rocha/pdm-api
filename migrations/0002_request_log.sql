-- Um registo por pedido à API: quem, o quê, quando e com que resultado.
-- Serve ao docente para ver como cada aluno usa a API (também vai para o Datadog).
CREATE TABLE request_log (
    id          BIGSERIAL PRIMARY KEY,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    client_id   TEXT,
    name        TEXT,
    method      TEXT NOT NULL,
    path        TEXT NOT NULL,
    status      INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    ip          TEXT,
    user_agent  TEXT,
    event       TEXT
);
CREATE INDEX request_log_client_at ON request_log (client_id, at DESC);
CREATE INDEX request_log_at ON request_log (at DESC);
