# pdm-api · API Tempo Açores

API da UC de PDM (CTeSP DAW, ESTA, Universidade dos Açores, 2026/27), para a app Flutter dos alunos.

Em produção em **https://api-pdm.sublimecipher.com** (documentação em `/docs`). O endereço antigo `pdm.188-245-31-95.sslip.io` reencaminha para lá; a rede da escola bloqueia o sslip.io.

- Ilhas e trilhos dos Açores, e a previsão a 5 dias do IPMA (dados abertos, com cache de 30 minutos).
- Outros dados do IPMA, normalizados e com cache: avisos meteorológicos (`/v1/warnings`), estado do mar (`/v1/islands/{id}/sea`), observações das estações (`/v1/islands/{id}/observations`) e sismos (`/v1/earthquakes`). As apps dos alunos nunca falam com o IPMA diretamente.
- Favoritos (ilhas e trilhos) de cada utilizador, em `/v1/me/favorites`: `PUT` e `DELETE` em `/v1/me/favorites/{islands|trails}/{id}`, ambos idempotentes.
- Relatos da turma: texto e fotografia sobre o tempo numa ilha, para exercitar POST, PUT e DELETE.
- Autenticação em dois níveis. A app (de um aluno ou de um grupo) tem `clientId` e `clientSecret`, trocados por um JWT em `POST /v1/auth/token`. Dentro de cada app, as pessoas criam conta e fazem login (`/v1/auth/register`, `/login`, `/refresh`, `/logout`): access token de 15 minutos e refresh token de 30 dias, guardado só em hash. `DELETE /v1/me` apaga a conta.
- Documentação interativa em `/docs` (Swagger UI) e a especificação em `/openapi.json`.

Rust (axum, sqlx), PostgreSQL. No servidor: serviço systemd `pdm-api` na porta 3002, atrás do Caddy; variáveis em `/etc/pdm-api.env`; fotografias em `/var/lib/pdm-api/media`.

## Docente

Rotas em `/admin`, com o header `X-Admin-Key`:

- `POST /admin/students` com `{"name": "...", "github": "..."}`: cria um aluno e devolve o `clientId` e o `clientSecret` (o segredo só aparece aqui).
- `GET /admin/students`: alunos e grupos (as apps), último acesso, número de relatos e de utilizadores.
- `POST /admin/students/{clientId}/reset-secret`: gera um segredo novo.
- `DELETE /admin/students/{clientId}`: apaga um aluno e os seus relatos.
- `DELETE /admin/reports/{id}`: apaga qualquer relato.
- `GET /admin/usage`: uso da API por aluno (pedidos, últimas 24 h, dias ativos, logins falhados, relatos, endpoints mais usados).

## Registo de uso

Cada pedido (menos `/health`) fica na tabela `request_log` com o aluno, o método, o caminho, o código, o tempo, o IP e o user agent. Se `O2_URL`, `O2_USER` e `O2_PASSWORD` estiverem definidas, os mesmos eventos vão para o OpenObserve (`O2_URL` é o endpoint `_json` de uma stream, por exemplo `http://127.0.0.1:5080/api/default/pdm-api/_json`). Se `DD_API_KEY` estiver definida em `/etc/pdm-api.env`, vão também para os logs do Datadog (`DD_SITE`, por defeito `datadoghq.eu`), com `service:pdm-api` e as tags `client_id:...` e `event:login|login_failed`.

## Deploy

```bash
./deploy.sh
```
