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
    pub py_deps: BTreeMap<String, String>,
}

impl Project {
    /// Walk up from `start` to the nearest directory holding rikki.toml.
    pub fn find(start: &Path) -> Option<PathBuf> {
        let mut dir = if start.is_dir() {
            start
        } else {
            start.parent()?
        }
        .to_path_buf();
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
        let mut py_deps = BTreeMap::new();
        if let Some(deps) = doc.get("py-deps").and_then(|v| v.as_table()) {
            for (k, v) in deps {
                py_deps.insert(k.clone(), v.as_str().unwrap_or("*").to_string());
            }
        }
        Ok(Project {
            root: root.to_path_buf(),
            name,
            python,
            py_deps,
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let mut out = String::new();
        out.push_str("[project]\n");
        out.push_str(&format!("name = {:?}\n", self.name));
        out.push_str(&format!("python = {:?}\n", self.python));
        if !self.py_deps.is_empty() {
            out.push_str("\n[py-deps]\n");
            for (k, v) in &self.py_deps {
                out.push_str(&format!("{k} = {v:?}\n"));
            }
        }
        std::fs::write(self.root.join("rikki.toml"), out)
            .map_err(|e| format!("write rikki.toml: {e}"))
    }

    pub fn venv(&self) -> PathBuf {
        self.root.join(".rikki").join("venv")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("rikki.lock")
    }

    fn requirement_lines(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.py_deps {
            if v == "*" {
                s.push_str(&format!("{k}\n"));
            } else {
                s.push_str(&format!("{k}{v}\n"));
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
        std::fs::write(self.lock_path(), lock).map_err(|e| format!("write lock: {e}"))
    }

    /// Create/refresh the venv so it matches the lock. Idempotent: skips work
    /// when the lock hash matches the last sync marker.
    pub fn ensure_env(&self, uv_bin: &str) -> Result<(), String> {
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
        } else {
            self.uv(
                uv_bin,
                &["venv", ".rikki/venv", "--python", &self.python],
                None,
            )?;
        }
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

    pub fn py_add(&mut self, pkg: &str, uv_bin: &str) -> Result<(), String> {
        self.py_deps
            .entry(pkg.to_string())
            .or_insert_with(|| "*".to_string());
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
        let d = std::env::temp_dir().join(format!("rikki-proj-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
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
            py_deps: [("torch".to_string(), "*".to_string())].into(),
        };
        p.save().unwrap();
        let q = Project::load(&d).unwrap();
        assert_eq!(q.name, "hello");
        assert_eq!(q.python, "3.12");
        assert_eq!(q.py_deps.get("torch").map(String::as_str), Some("*"));
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

    #[test]
    fn py_add_drives_uv() {
        let d = tempdir("uv");
        let uv = fake_uv(&d);
        let mut p = Project {
            root: d.clone(),
            name: "hello".into(),
            python: "3.12".into(),
            py_deps: BTreeMap::new(),
        };
        p.save().unwrap();
        p.py_add("torch", &uv).unwrap();
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
