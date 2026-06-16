# BlackBox Troubleshooting Guide

**Version:** 1.0.0
**Last Updated:** 2026-01-06

---

## Table of Contents

1. [Recording Issues](#recording-issues)
2. [Replay Issues](#replay-issues)
3. [Performance Issues](#performance-issues)
4. [Integration Issues](#integration-issues)
5. [CLI Issues](#cli-issues)
6. [Common Error Messages](#common-error-messages)
7. [Diagnostic Commands](#diagnostic-commands)

---

## Recording Issues

### Problem: Journal file not created

**Symptoms:**
- No `.journal` file appears after running
- `JournalWriter::new()` returns success but file is empty

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Directory doesn't exist | Create parent directory: `mkdir -p /path/to/journals` |
| Permission denied | Check write permissions: `chmod 755 /path/to/journals` |
| Disk full | Free disk space or change journal location |
| Path is a directory | Specify full file path, not directory |

**Diagnostic:**
```bash
# Check directory exists and is writable
ls -la /path/to/journals/
touch /path/to/journals/test.file && rm /path/to/journals/test.file
```

---

### Problem: Zero records in journal

**Symptoms:**
- Journal file exists but `record_count = 0`
- `blackbox stats` shows empty journal

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Tap not connected to components | Pass tap to all components via `with_tap()` |
| Feature flag not enabled | Build with `--features blackbox` |
| NullTap used accidentally | Verify JournalTap is instantiated |
| Writer dropped too early | Keep writer alive for session duration |

**Diagnostic:**
```rust
// Add logging to verify tap is active
if tap.is_active() {
    println!("Tap is recording");
} else {
    println!("WARNING: Tap is NOT recording");
}
```

---

### Problem: Journal files growing too fast

**Symptoms:**
- Disk space depleting rapidly
- Multiple GB journals per hour

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Recording too much data | Filter what's recorded (critical events only) |
| No file rotation | Implement rotation based on size/time |
| Binary data not compressed | Enable schema compression |
| Large payloads | Truncate or summarize large payloads |

**Mitigation:**
```rust
// Rotate journals by size
if current_size > 100 * 1024 * 1024 { // 100 MB
    let new_path = format!("session_{}.journal", timestamp);
    writer = JournalWriter::new(&new_path, config)?;
    tap = JournalTap::new(writer);
}
```

---

### Problem: High latency spikes during recording

**Symptoms:**
- P99 latency > 1μs during writes
- Occasional pauses in trading

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| MMAP page faults | Enable `prefault_pages = true` |
| Disk I/O blocking | Use SSD or increase ring buffer |
| Ring buffer overflow | Increase `ring_buffer_capacity` |
| Background thread starvation | Ensure adequate CPU resources |

**Configuration:**
```rust
let config = WriterConfig {
    ring_buffer_capacity: 131072, // 128K entries
    prefault_pages: true,
    ..Default::default()
};
```

---

## Replay Issues

### Problem: Replay produces different results

**Symptoms:**
- State hash mismatch at checkpoints
- Different order book state after replay

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Non-deterministic code | Remove random(), use seeded RNG |
| System time used | Inject SimulatedClock everywhere |
| Floating point rounding | Use fixed-point arithmetic |
| Missing events | Ensure all ingress recorded |

**Diagnostic:**
```rust
// Compare specific state
println!("Expected hash: {}", expected.to_hex());
println!("Actual hash:   {}", actual.to_hex());

// Identify divergence point
for (i, (exp, act)) in expected_hashes.iter().zip(actual_hashes.iter()).enumerate() {
    if exp != act {
        println!("First mismatch at checkpoint {}", i);
        break;
    }
}
```

---

### Problem: Replay hangs or is very slow

**Symptoms:**
- Replay takes hours for short sessions
- CPU usage near 0% during replay

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Real-time mode on idle periods | Use WarpConfig::warp_speed() |
| Blocking I/O in replay | Use async/buffered reads |
| Large journal file | Use memory-mapped reading |
| Infinite loop in replay logic | Check step() return value |

**Fast Replay:**
```rust
// Skip idle periods automatically
let config = WarpConfig::warp_speed();
let mut engine = ReplayEngine::with_data_source(source, config);

engine.play();
engine.run_to_completion(); // Fast as possible
```

---

### Problem: "Unknown schema version" error

**Symptoms:**
- Error when opening old journal files
- Schema version mismatch warnings

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Journal from newer version | Update blackbox crate |
| Corrupted schema block | Verify file integrity with checksum |
| Custom schema not registered | Add codec to CodecRegistry |

**Diagnostic:**
```bash
# Check journal schema version
blackbox info old_session.journal

# Compare with current version
cargo run --bin blackbox -- info old_session.journal --schema
```

---

## Performance Issues

### Problem: NullTap has non-zero overhead

**Symptoms:**
- Benchmark shows >1ns for NullTap operations
- Unexpected performance degradation

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Dynamic dispatch (dyn Tap) | Use generics: `<T: Tap>` |
| Optimizer not inlining | Add `#[inline(always)]` |
| Debug build | Use `--release` mode |
| Benchmark measurement error | Increase sample size |

**Verification:**
```rust
// NullTap should be zero-sized
assert_eq!(std::mem::size_of::<NullTap>(), 0);

// Methods should be inlined away
// Check assembly: cargo asm --release
```

---

### Problem: JournalTap latency exceeds target

**Symptoms:**
- >1μs overhead for JournalTap
- Latency increases with payload size

**Expected Performance:**
| Payload Size | Expected Latency |
|--------------|------------------|
| 0 bytes | ~65ns |
| 64 bytes | ~100ns |
| 256 bytes | ~150ns |
| 4KB | ~650ns |

**Solutions:**

| Cause | Solution |
|-------|----------|
| Large payloads | Summarize or truncate data |
| Contention on mutex | Reduce recording frequency |
| Memory allocation | Pre-allocate buffers |
| Disk I/O stalls | Increase ring buffer |

---

## Integration Issues

### Problem: Circular dependency error

**Symptoms:**
- Cargo build fails with cycle error
- "cyclic package dependency" message

**Solution:**
Use `blackbox-types` as intermediate crate:

```
blackbox-types  <──  blackbox  <──  trading-engine
    │                   │                  │
 (Shared)          (Journal)           (Trading)
  Types              Replay              System
```

**Cargo.toml:**
```toml
# In the host engine
[dependencies]
blackbox-types = { workspace = true }
blackbox = { workspace = true, optional = true }

# In blackbox
[dependencies]
blackbox-types = { workspace = true }
# NO dependency on the host engine
```

---

### Problem: Type mismatch between crates

**Symptoms:**
- "expected Exchange, found Exchange" error
- Cannot convert between crate types

**Solution:**
Use type conversions:

```rust
// In your blackbox.rs module
pub fn to_blackbox_exchange(e: crate::Exchange) -> blackbox_types::Exchange {
    match e {
        crate::Exchange::Deribit => blackbox_types::Exchange::Deribit,
        crate::Exchange::Binance => blackbox_types::Exchange::Binance,
        crate::Exchange::Oanda => blackbox_types::Exchange::Unknown,
    }
}
```

---

### Problem: Feature flag not working

**Symptoms:**
- BlackBox code compiled even when feature disabled
- Missing methods when feature enabled

**Checklist:**
```toml
# 1. Define feature in Cargo.toml
[features]
default = []
blackbox = ["dep:blackbox"]

# 2. Use optional dependency
[dependencies]
blackbox = { workspace = true, optional = true }

# 3. Gate code with cfg
#[cfg(feature = "blackbox")]
use blackbox::tap::JournalTap;
```

---

## CLI Issues

### Problem: Command not found

**Symptoms:**
- `blackbox: command not found`
- PATH error

**Solution:**
```bash
# Install globally
cargo install --path crates/blackbox

# Or run directly
cargo run --package blackbox --bin blackbox -- info session.journal
```

---

### Problem: "Invalid journal file" error

**Symptoms:**
- CLI refuses to open file
- Magic number mismatch

**Causes:**
| Cause | Solution |
|-------|----------|
| Not a journal file | Check file extension and source |
| Corrupted file | Restore from backup |
| Incomplete write | Journal may have been truncated |
| Wrong version | Check format compatibility |

**Diagnostic:**
```bash
# Check file header
xxd -l 16 session.journal
# Should show: 4153 5452 4142 4c4b (BLKBOXJL)
```

---

## Common Error Messages

### `InvalidMagic`
**Cause:** File does not start with "BLKBOXJL" magic bytes.
**Solution:** Verify file is a valid journal file.

### `UnsupportedVersion`
**Cause:** Journal was created with incompatible version.
**Solution:** Update blackbox or use older version.

### `SchemaHashMismatch`
**Cause:** Embedded schema doesn't match expected hash.
**Solution:** File may be corrupted; use `skip_schema_hash()` for testing.

### `CorruptedRecord`
**Cause:** CRC checksum failed for record.
**Solution:** Skip record or restore from backup.

### `RingBufferFull`
**Cause:** Writer can't keep up with recording rate.
**Solution:** Increase ring_buffer_capacity or reduce recording rate.

### `DiskFull`
**Cause:** No space left on device.
**Solution:** Free disk space or implement rotation.

---

## Diagnostic Commands

### Check Journal Health
```bash
# Basic info
blackbox info session.journal

# Detailed stats
blackbox stats session.journal --detailed

# Dump first records
blackbox dump session.journal --limit 10
```

### Verify Integrity
```bash
# Run verification
blackbox verify session.journal

# Stop on first error
blackbox verify session.journal --stop-on-mismatch

# JSON output for automation
blackbox verify session.journal --format json > report.json
```

### Debug Recording
```rust
// Add debug logging
env_logger::init();
log::debug!("Recording ingress: {} bytes", data.len());

// Check tap state
println!("Tap active: {}", tap.is_active());
```

### Performance Profiling
```bash
# Run benchmarks
cargo bench --package blackbox

# Profile specific benchmark
cargo bench --package blackbox -- tap_latency

# Generate flamegraph
cargo flamegraph --bench journal_bench
```

---

## Getting Help

If you can't resolve an issue:

1. **Check documentation:**
   - [USAGE.md](USAGE.md)
   - [INTEGRATION.md](INTEGRATION.md)
   - [PERFORMANCE.md](PERFORMANCE.md)

2. **Run diagnostics:**
   ```bash
   blackbox info <journal> --schema
   blackbox verify <journal> --format json
   ```

3. **Collect information:**
   - BlackBox version
   - Operating system
   - Error messages
   - Minimal reproduction steps

4. **Report issue:**
   - Create GitHub issue with collected information
   - Include relevant code snippets
   - Attach minimal test case if possible

---

*Generated for BlackBox v1.0.0*
