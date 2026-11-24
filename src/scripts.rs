//! Shell scripts run on the head unit, and parsing of their output.

/// Minimum free space on `/` required by the documentation (16 MB).
pub const MIN_FREE_KB: u64 = 16 * 1024;

/// Version, model name...: only characters that are harmless in a shell.
pub fn is_safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn with_version(template: &str, version: &str) -> String {
    assert!(is_safe_token(version), "unsafe version string");
    template.replace("__V__", version)
}

/// Runs a script so it survives a dropped connection (HUP/PIPE ignored, output
/// to a log), holding the car-side lock. Refuses while the USB-stick update runs.
fn detached(name: &str, body: &str) -> String {
    format!(
        r#"exec 2>&1
trap '' HUP PIPE
LOG=/tmp/entrynav2-installer-{name}.log
LOCK=/tmp/entrynav2-installer.lock
(
if ! mkdir "$LOCK" 2>/dev/null; then
    OTHER=$(cat "$LOCK/pid" 2>/dev/null)
    if [ -n "$OTHER" ] && kill -0 "$OTHER" 2>/dev/null; then
        echo '@@ERR|another installer is changing this car right now. Wait for it to finish. Nothing was changed.'
        exit 1
    fi
    rm -rf "$LOCK"
    mkdir "$LOCK" || {{ echo '@@ERR|cannot take the installer lock in /tmp. Nothing was changed.'; exit 1; }}
fi
echo $$ > "$LOCK/pid"
if [ -e /tmp/entry-aa-update.lock ] && command -v flock >/dev/null 2>&1; then
    if ! flock -n /tmp/entry-aa-update.lock true; then
        echo '@@ERR|a USB-stick update is running on this car. Wait for it to finish. Nothing was changed.'
        exit 1
    fi
fi
{body}
) >"$LOG" 2>&1 </dev/null
RC=$?
[ "$(cat "$LOCK/pid" 2>/dev/null)" = "$$" ] && rm -rf "$LOCK"
cat "$LOG"
rm -f "$LOG"
exit $RC
"#
    )
}

/// Reads the "Available" column of the last line of `df -k` output.
pub fn parse_df_available(line: &str) -> Option<u64> {
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() >= 4 {
        f[f.len() - 3].parse().ok()
    } else {
        None
    }
}

/// Car state. The installed version is found by looking for the known release
/// numbers in /opt/aa/entry-aad, where the project compiles its version.
pub fn info_script(known_versions: &[String]) -> String {
    let mut s = String::from(INFO_BASE);
    s.push_str("if [ -f /opt/aa/entry-aad ]; then\n");
    for v in known_versions.iter().filter(|v| is_safe_token(v)) {
        s.push_str(&format!(
            "    grep -q -F {q} /opt/aa/entry-aad 2>/dev/null && echo '@@BINVER|{v}'\n",
            q = sh_quote(v)
        ));
    }
    s.push_str("    :\nfi\nexit 0\n");
    s
}

const INFO_BASE: &str = r#"exec 2>&1
echo "@@SYSVER|$(head -n 1 /etc/version 2>/dev/null)"
if [ -f /opt/aa/.installer-version ]; then
    echo "@@APPVER|$(cat /opt/aa/.installer-version)"
elif [ -f /opt/aa/.installed-by-entryaad ] || [ -f /opt/hmi/data/ro/Plugins/EntryAad/.installed-by-entryaad ]; then
    echo "@@APPVER|?"
else
    echo "@@APPVER|"
fi
[ -f /var/local/data/AA/app.disable ] && echo '@@DISABLED'
[ -f /var/local/data/AA/app-guard ] && echo '@@GUARD'
pidof aa-supervisor entry-aad >/dev/null 2>&1 && echo '@@RUNNING'
[ -e /var/local/data/AA/usb ] && echo '@@USB'
# Archives already checked and still waiting in /tmp (gone after a reboot).
for d in /tmp/entrynav2-android-auto-*; do
    [ -d "$d" ] && echo "@@STAGED|${d#/tmp/entrynav2-android-auto-}"
done
"#;

#[derive(Debug, Clone, PartialEq)]
pub enum Installed {
    None,
    Unknown,
    Version(String),
}

#[derive(Debug, Clone)]
pub struct CarInfo {
    pub sysver: String,
    pub installed: Installed,
    /// `/var/local/data/AA/app.disable` exists: the app was switched off by hand.
    pub disabled: bool,
    /// `/var/local/data/AA/app-guard` exists: the safety guard is counting starts.
    pub guard: bool,
    /// `aa-supervisor` or `entry-aad` is running.
    pub running: bool,
    /// `/var/local/data/AA/usb` exists: Android Auto over a USB cable.
    pub usb_mode: bool,
    /// Versions whose checked archive is still extracted in /tmp.
    pub staged: Vec<String>,
}

