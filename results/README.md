# Measurements

Produced by `cargo run --release --features bench --bin ephemeris-bench`.

**These are measurements of one machine at one moment, not properties of
`ephemeris`.** Every file records the host that produced it — CPU, core count,
OS, the sync call used, and the measured cost of that call. Read those fields
before reading any throughput number.

**Do not compare across platforms without checking `sync_call` and `sync_us`.**
On macOS `File::sync_data` is `F_FULLFSYNC` and forces a drive cache flush,
costing milliseconds. On Linux it is `fdatasync`, which on typical hardware does
not. A Linux run may look an order of magnitude faster while providing *weaker*
durability, so the comparison is meaningless unless both figures are quoted.

`prediction` in each file is what the design expected **before** anything was
measured. It is recorded so a contradicting result reads as a contradiction
rather than quietly becoming the new expectation.
