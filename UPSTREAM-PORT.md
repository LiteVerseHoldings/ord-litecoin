# Litecoin Upstream Port

## Scope

The upstream source is [ordinals/ord](https://github.com/ordinals/ord).
The latest stable release checked on 2026-10-26 is
[0.29.1](https://github.com/ordinals/ord/releases/tag/0.29.1), commit
`d2ff0a789033e3d784048a23e856f15c54e88806`.

This fork's common upstream ancestor is 0.20.1, commit
`80f8caed9fe670443b59e895aabaff998adea9b7`. The first update is a set of
compatible backports, not a wholesale 0.29.1 merge or a new protocol version.
The package version remains 0.20.1 to avoid implying that the full port is done.

## First Backports

| Upstream change | Litecoin adaptation |
| --- | --- |
| [Reject service-worker script requests](https://github.com/ordinals/ord/commit/ed4f26bb5b3eb283d3cf75056ecec6e79f681c54) | Apply middleware to content, previews, proxied content, and fallback paths using the existing Axum 0.6 API. Keep sandboxed HTML previews. |
| [Checked decimal addition](https://github.com/ordinals/ord/commit/00d410a7) and [excessive precision errors](https://github.com/ordinals/ord/commit/899e3094) | Reject multiplication/addition overflow and excessive precision rather than panicking or wrapping amounts. Include a maximum representable amount regression test. |
| [JSON API batch limits](https://github.com/ordinals/ord/commit/140dd899) | Limit existing `/outputs` and `/inscriptions` batch endpoints; reject null and unbound aggregate outputs. Use a default of 100 items and a 2 MiB body limit rather than leaving public requests unbounded. |
| [Honor configured HTTP port](https://github.com/ordinals/ord/commit/fd152bf9) | Read `http_port` from existing settings, with a command-line argument taking precedence. Preserve HTTPS-only behavior without an explicit HTTP port. |
| [Synchronize wallet test commands](https://github.com/ordinals/ord/commit/54d2a3eb) | Synchronize the isolated test index before invoking wallet commands, honor test-server authentication, and preserve no-sync tests. Avoid racing the background indexer on busy CI runners. |

`server --batch-limit N`, `ORD_BATCH_LIMIT=N`, or `batch_limit: N` in
`ord.yaml` changes the item limit. A limit of zero allows only empty batches;
it does not disable the limit. Batches larger than the configured limit return
HTTP 400, and JSON bodies larger than 2 MiB return HTTP 413. Large callers must
split requests. Disabling the JSON API still disables those endpoints.

The RPC cookie search now uses Litecoin's default data directory, not the
upstream network's directory. Explicit data-directory and cookie-file options
continue to override the defaults.

## Litecoin Dependencies

The cryptographic/network dependency remains the existing Litecoin code at
`c4938f90d533eb6d730f4f7677f4ac155033cf97`, now pinned to the owned
[rust-litecoin](https://github.com/LiteVerseHoldings/rust-litecoin) repository.
The crate retains its upstream Rust package name for API compatibility; it is
not the unmodified crates.io implementation.

Both RPC crates are pinned to
`9268a687f419ea96de0faffa164f3c6e1cdaac7f` in the owned
[RPC fork](https://github.com/LiteVerseHoldings/rust-bitcoincore-rpc).
This adds the existing fork's MWEB peg-in and HogAddr response types and avoids
mixing revisions of the RPC client and JSON types. `Cargo.lock` records these
sources. Build and test with `--locked`.

Regression tests guard the Litecoin mainnet genesis block and transaction,
840,000-block halving interval, 2,016-block difficulty interval, exact litoshi
supply, address prefixes, RPC ports, MWEB RPC variants, and existing activation
heights. These tests do not claim full MWEB consensus validation.

## Full Port Still Required

| Area | Remaining work before a full 0.29.1 port |
| --- | --- |
| Rust and dependencies | Upstream requires Rust 1.95 and the 2024 edition, network crate 0.32, and RPC crate 0.19. Port Litecoin network, serialization, and RPC adaptations before updating these APIs. Do not replace them with unmodified upstream dependencies. |
| Index format | This fork uses index schema 28; upstream uses 34 and redb 3 rather than redb 2. Validate a separate rebuild/shadow index. Never point a candidate at the only production index or delete that index as an upgrade step. |
| Historical semantics | Preserve Litecoin inscription start, Jubilee, rune activation/unlock schedule, supply, halving/cycle rules, MWEB filtering, and transaction/output identity. Compare historical results against the current production index. |
| Wallet operations | Upstream's descriptor-wallet requirements do not match this fork's legacy Litecoin wallet path. Adapt and test against the actual supported Litecoin Core version, including address generation, PSBT signing, fee units, restoration, and inscription preservation. Do not bypass version checks by inventing a newer Core version. |
| Raw block API | `/r/block/<query>` must preserve actual Litecoin/MWEB wire bytes. Do not advertise a reserialized, potentially incomplete block as raw block hex. |
| New protocols and endpoints | Review gallery/metadata changes, offers, rune changes, pay-to-anchor assumptions, and recursive APIs individually. Upstream activation rules must not silently redefine existing Litecoin assets. |
| Reorganizations | Audit and preserve the custom production recovery build, then port upstream savepoint/reorganization fixes with restart and rollback tests. |
| Dependency advisories | GitHub reported 46 open alerts, including 13 high-severity alerts, after the first backport push. Alerts cover both the application and the separate fuzz lockfile. Prioritize the TLS/certificate stack and compatible patched transitive versions; tests passing does not resolve these alerts. |

CI validates the inherited handbook using mdBook 0.4.52 and gettext helpers
0.3.5, which support its existing theme and preprocessor format. Automatic
Pages publication is disabled in this fork, and the
inherited upstream CNAME is removed. Litecoin-specific documentation and an
explicitly owned destination must be established before enabling publication.

## Verification And Deployment

Local tests use mock nodes and temporary data directories, never a funded
wallet. Run:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all --all-targets
cargo test --locked --all -- --test-threads=4
```

The Windows verification on 2026-10-26 passed 1,081 tests: 667 library tests,
253 integration tests, and 161 ordinals tests. Formatting and Clippy with
`-D warnings` also passed. Cargo still reports a future-compatibility notice
for the existing transitive `net2 0.2.39` dependency; this is not a clean bill of
health for every dependency or a substitute for the full dependency port.

A real-node smoke test is available on Windows:

```powershell
.\scripts\test-litecoin-regtest.ps1 -Litecoind C:\path\litecoind.exe -LitecoinCli C:\path\litecoin-cli.exe
```

The harness refuses inherited `ORD_*` overrides, creates new temporary data
directories, explicitly selects regtest, disables peer connections, and binds
RPC/HTTP to loopback. It checks chain/genesis/output identity, live block
progression, batch limits, and service-worker rejection, then stops both test
processes. It never connects to the existing node or creates/loads a wallet.
The local node used for this smoke test reports `v0.21.5.4-unk` and is a local
signet-capable build, not the production `v0.21.2.2` binary. Passing this smoke
test does not establish compatibility with every Core version or MWEB history.

The production service inspected on 2026-10-26 reports `ord-litecoin 0.18.3`
and runs a custom recovery executable, not this checkout. The available source
directory is not a Git checkout. That means even these schema-28 backports
cannot be assumed to be a drop-in replacement for the deployed index.

Before production deployment:

1. Archive and hash the exact running binary and matching source, preserve
   configuration privately, and identify all custom recovery changes.
2. Pass Linux and Windows tests, then exercise an isolated Litecoin regtest node
   and real historical fixtures including MWEB peg-in/HogEx cases.
3. Build a separate shadow index without access to production wallet RPCs.
   Compare tip identity, historical inscription IDs/numbers, ownership outputs,
   rune balances, metadata, and website API contracts.
4. Verify interrupted sync, restart, short reorganization, deep reorganization,
   missing/pruned data, and index-format rejection behavior.
5. Schedule a backed-up cutover with a retained binary/index rollback pair and
   read-only smoke tests. No real purchases, inscription orders, or broadcasts
   are part of this verification.

The running Ord service and its wallet/index have not been replaced by this
backport pass. Build success alone is not production upgrade approval.