pub fn parse_info(lines: &[String]) -> CarInfo {
    let mut info = CarInfo {
        sysver: String::new(),
        installed: Installed::None,
        disabled: false,
        guard: false,
        running: false,
        usb_mode: false,
        staged: Vec::new(),
    };
    // Versions found in the program: the longest wins ("0.13.0-rc.10"
    // contains "0.13.0-rc.1" and "0.13.0").
    let mut in_binary: Option<String> = None;
    for l in lines {
        if let Some(v) = l.strip_prefix("@@BINVER|") {
            let v = v.trim();
            let longer = in_binary.as_ref().is_none_or(|b| {
                v.len() > b.len()
                    || (v.len() == b.len() && crate::github::compare_versions(v, b).is_gt())
            });
            if is_safe_token(v) && longer {
                in_binary = Some(v.to_string());
            }
            continue;
        }
        if let Some(v) = l.strip_prefix("@@STAGED|") {
            if is_safe_token(v.trim()) {
                info.staged.push(v.trim().to_string());
            }
            continue;
        }
        match l.as_str() {
            "@@DISABLED" => info.disabled = true,
            "@@GUARD" => info.guard = true,
            "@@RUNNING" => info.running = true,
            "@@USB" => info.usb_mode = true,
            _ => {
                if let Some(v) = l.strip_prefix("@@SYSVER|") {
                    info.sysver = v.trim().to_string();
                } else if let Some(v) = l.strip_prefix("@@APPVER|") {
                    info.installed = match v.trim() {
                        "" => Installed::None,
                        "?" => Installed::Unknown,
                        v => Installed::Version(v.to_string()),
                    };
                }
            }
        }
    }
    // The version read in the program wins; .installer-version is a fallback.
    if info.installed != Installed::None {
        if let Some(v) = in_binary {
            info.installed = Installed::Version(v);
        }
    }
    info
}

pub const TMP_SPACE_SCRIPT: &str = "df -k /tmp | tail -n 1";

/// Removes earlier checks of other versions from /tmp.
const CLEAN_OTHERS_TEMPLATE: &str = r#"for f in /tmp/entrynav2-android-auto-*; do
    case "$f" in
        /tmp/entrynav2-android-auto-__V__|/tmp/entrynav2-android-auto-__V__-armhf.tar) ;;
        *) [ -e "$f" ] && rm -rf "$f" && echo ">>> removed an earlier check: $f" ;;
    esac
done
exit 0
"#;

pub fn clean_others_script(version: &str) -> String {
    with_version(CLEAN_OTHERS_TEMPLATE, version)
}

const CHECK_TEMPLATE: &str = r#"exec 2>&1
V=__V__
TOP=entrynav2-android-auto-$V
cd /tmp || { echo '@@ERR|cannot enter /tmp'; exit 1; }
rm -rf "$TOP"
# The archive is extracted as root by the car's (old) BusyBox: refuse links,
# devices and any path outside its own folder before extracting anything.
tar -tvf "$TOP-armhf.tar" >/dev/null 2>&1 || { echo '@@ERR|the archive cannot be read'; exit 1; }
if tar -tvf "$TOP-armhf.tar" 2>/dev/null | grep -q '^[^-d]'; then
    echo '@@ERR|the archive contains links or special files: refused'; exit 1
