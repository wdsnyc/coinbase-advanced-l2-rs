# coinbase-advanced-l2-rs

A Rust port of [`coinbase-advanced-l2-cpp`](https://github.com/wdsnyc/coinbase-advanced-l2-cpp) — a feed handler for Coinbase Advanced Trade's `level2` WebSocket channel that maintains a real-time, per-symbol order book and displays it interactively in the terminal.

## Overview

The feed connects to Coinbase's Advanced Trade WebSocket API, subscribes to the `level2` channel for one or more products, and applies incoming snapshot/update messages to an in-memory order book. A second thread lets you pause the display and switch which symbol's book is currently shown, without interrupting the underlying feed.

A few deliberate design choices worth knowing before reading the code:

- **Synchronous, not async.** The connection uses the plain [`tungstenite`](https://docs.rs/tungstenite) crate (blocking `.read()`/`.send()`), not `tokio`. This mirrors the C++ version's synchronous Boost.Asio design — there was never a concurrency requirement that justified the added complexity of an async runtime.
- **Authentication goes through Coinbase's own Python SDK, not hand-rolled crypto.** Both WS and REST JWTs are built by calling `coinbase-advanced-py`'s `jwt_generator` via [PyO3](https://pyo3.rs), rather than reimplementing ES256 signing directly in Rust. This avoids drift from Coinbase's own sanctioned implementation.
- **Prices and sizes are fixed-point, not floating-point.** Every price/quantity is converted to an `i64` "ticks" representation at parse time, using each product's actual `quote_increment`/`base_increment` (fetched from Coinbase's REST API at startup) rather than a guessed or hardcoded scale — necessary since different products have genuinely different tick sizes.

## Requirements

- Rust (2021 edition toolchain)
- Python 3, with [`coinbase-advanced-py`](https://github.com/coinbase/coinbase-advanced-py) installed and importable by the same `python3` your build links against (PyO3 embeds this interpreter — it needs to already have the package available, both at build time and run time)
- Coinbase Developer Platform (CDP) API credentials

## Setup

Create a directory containing two files, matching the format shown in the dummy `secrets/` directory committed to this repo (`secrets/api_key.txt`, `secrets/api_secret.pem` — placeholder content only, safe to commit):

- `api_key.txt` — your CDP API key name (e.g. `organizations/{org_id}/apiKeys/{key_id}`)
- `api_secret.pem` — the corresponding EC private key, in SEC1 PEM format (`-----BEGIN EC PRIVATE KEY-----`)

This directory's path is passed via `--secrets_dir` at runtime (see below). Your real credentials should live outside this repo — the `secrets/` directory here is a format example only, not where you should point `--secrets_dir` for real use.

## Building

```bash
cargo build
```

`Cargo.lock` is committed deliberately, not gitignored — standard practice for a binary crate (as opposed to a library), and worth doing here in particular: `pyo3` in this project had real breaking changes between versions during development (an early version didn't support the Python version this was built against, and its import/GIL API changed between releases), so pinning the exact versions known to build correctly is more valuable than usual.

## Usage

```bash
./target/debug/coinbase-advanced-l2-rs \
    --symbol_list "BTC-USD,ETH-USD" \
    --secrets_dir /path/to/secrets \
    [--no_snapshots]
```

| Flag | Required | Description |
|---|---|---|
| `--symbol_list` | Yes | Comma-separated list of product IDs, e.g. `"BTC-USD,ETH-USD"` |
| `--secrets_dir` | Yes | Directory containing `api_key.txt` and `api_secret.pem` |
| `--no_snapshots` | No | Skip applying snapshot messages — only incremental updates are applied. The book will be missing most of its levels for the entire session as a result (Coinbase sends exactly one snapshot per symbol, at subscribe time, not periodically), which is mainly useful for observing update volume without the initial full-book flood. |

### Interactive controls

Once connected, the order book for the first symbol in `--symbol_list` displays continuously, redrawing on each message. Press **Enter** to pause the display; you'll be prompted for a symbol from your subscribed list to switch the display to. The underlying feed and book keep updating in the background regardless of what's currently displayed.

## Testing

```bash
cargo test
```

Unit tests cover the pure-logic modules — order book mechanics, tick conversion, JSON message parsing, and CLI argument parsing. Code that requires a live network connection (REST calls, JWT generation, the WebSocket loop itself) isn't unit-tested; validating that requires a real run against Coinbase with real credentials.

## Project layout

| File | Responsibility |
|---|---|
| `main.rs` | Entry point — parses CLI args, runs a couple of standalone Arc/Mutex exercises, hands off to `feed::run` |
| `cli.rs` | Command-line argument definitions (`clap`) |
| `feed.rs` | WebSocket connect/subscribe/read loop, message dispatch, symbol-switch thread |
| `subscription.rs` | Builds the WS subscribe message and its JWT |
| `products.rs` | REST product-metadata fetch, JWT (REST variant), tick conversion, precision table |
| `event.rs` | Serde types for incoming WS messages (`l2_data`, `subscriptions`) |
| `side.rs` | `Side` enum (`Bid`/`Offer`) |
| `order_book.rs` | `PriceBook`/`OrderBook`/`PriceMap` — the actual book storage |
| `display.rs` | Terminal rendering (`dump`, `top_of_book`) with ANSI color |
| `trade.rs` | Placeholder — unused; would hold trade/match event handling if a trades channel is added later |

## Known limitations

- **No sequence-number gap detection.** Each message's `sequence_num` is parsed but not currently checked. A dropped WebSocket message would silently desync the book rather than triggering a resubscribe.
- **Symbol list and secrets directory only accept comma-separated strings / plain paths** — no validation that a given symbol is actually a valid Coinbase product until the REST fetch fails at startup.
- **`trade.rs` is an unused placeholder** for a possible future trades-channel feature.

## Relationship to the C++ version

This is a from-scratch Rust implementation, not a mechanical transliteration — a few things intentionally diverge from `coinbase-advanced-l2-cpp`:

- Async was considered and deliberately rejected in favor of matching the C++ version's synchronous design (see above).
- `--no_snapshots` is the opposite polarity of the C++ version's `--snapshots`: this version processes snapshots by default, and the flag turns that off, rather than defaulting off and opting in.
- Order book price/size precision is derived per-product from Coinbase's own REST API rather than a fixed or guessed value.
