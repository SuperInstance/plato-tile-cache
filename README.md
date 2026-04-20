# plato-tile-cache

LRU cache with TTL eviction for tiles - hit rate tracking, top hits, expiration

Part of the [PLATO framework](https://github.com/SuperInstance) - 72 crates for deterministic AI knowledge management.

## Tile Pipeline

This crate fits into the PLATO tile lifecycle:

`validate` -> `score` -> `store` -> `search` -> `rank` -> `prompt` -> `inference`

## Usage

Add to `Cargo.toml`:

```toml
[dependencies]
plato-tile-cache = "0.1"
```

Zero external dependencies. Works with `cargo 1.75+`.

[GitHub](https://github.com/SuperInstance/plato-tile-cache)
