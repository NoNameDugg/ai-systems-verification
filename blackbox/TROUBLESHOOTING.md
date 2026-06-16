# BlackBox Troubleshooting Guide

> **Note**: This is a reference copy. The canonical version is maintained at [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md).

**Version:** 1.0.0
**Last Updated:** 2026-01-08

---

## Table of Contents

1. [Recording Issues](#1-recording-issues)
2. [Replay Issues](#2-replay-issues)
3. [Performance Issues](#3-performance-issues)
4. [Integration Issues](#4-integration-issues)
5. [CLI Issues](#5-cli-issues)
6. [Common Error Messages](#6-common-error-messages)
7. [Diagnostic Commands](#7-diagnostic-commands)

---

## 1. Recording Issues

### Journal file not created

| Symptom | No `.journal` file appears after running |
|---------|------------------------------------------|
| **Cause 1** | Directory doesn't exist |
| **Solution** | `mkdir -p /path/to/journals` |
| **Cause 2** | Permission denied |
| **Solution** | `chmod 755 /path/to/journals` |
| **Cause 3** | Disk full |
| **Solution** | Free disk space or change journal location |

**Diagnostic:**
```bash
ls -la /path/to/journals/
touch /path/to/journals/test.file && rm /path/to/journals/test.file
```

### Zero records in journal

| Symptom | Journal file exists but `record_count = 0` |
|---------|-------------------------------------------|
| **Cause** | Tap not connected / NullTap used accidentally |
| **Solution** | Verify JournalTap is instantiated and passed to components |

```rust
// Verify tap is active
if tap.is_active() {
    println!("Tap is recording");
}
```

### High latency spikes during recording

| Symptom | P99 latency > 1μs during writes |
|---------|--------------------------------|
| **Cause** | MMAP page faults or ring buffer overflow |
| **Solution** | Enable `prefault_pages` and increase buffer |

```rust
let config = WriterConfig {
    ring_buffer_capacity: 131072, // 128K entries
    prefault_pages: true,
    ..Default::default()
};
```

---

## 2. Replay Issues

### Replay produces different results

| Symptom | State hash mismatch at checkpoints |
|---------|-----------------------------------|
| **Common Causes** | Non-deterministic code, system time used, floating point issues |

**Checklist:**
- [ ] Replace `datetime.now()` with injected SimulatedClock
- [ ] Use fixed seeds for random number generators
- [ ] Use `BTreeMap` instead of `HashMap` for deterministic iteration
- [ ] Use fixed-point arithmetic instead of floating point

### Replay hangs or is very slow

| Symptom | Replay takes hours for short sessions |
|---------|--------------------------------------|
| **Cause** | Real-time mode on idle periods |
| **Solution** | Use WarpConfig::warp_speed() |

```rust
let config = WarpConfig::warp_speed();
let mut engine = ReplayEngine::with_data_source(source, config);
engine.play();
engine.run_to_completion(); // Fast as possible
```

### "Unknown schema version" error

| Symptom | Error when opening old journal files |
|---------|-------------------------------------|
| **Cause** | Journal from newer/older version |
| **Solution** | Update blackbox or use skip_schema_hash() |

```bash
blackbox info old_session.journal --schema
```

---

## 3. Performance Issues

### NullTap has non-zero overhead

| Symptom | Benchmark shows >1ns for NullTap |
|---------|----------------------------------|
| **Cause** | Dynamic dispatch or debug build |
| **Solution** | Use generics `<T: Tap>` and `--release` mode |

```rust
assert_eq!(std::mem::size_of::<NullTap>(), 0); // Should be zero-sized
```

### JournalTap latency exceeds target

| Payload Size | Expected Latency |
|--------------|------------------|
| 0 bytes | ~65ns |
| 64 bytes | ~100ns |
| 256 bytes | ~150ns |
| 4KB | ~650ns |

**Solutions:** Summarize large payloads, reduce recording frequency, increase ring buffer.

---

## 4. Integration Issues

### Circular dependency error

**Solution:** Use `blackbox-types` as intermediate crate:
```
blackbox-types  <──  blackbox  <──  trading-engine
```

### Type mismatch between crates

**Solution:** Use type conversions in a bridge module:
```rust
pub fn to_blackbox_exchange(e: crate::Exchange) -> blackbox_types::Exchange {
    match e {
        crate::Exchange::Deribit => blackbox_types::Exchange::Deribit,
        // ...
    }
}
```

### Feature flag not working

**Checklist:**
```toml
# 1. Define feature
[features]
blackbox = ["dep:blackbox"]

# 2. Use optional dependency
[dependencies]
blackbox = { workspace = true, optional = true }
```

```rust
// 3. Gate code with cfg
#[cfg(feature = "blackbox")]
use blackbox::tap::JournalTap;
```

---

## 5. CLI Issues

### Command not found

```bash
# Install globally
cargo install --path crates/blackbox

# Or run directly
cargo run --package blackbox --bin blackbox -- info session.journal
```

### "Invalid journal file" error

| Cause | Solution |
|-------|----------|
| Not a journal file | Check file extension and source |
| Corrupted file | Restore from backup |
| Wrong version | Check format compatibility |

**Diagnostic:**
```bash
xxd -l 16 session.journal
# Should show: 4153 5452 4142 4c4b (BLKBOXJL)
```

---

## 6. Common Error Messages

| Error | Meaning | Action |
|-------|---------|--------|
| `InvalidMagic` | Not a journal file | Verify file path |
| `UnsupportedVersion` | Incompatible version | Update BlackBox |
| `SchemaHashMismatch` | Schema changed | Use skip_schema_hash() for testing |
| `CorruptedRecord` | CRC check failed | Skip record or restore backup |
| `RingBufferFull` | Events too fast | Increase ring_buffer_capacity |
| `DiskFull` | No space left | Free disk or implement rotation |

---

## 7. Diagnostic Commands

### Check Journal Health
```bash
blackbox info session.journal
blackbox stats session.journal --detailed
blackbox dump session.journal --limit 10
```

### Verify Integrity
```bash
blackbox verify session.journal
blackbox verify session.journal --stop-on-mismatch
blackbox verify session.journal --format json > report.json
```

### Performance Profiling
```bash
cargo bench --package blackbox
cargo bench --package blackbox -- tap_latency
cargo flamegraph --bench journal_bench
```

---

## Getting Help

1. Check comprehensive documentation: [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md)
2. Run diagnostics: `blackbox verify <journal> --format json`
3. Check logs: Enable `RUST_LOG=debug`
4. Report issue with version, OS, and reproduction steps

---

*BlackBox v1.0.0*
