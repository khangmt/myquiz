# myquiz

A Kahoot-like quiz platform: create quizzes, host live games with a PIN, and download reports afterwards.

- Backend: Rust (Axum on tokio), PostgreSQL 16, Redis 7, plain WebSockets
- Frontend: React + TypeScript (Vite)
- Deployment: Docker Compose, memory-capped at about 9 GB in total

The work follows the requirements spec (v1.0) and the design document. Requirement IDs such as
`FR-GM-06` or `SEC-15` in code and PRs refer to the spec.
