# Source Code Directory

This directory contains the core implementation of the ASTRA Correlation Gate.

## Module Structure

```
src/
├── __init__.py          # Package initialization, exports public API
├── api.py               # High-level API (CorrelationGateAPI)
├── basket.py            # Basket exposure calculation logic
├── directional.py       # Directional mapping (instrument → currency exposure)
├── exceptions.py        # Custom exception hierarchy
├── gate.py              # Core gate logic (CorrelationGate class)
├── models.py            # Data models (Position, TradeSignal, GateDecision)
├── providers/           # Position data providers
└── security/            # Security components (atomic ops, audit)
```

## Core Modules

### `gate.py` - Main Gate Engine
The heart of the system. Contains:
- `CorrelationGate`: Main class for trade signal evaluation
- Gate states (INITIALIZING, SYNCHRONIZING, READY, FAILED)
- Atomic evaluation with race protection
- Pending signal management

### `directional.py` - Directional Mapping
Parses forex instruments into constituent currency exposures:
- `EUR_USD LONG` → `{EUR: LONG, USD: SHORT}`
- Supports multiple formats (underscore, slash, no separator)
- Cross-pair detection

### `basket.py` - Basket Calculator
Calculates aggregate directional exposure per currency:
- Position count tracking
- Weighted notional calculation (future)
- Net exposure calculation

### `models.py` - Data Models
Core data structures:
- `Position`: Open position representation
- `TradeSignal`: Trade signal to evaluate
- `GateDecision`: Evaluation result
- `BasketExposure`: Currency basket state

### `exceptions.py` - Exception Hierarchy
Custom exceptions for precise error handling:
- `GateError` (base)
- `InvalidSignalError`
- `GateUnavailableError`
- `AtomicOperationTimeout`
- Provider exceptions

### `api.py` - Public API
High-level interface for external systems:
- `CorrelationGateAPI.create()` - Factory method
- Simplified evaluation interface
- Configuration helpers

## Subdirectories

### `providers/`
Position data provider implementations. See [providers/README.md](providers/README.md).

### `security/`
Security-critical components. See [security/README.md](security/README.md).

## Thread Safety

All public methods in this package are **thread-safe**. The gate uses `RLock` for atomic operations and supports concurrent evaluation from multiple threads.

## Usage

```python
from src import CorrelationGate, GateConfig, TradeSignal
from src.providers import InMemoryPositionProvider

# Create and initialize
config = GateConfig(soft_warning_count=2, hard_block_count=3)
provider = InMemoryPositionProvider()
gate = CorrelationGate(config, provider)
gate.initialize()

# Evaluate signals
decision = gate.evaluate(TradeSignal("EUR_USD", "LONG", 100000))
```

