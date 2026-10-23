# Salsa 0.25.2: Scarlet pending-write cancellation

This directory was extracted from the exact crates.io `salsa-0.25.2.crate`
archive in the local Cargo cache, without modifying that cache. The archive's
SHA-256 was verified before extraction:

```text
e2e2aa2fca57727371eeafc975acc8e6f4c52f8166a78035543f6ee1c74c2dcc
```

This is also the registry checksum in rust-analyzer's original lock entry.
The preserved `.cargo_vcs_info.json` identifies upstream commit
`e72b228b4cb86541d3bba58288f375118dabae37` in
<https://github.com/salsa-rs/salsa>. All original package files, licenses,
tests, examples, book content, `Cargo.lock`, and `Cargo.toml.orig` are retained.

Local differences from that archive are limited to:

- `src/zalsa.rs`: on Scarlet, skip the pending-write unwind at
  `Zalsa::unwind_if_revision_cancelled`. Its cancellation-check event remains.
- `Cargo.toml`: add an opt-in host test feature, register the focused integration
  test, and isolate upstream development targets with an empty `[workspace]`.
- Add this provenance/policy note and `tests/scarlet_deferred_cancellation.rs`.

Scarlet uses `panic=abort`. Salsa's ordinary `Cancelled::PendingWrite` calls
`resume_unwind`, which would abort the server when an edit overlaps analysis.
The Scarlet path instead lets existing snapshots finish. `Storage::cancel_others`
is unchanged: it sets the flag and waits for every other storage handle to drop
before granting exclusive mutable access and resetting the flag. Revision
advancement, input mutation, memo invalidation, and garbage-collection barriers
are unchanged. `Cancelled::throw`, `PropagatedPanic`, and other failures are not
suppressed or replaced with fabricated results.

This preserves consistency but makes cancellation slower: the writer may wait
for finite analysis, queued snapshots, or indexing to finish. There is no bounded
latency guarantee. A leaked snapshot, nonterminating computation, or snapshot
owner depending on the waiting writer can still hang. Merely finishing a query
is insufficient; its storage handle must also be released. LSP clients must
continue rejecting responses for obsolete document revisions.

Non-Scarlet behavior is unchanged by default. The explicit
`scarlet-defer-cancellation-test` feature enables exactly the Scarlet branch on
a host for regression testing. Do not enable it for normal host builds or use
`--all-features` as a way to test the upstream cancellation policy.

From a Rust development shell, run only this focused test, with a target
directory separate from the native rust-analyzer build:

```sh
cargo test --manifest-path src/tools/rust-analyzer/scarlet/vendor/salsa/Cargo.toml --target-dir target/salsa-cancellation-probe --test scarlet_deferred_cancellation --features scarlet-defer-cancellation-test -- --exact pending_write_preserves_snapshot_and_updates_next_revision
```

Repeat without `--features scarlet-defer-cancellation-test` to verify the default
host cancellation behavior. No full suite is required for this focused check.
The test uses actual tracked queries and an input setter. Channel handshakes
pause the query after its first read, observe the writer's cancellation flag,
resume the query through both an explicit checkpoint and another tracked fetch,
retain the snapshot after query completion, and finally release it. Under the
Scarlet policy, the old query must return 42 before snapshot release; the waiting
writer must subsequently return 100 from a fresh query of the same input.
Without the feature on a host, the old query must instead report PendingWrite.
Handshake timeouts detect test failures; they do not promise production latency.

The Scarlet-policy regression passed on the host. The default-policy comparison
could not complete in the current macOS toolchain environment: unwind initiation
aborted with error 5 (also with the unmodified stock host compiler). This is not
a passing default-policy result. Native server acceptance is recorded separately.
