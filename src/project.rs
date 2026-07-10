//! rikki.toml + rikki.lock + the hidden .rikki/ venv, provisioned by
//! driving uv. The toml and lock fully determine the Python environment;
//! .rikki/ is disposable and regenerates.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub python: String,
    /// the rikki version the project was built against (spec 17.4);
    /// mismatches warn so breaks say "built against 0.1.5" instead of
    /// producing mystifying compile errors
    pub rikki: Option<String>,
    pub py_deps: BTreeMap<String, PyDep>,
}

/// One [py-deps] entry: a version constraint, and optionally the import
/// name the package satisfies when the two differ (mlflow-skinny ->
/// mlflow; spec 17.5). Only the version reaches uv.
#[derive(Debug, Clone, PartialEq)]
pub struct PyDep {
    pub version: String,
    pub module: Option<String>,
}

impl PyDep {
    pub fn any() -> Self {
        PyDep {
            version: "*".into(),
            module: None,
        }
    }
}

impl Project {
    /// Walk up from `start` to the nearest directory holding rikki.toml.
    /// A relative start is resolved against the working directory first, so
    /// the returned root is always absolute (a root of "" breaks every later
    /// Command::current_dir on it).
    pub fn find(start: &Path) -> Option<PathBuf> {
        let start = if start.is_absolute() {
            start.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(start)
        };
        let mut dir = if start.is_dir() {
            start
        } else {
            start.parent()?.to_path_buf()
        };
        loop {
            if dir.join("rikki.toml").exists() {
                return Some(dir);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    pub fn load(root: &Path) -> Result<Project, String> {
        let path = root.join("rikki.toml");
        let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc: toml::Table =
            toml::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = doc
            .get("project")
            .and_then(|p| p.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unnamed")
            .to_string();
        let python = doc
            .get("project")
            .and_then(|p| p.get("python"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(crate::bridge::embedded_python);
        let rikki = doc
            .get("project")
            .and_then(|p| p.get("rikki"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let mut py_deps = BTreeMap::new();
        if let Some(deps) = doc.get("py-deps").and_then(|v| v.as_table()) {
            for (k, v) in deps {
                let dep = match v.as_table() {
                    Some(t) => PyDep {
                        version: t
                            .get("version")
                            .and_then(|x| x.as_str())
                            .unwrap_or("*")
                            .to_string(),
                        module: t
                            .get("module")
                            .and_then(|x| x.as_str())
                            .map(str::to_string),
                    },
                    None => PyDep {
                        version: v.as_str().unwrap_or("*").to_string(),
                        module: None,
                    },
                };
                py_deps.insert(k.clone(), dep);
            }
        }
        Ok(Project {
            root: root.to_path_buf(),
            name,
            python,
            rikki,
            py_deps,
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let s = |v: &str| toml::Value::String(v.to_string());
        let mut project = toml::Table::new();
        project.insert("name".into(), s(&self.name));
        project.insert("python".into(), s(&self.python));
        if let Some(r) = &self.rikki {
            project.insert("rikki".into(), s(r));
        }
        let mut doc = toml::Table::new();
        doc.insert("project".into(), toml::Value::Table(project));
        if !self.py_deps.is_empty() {
            let mut deps = toml::Table::new();
            for (k, v) in &self.py_deps {
                let val = match &v.module {
                    None => s(&v.version),
                    Some(m) => {
                        let mut t = toml::Table::new();
                        t.insert("version".into(), s(&v.version));
                        t.insert("module".into(), s(m));
                        toml::Value::Table(t)
                    }
                };
                deps.insert(k.clone(), val);
            }
            doc.insert("py-deps".into(), toml::Value::Table(deps));
        }
        let out = toml::to_string(&doc).map_err(|e| format!("serialize rikki.toml: {e}"))?;
        std::fs::write(self.root.join("rikki.toml"), out)
            .map_err(|e| format!("write rikki.toml: {e}"))
    }

    pub fn venv(&self) -> PathBuf {
        self.root.join(".rikki").join("venv")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("rikki.lock")
    }

    /// First line of every lock: a fingerprint of exactly the inputs that
    /// determine resolution (python pin + requirement lines). When the
    /// manifest drifts from it, hand edits included, the lock is stale.
    fn manifest_stamp(&self) -> String {
        format!(
            "# rikki-manifest: {}",
            cheap_hash(&format!("{}\n{}", self.python, self.requirement_lines()))
        )
    }

    fn lock_fresh(&self) -> bool {
        match std::fs::read_to_string(self.lock_path()) {
            Ok(lock) => lock.lines().next() == Some(self.manifest_stamp().as_str()),
            // no lock and no deps is the fresh empty state
            Err(_) => self.py_deps.is_empty(),
        }
    }

    fn requirement_lines(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.py_deps {
            if v.version == "*" {
                s.push_str(&format!("{k}\n"));
            } else {
                s.push_str(&format!("{k}{}\n", v.version));
            }
        }
        s
    }

    fn uv(&self, uv_bin: &str, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        use std::io::Write;
        use std::process::Stdio;
        let mut cmd = Command::new(uv_bin);
        cmd.args(args).current_dir(&self.root);
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("uv: {e} (is uv installed?)"))?;
        if let Some(input) = stdin {
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(input.as_bytes())
                .map_err(|e| format!("uv stdin: {e}"))?;
        }
        let out = child.wait_with_output().map_err(|e| format!("uv: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "uv {} failed:\n{}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Regenerate rikki.lock from the declared deps.
    pub fn compile_lock(&self, uv_bin: &str) -> Result<(), String> {
        if self.py_deps.is_empty() {
            let _ = std::fs::remove_file(self.lock_path());
            return Ok(());
        }
        let reqs = self.requirement_lines();
        let lock = self.uv(
            uv_bin,
            &[
                "pip",
                "compile",
                "--python-version",
                &self.python,
                "--no-header",
                "-",
            ],
            Some(&reqs),
        )?;
        let stamped = format!("{}\n{lock}", self.manifest_stamp());
        std::fs::write(self.lock_path(), stamped).map_err(|e| format!("write lock: {e}"))
    }

    /// Create/refresh the venv so it matches the lock. Idempotent: skips work
    /// when the lock hash matches the last sync marker.
    pub fn ensure_env(&self, uv_bin: &str) -> Result<(), String> {
        // the lock follows the manifest: re-resolve whenever [py-deps] or
        // the python pin changed since the lock was written, hand edits
        // included (spec 17.5). compile_lock also clears a lock whose deps
        // were all removed.
        if !self.lock_fresh() {
            self.compile_lock(uv_bin)?;
        }
        if self.py_deps.is_empty() && !self.lock_path().exists() {
            // still need a venv for the pinned interpreter itself
            if !self.venv().exists() {
                self.uv(
                    uv_bin,
                    &["venv", ".rikki/venv", "--python", &self.python],
                    None,
                )?;
            }
            return Ok(());
        }
        let lock = std::fs::read_to_string(self.lock_path())
            .map_err(|_| "rikki.lock missing; run: rikki py add <pkg>".to_string())?;
        let marker = self.root.join(".rikki").join("synced");
        let stamp = format!("{}:{}", self.python, cheap_hash(&lock));
        if self.venv().exists() {
            if let Ok(prev) = std::fs::read_to_string(&marker) {
                if prev == stamp {
                    return Ok(());
                }
            }
            // the lock changed: an in-place sync cannot be trusted across
            // package overlaps (removing full mlflow deletes the mlflow/
            // tree that mlflow-skinny also owns, leaving it half-installed).
            // Rebuild from scratch; uv's cache makes this cheap.
            std::fs::remove_dir_all(self.venv())
                .map_err(|e| format!("rebuild venv: {e}"))?;
        }
        self.uv(
            uv_bin,
            &["venv", ".rikki/venv", "--python", &self.python],
            None,
        )?;
        let py = self.venv().join("bin").join("python");
        self.uv(
            uv_bin,
            &[
                "pip",
                "sync",
                "--python",
                &py.to_string_lossy(),
                "rikki.lock",
            ],
            None,
        )?;
        std::fs::create_dir_all(marker.parent().unwrap()).ok();
        std::fs::write(&marker, stamp).map_err(|e| format!("write sync marker: {e}"))?;
        Ok(())
    }

    pub fn py_add(&mut self, pkg: &str, module: Option<&str>, uv_bin: &str) -> Result<(), String> {
        let dep = self
            .py_deps
            .entry(pkg.to_string())
            .or_insert_with(PyDep::any);
        if let Some(m) = module {
            dep.module = Some(m.to_string());
        }
        self.save()?;
        self.compile_lock(uv_bin)?;
        self.ensure_env(uv_bin)
    }
}

fn cheap_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        crate::testutil::tempdir(&format!("proj-{tag}"))
    }

    /// A fake uv that records its argv lines into uv-calls.log.
    fn fake_uv(dir: &Path) -> String {
        let bin = dir.join("uv");
        std::fs::write(
            &bin,
            "#!/bin/sh\necho \"$@\" >> uv-calls.log\nif [ \"$1\" = pip ] && [ \"$2\" = compile ]; then cat > /dev/null; echo 'torch==2.9.0'; fi\nif [ \"$1\" = venv ]; then mkdir -p .rikki/venv/bin; fi\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin.to_string_lossy().to_string()
    }

    #[test]
    fn toml_roundtrip() {
        let d = tempdir("toml");
        let p = Project {
            root: d.clone(),
            name: "hello".into(),
            python: "3.12".into(),
            rikki: None,
            py_deps: [("torch".to_string(), PyDep::any())].into(),
        };
        p.save().unwrap();
        let q = Project::load(&d).unwrap();
        assert_eq!(q.name, "hello");
        assert_eq!(q.python, "3.12");
        assert_eq!(q.py_deps.get("torch"), Some(&PyDep::any()));
    }

    #[test]
    fn toml_roundtrip_dotted_dep() {
        // PyPI has dotted package names (ruamel.yaml); a bare key would
        // parse back as a nested table and corrupt the dep list
        let d = tempdir("toml-dot");
        let p = Project {
            root: d.clone(),
            name: "hello".into(),
            python: "3.12".into(),
            rikki: None,
            py_deps: [(
                "ruamel.yaml".to_string(),
                PyDep {
                    version: "0.18".into(),
                    module: None,
                },
            )]
            .into(),
        };
        p.save().unwrap();
        let q = Project::load(&d).unwrap();
        assert_eq!(
            q.py_deps.get("ruamel.yaml").map(|d| d.version.as_str()),
            Some("0.18")
        );
    }

    #[test]
    fn toml_roundtrip_module_override() {
        let d = tempdir("toml-module");
        let p = Project {
            root: d.clone(),
            name: "hello".into(),
            python: "3.12".into(),
            rikki: None,
            py_deps: [(
                "mlflow-skinny".to_string(),
                PyDep {
                    version: "*".into(),
                    module: Some("mlflow".into()),
                },
            )]
            .into(),
        };
        p.save().unwrap();
        let q = Project::load(&d).unwrap();
        let dep = q.py_deps.get("mlflow-skinny").unwrap();
        assert_eq!(dep.version, "*");
        assert_eq!(dep.module.as_deref(), Some("mlflow"));
        // only the package name reaches uv
        assert_eq!(q.requirement_lines(), "mlflow-skinny\n");
    }

    #[test]
    fn hand_edited_manifest_relocks_on_ensure_env() {
        let d = tempdir("relock");
        let uv = fake_uv(&d);
        let mut p = Project {
            root: d.clone(),
            name: "h".into(),
            python: "3.12".into(),
            rikki: None,
            py_deps: BTreeMap::new(),
        };
        p.save().unwrap();
        p.py_add("mlflow", None, &uv).unwrap();
        // hand edit, as a user swapping to the table form would
        p.py_deps.clear();
        p.py_deps.insert(
            "mlflow-skinny".into(),
            PyDep {
                version: "*".into(),
                module: Some("mlflow".into()),
            },
        );
        p.save().unwrap();
        // canary: a lock change must rebuild the venv, not sync in place
        // (package overlaps corrupt in-place syncs)
        std::fs::write(p.venv().join("canary"), "x").unwrap();
        std::fs::write(d.join("uv-calls.log"), "").unwrap();
        p.ensure_env(&uv).unwrap();
        let log = std::fs::read_to_string(d.join("uv-calls.log")).unwrap();
        assert!(log.contains("pip compile"), "stale lock must re-resolve: {log}");
        assert!(log.contains("pip sync"), "{log}");
        assert!(log.contains("venv .rikki/venv"), "must recreate the venv: {log}");
        assert!(!p.venv().join("canary").exists(), "venv was synced in place");
        // the re-resolved lock is fresh: next provision is a no-op
        std::fs::write(d.join("uv-calls.log"), "").unwrap();
        p.ensure_env(&uv).unwrap();
        let log = std::fs::read_to_string(d.join("uv-calls.log")).unwrap();
        assert_eq!(log.trim(), "", "{log}");
    }

    #[test]
    fn py_add_keeps_and_sets_module_overrides() {
        let d = tempdir("readd");
        let uv = fake_uv(&d);
        let mut p = Project {
            root: d.clone(),
            name: "h".into(),
            python: "3.12".into(),
            rikki: None,
            py_deps: BTreeMap::new(),
        };
        p.py_deps.insert(
            "mlflow-skinny".into(),
            PyDep {
                version: "*".into(),
                module: Some("mlflow".into()),
            },
        );
        p.save().unwrap();
        // re-adding an existing dep re-locks without clobbering the table form
        p.py_add("mlflow-skinny", None, &uv).unwrap();
        assert_eq!(p.py_deps["mlflow-skinny"].module.as_deref(), Some("mlflow"));
        // and --module writes the table form first class
        p.py_add("pillow", Some("PIL"), &uv).unwrap();
        assert_eq!(p.py_deps["pillow"].module.as_deref(), Some("PIL"));
        let toml = std::fs::read_to_string(d.join("rikki.toml")).unwrap();
        assert!(toml.contains("module = \"PIL\""), "{toml}");
    }

    #[test]
    fn find_walks_up() {
        let d = tempdir("find");
        std::fs::write(d.join("rikki.toml"), "[project]\nname = \"x\"\n").unwrap();
        let deep = d.join("src").join("nested");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(Project::find(&deep).unwrap(), d);
        assert_eq!(Project::find(&d.join("src/nested/main.rk")).unwrap(), d);
    }

    /// A relative start (`tk src/main.rk`) must yield an absolute root, not
    /// the empty path: Command::current_dir("") is ENOENT, which surfaced as
    /// a bogus "is uv installed?" on any fresh checkout.
    #[test]
    fn find_from_relative_path_returns_absolute_root() {
        let d = tempdir("find-rel");
        std::fs::write(d.join("rikki.toml"), "[project]\nname = \"x\"\n").unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::env::set_current_dir(&d).unwrap();
        let root = Project::find(Path::new("src/main.rk")).unwrap();
        assert!(root.is_absolute(), "got {root:?}");
        assert!(root.join("rikki.toml").exists());
    }

    #[test]
    fn py_add_drives_uv() {
        let d = tempdir("uv");
        let uv = fake_uv(&d);
        let mut p = Project {
            root: d.clone(),
            name: "hello".into(),
            python: "3.12".into(),
            rikki: None,
            py_deps: BTreeMap::new(),
        };
        p.save().unwrap();
        p.py_add("torch", None, &uv).unwrap();
        let lock = std::fs::read_to_string(d.join("rikki.lock")).unwrap();
        assert!(lock.contains("torch==2.9.0"));
        let log = std::fs::read_to_string(d.join("uv-calls.log")).unwrap();
        assert!(log.contains("pip compile --python-version 3.12"), "{log}");
        assert!(log.contains("venv .rikki/venv --python 3.12"), "{log}");
        assert!(log.contains("pip sync"), "{log}");
        // second ensure_env is a no-op thanks to the sync marker
        std::fs::write(d.join("uv-calls.log"), "").unwrap();
        p.ensure_env(&uv).unwrap();
        let log = std::fs::read_to_string(d.join("uv-calls.log")).unwrap();
        assert_eq!(log.trim(), "", "expected no uv calls, got: {log}");
    }
}
