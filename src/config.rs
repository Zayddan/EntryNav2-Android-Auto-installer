//! Settings and remembered password on this computer.

use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    Some(base?.join("entrynav2-installer"))
}

/// Path of a settings file; tests and debug builds can override it.
pub fn file(name: &str, test_override: &str) -> Option<PathBuf> {
    #[cfg(any(test, debug_assertions))]
    if let Some(p) = std::env::var_os(test_override) {
        return Some(PathBuf::from(p));
    }
    #[cfg(not(any(test, debug_assertions)))]
    let _ = test_override;
    Some(dir()?.join(name))
}

fn settings_path() -> Option<PathBuf> {
    file("settings", "ENTRYNAV_SETTINGS")
}

#[derive(Debug, PartialEq)]
pub struct Connection {
    pub host: String,
    pub port: String,
    pub user: String,
    pub remember_password: bool,
}

pub fn load_connection() -> Option<Connection> {
    let text = std::fs::read_to_string(settings_path()?).ok()?;
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    Some(Connection {
        host: get("host")?,
        port: get("port")?,
        user: get("user")?,
        remember_password: get("remember_password").as_deref() != Some("0"),
    })
}

/// Saves the connection settings that just worked.
pub fn save_connection(c: &Connection) {
    let Some(path) = settings_path() else { return };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let clean = |s: &str| s.trim().replace(['\n', '\r'], "");
    let text = format!(
        "host={}\nport={}\nuser={}\nremember_password={}\n",
        clean(&c.host),
        clean(&c.port),
        clean(&c.user),
        u8::from(c.remember_password)
    );
    let _ = std::fs::write(path, text);
}

const KEYRING_SERVICE: &str = "entrynav2-installer";

/// One saved password per user name.
fn password_entry(user: &str) -> Option<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, user.trim()).ok()
}

pub fn load_password(user: &str) -> Option<String> {
    password_entry(user)?
        .get_password()
        .ok()
        .filter(|p| !p.is_empty())
}

/// Saves the password that just worked, or forgets it.
pub fn store_password(user: &str, password: &str, remember: bool) {
    let Some(entry) = password_entry(user) else {
        return;
    };
    if remember && !password.is_empty() {
        let _ = entry.set_password(password);
    } else {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes, reads and deletes a temporary entry in the real credential
    /// store: `cargo test password_store -- --ignored`
    #[test]
    #[ignore]
    fn password_store_round_trip() {
        let user = format!("entrynav2-test-{}", std::process::id());
        store_password(&user, "secret-for-test", true);
        assert_eq!(load_password(&user).as_deref(), Some("secret-for-test"));
        store_password(&user, "", false);
        assert_eq!(load_password(&user), None, "forgotten");
    }

    #[test]
    fn connection_round_trip_without_password() {
        let f =
            std::env::temp_dir().join(format!("entrynav2-settings-test-{}", std::process::id()));
        std::env::set_var("ENTRYNAV_SETTINGS", &f);
        save_connection(&Connection {
            host: " 169.254.1.0 ".into(),
            port: "22".into(),
            user: "root\nx".into(),
            remember_password: false,
        });
        assert_eq!(
            load_connection(),
            Some(Connection {
                host: "169.254.1.0".into(),
                port: "22".into(),
                user: "rootx".into(),
                remember_password: false,
            })
        );
        let text = std::fs::read_to_string(&f).unwrap();
        assert_eq!(text.lines().filter(|l| l.contains("password")).count(), 1);
        assert!(text.contains("remember_password=0"));
        let _ = std::fs::remove_file(&f);
        std::env::remove_var("ENTRYNAV_SETTINGS");
    }
}
