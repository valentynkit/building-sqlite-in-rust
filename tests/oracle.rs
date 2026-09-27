//! Runs every stage query through this binary and through the real `sqlite3`, and
//! requires identical output. Databases are gitignored; fetch them with
//! `./download_sample_databases.sh`. Missing databases are skipped, not failed.

use std::{path::Path, process::Command};

const CASES: &[(&str, &str)] = &[
    ("sample.db", ".tables"),
    ("sample.db", "SELECT COUNT(*) FROM apples"),
    ("sample.db", "select count(*) from oranges"),
    ("sample.db", "SELECT name FROM apples"),
    ("sample.db", "SELECT name, color FROM apples"),
    ("sample.db", "SELECT id, name FROM apples"),
    (
        "sample.db",
        "SELECT name, color FROM apples WHERE color = 'Yellow'",
    ),
    ("superheroes.db", "SELECT COUNT(*) FROM superheroes"),
    ("superheroes.db", "SELECT id, name FROM superheroes"),
    (
        "superheroes.db",
        "SELECT id, name FROM superheroes WHERE eye_color = 'Pink Eyes'",
    ),
    ("companies.db", "SELECT COUNT(*) FROM companies"),
    (
        "companies.db",
        "SELECT id, name FROM companies WHERE country = 'eritrea'",
    ),
    (
        "companies.db",
        "SELECT id, name FROM companies WHERE country = 'republic of the congo'",
    ),
    (
        "companies.db",
        "SELECT id, name FROM companies WHERE country = 'no such place'",
    ),
];

fn stdout(program: &str, db: &str, sql: &str) -> String {
    let out = Command::new(program)
        .args([db, sql])
        .env("RUST_LOG", "off")
        .output()
        .unwrap_or_else(|e| panic!("running {program}: {e}"));
    assert!(
        out.status.success(),
        "{program} {db} {sql:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

/// The shell pads `.tables` into columns; compare the names only.
fn normalize(sql: &str, output: &str) -> String {
    if sql == ".tables" {
        output.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        output.to_owned()
    }
}

#[test]
fn matches_sqlite3() {
    let ours = env!("CARGO_BIN_EXE_codecrafters-sqlite");
    let mut ran = 0;
    for &(db, sql) in CASES {
        if !Path::new(db).exists() {
            eprintln!("skipping {db}: not downloaded");
            continue;
        }
        let expected = normalize(sql, &stdout("sqlite3", db, sql));
        let actual = normalize(sql, &stdout(ours, db, sql));
        assert_eq!(actual, expected, "{db}: {sql}");
        ran += 1;
    }
    assert!(
        ran > 0,
        "no databases found; run ./download_sample_databases.sh"
    );
}
