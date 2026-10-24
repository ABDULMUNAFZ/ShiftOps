# Repository fixtures

Minimal repositories used by automated tests only. They are never shown in the
application UI and are not sample product data.

| Fixture        | Purpose                                                        |
| -------------- | -------------------------------------------------------------- |
| `node-library` | A package with no internal dependencies.                       |
| `node-service` | Depends on `@fixture/node-library`, to test migration ordering. |

The fixtures are stored as plain directories, not nested Git repositories.
Tests that need Git history copy a fixture to a temporary directory and run
`git init` there, so the outer repository stays clean.

Repository scanning is not implemented yet; these fixtures will be consumed by
the discovery tests in that step.
