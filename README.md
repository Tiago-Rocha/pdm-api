# pdm-api · API Tempo Açores

API da UC de PDM (CTeSP DAW, ESTA, Universidade dos Açores, 2026/27), para a app Flutter dos alunos.

- Ilhas e trilhos dos Açores, e a previsão a 5 dias do IPMA (dados abertos, com cache de 30 minutos).
- Relatos da turma: texto e fotografia sobre o tempo numa ilha, para exercitar POST, PUT e DELETE.
- Autenticação por aluno: `clientId` e `clientSecret` trocados por um JWT em `POST /v1/auth/token`.
- Documentação interativa em `/docs` (Swagger UI) e a especificação em `/openapi.json`.

Rust (axum, sqlx), PostgreSQL. No servidor: serviço systemd `pdm-api` na porta 3002, atrás do Caddy; variáveis em `/etc/pdm-api.env`; fotografias em `/var/lib/pdm-api/media`.

## Docente

Rotas em `/admin`, com o header `X-Admin-Key`:

- `POST /admin/students` com `{"name": "...", "github": "..."}`: cria um aluno e devolve o `clientId` e o `clientSecret` (o segredo só aparece aqui).
- `GET /admin/students`: alunos, último acesso e número de relatos.
- `POST /admin/students/{clientId}/reset-secret`: gera um segredo novo.
- `DELETE /admin/students/{clientId}`: apaga um aluno e os seus relatos.
- `DELETE /admin/reports/{id}`: apaga qualquer relato.

## Deploy

```bash
./deploy.sh
```
