//! Detectors: turn collected files into evidence-backed facts and conflicts.
//! Parsing is defensive: malformed files produce warnings, never panics.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::facts::{json_key_line, line_of, Category, Conflict, ConflictValue, Fact};
use super::files::{RelevantFile, ScanWarning};
use super::semver;

const MAX_DEPENDENCY_FACTS: usize = 600;

#[derive(Debug, Default)]
pub struct Findings {
    pub facts: Vec<Fact>,
    pub conflicts: Vec<Conflict>,
    pub warnings: Vec<ScanWarning>,
    dependency_count: usize,
}

impl Findings {
    fn push(&mut self, fact: Fact) {
        if fact.category == Category::Dependency {
            if self.dependency_count >= MAX_DEPENDENCY_FACTS {
                return;
            }
            self.dependency_count += 1;
        }
        self.facts.push(fact);
    }

    fn warn(&mut self, path: &str, message: impl Into<String>) {
        self.warnings.push(ScanWarning {
            path: path.to_string(),
            message: message.into(),
        });
    }
}

fn name_of(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(d, _)| d)
}

const JS_FRAMEWORKS: &[(&str, &str)] = &[
    ("next", "Next.js"),
    ("react", "React"),
    ("express", "Express"),
    ("@nestjs/core", "NestJS"),
    ("fastify", "Fastify"),
    ("vite", "Vite"),
    ("vitest", "Vitest"),
    ("jest", "Jest"),
    ("typescript", "TypeScript"),
    ("@angular/core", "Angular"),
    ("vue", "Vue"),
    ("nuxt", "Nuxt"),
    ("svelte", "Svelte"),
    ("@sveltejs/kit", "SvelteKit"),
    ("koa", "Koa"),
    ("@remix-run/node", "Remix"),
    ("astro", "Astro"),
    ("mocha", "Mocha"),
    ("@playwright/test", "Playwright"),
];

const PY_FRAMEWORKS: &[(&str, &str)] = &[
    ("fastapi", "FastAPI"),
    ("django", "Django"),
    ("flask", "Flask"),
    ("starlette", "Starlette"),
    ("uvicorn", "Uvicorn"),
    ("pydantic", "Pydantic"),
    ("pytest", "pytest"),
    ("celery", "Celery"),
];

const GO_FRAMEWORKS: &[(&str, &str)] = &[
    ("github.com/gin-gonic/gin", "Gin"),
    ("github.com/labstack/echo/v4", "Echo"),
    ("github.com/gofiber/fiber/v2", "Fiber"),
    ("github.com/go-chi/chi/v5", "chi"),
];

const RUST_FRAMEWORKS: &[(&str, &str)] = &[
    ("tokio", "Tokio"),
    ("actix-web", "Actix Web"),
    ("axum", "Axum"),
    ("rocket", "Rocket"),
    ("tauri", "Tauri"),
    ("serde", "Serde"),
];

