# BlackBox: Time Source Audit Report (T3.4)

**Project:** blackbox
**Task:** T3.4 - Audit datetime.now() calls
**Date:** 2026-01-05
**Status:** COMPLETE

---

## 1. Executive Summary

This audit identifies all time source calls (`SystemTime::now()`, `Utc::now()`, etc.) in the blackbox codebase that must be replaced with injected `Clock` for deterministic replay.

### Key Findings

| Category | Count | Action Required |
|----------|-------|-----------------|
| **HOT PATH** | 2 | MUST REPLACE |
| **COLD PATH** | 1 | OPTIONAL |
| **INFRASTRUCTURE** | 1 | NO CHANGE (by design) |
| **TEST CODE** | 3 | NO CHANGE |
| **BENCHMARK CODE** | 1 | NO CHANGE |
| **Total** | 8 | 2 mandatory, 1 optional |

---

## 2. Detailed Findings

### 2.1 HOT PATH (MUST REPLACE)

These time sources are called during trading operations and **must** be replaced with injected `Clock` for deterministic replay.

#### Finding #1: JournalWriter Record Timestamps

| Attribute | Value |
|-----------|-------|
| **File** | `crates/blackbox/src/journal/writer.rs` |
| **Line** | 689-696 |
| **Function** | `now_micros()` |
| **Impact** | HIGH - every record write uses this |

**Current Code:**
```rust
/// Get current time in microseconds since epoch.
#[inline]
fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}
```

**Used By:**
- Background writer thread for record timestamps
- Called on every `write()` operation

**Recommendation:**
Replace with injected `Clock` parameter. The `JournalWriter` should accept a generic `Clock` and use it for all timestamps:

```rust
pub struct JournalWriter<C: Clock = SystemClock> {
    clock: C,
    // ... other fields
}

impl<C: Clock> JournalWriter<C> {
    pub fn with_clock(path: &Path, config: WriterConfig, clock: C) -> Result<Self, WriterError> {
        // ...
    }
}
```

---

#### Finding #2: FileHeader Session Start Timestamp

| Attribute | Value |
|-----------|-------|
| **File** | `crates/blackbox/src/journal/format.rs` |
| **Line** | 109-114 |
| **Function** | `FileHeader::new()` |
| **Impact** | MEDIUM - session start time |

**Current Code:**
```rust
impl FileHeader {
    pub fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0);

        Self {
            // ...
            session_start: now,
            // ...
        }
    }
}
```

**Recommendation:**
Add a `new_with_clock<C: Clock>(clock: &C)` constructor:

```rust
impl FileHeader {
    pub fn new_with_clock<C: Clock>(clock: &C) -> Self {
        Self {
            // ...
            session_start: clock.now_micros(),
            // ...
        }
    }
}
```

---

### 2.2 INFRASTRUCTURE (BY DESIGN - NO CHANGE)

#### Finding #3: SystemClock Implementation

| Attribute | Value |
|-----------|-------|
| **File** | `crates/blackbox-types/src/clock.rs` |
| **Line** | 70-82 |
| **Function** | `SystemClock::now()` |
| **Impact** | NONE - this is the production clock |

**Current Code:**
```rust
impl Clock for SystemClock {
    #[inline]
    fn now(&self) -> Timestamp {
        use std::time::{SystemTime, UNIX_EPOCH};

        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before Unix epoch");

        Timestamp::from_micros(duration.as_micros() as i64)
    }
}
```

**Analysis:**
This is the **intentional** implementation of `Clock` for production use. When using `SystemClock`, real system time is expected. For replay, `SimulatedClock` is injected instead.

**Action:** NO CHANGE - working as designed.

---

### 2.3 TEST CODE (NO CHANGE)

These time sources are in test code only and do not affect production behavior.

#### Finding #4: writer.rs test helper

