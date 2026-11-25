//! End-to-end test against the fake head unit of tests/car (run with tests/car/run-e2e.sh).

use crate::car::{Car, ConnParams};
use crate::scripts::{Fuel, Installed, Side, Vehicle, Verdict};
use crate::worker::{self, Ctx, InstallOptions, Level, Msg, Source};
use anyhow::Result;
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::channel;

struct Run {
    result: Result<()>,
    msgs: Vec<Msg>,
}

impl Run {
    fn log(&self) -> String {
        self.msgs
            .iter()
            .filter_map(|m| match m {
                Msg::Log(l, s) => Some(format!("{l:?}: {s}")),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn ok(self, what: &str) -> Self {
        if let Err(e) = &self.result {
            panic!("{what} failed: {e:#}\n{}", self.log());
        }
        self
    }

    fn err(self, what: &str, needle: &str) -> Self {
        match &self.result {
            Ok(()) => panic!("{what} should have failed\n{}", self.log()),
            Err(e) => {
                let e = format!("{e:#}");
                assert!(
                    e.contains(needle),
                    "{what}: expected \"{needle}\", got \"{e}\""
                );
            }
        }
        self
    }

    fn has_error_log(&self) -> bool {
        self.msgs
            .iter()
            .any(|m| matches!(m, Msg::Log(Level::Error, _)))
    }
}

fn run(f: impl FnOnce(&Ctx) -> Result<()>) -> Run {
    let (tx, rx) = channel();
    let ctx = Ctx::new(tx, egui::Context::default());
    let result = f(&ctx);
    drop(ctx);
    Run {
        result,
        msgs: rx.try_iter().collect(),
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set: run tests/car/run-e2e.sh"))
}

fn sh(conn: &ConnParams, script: &str) -> String {
    let car = Car::connect(conn).expect("connect for a shell command");
    let (_, lines) = car.run_collect(script).expect("shell command");
    lines.join("\n")
}

fn exists(conn: &ConnParams, path: &str) -> bool {
    sh(conn, &format!("[ -e '{path}' ] && echo yes")) == "yes"
}

fn car_info(r: &Run) -> crate::scripts::CarInfo {
    r.msgs
        .iter()
        .rev()
        .find_map(|m| match m {
            Msg::CarInfo(i) => Some(i.clone()),
            _ => None,
        })
        .expect("car info")
}

#[test]
#[ignore]
fn e2e_fake_head_unit() {
    let conn = ConnParams {
        host: env("E2E_HOST"),
        port: env("E2E_PORT").parse().unwrap(),
        user: "root".into(),
        password: env("E2E_PASSWORD"),
    };
    let version = env("E2E_VERSION");
    let archive = PathBuf::from(env("E2E_ARCHIVE"));
    let sha = crate::github::sha256_file(&archive).unwrap();
    let source = || Source::Local(archive.clone(), version.clone(), Some(sha.clone()));
    let opts = |auto_stop| InstallOptions {
        version: version.clone(),
        vehicle: Some(Vehicle {
            model: "Mini One".into(),
            year: "2015".into(),
            fuel: Fuel::Petrol,
            side: Side::Auto,
        }),
        auto_stop,
        usb_mode: true,
        reboot: true,
    };

    sh(
        &conn,
        "rm -rf /opt/aa /opt/hmi /var/local/data/AA /tmp/entrynav2-* /tmp/entry-aa-service; \
         rm -f /sim/log/*",
    );

    println!("connection");
    let mut bad = conn.clone();
    bad.password = "wrong".into();
    run(|c| worker::test_connection(c, bad)).err("wrong password", "authentication refused");
    let r = run(|c| worker::test_connection(c, conn.clone())).ok("connection");
    let info = car_info(&r);
    assert_eq!(info.sysver, "EntryNav2_SIM");
    assert_eq!(info.installed, Installed::None);
    assert!(!info.running);

    println!("check");
    let wrong = Source::Local(archive.clone(), version.clone(), Some("0".repeat(64)));
    run(|c| worker::prepare(c, conn.clone(), wrong, false))
        .err("wrong SHA-256", "SHA-256 mismatch");
    assert!(!exists(
        &conn,
        &format!("/tmp/entrynav2-android-auto-{version}-armhf.tar")
    ));
    let r = run(|c| worker::prepare(c, conn.clone(), source(), false)).ok("check");
    let report = r
        .msgs
        .iter()
        .find_map(|m| match m {
            Msg::Checked(rep) => Some(rep.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no check report\n{}", r.log()));
    assert_eq!(report.verdict(), Verdict::Ok, "{report:?}\n{}", r.log());
    assert!(report.archive_verified);
    assert!(exists(
        &conn,
        &format!("/tmp/entrynav2-android-auto-{version}")
    ));

    println!("install while Android Auto runs");
    sh(
        &conn,
        "setsid /sim/fake/aa-supervisor </dev/null >/dev/null 2>&1 &",
    );
    let r = run(|c| worker::test_connection(c, conn.clone())).ok("status");
    assert!(
        car_info(&r).running,
        "the fake supervisor is not seen\n{}",
        r.log()
    );
    run(|c| worker::install(c, conn.clone(), opts(false))).err("install while running", "running");
    assert!(!exists(&conn, "/opt/aa"));
    assert!(!exists(&conn, "/sim/log/mount.log"), "/ was remounted");

    println!("install");
    let r = run(|c| worker::install(c, conn.clone(), opts(true))).ok("install");
    assert!(r
        .msgs
        .iter()
        .any(|m| matches!(m, Msg::Installed(v) if *v == version)));
    assert!(!r.has_error_log(), "{}", r.log());
    assert_eq!(sh(&conn, "cat /opt/aa/.installer-version"), version);
    for f in [
        "/opt/aa/entry-aad",
        "/opt/aa/aa-supervisor",
        "/opt/aa/.installed-by-entryaad",
        "/opt/hmi/data/ro/Plugins/EntryAad/libEntryAad.so",
        "/opt/hmi/data/ro/Plugins/EntryAad/.installed-by-entryaad",
        "/var/local/data/AA/usb",
    ] {
        assert!(exists(&conn, f), "{f} missing after install");
    }
    assert_eq!(
        sh(&conn, "cat /sim/log/mount.log"),
        "-o remount,rw /\n-o remount,ro /"
    );
    assert_eq!(
        sh(&conn, "cat /var/local/data/AA/vehicle"),
        "model=Mini One\nyear=2015\nfuel=petrol"
    );
    assert!(!exists(
        &conn,
        &format!("/tmp/entrynav2-android-auto-{version}")
    ));
    assert!(!exists(&conn, "/tmp/entrynav2-installer.lock"));
    assert_eq!(sh(&conn, "cat /sim/log/reboot.log"), "reboot");
    let r = run(|c| worker::test_connection(c, conn.clone())).ok("status");
    let info = car_info(&r);
    assert_eq!(info.installed, Installed::Version(version.clone()));
    assert!(!info.running && info.usb_mode);

    println!("disable / enable");
    run(|c| worker::set_enabled(c, conn.clone(), true, false)).err("enable", "already enabled");
    run(|c| worker::set_enabled(c, conn.clone(), false, false)).ok("disable");
    assert!(exists(&conn, "/var/local/data/AA/app.disable"));
    run(|c| worker::set_enabled(c, conn.clone(), false, false)).err("disable", "already disabled");
    run(|c| worker::set_enabled(c, conn.clone(), true, false)).ok("enable");
    assert!(!exists(&conn, "/var/local/data/AA/app.disable"));

    println!("uninstall");
    sh(&conn, "rm -f /sim/log/*");
    let r = run(|c| worker::uninstall(c, conn.clone(), true, false, false)).ok("uninstall");
    assert!(r.msgs.iter().any(|m| matches!(m, Msg::Uninstalled)));
    assert!(!exists(&conn, "/opt/aa"));
    assert!(!exists(&conn, "/opt/hmi/data/ro/Plugins/EntryAad"));
    assert!(exists(&conn, "/opt/hmi/data/ro/Plugins"));
    assert!(
        exists(&conn, "/var/local/data/AA/vehicle"),
        "settings not kept"
    );
    assert_eq!(
        sh(&conn, "cat /sim/log/mount.log"),
        "-o remount,rw /\n-o remount,ro /"
    );
    run(|c| worker::uninstall(c, conn.clone(), true, false, false))
        .err("uninstall again", "not installed");

    println!("foreign /opt/aa");
    sh(
        &conn,
        "mkdir -p /opt/aa && echo x > /opt/aa/foreign && rm -f /sim/log/*",
    );
    run(|c| worker::prepare(c, conn.clone(), source(), false)).ok("check");
    let r = run(|c| worker::install(c, conn.clone(), opts(true)))
        .err("install over a foreign folder", "not created by EntryNav2");
    assert!(r.msgs.iter().any(|m| matches!(m, Msg::InstallFailed(_))));
    assert!(exists(&conn, "/opt/aa/foreign"), "foreign file touched");
    assert!(!exists(&conn, "/sim/log/mount.log"), "/ was remounted");
    assert!(
        !exists(&conn, "/sim/log/reboot.log"),
        "rebooted after a failure"
    );
}