pub fn detect(files: &[RelevantFile]) -> Findings {
    let mut f = Findings::default();
    for file in files {
        let name = name_of(&file.rel);
        let Some(text) = file.text.as_deref() else {
            record_presence_only(&mut f, file);
            continue;
        };
        match name {
            "package.json" => package_json(&mut f, &file.rel, text),
            ".nvmrc" | ".node-version" => {
                if let Some(v) = text
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty() && !l.starts_with('#'))
                {
                    f.push(
                        Fact::new(Category::Runtime, "node", v, name, 0.95)
                            .at(&file.rel, line_of(text, v, 1)),
                    );
                }
            }
            ".tool-versions" => tool_versions(&mut f, &file.rel, text),
            "package-lock.json" | "npm-shrinkwrap.json" => lockfile(&mut f, &file.rel, "npm"),
            "pnpm-lock.yaml" => lockfile(&mut f, &file.rel, "pnpm"),
            "yarn.lock" => lockfile(&mut f, &file.rel, "yarn"),
            "bun.lockb" | "bun.lock" => lockfile(&mut f, &file.rel, "bun"),
            "pnpm-workspace.yaml" => pnpm_workspace(&mut f, &file.rel, text),
            "lerna.json" | "turbo.json" | "nx.json" => {
                let tool = name.trim_end_matches(".json");
                f.push(
                    Fact::new(
                        Category::Workspace,
                        tool,
                        file.rel.clone(),
                        format!("{name} present"),
                        0.9,
                    )
                    .at(&file.rel, None),
                );
            }
            "pyproject.toml" => pyproject(&mut f, &file.rel, text),
            "Pipfile" => {
                manifest(&mut f, &file.rel, "Pipfile");
                f.push(
                    Fact::new(
                        Category::PackageManager,
                        "pipenv",
                        "detected",
                        "Pipfile",
                        0.85,
                    )
                    .at(&file.rel, None),
                );
            }
            "poetry.lock" => lockfile(&mut f, &file.rel, "poetry"),
            "uv.lock" => lockfile(&mut f, &file.rel, "uv"),
            "Pipfile.lock" => lockfile(&mut f, &file.rel, "pipenv"),
            ".python-version" => {
                if let Some(v) = text.lines().map(str::trim).find(|l| !l.is_empty()) {
                    f.push(
                        Fact::new(Category::Runtime, "python", v, ".python-version", 0.95)
                            .at(&file.rel, Some(1)),
                    );
                }
            }
            "runtime.txt" => {
                if let Some(v) = text.trim().strip_prefix("python-") {
                    f.push(
                        Fact::new(Category::Runtime, "python", v, "runtime.txt", 0.9)
                            .at(&file.rel, Some(1)),
                    );
                }
            }
            "setup.py" | "setup.cfg" => manifest(&mut f, &file.rel, name),
            "pom.xml" => pom(&mut f, &file.rel, text),
            "build.gradle" | "build.gradle.kts" => gradle(&mut f, &file.rel, text),
            "settings.gradle" | "settings.gradle.kts" => manifest(&mut f, &file.rel, name),
            "gradle-wrapper.properties" => {
                if let Some((n, line)) = text
                    .lines()
                    .enumerate()
                    .find(|(_, l)| l.starts_with("distributionUrl"))
                {
                    if let Some(v) = line
                        .split("gradle-")
                        .nth(1)
                        .and_then(|r| r.split('-').next())
                    {
                        f.push(
                            Fact::new(
                                Category::PackageManager,
                                "gradle",
                                v,
                                "Gradle wrapper",
                                0.95,
                            )
                            .at(&file.rel, Some(n as u32 + 1)),
                        );
                    }
                }
            }
            ".java-version" => {
                if let Some(v) = text.lines().map(str::trim).find(|l| !l.is_empty()) {
                    f.push(
                        Fact::new(Category::Runtime, "java", v, ".java-version", 0.95)
                            .at(&file.rel, Some(1)),
                    );
                }
            }
            "global.json" => global_json(&mut f, &file.rel, text),
            "Directory.Build.props" => target_frameworks(&mut f, &file.rel, text),
            "go.mod" => go_mod(&mut f, &file.rel, text),
            "go.work" => go_work(&mut f, &file.rel, text),
            "Cargo.toml" => cargo_toml(&mut f, &file.rel, text),
            "Cargo.lock" => lockfile(&mut f, &file.rel, "cargo"),
            "rust-toolchain" | "rust-toolchain.toml" => rust_toolchain(&mut f, &file.rel, text),
            ".gitlab-ci.yml" => {
                f.push(
                    Fact::new(
                        Category::Ci,
                        "gitlab_ci",
                        file.rel.clone(),
                        ".gitlab-ci.yml present",
                        1.0,
                    )
                    .at(&file.rel, None),
                );
                images(&mut f, &file.rel, text, Category::Ci, "gitlab_image");
            }
            "Jenkinsfile" => f.push(
                Fact::new(
                    Category::Ci,
                    "jenkins",
                    file.rel.clone(),
                    "Jenkinsfile present",
                    1.0,
                )
                .at(&file.rel, None),
            ),
            "azure-pipelines.yml" => f.push(
                Fact::new(
                    Category::Ci,
                    "azure_pipelines",
                    file.rel.clone(),
                    "azure-pipelines.yml present",
                    1.0,
                )
                .at(&file.rel, None),
            ),
            _ => {
                let lower = name.to_ascii_lowercase();
                if lower.starts_with("dockerfile") {
                    dockerfile(&mut f, &file.rel, text);
                } else if lower.starts_with("docker-compose") || lower.starts_with("compose.") {
                    f.push(
                        Fact::new(
                            Category::Docker,
                            "compose_file",
                            file.rel.clone(),
                            "Compose file present",
                            1.0,
                        )
                        .at(&file.rel, None),
                    );
                    images(&mut f, &file.rel, text, Category::Docker, "compose_image");
                } else if lower.starts_with("requirements") && lower.ends_with(".txt") {
                    requirements(&mut f, &file.rel, text);
                } else if lower.starts_with("tsconfig") {
                    f.push(
                        Fact::new(
                            Category::Manifest,
                            "tsconfig",
                            file.rel.clone(),
                            "tsconfig present",
                            1.0,
                        )
                        .at(&file.rel, None),
                    );
                } else if lower.ends_with(".csproj") || lower.ends_with(".fsproj") {
                    csproj(&mut f, &file.rel, text);
                } else if lower.ends_with(".sln") {
                    manifest(&mut f, &file.rel, "solution");
                } else if file.rel.starts_with(".github/workflows/") {
                    github_workflow(&mut f, &file.rel, text);
                } else if file.rel == ".circleci/config.yml" {
                    f.push(
                        Fact::new(
                            Category::Ci,
                            "circleci",
                            file.rel.clone(),
                            "CircleCI config present",
                            1.0,
                        )
                        .at(&file.rel, None),
                    );
                }
            }
        }
    }
    runtime_conflicts(&mut f);
    package_manager_conflicts(&mut f);
    f
}

fn record_presence_only(f: &mut Findings, file: &RelevantFile) {
    let name = name_of(&file.rel);
    match name {
        "package-lock.json" | "npm-shrinkwrap.json" => lockfile(f, &file.rel, "npm"),
        "pnpm-lock.yaml" => lockfile(f, &file.rel, "pnpm"),
        "yarn.lock" => lockfile(f, &file.rel, "yarn"),
        "bun.lockb" | "bun.lock" => lockfile(f, &file.rel, "bun"),
        "poetry.lock" => lockfile(f, &file.rel, "poetry"),
        "uv.lock" => lockfile(f, &file.rel, "uv"),
        "Pipfile.lock" => lockfile(f, &file.rel, "pipenv"),
        "Cargo.lock" => lockfile(f, &file.rel, "cargo"),
        _ => f.warn(
            &file.rel,
            format!(
                "Not parsed: {} bytes exceeds the size limit or is not UTF-8.",
                file.len
            ),
        ),
    }
}

fn manifest(f: &mut Findings, rel: &str, kind: &str) {
    f.push(
        Fact::new(
            Category::Manifest,
            kind,
            rel,
            format!("{kind} present"),
            1.0,
        )
        .at(rel, None),
    );
}

fn lockfile(f: &mut Findings, rel: &str, manager: &str) {
    f.push(Fact::new(Category::Lockfile, manager, rel, "lockfile present", 1.0).at(rel, None));
    f.push(
        Fact::new(
            Category::PackageManager,
            manager,
            "detected",
            format!("{} lockfile", name_of(rel)),
            0.9,
        )
        .at(rel, None),
    );
}

