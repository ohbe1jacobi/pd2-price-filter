//! End-to-end CLI checks against offline fixtures. No network access.
use std::{fs, process::Command};

fn scratch(name: &str) -> std::path::PathBuf {
    let root = std::env::var_os("TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("pd2-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("data")).unwrap();
    dir
}

fn fixture(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let source = dir.join("Autor filter é.filter");
    fs::write(&source, "// Author filter\r\nItemDisplayFilterName[Strictness]:\r\nItemDisplay[RUNE=30]: Ber Rune\r\nItemDisplay[hp1]:\r\nItemDisplay[UNI ID uap]: Shako\r\n").unwrap();
    let roster = dir.join("roster.json");
    fs::write(&roster, r#"{"items":{"Shako":{"type":"Unique","uniqueId":1},"Aldur's Advance":{"type":"Set"},"Runeword":{"type":"Runeword","uniqueId":9}}}"#).unwrap();
    fs::write(
        dir.join("data/base-codes.json"),
        r#"{"uniques":{"1":"uap"},"sets":{"Aldur's Advance":"xtb"}}"#,
    )
    .unwrap();
    fs::write(dir.join("data/pd2trader-items.json"), r#"[{"name":"Ber Rune","condition":"RUNE=30","category":"rune"},{"name":"Key of Terror","condition":"pk1","category":"uber"}]"#).unwrap();
    fs::write(dir.join("prices.json"), r#"{"Shako":{"median":2.5,"medianSamples":20,"fetchedAt":"2026-10-30T00:00:00Z"},"Aldur's Advance":{"median":0.1,"medianSamples":2,"fetchedAt":"2026-10-30T00:00:00Z"},"Ber Rune":{"median":3,"medianSamples":100,"fetchedAt":"2026-10-30T00:00:00Z"},"Runeword":{"median":9,"medianSamples":100,"fetchedAt":"2026-10-30T00:00:00Z"}}"#).unwrap();
    (source, roster)
}

fn run(
    dir: &std::path::Path,
    source: &std::path::Path,
    roster: &std::path::Path,
    output: &std::path::Path,
    extra: &[&str],
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pd2-price-filter"))
        .arg("--source")
        .arg(source)
        .arg("--output")
        .arg(output)
        .arg("--data-dir")
        .arg(dir.join("data"))
        .arg("--prices-file")
        .arg(dir.join("prices.json"))
        .arg("--roster-file")
        .arg(roster)
        .arg("--offline")
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn offline_run_writes_a_priced_derivative_without_potion_override() {
    let dir = scratch("happy");
    let (source, roster) = fixture(&dir);
    let original = fs::read(&source).unwrap();
    let output = dir.join("SC priced.filter");
    let result = run(&dir, &source, &roster, &output, &["--softcore"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let written = fs::read(&output).unwrap();
    let text = String::from_utf8(written.clone()).unwrap();
    assert!(text.contains("ItemDisplay[RUNE=30]: %NAME%%GOLD% | 3 HR%CONTINUE%"));
    assert!(text.contains("ItemDisplay[UNI ID uap]: %NAME%%GOLD% | 2.5 HR%CONTINUE%"));
    assert!(text.contains("ItemDisplay[SET ID xtb]: %NAME%%GOLD% | 0.1 HR?%CONTINUE%"));
    assert!(
        !text.contains("PD2 POTION DISPLAY"),
        "potion override must be removed"
    );
    assert!(
        !text.contains("Runeword"),
        "unpriced/roster-only runewords must not be priced"
    );
    assert!(
        text.contains("// Author filter\r\n"),
        "author CRLF bytes must survive"
    );
    assert!(text.find("PD2 PRICE TAGS").unwrap() < text.find("// Author filter").unwrap());
    assert_eq!(
        fs::read(&source).unwrap(),
        original,
        "source must be untouched"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn existing_output_requires_explicit_overwrite_and_matches_rerun_determinism() {
    let dir = scratch("overwrite");
    let (source, roster) = fixture(&dir);
    let output = dir.join("out.filter");
    assert!(run(&dir, &source, &roster, &output, &[]).status.success());
    let first = fs::read(&output).unwrap();
    let refused = run(&dir, &source, &roster, &output, &[]);
    assert!(!refused.status.success());
    assert_eq!(
        fs::read(&output).unwrap(),
        first,
        "refusal must not touch the previous output"
    );
    assert!(
        run(&dir, &source, &roster, &output, &["--overwrite"])
            .status
            .success()
    );
    assert_eq!(
        fs::read(&output).unwrap(),
        first,
        "regeneration must be deterministic"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn generated_input_and_bad_caches_fail_descriptively() {
    let dir = scratch("refuse");
    let (source, roster) = fixture(&dir);
    let output = dir.join("out.filter");
    assert!(run(&dir, &source, &roster, &output, &[]).status.success());
    let derived = dir.join("derived.filter");
    fs::copy(&output, &derived).unwrap();
    let result = run(&dir, &derived, &roster, &dir.join("again.filter"), &[]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("generated block"));

    fs::write(
        dir.join("data/price-cache-hardcore.json"),
        "{\"Ber Rune\":{\"median\":-1,\"medianSamples\":2,\"fetchedAt\":\"2026-10-30T00:00:00Z\"}}",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_pd2-price-filter"))
        .arg("--source")
        .arg(&source)
        .arg("--output")
        .arg(dir.join("other.filter"))
        .arg("--data-dir")
        .arg(dir.join("data"))
        .arg("--roster-file")
        .arg(&roster)
        .arg("--offline")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid cached price"));
    fs::remove_dir_all(dir).unwrap();
}