| Attribute | Value |
|-----------|-------|
| **File** | `crates/blackbox/src/journal/writer.rs` |
| **Line** | 842-846 |
| **Function** | `create_test_path()` |
| **Context** | Test helper to generate unique file names |

**Purpose:** Generate unique test file names using nanosecond timestamp.

**Action:** NO CHANGE - test infrastructure only.

---

#### Finding #5: reader.rs test helper

| Attribute | Value |
|-----------|-------|
| **File** | `crates/blackbox/src/journal/reader.rs` |
| **Line** | 722-726 |
| **Function** | `create_test_path()` |
| **Context** | Test helper to generate unique file names |

**Action:** NO CHANGE - test infrastructure only.

---

#### Finding #6: reader.rs timestamp test

| Attribute | Value |
|-----------|-------|
| **File** | `crates/blackbox/src/journal/reader.rs` |
| **Line** | 926-929 |
| **Function** | `test_record_timestamp()` |
| **Context** | Verify record timestamps are recent |

**Action:** NO CHANGE - test verification only.

---

### 2.4 BENCHMARK CODE (NO CHANGE)

#### Finding #7: Tap latency benchmark

| Attribute | Value |
|-----------|-------|
| **File** | `crates/blackbox/benches/tap_latency_bench.rs` |
| **Line** | 56 |
| **Context** | Benchmark timestamp generation |

**Action:** NO CHANGE - benchmark infrastructure only.

---

## 3. External Codebase (host engine)

The design notes reference time sources in the host engine that must be audited and replaced:

| File | Line | Usage | Status |
|------|------|-------|--------|
| `connector.rs` | ~628 | `chrono::Utc::now().timestamp_micros()` | NOT YET AUDITED |
| `types.rs` | ~597 | `Utc::now()` | NOT YET AUDITED |

**Note:** Full host-engine audit requires access to that codebase. This audit covers only blackbox.

---

## 4. Implementation Plan (T3.5)

### Priority 1: JournalWriter Clock Injection

1. Add generic `Clock` parameter to `JournalWriter`
2. Update `now_micros()` helper to use injected clock
3. Add `JournalWriter::with_clock()` constructor
4. Default to `SystemClock` for backward compatibility

### Priority 2: FileHeader Clock Injection

1. Add `FileHeader::new_with_clock()` constructor
2. Pass clock from `JournalWriter` to `FileHeader::new_with_clock()`

### Priority 3: Tap Interface

1. Verify `Tap` trait methods receive timestamps from caller
2. Ensure tap points pass Clock-derived timestamps

---

## 5. Verification Criteria

After T3.5 implementation, verify:

- [ ] `grep -r "SystemTime::now()" --include="*.rs" crates/blackbox/src/` returns 0 results (excluding tests)
- [ ] `grep -r "Utc::now()" --include="*.rs" crates/blackbox/src/` returns 0 results
- [ ] All production code paths accept injected `Clock`
- [ ] Replay tests demonstrate deterministic timestamps
- [ ] Existing tests continue to pass

---

## 6. Summary

| File | Line | Category | Action |
|------|------|----------|--------|
| `journal/writer.rs` | 689-696 | HOT PATH | **MUST REPLACE** |
| `journal/format.rs` | 109-114 | HOT PATH | **MUST REPLACE** |
| `blackbox-types/clock.rs` | 70-82 | INFRASTRUCTURE | No change (by design) |
| `journal/writer.rs` | 842-846 | TEST | No change |
| `journal/reader.rs` | 722-726 | TEST | No change |
| `journal/reader.rs` | 926-929 | TEST | No change |
| `benches/tap_latency_bench.rs` | 56 | BENCHMARK | No change |

**Conclusion:** 2 time sources must be replaced for deterministic replay. The `JournalWriter` and `FileHeader` need to accept injected `Clock` instead of calling `SystemTime::now()` directly.

---

*Audit completed: 2026-01-05*
*Auditor: Claude Opus 4.5*