fn package_json(f: &mut Findings, rel: &str, text: &str) {
    manifest(f, rel, "package.json");
    let json: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            f.warn(rel, format!("Malformed package.json, not analysed: {e}"));
            return;
        }
    };
    let Some(obj) = json.as_object() else {
        f.warn(rel, "package.json is not a JSON object.");
        return;
    };
    if let Some(n) = obj.get("name").and_then(Value::as_str) {
        f.push(
            Fact::new(
                Category::Metadata,
                "package.name",
                n,
                "package.json name",
                1.0,
            )
            .at(rel, json_key_line(text, None, "name"))
            .pointer("/name"),
        );
    }
    if let Some(node) = obj
        .get("engines")
        .and_then(|e| e.get("node"))
        .and_then(Value::as_str)
    {
        f.push(
            Fact::new(Category::Runtime, "node", node, "package.json engines", 0.9)
                .at(rel, json_key_line(text, Some("engines"), "node"))
                .pointer("/engines/node"),
        );
    }
    if let Some(pm) = obj.get("packageManager").and_then(Value::as_str) {
        let (manager, version) = pm.split_once('@').unwrap_or((pm, "unspecified"));
        let version = version.split('+').next().unwrap_or(version);
        f.push(
            Fact::new(
                Category::PackageManager,
                manager,
                version,
                "package.json packageManager",
                0.95,
            )
            .at(rel, json_key_line(text, None, "packageManager"))
            .pointer("/packageManager"),
        );
    }
    if let Some(node) = obj
        .get("volta")
        .and_then(|v| v.get("node"))
        .and_then(Value::as_str)
    {
        f.push(
            Fact::new(Category::Runtime, "node", node, "Volta", 0.95)
                .at(rel, json_key_line(text, Some("volta"), "node"))
                .pointer("/volta/node"),
        );
    }
    if let Some(scripts) = obj.get("scripts").and_then(Value::as_object) {
        for (k, v) in scripts {
            if let Some(cmd) = v.as_str() {
                f.push(
                    Fact::new(Category::Script, k, cmd, "package.json scripts", 1.0)
                        .at(rel, json_key_line(text, Some("scripts"), k))
                        .pointer(format!("/scripts/{k}")),
                );
            }
        }
    }
    let workspaces = obj.get("workspaces").and_then(|w| {
        w.as_array()
            .or_else(|| w.get("packages").and_then(Value::as_array))
    });
    if let Some(ws) = workspaces {
        let patterns: Vec<&str> = ws.iter().filter_map(Value::as_str).collect();
        f.push(
            Fact::new(
                Category::Workspace,
                "npm_workspaces",
                patterns.join(", "),
                "package.json workspaces",
                0.95,
            )
            .at(rel, json_key_line(text, None, "workspaces"))
            .pointer("/workspaces"),
        );
    }
    for section in ["dependencies", "devDependencies", "peerDependencies"] {
        let Some(deps) = obj.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (dep, version) in deps {
            let version = version.as_str().unwrap_or("");
            let line = json_key_line(text, Some(section), dep);
            if section != "peerDependencies" {
                if let Some((_, label)) = JS_FRAMEWORKS.iter().find(|(n, _)| n == dep) {
                    f.push(
                        Fact::new(
                            Category::Framework,
                            *label,
                            version,
                            format!("package.json {section}"),
                            0.95,
                        )
                        .at(rel, line)
                        .pointer(format!("/{section}/{dep}")),
                    );
                }
            }
            f.push(
                Fact::new(
                    Category::Dependency,
                    dep,
                    version,
                    format!("package.json {section}"),
                    1.0,
                )
                .at(rel, line)
                .pointer(format!("/{section}/{dep}")),
            );
        }
    }
}

fn tool_versions(f: &mut Findings, rel: &str, text: &str) {
    for (i, line) in text.lines().enumerate() {
        let mut parts = line.split_whitespace();
        let (Some(tool), Some(version)) = (parts.next(), parts.next()) else {
            continue;
        };
        let runtime = match tool {
            "nodejs" | "node" => "node",
            "python" => "python",
            "java" => "java",
            "golang" | "go" => "go",
            "rust" => "rust",
            "dotnet" | "dotnet-core" => "dotnet",
            _ => continue,
        };
        f.push(
            Fact::new(Category::Runtime, runtime, version, ".tool-versions", 0.95)
                .at(rel, Some(i as u32 + 1)),
        );
    }
}

fn pnpm_workspace(f: &mut Findings, rel: &str, text: &str) {
    let mut in_packages = false;
    let mut patterns = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("packages:") {
            in_packages = true;
            continue;
        }
        if in_packages {
            let t = line.trim();
            if let Some(p) = t.strip_prefix("- ") {
                patterns.push(p.trim_matches(['"', '\'']).to_string());
            } else if !t.is_empty() && !line.starts_with(' ') {
                break;
            }
        }
    }
    f.push(
        Fact::new(
            Category::Workspace,
            "pnpm_workspace",
            patterns.join(", "),
            "pnpm-workspace.yaml",
            0.95,
        )
        .at(rel, line_of(text, "packages:", 1)),
    );
    f.push(
        Fact::new(
            Category::PackageManager,
            "pnpm",
            "detected",
            "pnpm-workspace.yaml",
            0.9,
        )
        .at(rel, None),
    );
}

fn pep_name(spec: &str) -> &str {
    let end = spec
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .unwrap_or(spec.len());
    &spec[..end]
}

fn python_dependency(f: &mut Findings, rel: &str, spec: &str, line: Option<u32>, method: &str) {
    let name = pep_name(spec.trim());
    if name.is_empty() {
        return;
    }
    let version = spec.trim()[name.len()..].trim();
    let lname = name.to_ascii_lowercase();
    if let Some((_, label)) = PY_FRAMEWORKS.iter().find(|(n, _)| *n == lname) {
        f.push(Fact::new(Category::Framework, *label, version, method, 0.95).at(rel, line));
    }
    f.push(Fact::new(Category::Dependency, name, version, method, 1.0).at(rel, line));
}

