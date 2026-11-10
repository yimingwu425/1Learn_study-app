use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Owns the server. Startup failures and normal shutdown both reap the child.
pub struct Core {
    child: Child,
    ready_dir: PathBuf,
}

impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.ready_dir);
    }
}

impl Core {
    pub fn start(binary: &Path, root: &Path, data: &Path) -> io::Result<(Self, String)> {
        if !root.join("web/dist/index.html").is_file() {
            return Err(io::Error::other(
                "Missing web/dist/index.html; build or stage the UI first",
            ));
        }
        fs::create_dir_all(data)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let ready_dir = data.join(format!(".desktop-{}-{nonce}", std::process::id()));
        fs::create_dir(&ready_dir)?;
        let ready_file = ready_dir.join("ready");
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(data.join("core.log"))?;
        let mut command = Command::new(binary);
        command
            .env("STUDY_ROOT", root)
            .env("STUDY_DATA_DIR", data)
            .env("STUDY_PORT", "0")
            .env("STUDY_READY_FILE", &ready_file)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let spawned = command.spawn();
        let child = match spawned {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_dir_all(&ready_dir);
                return Err(error);
            }
        };
        let mut core = Self { child, ready_dir };
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = core.child.try_wait()? {
                return Err(io::Error::other(format!(
                    "Study core exited ({status}); see {}",
                    data.join("core.log").display()
                )));
            }
            if let Ok(value) = fs::read_to_string(&ready_file) {
                if let Some(url) = local_url(&value) {
                    return Ok((core, url));
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Study core did not become ready in 20 seconds; see core.log",
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

fn local_url(value: &str) -> Option<String> {
    let value = value.trim();
    let port: u16 = value.strip_prefix("http://127.0.0.1:")?.parse().ok()?;
    (port != 0).then(|| format!("http://127.0.0.1:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_loopback_origin_is_accepted() {
        assert_eq!(
            local_url("http://127.0.0.1:4317\n"),
            Some("http://127.0.0.1:4317".into())
        );
        for invalid in [
            "http://evil.test:4317",
            "http://127.0.0.1:0",
            "http://127.0.0.1:4317/path",
            "http://127.0.0.1:4317@evil.test",
            "http://127.0.0.1:65536",
        ] {
            assert!(local_url(invalid).is_none());
        }
    }
    #[test]
    #[cfg(unix)]
    fn owned_child_is_reaped_and_readiness_is_removed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("study-shell-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("web/dist")).unwrap();
        fs::write(dir.join("web/dist/index.html"), "test").unwrap();
        let binary = dir.join("fake-core");
        fs::write(
            &binary,
            "#!/bin/sh\nprintf 'http://127.0.0.1:54321' > \"$STUDY_READY_FILE\"\nexec sleep 30\n",
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        let (core, address) = Core::start(&binary, &dir, &dir.join("data")).unwrap();
        assert_eq!(address, "http://127.0.0.1:54321");
        let pid = core.child.id();
        let ready_dir = core.ready_dir.clone();
        drop(core);
        assert!(!ready_dir.exists());
        assert!(!Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        fs::remove_dir_all(dir).unwrap();
    }
}