fi
tar -tf "$TOP-armhf.tar" | while read -r name; do
    case "$name" in
        /*|..|../*|*/..|*/../*) echo "@@ERR|unsafe path in the archive: $name"; exit 1 ;;
        "$TOP"|"$TOP/"*) ;;
        *) echo "@@ERR|unexpected path in the archive: $name"; exit 1 ;;
    esac
done || exit 1
tar -xf "$TOP-armhf.tar" || { echo '@@ERR|could not extract the archive'; exit 1; }
cd "$TOP" || { echo '@@ERR|extracted folder not found'; exit 1; }
for f in MD5SUMS opt/aa/entry-aad opt/aa/aa-supervisor opt/hmi/data/ro/Plugins/EntryAad/libEntryAad.so; do
    [ -f "$f" ] || { echo "@@ERR|$f is missing from the archive (layout of an older version?)"; exit 1; }
done
# This installer copies two folders only. A version that installs files
# anywhere else needs a procedure it does not know: refuse it.
while read -r sum name; do
    name=${name#\*}  # md5sum's optional "binary" marker
    case "$name" in
        opt/aa/*/*|opt/hmi/data/ro/Plugins/EntryAad/*/*) echo "@@UNSUPPORTED|$name" ;;
        opt/aa/*|opt/hmi/data/ro/Plugins/EntryAad/*) ;;
        *) echo "@@UNSUPPORTED|$name" ;;
    esac
done < MD5SUMS
echo '@@SECTION|md5'
md5sum -c MD5SUMS
echo "@@MD5RC|$?"
echo "@@SYSVER|$(head -n 1 /etc/version)"
df -k / | tail -n 1 | sed 's/^/@@DF|/'
echo '@@SECTION|ld'
L=/usr/local/lib:/opt/hmi/lib:/usr/lib:/lib
for f in opt/aa/entry-aad opt/aa/aa-supervisor opt/hmi/data/ro/Plugins/EntryAad/libEntryAad.so; do
    LD_TRACE_LOADED_OBJECTS=1 LD_WARN=yes LD_BIND_NOW=yes LD_LIBRARY_PATH=$L /lib/ld-linux-armhf.so.3 ./$f 2>&1 |
        grep -E 'not found|undefined symbol|not defined in file' | sed "s#^#@@LD|$f: #"
done
echo '@@SECTION|compat'
LD_LIBRARY_PATH=$L XDG_RUNTIME_DIR=/run/system/0 WAYLAND_DISPLAY=wayland-0 HOME=/tmp \
    busybox timeout -t 60 ./opt/aa/aa-supervisor --compat </dev/null
echo "@@COMPATRC|$?"
echo '@@DONE'
"#;

pub fn check_script(version: &str) -> String {
    with_version(CHECK_TEMPLATE, version)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompatLevel {
    Ok,
    Warn,
    Block,
}

#[derive(Debug, Clone)]
pub struct CompatLine {
    pub level: CompatLevel,
    pub text: String,
    /// `@block` that only affects wireless (USB is still possible).
    pub wireless_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub version: String,
    /// Address of the head unit the check ran on.
    pub host: String,
    /// The archive that was checked (`worker::Source::key`).
    pub source: String,
    /// Signed by the project, or matched a known SHA-256.
    pub archive_verified: bool,
    /// The archive carries the project's signature.
    pub signed: bool,
    pub error: Option<String>,
    /// Files the archive would install outside the two known folders.
    pub unsupported: Vec<String>,
    pub md5_rc: Option<i32>,
    pub md5_failures: Vec<String>,
    pub sysver: String,
    pub free_kb: Option<u64>,
    pub ld_issues: Vec<String>,
    pub compat: Vec<CompatLine>,
    pub compat_rc: Option<i32>,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Everything is fine.
    Ok,
    /// Can be installed, but wireless Android Auto is not available.
    WiredOnly,
    /// Do not install.
    Blocked(Vec<String>),
}

fn parse_compat(line: &str) -> Option<CompatLine> {
    let (level, rest) = if let Some(r) = line.strip_prefix("@ok|") {
        (CompatLevel::Ok, r)
    } else if let Some(r) = line.strip_prefix("@warn|") {
        (CompatLevel::Warn, r)
    } else {
        let r = line.strip_prefix("@block|")?;
        (CompatLevel::Block, r)
    };
    let lower = rest.to_ascii_lowercase();
    let wireless_only = level == CompatLevel::Block
        && lower.contains("wireless only")
        && lower
            .split(|c: char| c == '|' || c.is_whitespace() || c == ':' || c == '=')
            .any(|f| matches!(f, "alps" | "uuid" | "spp"));
    Some(CompatLine {
        level,
        text: rest.replace('|', "  ·  "),
        wireless_only,
    })
}

pub fn parse_check(version: &str, lines: &[String]) -> CheckReport {
    let mut r = CheckReport {
        version: version.to_string(),
        ..Default::default()
    };
    let mut section = "";
    for l in lines {
        let l = l.trim_end();
        if let Some(s) = l.strip_prefix("@@SECTION|") {
            section = match s {
                "md5" => "md5",
                "ld" => "ld",
                "compat" => "compat",
                _ => "",
            };
        } else if let Some(e) = l.strip_prefix("@@ERR|") {
            r.error = Some(e.to_string());
        } else if let Some(f) = l.strip_prefix("@@UNSUPPORTED|") {
            r.unsupported.push(f.to_string());
        } else if let Some(v) = l.strip_prefix("@@MD5RC|") {
            r.md5_rc = v.trim().parse().ok();
            section = "";
        } else if let Some(v) = l.strip_prefix("@@SYSVER|") {
            r.sysver = v.trim().to_string();
        } else if let Some(v) = l.strip_prefix("@@DF|") {
            r.free_kb = parse_df_available(v);
        } else if let Some(v) = l.strip_prefix("@@LD|") {
            r.ld_issues.push(v.to_string());
        } else if let Some(v) = l.strip_prefix("@@COMPATRC|") {
            r.compat_rc = v.trim().parse().ok();
        } else if l == "@@DONE" {
            r.complete = true;
        } else if section == "md5" {
            if l.contains("FAILED") || l.contains("No such file") {
                r.md5_failures.push(l.to_string());
            }
        } else if section == "compat" {
            if let Some(c) = parse_compat(l) {
                r.compat.push(c);
            }
        }
    }
    r
}

impl CheckReport {
    /// Fail-safe: anything missing or unreadable blocks the installation.
    pub fn verdict(&self) -> Verdict {
        let mut reasons = Vec::new();
        if let Some(e) = &self.error {
            reasons.push(format!("Error: {e}"));
        }
        if !self.unsupported.is_empty() {
            reasons.push(format!(
                "Not supported by this installer: this version installs files outside /opt/aa and the \
                 iDrive plugin folder ({}). Install it by hand, following its INSTALLATION.md.",
                self.unsupported.join(", ")
            ));
        }
        if !self.complete && self.error.is_none() {
            reasons.push("The check did not finish".into());
        }
        if self.md5_rc != Some(0) || !self.md5_failures.is_empty() {
            reasons.push("Corrupted archive (md5sum failed)".into());
        }
        match self.free_kb {
            Some(kb) if kb < MIN_FREE_KB => reasons.push(format!(
                "Not enough free space: {} MB (16 MB required)",
                kb / 1024
            )),
            None if self.complete => reasons.push("Could not read the free space".into()),
            _ => {}
        }
        if !self.ld_issues.is_empty() {
            reasons.push(format!(
                "{} missing librar(y/ies) or symbol(s)",
                self.ld_issues.len()
            ));
        }
        // busybox timeout ends the process with SIGTERM (143) or SIGKILL (137).
        if matches!(self.compat_rc, Some(124 | 137 | 143)) {
            reasons.push("The compatibility test timed out".into());
        }
        if self.complete && self.compat.is_empty() {
            reasons.push("The compatibility test returned nothing".into());
        }
        for c in &self.compat {
            if c.level == CompatLevel::Block && !c.wireless_only {
                reasons.push(format!("Blocking: {}", c.text));
            }
        }
        if !reasons.is_empty() {
            Verdict::Blocked(reasons)
        } else if self.compat.iter().any(|c| c.wireless_only) {
            Verdict::WiredOnly
        } else {
            Verdict::Ok
        }
    }
}

const INSTALL_BODY: &str = r#"set -e
V=__V__
D=/tmp/entrynav2-android-auto-$V
P=/opt/hmi/data/ro/Plugins
PD=$P/EntryAad
fail() { echo "@@ERR|$1"; exit 1; }

# ---- Pre-flight checks: nothing has been written yet.
[ -d "$D" ] || fail "the checked archive is no longer in /tmp (head unit restarted?). Run the check again."
cd "$D"
md5sum -c MD5SUMS >/dev/null 2>&1 || fail "the archive in /tmp no longer matches MD5SUMS. Run the check again."
if pidof aa-supervisor entry-aad >/dev/null 2>&1; then
    fail "Android Auto is running. Stop it from its iDrive menu (Stop), then try again."
fi
for dir in /opt/aa "$PD"; do
    if [ -d "$dir" ] && [ -n "$(ls -A "$dir" 2>/dev/null)" ] && [ ! -f "$dir/.installed-by-entryaad" ]; then
        fail "$dir exists but was not created by EntryNav2-Android-Auto. Nothing was changed."
    fi
done
AVAIL=$(df -k / | tail -n 1 | awk '{print $(NF-2)}')
[ "$AVAIL" -ge 16384 ] 2>/dev/null || fail "not enough free space on / (16 MB required)."

STAGE=pre
on_exit() {
    rc=$?
    if [ "$STAGE" = copy ]; then
        echo '>>> Copy failed: removing the partial installation so the iDrive does not load a broken plugin'
        rm -rf "$PD" /opt/aa
    fi
    sync
    mount -o remount,ro / 2>/dev/null || echo '@@WARN|Could not make / read-only again. Reboot the head unit.'
    exit $rc
}
trap on_exit EXIT

# ---- Write phase: keep the ignition on.
echo '>>> mount -o remount,rw /'
mount -o remount,rw /
STAGE=copy
mkdir -p "$PD" /opt/aa
rm -f "$PD"/* /opt/aa/*
echo '>>> Copying files'
cp opt/hmi/data/ro/Plugins/EntryAad/* "$PD"/
cp opt/aa/* /opt/aa/
chmod 755 /opt/aa/entry-aad /opt/aa/aa-supervisor "$PD"/libEntryAad.so
touch /opt/aa/.installed-by-entryaad "$PD"/.installed-by-entryaad
printf '%s\n' "$V" > /opt/aa/.installer-version
# Since rc.10: an interrupted USB-update journal must go (docs/INSTALLATION.md).
rm -f /var/local/data/AA/update-pending
echo '>>> sync'
sync
STAGE=done
trap - EXIT
# Everything is written and synced. If / cannot be made read-only now, the
# next reboot does it (docs/INSTALLATION.md): a warning, not a failure.
mount -o remount,ro / || echo '@@WARN|Could not make / read-only again: the next reboot of the head unit will.'

# ---- Read back the installed files.
echo '>>> Verifying installed files'
cd /
if ! md5sum -c "$D/MD5SUMS"; then
    fail "installed files do not match MD5SUMS. Do NOT reboot: install again or uninstall."
fi
echo '@@INSTALL_OK'"#;

pub fn install_script(version: &str) -> String {
    detached("install", &with_version(INSTALL_BODY, version))
}

const CLEANUP_TEMPLATE: &str = r#"rm -rf /tmp/entrynav2-android-auto-__V__ /tmp/entrynav2-android-auto-__V__-armhf.tar
echo '@@CLEAN_OK'
"#;

pub fn cleanup_script(version: &str) -> String {
    with_version(CLEANUP_TEMPLATE, version)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fuel {
    Petrol,
    Diesel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Auto,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub struct Vehicle {
    pub model: String,
    pub year: String,
    pub fuel: Fuel,
    pub side: Side,
}

impl Vehicle {
    /// Rules of docs/INSTALLATION.md: at most 24 characters, unaccented
    /// letters, digits, spaces, '-', '.' and '_'.
    pub fn validate(&self) -> Result<(), String> {
        let model = self.model.trim();
        if model.len() > 24 || !is_safe_token(&model.replace(' ', "_")) {
            return Err(
                "Model: up to 24 characters, unaccented letters, digits, spaces, '.', '-' or '_'"
                    .into(),
            );
        }
        if self.year.len() != 4 || !self.year.chars().all(|c| c.is_ascii_digit()) {
            return Err("Year: 4 digits (e.g. 2019)".into());
        }
        Ok(())
    }

    pub fn script(&self) -> String {
        let fuel = match self.fuel {
            Fuel::Petrol => "petrol",
            Fuel::Diesel => "diesel",
        };
        let side = match self.side {
            Side::Auto => String::new(),
            Side::Left => r"side=left\n".into(),
            Side::Right => r"side=right\n".into(),
        };
        format!(
            "exec 2>&1\nset -e\nmkdir -p /var/local/data/AA && printf 'model=%s\\nyear=%s\\nfuel=%s\\n{side}' {} {} {} > /var/local/data/AA/vehicle\necho '@@VEHICLE_OK'\n",
            sh_quote(self.model.trim()),
            sh_quote(&self.year),
            fuel
        )
    }
}

pub fn uninstall_script(keep_settings: bool) -> String {
    let settings = if keep_settings {
        "echo '>>> Settings kept in /var/local/data/AA'"
    } else {
        "rm -rf /var/local/data/AA"
    };
    let body = format!(
        r#"set -e
P=/opt/hmi/data/ro/Plugins
PD=$P/EntryAad
fail() {{ echo "@@ERR|$1"; exit 1; }}

if pidof aa-supervisor entry-aad >/dev/null 2>&1; then
    fail "Android Auto is running. Stop it from its iDrive menu (Stop), then try again."
fi
for dir in /opt/aa "$PD"; do
    if [ -d "$dir" ] && [ ! -f "$dir/.installed-by-entryaad" ]; then
        fail "$dir was not created by EntryNav2-Android-Auto. Nothing was removed."
    fi
done

trap 'sync; mount -o remount,ro / 2>/dev/null || echo "@@WARN|Could not make / read-only again. Reboot the head unit."' EXIT
mount -o remount,rw /
rm -rf "$PD" /opt/aa
sync
trap - EXIT
mount -o remount,ro / || echo "@@WARN|Could not make / read-only again: the next reboot of the head unit will."
{settings}
echo '@@UNINSTALL_OK'"#
    );
    detached("uninstall", &body)
}

/// Turns the app back on after the safety guard or `app.disable` switched it off.
pub const ENABLE_SCRIPT: &str = r#"exec 2>&1
rm -f /var/local/data/AA/app.disable /var/local/data/AA/app-guard
echo '@@ENABLE_OK'
"#;

/// Keeps the app switched off without uninstalling it (`app.disable` file).
pub const DISABLE_SCRIPT: &str = r#"exec 2>&1
mkdir -p /var/local/data/AA && touch /var/local/data/AA/app.disable && echo '@@DISABLE_OK'
"#;

/// Stops Android Auto like the iDrive menu (STOP file, else SIGTERM). The
/// supervisor is never killed: it must give the car's Bluetooth back.
pub const STOP_SCRIPT: &str = r#"exec 2>&1
running() { pidof aa-supervisor entry-aad >/dev/null 2>&1; }
if ! running; then echo '@@STOP_OK|not running'; exit 0; fi
if pidof aa-supervisor >/dev/null 2>&1; then
    if [ -d /tmp/entry-aa-service ]; then
        echo '>>> Asking the supervisor to stop (STOP file, as the iDrive menu does)'
        : > /tmp/entry-aa-service/STOP
    else
        echo '>>> Asking the supervisor to stop (SIGTERM)'
        kill -TERM $(pidof aa-supervisor)
    fi
else
    echo '>>> Stopping entry-aad (SIGTERM)'
    kill -TERM $(pidof entry-aad)
fi
i=0
while running; do
    i=$((i+1))
    if [ "$i" = 20 ] && ! pidof aa-supervisor >/dev/null 2>&1 && pidof entry-aad >/dev/null 2>&1; then
        echo '>>> entry-aad still running after 20 s: SIGKILL'
        kill -KILL $(pidof entry-aad)
    fi
    if [ "$i" -ge 60 ]; then
        echo '@@ERR|Android Auto is still running after 60 s. Nothing was forced: reboot the head unit.'
        exit 1
    fi
    sleep 1
done
if [ -f /tmp/entry-aa-service/bluetooth-retour.txt ]; then
    echo '@@WARN|The Bluetooth restore was not confirmed. Rebooting the head unit gives the Bluetooth back.'
fi
echo '@@STOP_OK|stopped'
"#;

/// Selects Android Auto over a USB cable (docs/USB.md), needed when the
/// wireless is not compatible. An existing file (maybe a port name) is kept.
pub const USB_MODE_SCRIPT: &str = r#"exec 2>&1
mkdir -p /var/local/data/AA
if [ -e /var/local/data/AA/usb ]; then
    echo '>>> USB mode already selected'
else
    touch /var/local/data/AA/usb
fi
echo '@@USB_OK'
"#;

/// Collects the `@@ERR|` and `@@WARN|` messages of a script output.
pub fn markers<'a>(lines: &'a [String], prefix: &str) -> Vec<&'a str> {
    lines
        .iter()
        .filter_map(|l| l.strip_prefix(prefix))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    const GOOD: &str = "@@SECTION|md5
opt/aa/entry-aad: OK
opt/aa/aa-supervisor: OK
@@MD5RC|0
@@SYSVER|EntryNav2_19-07
@@DF|/dev/root 524288 400000 124288 76% /
@@SECTION|ld
@@SECTION|compat
@ok|screen|1280x480
@warn|usb|slow
@@COMPATRC|0
@@DONE";

    #[test]
    fn check_ok() {
        let r = parse_check("0.12.0", &lines(GOOD));
        assert_eq!(r.free_kb, Some(124288));
        assert_eq!(r.sysver, "EntryNav2_19-07");
        assert_eq!(r.compat.len(), 2);
        assert_eq!(r.verdict(), Verdict::Ok);
    }

    #[test]
    fn check_wireless_only() {
        let s = GOOD.replace("@warn|usb|slow", "@block|spp|wireless only");
        let r = parse_check("0.12.0", &lines(&s));
        assert!(r.compat[1].wireless_only);
        assert_eq!(r.verdict(), Verdict::WiredOnly);
    }

    #[test]
    fn check_blocked() {
        let s = GOOD
            .replace("@warn|usb|slow", "@block|screen|unsupported resolution")
            .replace("opt/aa/entry-aad: OK", "opt/aa/entry-aad: FAILED")
            .replace("@@MD5RC|0", "@@MD5RC|1")
            .replace(
                "@@SECTION|ld",
                "@@SECTION|ld\n@@LD|opt/aa/entry-aad: libfoo.so => not found",
            );
        match parse_check("0.12.0", &lines(&s)).verdict() {
            Verdict::Blocked(r) => assert_eq!(r.len(), 3),
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn check_fail_safe() {
        let blocked =
            |s: &str| matches!(parse_check("x", &lines(s)).verdict(), Verdict::Blocked(_));
        assert!(blocked(&GOOD.replace("124288", "1000")), "low space");
        assert!(blocked(&GOOD.replace("@@DONE", "")), "incomplete");
        assert!(
            blocked(&GOOD.replace("@@COMPATRC|0", "@@COMPATRC|143")),
            "timeout"
        );
        assert!(
            blocked(&GOOD.replace("@ok|screen|1280x480\n@warn|usb|slow\n", "")),
            "no compat output"
        );
        assert!(
            blocked(&GOOD.replace("@@DF|/dev/root 524288 400000 124288 76% /", "")),
            "no df"
        );
    }

    #[test]
    fn info() {
        let i = parse_info(&lines("@@SYSVER|abc\n@@APPVER|0.12.0"));
        assert_eq!(i.installed, Installed::Version("0.12.0".into()));
        assert!(!i.disabled && !i.guard && !i.running);
        assert_eq!(
            parse_info(&lines("@@APPVER|?")).installed,
            Installed::Unknown
        );
        assert_eq!(parse_info(&lines("@@APPVER|")).installed, Installed::None);
        let i = parse_info(&lines(
            "@@APPVER|0.12.0\n@@DISABLED\n@@GUARD\n@@RUNNING\n@@USB",
        ));
        assert!(i.disabled && i.guard && i.running && i.usb_mode);
    }

    #[test]
    fn version_read_in_the_program() {
        // rc.10's program contains "0.13.0", "0.13.0-rc.1" and "0.13.0-rc.10".
        let i = parse_info(&lines(
            "@@APPVER|0.13.0-rc.1\n@@BINVER|0.13.0\n@@BINVER|0.13.0-rc.10\n@@BINVER|0.13.0-rc.1",
        ));
        assert_eq!(
            i.installed,
            Installed::Version("0.13.0-rc.10".into()),
            "program wins over the file"
        );
        // Installed by hand: no installer file, the program says it.
        let i = parse_info(&lines("@@APPVER|?\n@@BINVER|0.12.0"));
        assert_eq!(i.installed, Installed::Version("0.12.0".into()));
        // Unknown build (not a release): installer file, else unknown.
        assert_eq!(
            parse_info(&lines("@@APPVER|?")).installed,
            Installed::Unknown
        );
        // Nothing installed (no markers): a stray program is ignored.
        assert_eq!(
            parse_info(&lines("@@APPVER|\n@@BINVER|0.12.0")).installed,
            Installed::None
        );

        let s = info_script(&["0.12.0".into(), "bad;rm".into()]);
        assert!(s.contains("grep -q -F '0.12.0' /opt/aa/entry-aad"));
        assert!(
            !s.contains("bad;rm"),
            "unsafe versions are never put in the script"
        );
    }

    #[test]
    fn staged_archives_and_unsupported_files() {
        let i = parse_info(&lines(
            "@@STAGED|0.12.0\n@@STAGED|bad;name\n@@STAGED|0.13.0-rc.10",
        ));
        assert_eq!(
            i.staged,
            vec!["0.12.0".to_string(), "0.13.0-rc.10".to_string()]
        );
        let s = GOOD.replace(
            "@@SECTION|md5",
            "@@UNSUPPORTED|etc/init.d/aa\n@@SECTION|md5",
        );
        match parse_check("x", &lines(&s)).verdict() {
            Verdict::Blocked(r) => {
                assert!(r[0].contains("Not supported by this installer"), "{r:?}")
            }
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn archive_is_inspected_before_extraction() {
        let s = check_script("0.12.0");
        let extract = s.find("tar -xf").unwrap();
        assert!(
            s.find("grep -q '^[^-d]'").unwrap() < extract,
            "links refused before extraction"
        );
        assert!(s.find("unsafe path in the archive").unwrap() < extract);
        assert!(s.contains("@@UNSUPPORTED|"));
    }

    #[test]
    fn changes_take_the_car_lock_and_clear_update_pending() {
        for s in [install_script("0.12.0"), uninstall_script(true)] {
            let lock = s.find("mkdir \"$LOCK\"").unwrap();
            assert!(
                lock < s.find("mount -o remount,rw").unwrap(),
                "lock before any write"
            );
            assert!(s.contains("flock -n /tmp/entry-aa-update.lock"));
            assert!(
                s.contains("= \"$$\" ] && rm -rf \"$LOCK\""),
                "only the owner releases the lock"
            );
        }
        let s = install_script("0.12.0");
        assert!(s.contains("rm -f /var/local/data/AA/update-pending"));
        let c = clean_others_script("0.12.0");
        assert!(c.contains("/tmp/entrynav2-android-auto-0.12.0|"));
    }

    #[test]
    fn stop_script_never_kills_the_supervisor() {
        assert!(!STOP_SCRIPT.contains("-KILL $(pidof aa-supervisor"));
        assert!(STOP_SCRIPT.contains("/tmp/entry-aa-service/STOP"));
    }

    #[test]
    fn check_script_requires_the_programs() {
        let s = check_script("0.12.0");
        assert!(s.find("is missing from the archive").unwrap() < s.find("md5sum -c").unwrap());
    }

    #[test]
    fn df_parsing() {
        assert_eq!(
            parse_df_available("tmpfs 102400 2048 100352 2% /tmp"),
            Some(100352)
        );
        assert_eq!(
            parse_df_available("  102400 2048 100352 2% /tmp"),
            Some(100352)
        );
        assert_eq!(parse_df_available("garbage"), None);
    }

    #[test]
    fn install_script_is_detached_and_safe() {
        let s = install_script("0.12.0");
        assert!(s.contains("V=0.12.0") && !s.contains("__V__"));
        assert!(s.contains("trap '' HUP PIPE"));
        assert!(s.contains("pidof aa-supervisor entry-aad"));
        assert!(s.contains(".installed-by-entryaad\" ]"));
        // The pre-flight checks come before the filesystem is made writable.
        assert!(s.find("pidof").unwrap() < s.find("mount -o remount,rw").unwrap());
        assert!(s.find("md5sum -c MD5SUMS").unwrap() < s.find("mount -o remount,rw").unwrap());
        assert!(check_script("0.13.0-rc.9").contains("V=0.13.0-rc.9"));
    }

    #[test]
    fn uninstall_script_guards() {
        let s = uninstall_script(true);
        assert!(s.contains("trap '' HUP PIPE"));
        assert!(s.find("pidof").unwrap() < s.find("mount -o remount,rw").unwrap());
        assert!(!s.contains("rm -rf /var/local/data/AA"));
        assert!(uninstall_script(false).contains("rm -rf /var/local/data/AA"));
    }

    #[test]
    fn quoting_and_vehicle() {
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
        let v = Vehicle {
            model: "320d".into(),
            year: "2019".into(),
            fuel: Fuel::Diesel,
            side: Side::Left,
        };
        assert!(v.validate().is_ok());
        let s = v.script();
        assert!(
            s.contains(r"fuel=%s\nside=left\n' '320d' '2019' diesel"),
            "{s}"
        );
        let ok = Vehicle {
            model: "Cooper S".into(),
            ..v.clone()
        };
        assert!(ok.validate().is_ok());
        let too_long = Vehicle {
            model: "a".repeat(25),
            ..v.clone()
        };
        assert!(too_long.validate().is_err());
        let bad = Vehicle {
            model: "a;rm -rf /".into(),
            ..v
        };
        assert!(bad.validate().is_err());
    }

    /// Writes every generated script to $SCRIPT_DUMP_DIR so they can be
    /// reviewed or linted (`sh -n`, shellcheck). Run with:
    /// `SCRIPT_DUMP_DIR=out cargo test dump_scripts -- --ignored`
    #[test]
    #[ignore]
    fn dump_scripts() {
        let dir =
            std::path::PathBuf::from(std::env::var("SCRIPT_DUMP_DIR").expect("SCRIPT_DUMP_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let v = Vehicle {
            model: "320d".into(),
            year: "2019".into(),
            fuel: Fuel::Diesel,
            side: Side::Right,
        };
        for (name, s) in [
            (
                "info.sh",
                info_script(&[
                    "0.12.0".into(),
                    "0.13.0".into(),
                    "0.13.0-rc.1".into(),
                    "0.13.0-rc.10".into(),
                ]),
            ),
            ("check.sh", check_script("0.12.0")),
            ("install.sh", install_script("0.12.0")),
            ("cleanup.sh", cleanup_script("0.12.0")),
            ("vehicle.sh", v.script()),
            ("uninstall-keep.sh", uninstall_script(true)),
            ("uninstall-wipe.sh", uninstall_script(false)),
            ("enable.sh", ENABLE_SCRIPT.to_string()),
            ("disable.sh", DISABLE_SCRIPT.to_string()),
            ("stop.sh", STOP_SCRIPT.to_string()),
            ("usb.sh", USB_MODE_SCRIPT.to_string()),
            ("clean-others.sh", clean_others_script("0.12.0")),
        ] {
            std::fs::write(dir.join(name), s).unwrap();
        }
    }

    #[test]
    #[should_panic]
    fn unsafe_version_rejected() {
        install_script("1.0;reboot");
    }
}