fn pyproject(f: &mut Findings, rel: &str, text: &str) {
    manifest(f, rel, "pyproject.toml");
    let doc: toml::Table = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            f.warn(
                rel,
                format!("Malformed pyproject.toml, not analysed: {}", e.message()),
            );
            return;
        }
    };
    if let Some(project) = doc.get("project").and_then(|v| v.as_table()) {
        if let Some(rp) = project.get("requires-python").and_then(|v| v.as_str()) {
            f.push(
                Fact::new(
                    Category::Runtime,
                    "python",
                    rp,
                    "pyproject requires-python",
                    0.9,
                )
                .at(rel, line_of(text, "requires-python", 1))
                .pointer("/project/requires-python"),
            );
        }
        if let Some(deps) = project.get("dependencies").and_then(|v| v.as_array()) {
            for d in deps.iter().filter_map(|d| d.as_str()) {
                python_dependency(f, rel, d, line_of(text, d, 1), "pyproject dependencies");
            }
        }
    }
    if let Some(poetry) = doc
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|v| v.as_table())
    {
        f.push(
            Fact::new(
                Category::PackageManager,
                "poetry",
                "detected",
                "pyproject [tool.poetry]",
                0.9,
            )
            .at(rel, line_of(text, "[tool.poetry", 1)),
        );
        if let Some(deps) = poetry.get("dependencies").and_then(|v| v.as_table()) {
            for (name, spec) in deps {
                let version = spec
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| {
                        spec.get("version")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
                if name == "python" {
                    f.push(
                        Fact::new(
                            Category::Runtime,
                            "python",
                            version,
                            "pyproject [tool.poetry.dependencies]",
                            0.9,
                        )
                        .at(rel, line_of(text, "python =", 1)),
                    );
                } else {
                    python_dependency(
                        f,
                        rel,
                        &format!("{name}{version}"),
                        line_of(text, name, 1),
                        "pyproject poetry dependencies",
                    );
                }
            }
        }
    }
    if doc.get("tool").and_then(|t| t.get("pytest")).is_some() {
        f.push(
            Fact::new(
                Category::Framework,
                "pytest",
                "configured",
                "pyproject [tool.pytest]",
                0.9,
            )
            .at(rel, line_of(text, "[tool.pytest", 1)),
        );
    }
}

fn requirements(f: &mut Findings, rel: &str, text: &str) {
    manifest(f, rel, "requirements");
    f.push(
        Fact::new(
            Category::PackageManager,
            "pip",
            "detected",
            name_of(rel),
            0.85,
        )
        .at(rel, None),
    );
    for (i, line) in text.lines().enumerate() {
        let l = line.split('#').next().unwrap_or("").trim();
        if l.is_empty() || l.starts_with('-') {
            continue;
        }
        python_dependency(f, rel, l, Some(i as u32 + 1), "requirements file");
    }
}

/// Text between `<tag>` and `</tag>` on one line, with its line number.
fn xml_values<'a>(text: &'a str, tag: &str) -> Vec<(u32, &'a str)> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let start = l.find(&open)? + open.len();
            let end = start + l[start..].find(&close)?;
            Some((i as u32 + 1, l[start..end].trim()))
        })
        .collect()
}

fn pom(f: &mut Findings, rel: &str, text: &str) {
    manifest(f, rel, "pom.xml");
    f.push(
        Fact::new(
            Category::PackageManager,
            "maven",
            "detected",
            "pom.xml",
            0.9,
        )
        .at(rel, None),
    );
    for tag in [
        "java.version",
        "maven.compiler.release",
        "maven.compiler.source",
    ] {
        if let Some((line, v)) = xml_values(text, tag).into_iter().next() {
            f.push(
                Fact::new(
                    Category::Runtime,
                    "java",
                    v,
                    format!("pom.xml <{tag}>"),
                    0.9,
                )
                .at(rel, Some(line)),
            );
            break;
        }
    }
    if let Some(parent_line) = line_of(
        text,
        "<artifactId>spring-boot-starter-parent</artifactId>",
        1,
    ) {
        let version = xml_values(text, "version")
            .into_iter()
            .find(|(l, _)| *l > parent_line)
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        f.push(
            Fact::new(
                Category::Framework,
                "Spring Boot",
                version,
                "pom.xml parent",
                0.95,
            )
            .at(rel, Some(parent_line)),
        );
    } else if let Some(line) = line_of(text, "<artifactId>spring-boot-starter", 1) {
        f.push(
            Fact::new(
                Category::Framework,
                "Spring Boot",
                "",
                "pom.xml dependency",
                0.85,
            )
            .at(rel, Some(line)),
        );
    }
    for (line, artifact) in xml_values(text, "artifactId").into_iter().skip(1) {
        f.push(
            Fact::new(
                Category::Dependency,
                artifact,
                "",
                "pom.xml artifactId",
                0.8,
            )
            .at(rel, Some(line)),
        );
    }
}

fn gradle(f: &mut Findings, rel: &str, text: &str) {
    manifest(f, rel, name_of(rel));
    f.push(
        Fact::new(
            Category::PackageManager,
            "gradle",
            "detected",
            name_of(rel),
            0.9,
        )
        .at(rel, None),
    );
    for (i, line) in text.lines().enumerate() {
        let l = line.trim();
        let n = Some(i as u32 + 1);
        if let Some(v) = l
            .split("JavaLanguageVersion.of(")
            .nth(1)
            .and_then(|r| r.split(')').next())
        {
            f.push(
                Fact::new(Category::Runtime, "java", v.trim(), "Gradle toolchain", 0.9).at(rel, n),
            );
        } else if let Some(v) = l.split("JavaVersion.VERSION_").nth(1) {
            let v: String = v
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '_')
                .collect();
            f.push(
                Fact::new(
                    Category::Runtime,
                    "java",
                    v.replace('_', "."),
                    "Gradle sourceCompatibility",
                    0.85,
                )
                .at(rel, n),
            );
        }
        if l.contains("org.springframework.boot") {
            let version = l
                .split("version")
                .nth(1)
                .map(|v| v.trim().trim_matches(['"', '\'', ' ']).to_string())
                .unwrap_or_default();
            f.push(
                Fact::new(
                    Category::Framework,
                    "Spring Boot",
                    version,
                    "Gradle plugin",
                    0.9,
                )
                .at(rel, n),
            );
        }
    }
}

fn global_json(f: &mut Findings, rel: &str, text: &str) {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => {
            if let Some(sdk) = v
                .get("sdk")
                .and_then(|s| s.get("version"))
                .and_then(Value::as_str)
            {
                f.push(
                    Fact::new(Category::Runtime, "dotnet", sdk, "global.json sdk", 0.9)
                        .at(rel, json_key_line(text, Some("sdk"), "version"))
                        .pointer("/sdk/version"),
                );
            }
        }
        Err(e) => f.warn(rel, format!("Malformed global.json: {e}")),
    }
}

