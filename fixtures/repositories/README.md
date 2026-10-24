# Repository fixtures

Minimal repositories used by automated tests only. They are never shown in the application UI and are not sample product data. Prettier ignores this folder so files stay byte-exact (one is malformed on purpose).

| Fixture                  | Purpose                                                                                                   |
| ------------------------ | --------------------------------------------------------------------------------------------------------- |
| `node-library`           | A package with no internal dependencies.                                                                  |
| `node-service`           | Depends on `@fixture/node-library`, to test migration ordering.                                           |
| `node18-express`         | Node 18 Express service: `engines`, `.nvmrc`, npm lockfile, `node:18` Dockerfile, Jest.                  |
| `node20-next-pnpm`       | Next.js on Node 20 with pnpm: `packageManager`, `.node-version`, pnpm lockfile, TypeScript, Vitest.        |
| `shared-typescript`      | Shared TypeScript library with build and test scripts; used for cache tests.                              |
| `runtime-conflict`       | Disagreeing Node declarations (engines, Volta, `.nvmrc`, Dockerfile, GitHub Actions) and two lockfiles.   |
| `fastapi-service`        | FastAPI with `pyproject.toml`, `.python-version` and a Python Dockerfile.                                 |
| `spring-boot-service`    | Spring Boot 3 Maven project on Java 21.                                                                   |
| `dotnet-api`             | ASP.NET Core project targeting `net8.0`, with `global.json`.                                              |
| `go-service`             | Go module with a `go` directive, toolchain and Gin.                                                       |
| `rust-workspace`         | Cargo workspace with two members, `rust-version` and `rust-toolchain.toml`.                              |
| `js-monorepo`            | Yarn workspaces monorepo with Turborepo and nested React, Vite and NestJS packages.                       |
| `malformed-package-json` | A truncated `package.json`; the scan must complete with a warning.                                        |

The fixtures are plain directories, not nested Git repositories. Tests copy a fixture to a temporary directory before use. They create worktree `.git` files, symlinks and `git init` histories there, because Git refuses to commit a path named `.git`. The outer repository stays clean.
