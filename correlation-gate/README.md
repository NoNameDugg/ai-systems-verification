# ASTRA Correlation Gate

**A fail-closed correlation risk gate for forex trading**

[![Version](https://img.shields.io/badge/version-1.0.0-blue.svg)](#)
[![Python](https://img.shields.io/badge/python-3.10+-green.svg)](https://python.org)
[![Tests](https://img.shields.io/badge/tests-456-brightgreen.svg)](tests/)
[![Coverage](https://img.shields.io/badge/coverage-99%25-brightgreen.svg)](htmlcov/)

---

## Overview

The ASTRA Correlation Gate is a risk-management component that intercepts trade
signals and evaluates them against current portfolio exposure. The goal is to
limit over-exposure to correlated currency directions before an order is placed.

It is designed as a sidecar/library: you give it a way to read your open
positions, and it returns an ALLOW / SOFT_WARNING / HARD_BLOCK decision for each
new signal.

### Design Goals

- **Fail-closed** - Any error (provider failure, timeout, bad config) results in
  a block rather than letting a trade through.
- **Thread-safe** - RLock-based protection so it can be called concurrently.
- **Dual-metric gating** - Limits exposure by both position *count* and
  *notional* per currency direction.
- **Pluggable providers** - Read positions from OANDA, an in-memory store, or a
  simulator via a small provider interface.
- **Windows-friendly audit logging** - Avoids file-locking issues seen on
  Windows.

### Testing

The suite has **456 tests (455 passing, 1 skipped)** at **99% line coverage**
(`pytest --cov=src`). See [Testing](#testing) for how to run it.

---

## Quick Start

### Installation

This is a source package (no `pip install` of the project itself yet). Clone it
and run from the repository root so that `src` is importable:

```bash
# Get the code
cd correlation-gate

# Install test dependencies (the runtime is pure standard library)
pip install -r requirements.txt

# Verify installation
python -m pytest tests/ -q
```

### Basic Usage (high-level API)

The simplest path is the `create_gate()` convenience function, which builds and
initializes a gate backed by an in-memory provider:

```python
from src import create_gate, TradeSignal

# Build + initialize a gate (defaults: soft_warning=2, hard_block=3)
gate = create_gate(soft_warning=2, hard_block=3)

# Evaluate a trade signal
signal = TradeSignal("EUR_USD", "LONG", units=10000)
decision = gate.evaluate(signal)

if decision.decision == "ALLOW":
    # ... execute the trade in your system ...
    gate.confirm_execution(decision.pending_id)
elif decision.decision == "HARD_BLOCK":
    print(f"Blocked: {decision.reason}")
```

### Wiring your own position source

To feed the gate real positions, supply a provider that implements
`fetch_positions()`, `is_available()`, and `get_last_fetch_time()`. The package
ships an in-memory provider and an OANDA provider; you can also write your own.

```python
from src import CorrelationGate, GateConfig
from src.providers import InMemoryPositionProvider

config = GateConfig(soft_warning_count=2, hard_block_count=3)

# In-memory provider (start empty, update as positions open/close)
provider = InMemoryPositionProvider()

gate = CorrelationGate(config, provider)
gate.initialize()

# ... later, reflect open positions and evaluate signals ...
```

---

## Repository Structure

```
correlation-gate/
│
├── README.md                    # This file
├── requirements.txt             # Python dependencies
│
├── src/                         # Source code
│   ├── __init__.py             # Package initialization
│   ├── api.py                  # High-level public API
│   ├── basket.py               # Basket exposure calculation
│   ├── directional.py          # Directional mapping algorithm
│   ├── exceptions.py           # Custom exception hierarchy
│   ├── gate.py                 # Core gate logic (main module)
│   ├── models.py               # Data models (Position, Signal, Decision)
│   │
│   ├── providers/              # Position data providers
│   │   ├── __init__.py
│   │   ├── base.py             # Abstract base class
│   │   ├── cache.py            # Thread-safe caching
│   │   ├── default.py          # In-memory default provider
│   │   ├── factory.py          # Provider factory
│   │   ├── fallback.py         # Automatic failover
│   │   ├── health.py           # Health monitoring
│   │   ├── oanda.py            # OANDA REST API provider
│   │   └── simulator.py        # Simulator portfolio provider
│   │
│   └── security/               # Security components
│       ├── __init__.py
│       ├── atomic.py           # Atomic operations (RLock)
│       └── audit.py            # Audit logging (Windows-safe)
│
├── tests/                       # Test suite (456 tests)
│   ├── unit/                   # Unit tests
│   │   ├── test_logic_core.py
│   │   ├── providers/          # Provider tests
│   │   └── security/           # Security tests
│   ├── integration/            # Integration tests
│   ├── performance/            # Benchmark tests
│   ├── race/                   # Race condition tests
│   ├── benchmarks/             # Additional benchmarks
│   └── fixtures/               # Test fixtures
│
├── config/                      # Configuration files
│   ├── README.md
│   └── gate_config.example.yaml
│
├── docs/                        # Documentation (6,313+ lines)
│   ├── README.md               # Documentation index
│   ├── PLANNING.md             # Master project plan
│   ├── PROGRESS.md             # Progress tracker
│   ├── STANDARDS.md            # Coding standards
│   ├── COMPONENT_REGISTRY.md   # Component documentation
│   │
│   ├── api/                    # API documentation
│   │   ├── API_REFERENCE.md
│   │   ├── ASTRA_FOREX_INTEGRATION_GUIDE.md
│   │   ├── MARKETSIM_INTEGRATION_GUIDE.md
│   │   ├── PHASE2_PROVIDERS_API.md
│   │   ├── PHASE3_SECURITY_API.md
│   │   └── *_TROUBLESHOOTING.md
│   │
│   ├── architecture/           # Technical specifications
│   │   ├── LOGIC_CORE_SPEC.md
│   │   ├── PHASE2_DATA_INTEGRATION_SPEC.md
│   │   └── PHASE3_SECURITY_HARDENING_SPEC.md
│   │
│   ├── guides/                 # Implementation guides
│   │   ├── LOGIC_CORE_INTEGRATION.md
│   │   └── TROUBLESHOOTING.md
│   │
│   └── artifacts/              # HTML documentation
│       ├── PROJECT_SUMMARY.html
│       └── USER_HANDBOOK.html
│
├── quality_review/              # Quality review documents
│   ├── Phase_1/
│   ├── Phase_2/
│   ├── Phase_3/
│   └── Phase_4/
│
└── htmlcov/                     # Code coverage report
    └── index.html
```

---

## Documentation

### Quick Links

The public interface is `CorrelationGateAPI` in [`src/api.py`](src/api.py); behaviour is documented inline and
demonstrated across the `tests/` suite (455 tests). Integration is provider-based — implement the `PositionProvider`
interface (see `src/providers/`) to wire the gate to any trading engine or market simulator.

### Architecture

The gate follows a layered architecture:

1. **API Layer** (`api.py`) - High-level interface for external systems
2. **Gate Layer** (`gate.py`) - Core evaluation and state management
3. **Calculation Layer** (`directional.py`, `basket.py`) - Exposure calculation
4. **Provider Layer** (`providers/`) - Position data abstraction
5. **Security Layer** (`security/`) - Atomic operations and audit logging

---

## Core Concepts

### Directional Mapping

When trading EUR_USD LONG, you are:
- **LONG EUR** (buying euros)
- **SHORT USD** (selling dollars)

The gate tracks this implicit exposure across all pairs.

### Basket Exposure

A currency basket represents aggregate directional exposure:

```
Open Positions:
- EUR_USD LONG  → USD: SHORT x1
- GBP_USD LONG  → USD: SHORT x2
- AUD_USD LONG  → USD: SHORT x3  ← BLOCKED (exceeds threshold)
```

### Gate Decisions

| Decision | Condition | Action |
|----------|-----------|--------|
| `ALLOW` | Exposure below soft threshold | Execute trade |
| `SOFT_WARNING` | Exposure at soft threshold | Execute with warning |
| `HARD_BLOCK` | Exposure at/above hard threshold | Block execution |

---

## Testing

Run tests from the repository root so `src` and `tests` are importable:

```bash
# Run all tests
python -m pytest tests/ -q

# Run with coverage
python -m pytest tests/ --cov=src --cov-report=html

# Run specific test categories
python -m pytest tests/unit/ -q           # Unit tests
python -m pytest tests/integration/ -q    # Integration tests
python -m pytest tests/race/ -q           # Race condition tests
python -m pytest tests/performance/ -q    # Performance tests
```

### Test Statistics

| Metric | Value |
|--------|-------|
| Total tests | 456 (455 passing, 1 skipped) |
| Line coverage | 99% (`pytest --cov=src`) |

Tests are organized into unit, integration, race-condition, and performance
suites under `tests/`.

---

## Configuration

### Environment Variables

```bash
# OANDA Configuration
OANDA_ACCOUNT_ID=your_account_id
OANDA_API_TOKEN=your_api_token
OANDA_API_URL=https://api-fxpractice.oanda.com

# Gate Thresholds (optional)
GATE_SOFT_WARNING_THRESHOLD=2
GATE_HARD_BLOCK_THRESHOLD=3
```

### Configuration File

See [config/gate_config.example.yaml](config/gate_config.example.yaml) for a complete configuration template.

---

## Project Phases

The project was completed in 4 phases following the TDI 9-Step Methodology:

| Phase | Name | Status | Components |
|-------|------|--------|------------|
| 0 | Planning & Setup | Complete | Architecture, Standards, Red Team Review |
| 1 | Core Infrastructure | Complete | Directional Mapper, Basket Calculator, Gate Engine |
| 2 | Data Integration | Complete | OANDA Provider, Simulator, Fallback, Cache |
| 3 | Security Hardening | Complete | Atomic Operations, Race Tests, Audit Logging |
| 4 | Documentation & Polish | Complete | API Docs, Guides, Troubleshooting |

---

## Performance

The gate is designed to add minimal latency to the trade path: evaluation does a
small amount of in-memory arithmetic plus a (usually cached) position read. The
table below lists the design targets used during development. Treat them as
goals, not benchmarked guarantees — actual numbers depend on your hardware and
your position provider.

| Operation | Design target |
|-----------|---------------|
| Gate evaluation | low single-digit ms |
| Directional parsing | sub-millisecond |
| Basket calculation | sub-millisecond |
| Position fetch (cache hit) | sub-millisecond |
| Position fetch (provider API) | bounded by your provider |

Benchmark scaffolding lives under `tests/performance/` and `tests/benchmarks/`.

---

## Requirements

- Python 3.10+
- Dependencies listed in `requirements.txt`

### Optional Dependencies

- `pytest-benchmark` - For performance benchmarks
- `pytest-xdist` - For parallel test execution

---

## Contributing

1. Follow the coding standards in [STANDARDS.md](docs/STANDARDS.md)
2. Write tests first
3. Keep coverage high for new code (the suite currently sits at 99%)
4. Update documentation for API changes

---

## License

No license is granted at this time. This repository is shared as a portfolio /
work sample; please contact the author before reusing the code.

---

## Support

- **Documentation**: [docs/](docs/)
- **Troubleshooting**: [docs/api/PHASE2_TROUBLESHOOTING.md](docs/api/PHASE2_TROUBLESHOOTING.md)
- **User Handbook**: [docs/artifacts/USER_HANDBOOK.html](docs/artifacts/USER_HANDBOOK.html)

---

**ASTRA Correlation Gate** - fail-closed correlation risk gating for currency exposure

*Version 1.0.0*