fn target_frameworks(f: &mut Findings, rel: &str, text: &str) {
    for tag in ["TargetFramework", "TargetFrameworks"] {
        for (line, v) in xml_values(text, tag) {
            for tfm in v.split(';').filter(|s| !s.is_empty()) {
                f.push(
                    Fact::new(Category::Runtime, "dotnet", tfm, format!("<{tag}>"), 0.95)
                        .at(rel, Some(line)),
                );
            }
        }
    }
}

fn csproj(f: &mut Findings, rel: &str, text: &str) {
    manifest(f, rel, "project");
    f.push(
        Fact::new(
            Category::PackageManager,
            "nuget",
            "detected",
            "SDK-style project",
            0.85,
        )
        .at(rel, None),
    );
    target_frameworks(f, rel, text);
    if text.contains("Sdk=\"Microsoft.NET.Sdk.Web\"") {
        f.push(
            Fact::new(
                Category::Framework,
                "ASP.NET Core",
                "",
                "Microsoft.NET.Sdk.Web",
                0.95,
            )
            .at(rel, line_of(text, "Microsoft.NET.Sdk.Web", 1)),
        );
    }
    for (i, line) in text.lines().enumerate() {
        if let Some(rest) = line.split("<PackageReference Include=\"").nth(1) {
            let name = rest.split('"').next().unwrap_or("");
            let version = line
                .split("Version=\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .unwrap_or("");
            f.push(
                Fact::new(Category::Dependency, name, version, "PackageReference", 1.0)
                    .at(rel, Some(i as u32 + 1)),
            );
        }
    }
}

fn go_mod(f: &mut Findings, rel: &str, text: &str) {
    manifest(f, rel, "go.mod");
    f.push(
        Fact::new(
            Category::PackageManager,
            "go_modules",
            "detected",
            "go.mod",
            0.95,
        )
        .at(rel, None),
    );
    let mut in_require = false;
    for (i, line) in text.lines().enumerate() {
        let l = line.trim();
        let n = Some(i as u32 + 1);
        if let Some(m) = l.strip_prefix("module ") {
            f.push(
                Fact::new(
                    Category::Metadata,
                    "go.module",
                    m.trim(),
                    "go.mod module",
                    1.0,
                )
                .at(rel, n),
            );
        } else if let Some(v) = l.strip_prefix("go ") {
            f.push(
                Fact::new(
                    Category::Runtime,
                    "go",
                    v.trim(),
                    "go.mod go directive",
                    0.95,
                )
                .at(rel, n),
            );
        } else if let Some(v) = l.strip_prefix("toolchain go") {
            f.push(
                Fact::new(Category::Runtime, "go", v.trim(), "go.mod toolchain", 0.9).at(rel, n),
            );
        } else if l.starts_with("require (") {
            in_require = true;
        } else if in_require && l == ")" {
            in_require = false;
        } else if in_require || l.starts_with("require ") {
            let spec = l
                .trim_start_matches("require ")
                .split("//")
                .next()
                .unwrap_or("")
                .trim();
            let mut parts = spec.split_whitespace();
            if let (Some(module), Some(version)) = (parts.next(), parts.next()) {
                if let Some((_, label)) = GO_FRAMEWORKS.iter().find(|(m, _)| *m == module) {
                    f.push(
                        Fact::new(Category::Framework, *label, version, "go.mod require", 0.95)
                            .at(rel, n),
                    );
                }
                f.push(
                    Fact::new(Category::Dependency, module, version, "go.mod require", 1.0)
                        .at(rel, n),
                );
            }
        }
    }
}

fn go_work(f: &mut Findings, rel: &str, text: &str) {
    let uses: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("./") || l.starts_with("use ./"))
        .map(|l| l.trim_start_matches("use ").to_string())
        .collect();
    f.push(
        Fact::new(
            Category::Workspace,
            "go_work",
            uses.join(", "),
            "go.work",
            0.95,
        )
        .at(rel, line_of(text, "use", 1)),
    );
}

