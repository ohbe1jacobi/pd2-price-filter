# PD2 Price Filter

Injects live market prices into a Project Diablo 2 loot filter, so the ground label tells you what an item is worth before you pick it up.

[![Tests](https://github.com/314159DD/pd2-price-filter/actions/workflows/test.yml/badge.svg)](https://github.com/314159DD/pd2-price-filter/actions/workflows/test.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.95-000000?logo=rust&logoColor=white)

```
Griswold's Valor | 0.1 HR
Ber Rune | 2.4 HR
```

Identified uniques, set items, runes, and uber materials get a price tag (median price over the last 30 days, in High Runes). The tool no longer adds its own potion rules: potion visibility stays exactly as the selected filter's author defined it.

The application is a Rust CLI. A GUI and native packaging are planned but not implemented yet.

## How it works

```
source filter (online/)          pd2trader.com (30-day sales)        pd2-aggregator snapshot
        |                                  |                                   |
        |                          median per item, per mode          roster of uniques / sets
        |                                  |                                   |
        +---------------> decorate: rewrite ItemDisplay rules <----------------+
                                           |
                     filters/local/HC_Stonks_<name>.filter  (or SC_Stonks_)
```

- **Prices** are the median of recorded sales on pd2trader over a 30-day window. Hardcore and softcore are priced separately. A run targets one mode.
- **Roster** of which uniques and set items exist comes from the [pd2-aggregator](https://github.com/314159DD/pd2-aggregator) snapshot. Filter base codes are harvested from the PD2 market API and cached in `data/base-codes.json`.
- **Cache**: prices are cached per mode with a 24-hour freshness window (`data/price-cache-hc.json`, `data/price-cache-sc.json`). The first run takes a few minutes. A same-day re-run is near-instant.
- **Fixed items**: `data/pd2trader-items.json` is the curated list of runes and uber materials. Add an entry (`name`, `condition`, `category`) to price any other fixed item.
- **Potion behavior**: unchanged from the source filter. Earlier releases injected an overriding potion block; Rust generation strips legacy potion blocks from a previous output and never adds a new one.

## Usage (Rust)

```
cargo build --release --locked
./target/release/pd2-price-filter --help
./target/release/pd2-price-filter                       # hardcore filter from the default PD2 path
./target/release/pd2-price-filter --softcore            # softcore filter
./target/release/pd2-price-filter SomeOther.filter      # another source filter
./target/release/pd2-price-filter --refresh-prices      # refetch prices, keeping prior successes on failure
./target/release/pd2-price-filter --refresh-bases       # full base-code re-harvest
./target/release/pd2-price-filter --source in.filter --output out.filter --data-dir data --overwrite
```

Defaults match the previous CLI: hardcore ladder, paths under `C:\Program Files (x86)\Diablo II\ProjectD2\filters`, an output named `HC_Stonks_<name>.filter` (`SC_Stonks_` in softcore), and a `data/` directory (override with `--data-dir` or `PD2_PRICE_FILTER_DATA`). The source filter is never modified. An existing output requires `--overwrite`.

`--offline --roster-file FILE` is an explicit development/test mode: it reuses the local cache without network access.

Select the generated copy in the launcher via **Local Filter**; the tool does not change launcher settings.

The Rust CLI needs no Node runtime. It still expects a Project Diablo 2 install for the filter files.

## Limitations

- Identified uniques, set items, runes, and uber materials only. Rares, magics, and bases are untouched.
- A base shared by multiple uniques shows a combined price range.
- Ladder prices only.

## Development

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets          # 65 tests: unit + offline end-to-end CLI
```

Provider, roster and base-code tests run against a loopback HTTP fixture; the suite makes no public-network requests. `nix-shell` provides the Rust toolchain plus `gcc`.

## Scope status

This release is a behavior-preserving Rust conversion plus two narrow changes: the
potion override is gone, and price/cache handling now distinguishes no-data from
failure, validates responses, bounds requests, keeps prior successes across a
refresh, and writes caches atomically. The following are deliberately deferred and
are not implemented:

- a preservation-safe filter transformer that annotates a proven final visible-name rule instead of inserting an early price block;
- a build-time item catalog (official MPQ acquisition, extraction, normalization, embedded JSON);
- batch/date-bounded pricing, season-aware cache keys, 10-sample confidence policy, stale-cache policy;
- the desktop GUI and Windows/Linux/Nix/Flatpak packaging.

The catalog architecture decision (build-time acquisition and extraction in CI,
hash-pinned snapshots, an embedded read-only JSON catalog at runtime) is an
accepted design, not implemented behavior. Internal planning documents, the
phased roadmap, and design notes are maintained outside this repository.

## License

MIT. See [LICENSE](LICENSE).
