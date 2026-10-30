use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, PartialEq)]
pub struct Options {
    pub source: PathBuf,
    pub output: PathBuf,
    pub mode: String,
    pub values: BTreeMap<String, String>,
    pub refresh: bool,
    pub offline: bool,
    pub refresh_bases: bool,
    pub overwrite: bool,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut values = BTreeMap::new();
    let mut flags = std::collections::BTreeSet::new();
    let mut positional = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source" | "--output" | "--data-dir" | "--roster-file" | "--fixed-items"
            | "--prices-file" => {
                let value = args
                    .next()
                    .filter(|s| !s.is_empty() && !s.starts_with("--"))
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                if values.insert(arg.clone(), value).is_some() {
                    return Err(format!("duplicate {arg}"));
                }
            }
            "--hardcore" | "--softcore" | "--refresh-prices" | "--refresh-bases" | "--offline"
            | "--overwrite" => {
                if !flags.insert(arg.clone()) {
                    return Err(format!("duplicate {arg}"));
                }
            }
            _ if !arg.starts_with('-') && positional.is_none() => positional = Some(arg),
            _ => return Err(format!("unknown argument: {arg}; use --help")),
        }
    }
    if flags.contains("--hardcore") && flags.contains("--softcore") {
        return Err("choose only one game mode".into());
    }
    if flags.contains("--offline") && flags.contains("--refresh-prices") {
        return Err("--offline conflicts with --refresh-prices".into());
    }
    if flags.contains("--offline") && flags.contains("--refresh-bases") {
        return Err("--offline conflicts with --refresh-bases".into());
    }
    if positional.is_some() && values.contains_key("--source") {
        return Err("choose a positional filter or --source, not both".into());
    }
    let mode = if flags.contains("--softcore") {
        "softcore"
    } else {
        "hardcore"
    }
    .to_string();
    let filters = PathBuf::from(r"C:\Program Files (x86)\Diablo II\ProjectD2\filters");
    let source: PathBuf = values
        .remove("--source")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            filters
                .join("online")
                .join(positional.unwrap_or_else(|| "Hiim_Crafting_Paladin_Focused.filter".into()))
        });
    let stem = source
        .file_stem()
        .ok_or("source needs a filename")?
        .to_str()
        .ok_or("source filename must be Unicode")?;
    let prefix = if mode == "hardcore" { "HC" } else { "SC" };
    let output = values
        .remove("--output")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            filters
                .join("local")
                .join(format!("{prefix}_Stonks_{stem}.filter"))
        });
    Ok(Options {
        source,
        output,
        mode,
        values,
        refresh: flags.contains("--refresh-prices"),
        offline: flags.contains("--offline"),
        refresh_bases: flags.contains("--refresh-bases"),
        overwrite: flags.contains("--overwrite"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_args(args: &[&str]) -> Result<Options, String> {
        parse(args.iter().map(|s| s.to_string()))
    }
    #[test]
    fn preserves_hardcore_default_and_positional_filter() {
        let o = parse_args(&[]).unwrap();
        assert_eq!(o.mode, "hardcore");
        assert!(!o.refresh);
        let o = parse_args(&["Other.filter", "--softcore"]).unwrap();
        assert!(o.source.ends_with("Other.filter"));
        assert!(o.output.ends_with("SC_Stonks_Other.filter"));
    }
    #[test]
    fn rejects_ambiguous_or_unknown_flags() {
        for args in [
            vec!["--source", "a", "--output", "b", "--hardcore", "--softcore"],
            vec!["--source", "a", "--output", "b", "--wat"],
            vec!["--source", "a", "--source", "b", "--output", "c"],
            vec![
                "--source",
                "a",
                "--output",
                "b",
                "--refresh-prices",
                "--offline",
            ],
        ] {
            assert!(parse_args(&args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn preserves_unicode_paths_and_original_refresh_flags() {
        let o = parse_args(&[
            "--source",
            "é a",
            "--output",
            "目标",
            "--hardcore",
            "--data-dir",
            "缓存",
            "--refresh-bases",
            "--overwrite",
        ])
        .unwrap();
        assert_eq!(o.source, PathBuf::from("é a"));
        assert_eq!(o.mode, "hardcore");
        assert!(o.overwrite && o.refresh_bases);
        assert_eq!(o.values["--data-dir"], "缓存");
    }
}