fn cargo_toml(f: &mut Findings, rel: &str, text: &str) {
    manifest(f, rel, "Cargo.toml");
    f.push(
        Fact::new(
            Category::PackageManager,
            "cargo",
            "detected",
            "Cargo.toml",
            0.95,
        )
        .at(rel, None),
    );
    let doc: toml::Table = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            f.warn(
                rel,
                format!("Malformed Cargo.toml, not analysed: {}", e.message()),
            );
            return;
        }
    };
    if let Some(ws) = doc.get("workspace").and_then(|v| v.as_table()) {
        let members: Vec<String> = ws
            .get("members")
            .and_then(|m| m.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        f.push(
            Fact::new(
                Category::Workspace,
                "cargo_workspace",
                members.join(", "),
                "Cargo.toml [workspace]",
                0.95,
            )
            .at(rel, line_of(text, "[workspace]", 1)),
        );
    }
    if let Some(pkg) = doc.get("package").and_then(|v| v.as_table()) {
        if let Some(rv) = pkg.get("rust-version").and_then(|v| v.as_str()) {
            f.push(
                Fact::new(
                    Category::Runtime,
                    "rust",
                    rv,
                    "Cargo.toml rust-version",
                    0.9,
                )
                .at(rel, line_of(text, "rust-version", 1)),
            );
        }
        if let Some(ed) = pkg.get("edition").and_then(|v| v.as_str()) {
            f.push(
                Fact::new(
                    Category::Metadata,
                    "rust.edition",
                    ed,
                    "Cargo.toml edition",
                    1.0,
                )
                .at(rel, line_of(text, "edition", 1)),
            );
        }
    }
    for section in ["dependencies", "dev-dependencies"] {
        let Some(deps) = doc.get(section).and_then(|v| v.as_table()) else {
            continue;
        };
        for (name, spec) in deps {
            let version = spec
                .as_str()
                .map(str::to_string)
                .or_else(|| {
                    spec.get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let line = line_of(text, &format!("{name} "), 1).or_else(|| line_of(text, name, 1));
            if let Some((_, label)) = RUST_FRAMEWORKS.iter().find(|(n, _)| n == name) {
                f.push(
                    Fact::new(
                        Category::Framework,
                        *label,
                        version.clone(),
                        format!("Cargo.toml [{section}]"),
                        0.95,
                    )
                    .at(rel, line),
                );
            }
            f.push(
                Fact::new(
                    Category::Dependency,
                    name,
                    version,
                    format!("Cargo.toml [{section}]"),
                    1.0,
                )
                .at(rel, line),
            );
        }
    }
}

fn rust_toolchain(f: &mut Findings, rel: &str, text: &str) {
    let channel = if rel.ends_with(".toml") {
        text.parse::<toml::Table>().ok().and_then(|d| {
            d.get("toolchain")
                .and_then(|t| t.get("channel"))
                .and_then(|c| c.as_str().map(str::to_string))
        })
    } else {
        text.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
    };
    if let Some(c) = channel {
        f.push(
            Fact::new(Category::Runtime, "rust", c, name_of(rel), 0.95)
                .at(rel, line_of(text, "channel", 1).or(Some(1))),
        );
    }
}

/// Runtime implied by a container base image, e.g. `node:20-alpine`.
fn image_runtime(image: &str) -> Option<(&'static str, String)> {
    let (repo, tag) = image.rsplit_once(':')?;
    let base = repo.rsplit('/').next().unwrap_or(repo);
    let runtime = match base {
        "node" => "node",
        "python" => "python",
        "golang" => "go",
        "rust" => "rust",
        "openjdk" | "eclipse-temurin" | "amazoncorretto" => "java",
        "sdk" | "aspnet" | "runtime" if repo.contains("dotnet") => "dotnet",
        _ => return None,
    };
    Some((runtime, tag.to_string()))
}

fn dockerfile(f: &mut Findings, rel: &str, text: &str) {
    f.push(
        Fact::new(
            Category::Docker,
            "dockerfile",
            rel,
            "Dockerfile present",
            1.0,
        )
        .at(rel, None),
    );
    for (i, line) in text.lines().enumerate() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("FROM ").or_else(|| l.strip_prefix("from ")) else {
            continue;
        };
        let image = rest
            .split_whitespace()
            .find(|t| !t.starts_with("--"))
            .unwrap_or("");
        if image.is_empty() || image.contains('$') {
            continue;
        }
        let n = Some(i as u32 + 1);
        f.push(
            Fact::new(
                Category::Docker,
                "base_image",
                image,
                "Dockerfile FROM",
                1.0,
            )
            .at(rel, n),
        );
        if let Some((runtime, tag)) = image_runtime(image) {
            f.push(Fact::new(Category::Runtime, runtime, tag, "Dockerfile FROM", 0.8).at(rel, n));
        }
    }
}

fn images(f: &mut Findings, rel: &str, text: &str, category: Category, key: &str) {
    for (i, line) in text.lines().enumerate() {
        if let Some(v) = line.trim().strip_prefix("image:") {
            let image = v.trim().trim_matches(['"', '\'']);
            if !image.is_empty() && !image.contains('$') {
                f.push(
                    Fact::new(category, key, image, "image declaration", 1.0)
                        .at(rel, Some(i as u32 + 1)),
                );
                if let Some((runtime, tag)) = image_runtime(image) {
                    f.push(
                        Fact::new(Category::Runtime, runtime, tag, key.to_string(), 0.75)
                            .at(rel, Some(i as u32 + 1)),
                    );
                }
            }
        }
    }
}

const SETUP_ACTIONS: &[(&str, &str, &str)] = &[
    ("actions/setup-node", "node-version", "node"),
    ("actions/setup-python", "python-version", "python"),
    ("actions/setup-java", "java-version", "java"),
    ("actions/setup-go", "go-version", "go"),
    ("actions/setup-dotnet", "dotnet-version", "dotnet"),
];

/// Values of a matrix entry such as `node: [18, 20]` or `node-version: [18.x, 20.x]`.
fn matrix_values(text: &str, key: &str) -> Vec<(u32, String)> {
    let needle = format!("{key}:");
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let rest = l.trim().strip_prefix(&needle)?.trim();
            let inner = rest.strip_prefix('[')?.strip_suffix(']')?;
            Some(
                inner
                    .split(',')
                    .map(|v| v.trim().trim_matches(['"', '\'']).to_string())
                    .filter(|v| !v.is_empty())
                    .map(|v| (i as u32 + 1, v))
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

fn github_workflow(f: &mut Findings, rel: &str, text: &str) {
    f.push(Fact::new(Category::Ci, "github_actions", rel, "workflow file", 1.0).at(rel, None));
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some((action, input, runtime)) = SETUP_ACTIONS
            .iter()
            .find(|(a, _, _)| line.contains(&format!("uses: {a}@")))
        else {
            continue;
        };
        f.push(
            Fact::new(
                Category::Ci,
                *action,
                line.trim()
                    .trim_start_matches("- ")
                    .trim_start_matches("uses: "),
                "workflow step",
                1.0,
            )
            .at(rel, Some(i as u32 + 1)),
        );
        let Some((j, value)) = lines
            .iter()
            .enumerate()
            .skip(i + 1)
            .take(8)
            .find_map(|(j, l)| {
                l.trim()
                    .strip_prefix(&format!("{input}:"))
                    .map(|v| (j, v.trim().trim_matches(['"', '\'']).to_string()))
            })
        else {
            continue;
        };
        if let Some(var) = value
            .strip_prefix("${{")
            .and_then(|v| v.strip_suffix("}}"))
            .map(str::trim)
        {
            let key = var.strip_prefix("matrix.").unwrap_or(var);
            let values = matrix_values(text, key);
            if values.is_empty() {
                f.warn(
                    rel,
                    format!("{input} uses `{var}`, which could not be resolved."),
                );
            }
            for (line, v) in values {
                f.push(
                    Fact::new(Category::Runtime, *runtime, v, "GitHub Actions matrix", 0.7)
                        .at(rel, Some(line))
                        .pointer(format!("matrix.{key}")),
                );
            }
        } else if !value.is_empty() {
            f.push(
                Fact::new(
                    Category::Runtime,
                    *runtime,
                    value,
                    format!("GitHub Actions {action}"),
                    0.75,
                )
                .at(rel, Some(j as u32 + 1)),
            );
        }
    }
}

/// Comparable key for an exact runtime declaration, or `None` for ranges and aliases.
fn exact_key(runtime: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.starts_with(['>', '<', '^', '~', '=']) || v.contains("||") || v.contains(' ') {
        return None;
    }
    match runtime {
        "node" | "java" => semver::major_of(v).map(|m| m.to_string()),
        "dotnet" => {
            let n = v.trim_start_matches("net").trim_start_matches("coreapp");
            semver::major_of(n).map(|m| m.to_string())
        }
        "python" | "go" => {
            let digits: Vec<&str> = v.split(['-', '+']).next()?.split('.').take(2).collect();
            (digits.len() == 2
                && digits
                    .iter()
                    .all(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())))
            .then(|| digits.join("."))
        }
        _ => None,
    }
}

/// Directory a declaration applies to. CI configuration applies to the whole repository.
fn scope_of(fact: &Fact) -> String {
    let file = fact.file.as_deref().unwrap_or("");
    if file.starts_with(".github/") || file.starts_with(".circleci/") || file == ".gitlab-ci.yml" {
        String::new()
    } else {
        dir_of(file).to_string()
    }
}

/// `ancestor` is the same directory as `dir` or one of its parents.
fn applies_to(ancestor: &str, dir: &str) -> bool {
    ancestor.is_empty() || ancestor == dir || dir.starts_with(&format!("{ancestor}/"))
}

fn runtime_conflicts(f: &mut Findings) {
    let runtimes: BTreeSet<String> = f
        .facts
        .iter()
        .filter(|x| x.category == Category::Runtime)
        .map(|x| x.key.clone())
        .collect();
    let mut conflicts = Vec::new();
    for runtime in runtimes {
        let decls: Vec<&Fact> = f
            .facts
            .iter()
            .filter(|x| x.category == Category::Runtime && x.key == runtime)
            .collect();
        // Exact declarations in the same directory must agree. Matrix entries
        // test several versions on purpose and are not compared.
        let mut by_scope: BTreeMap<String, BTreeMap<String, Vec<&Fact>>> = BTreeMap::new();
        for d in decls.iter().filter(|d| d.method != "GitHub Actions matrix") {
            if let Some(k) = exact_key(&runtime, &d.value) {
                by_scope
                    .entry(scope_of(d))
                    .or_default()
                    .entry(k)
                    .or_default()
                    .push(d);
            }
        }
        for (scope, exact) in by_scope {
            if exact.len() < 2 {
                continue;
            }
            let summary: Vec<String> = exact
                .iter()
                .map(|(k, ds)| {
                    let files: BTreeSet<String> = ds
                        .iter()
                        .map(|d| d.file.clone().unwrap_or_default())
                        .collect();
                    format!("{k} ({})", files.into_iter().collect::<Vec<_>>().join(", "))
                })
                .collect();
            let place = if scope.is_empty() {
                String::new()
            } else {
                format!(" in {scope}")
            };
            conflicts.push(Conflict {
                subject: format!("runtime.{runtime}"),
                message: format!(
                    "{runtime} versions disagree{place}: {}",
                    summary.join(" vs ")
                ),
                values: exact
                    .values()
                    .flatten()
                    .map(|d| ConflictValue::from_fact(d))
                    .collect(),
            });
        }
        // An engines range must admit every exact declaration that applies to its package.
        if runtime == "node" {
            for range in decls.iter().filter(|d| d.method == "package.json engines") {
                let package_dir = scope_of(range);
                for d in decls.iter().filter(|d| d.method != "package.json engines") {
                    if !applies_to(&scope_of(d), &package_dir) {
                        continue;
                    }
                    let Some(major) = semver::major_of(&d.value)
                        .filter(|_| exact_key("node", &d.value).is_some())
                    else {
                        continue;
                    };
                    if semver::major_can_satisfy(major, &range.value) == Some(false) {
                        conflicts.push(Conflict {
                            subject: "runtime.node".into(),
                            message: format!(
                                "engines.node \"{}\" in {} excludes Node {} declared by {}",
                                range.value,
                                range.file.clone().unwrap_or_default(),
                                d.value,
                                d.method
                            ),
                            values: vec![
                                ConflictValue::from_fact(range),
                                ConflictValue::from_fact(d),
                            ],
                        });
                    }
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    conflicts.retain(|c| seen.insert(c.message.clone()));
    f.conflicts.extend(conflicts);
}

fn package_manager_conflicts(f: &mut Findings) {
    // Compare managers declared at the same directory level.
    let js = ["npm", "pnpm", "yarn", "bun"];
    let mut by_dir: BTreeMap<String, Vec<&Fact>> = BTreeMap::new();
    for fact in f
        .facts
        .iter()
        .filter(|x| x.category == Category::PackageManager && js.contains(&x.key.as_str()))
    {
        if fact.method == "pnpm-workspace.yaml" {
            continue;
        }
        by_dir
            .entry(dir_of(fact.file.as_deref().unwrap_or("")).to_string())
            .or_default()
            .push(fact);
    }
    let mut conflicts = Vec::new();
    for (dir, facts) in by_dir {
        let managers: BTreeSet<&str> = facts.iter().map(|x| x.key.as_str()).collect();
        if managers.len() > 1 {
            conflicts.push(Conflict {
                subject: "package_manager".into(),
                message: format!(
                    "Multiple JavaScript package managers declared in {}: {}",
                    if dir.is_empty() {
                        "the repository root"
                    } else {
                        &dir
                    },
                    managers.into_iter().collect::<Vec<_>>().join(", ")
                ),
                values: facts.iter().map(|x| ConflictValue::from_fact(x)).collect(),
            });
        }
    }
    f.conflicts.extend(conflicts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(rel: &str, text: &str) -> RelevantFile {
        RelevantFile {
            rel: rel.into(),
            text: Some(text.into()),
            len: text.len() as u64,
        }
    }

    fn find<'a>(f: &'a Findings, cat: Category, key: &str) -> Vec<&'a Fact> {
        f.facts
            .iter()
            .filter(|x| x.category == cat && x.key == key)
            .collect()
    }

    #[test]
    fn package_json_facts_carry_evidence() {
        let pj = "{\n  \"name\": \"api\",\n  \"engines\": {\n    \"node\": \">=18\"\n  },\n  \"packageManager\": \"pnpm@9.12.0+sha512.abc\",\n  \"scripts\": {\n    \"test\": \"vitest run\"\n  },\n  \"dependencies\": {\n    \"express\": \"^4.19.0\"\n  }\n}\n";
        let f = detect(&[file("package.json", pj)]);
        let node = find(&f, Category::Runtime, "node")[0];
        assert_eq!(
            (node.value.as_str(), node.line, node.pointer.as_deref()),
            (">=18", Some(4), Some("/engines/node"))
        );
        let pm = find(&f, Category::PackageManager, "pnpm")[0];
        assert_eq!(pm.value, "9.12.0");
        assert_eq!(find(&f, Category::Script, "test")[0].line, Some(8));
        let express = find(&f, Category::Framework, "Express")[0];
        assert_eq!(
            (express.value.as_str(), express.line),
            ("^4.19.0", Some(11))
        );
        assert!(f.conflicts.is_empty());
    }

    #[test]
    fn malformed_package_json_is_a_warning_not_a_failure() {
        let f = detect(&[file("package.json", "{ \"name\": ")]);
        assert_eq!(f.warnings.len(), 1);
        assert!(f.warnings[0].message.contains("Malformed package.json"));
        assert_eq!(find(&f, Category::Manifest, "package.json").len(), 1);
    }

    #[test]
    fn node_runtime_conflicts_are_explicit() {
        let f = detect(&[
            file("package.json", r#"{"engines":{"node":">=20"}}"#),
            file(".nvmrc", "18\n"),
            file("Dockerfile", "FROM node:20-alpine\n"),
        ]);
        let messages: Vec<&str> = f.conflicts.iter().map(|c| c.message.as_str()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.contains("node versions disagree")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("excludes Node 18")),
            "{messages:?}"
        );
        assert!(!messages.iter().any(|m| m.contains("excludes Node 20")));
    }

    #[test]
    fn ci_matrix_is_not_a_conflict_by_itself() {
        let wf = "jobs:\n  t:\n    strategy:\n      matrix:\n        node: [18, 20]\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n          node-version: ${{ matrix.node }}\n";
        let f = detect(&[file(".github/workflows/ci.yml", wf), file(".nvmrc", "20")]);
        let ci: Vec<_> = find(&f, Category::Runtime, "node")
            .into_iter()
            .filter(|x| x.method == "GitHub Actions matrix")
            .collect();
        assert_eq!(ci.len(), 2);
        assert_eq!(ci[0].line, Some(5));
        assert!(f.conflicts.is_empty(), "{:?}", f.conflicts);
    }

    #[test]
    fn conflicts_are_scoped_to_the_package_they_apply_to() {
        let f = detect(&[
            file("services/a/package.json", r#"{"engines":{"node":">=20"}}"#),
            file("services/b/.nvmrc", "18"),
            file("services/b/Dockerfile", "FROM node:22\n"),
            file(".nvmrc", "16"),
        ]);
        let messages: Vec<&str> = f.conflicts.iter().map(|c| c.message.as_str()).collect();
        // b's own declarations disagree; root .nvmrc 16 applies to a and is excluded by a's engines.
        assert!(
            messages
                .iter()
                .any(|m| m.contains("node versions disagree in services/b")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("excludes Node 16")),
            "{messages:?}"
        );
        assert!(
            !messages.iter().any(|m| m.contains("excludes Node 18")),
            "b's .nvmrc does not apply to a: {messages:?}"
        );
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn multiple_lockfiles_conflict() {
        let f = detect(&[
            file("package-lock.json", "{}"),
            file("yarn.lock", ""),
            file("packages/a/pnpm-lock.yaml", ""),
        ]);
        assert_eq!(f.conflicts.len(), 1);
        assert!(f.conflicts[0].message.contains("npm, yarn"));
    }

    #[test]
    fn python_java_dotnet_go_rust_metadata() {
        let f = detect(&[
            file("pyproject.toml", "[project]\nrequires-python = \">=3.11\"\ndependencies = [\"fastapi>=0.110\", \"uvicorn[standard]\"]\n"),
            file("pom.xml", "<project>\n<parent>\n<artifactId>spring-boot-starter-parent</artifactId>\n<version>3.3.0</version>\n</parent>\n<properties><java.version>21</java.version></properties>\n</project>"),
            file("api/Api.csproj", "<Project Sdk=\"Microsoft.NET.Sdk.Web\">\n<PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>\n<ItemGroup><PackageReference Include=\"Serilog\" Version=\"3.1.1\" /></ItemGroup>\n</Project>"),
            file("go.mod", "module example.com/svc\n\ngo 1.22\n\nrequire (\n\tgithub.com/gin-gonic/gin v1.10.0\n)\n"),
            file("Cargo.toml", "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\n"),
        ]);
        assert_eq!(find(&f, Category::Framework, "FastAPI")[0].value, ">=0.110");
        assert_eq!(find(&f, Category::Runtime, "python")[0].value, ">=3.11");
        assert_eq!(
            find(&f, Category::Framework, "Spring Boot")[0].value,
            "3.3.0"
        );
        assert_eq!(find(&f, Category::Runtime, "java")[0].value, "21");
        assert_eq!(find(&f, Category::Runtime, "dotnet")[0].value, "net8.0");
        assert_eq!(find(&f, Category::Framework, "ASP.NET Core").len(), 1);
        assert_eq!(find(&f, Category::Dependency, "Serilog")[0].value, "3.1.1");
        assert_eq!(find(&f, Category::Runtime, "go")[0].value, "1.22");
        assert_eq!(find(&f, Category::Framework, "Gin")[0].line, Some(6));
        assert_eq!(
            find(&f, Category::Workspace, "cargo_workspace")[0].value,
            "crates/a, crates/b"
        );
    }

    #[test]
    fn redacts_secrets_in_values() {
        let f = detect(&[file(
            "package.json",
            r#"{"scripts":{"deploy":"curl -H 'Authorization: Bearer abcdefgh12345678' x"}}"#,
        )]);
        assert!(!find(&f, Category::Script, "deploy")[0]
            .value
            .contains("abcdefgh12345678"));
    }
}
