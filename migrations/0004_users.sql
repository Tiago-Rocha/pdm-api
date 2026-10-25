-- Contas de utilizador dentro da app de cada grupo (cada linha de students é uma app,
-- com o seu clientId). Os favoritos passam a ser de cada utilizador e os relatos
-- guardam quem os escreveu.
CREATE TABLE users (
    id            UUID PRIMARY KEY,
    app_id        UUID NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    email         TEXT NOT NULL,
    name          TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen     TIMESTAMPTZ,
    UNIQUE (app_id, email)
);

-- Refresh tokens: só o hash argon2 do segredo; o id vai à frente no token.
CREATE TABLE refresh_tokens (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ
);
CREATE INDEX refresh_tokens_user ON refresh_tokens (user_id);

-- os favoritos da 0003 eram por app e ainda ninguém os usava
DROP TABLE favorites;
CREATE TABLE favorites (
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('island', 'trail')),
    item_id     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, kind, item_id)
);

ALTER TABLE reports ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE CASCADE;
