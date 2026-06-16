# Position Providers

This directory contains position data provider implementations for the ASTRA Correlation Gate.

## Provider Architecture

```
providers/
├── __init__.py      # Package exports
├── base.py          # Abstract base class (PositionProvider)
├── oanda.py         # OANDA API provider (production)
├── simulator.py     # In-memory simulator (testing/backtesting)
├── fallback.py      # Automatic failover provider
├── factory.py       # Provider factory (plugin system)
├── cache.py         # Thread-safe position cache
└── health.py        # Provider health monitoring
```

## Providers

### `OandaPositionProvider`
Production provider that fetches positions from OANDA REST API.

**Features:**
- Automatic authentication
- Request retry with backoff
- Response caching
- Rate limit handling

**Configuration:**
```python
provider = OandaPositionProvider(
    account_id="your_account",
    api_token="your_token",
    api_url="https://api-fxpractice.oanda.com",
    cache_ttl_seconds=5
)
```

### `SimulatorPositionProvider`
In-memory provider for testing and backtesting.

**Features:**
- Thread-safe position management
- Add/remove/set positions programmatically
- Zero network latency
- Perfect for unit tests

**Usage:**
```python
provider = SimulatorPositionProvider()
provider.add_position(Position("EUR_USD", "LONG", 100000))
provider.remove_position("position_id")
provider.clear_positions()
```

### `FallbackPositionProvider`
Automatic failover between primary and secondary providers.

**Features:**
- Health monitoring
- Automatic failover on errors
- Recovery detection
- Configurable thresholds

**Usage:**
```python
fallback = FallbackPositionProvider(
    primary=oanda_provider,
    secondary=simulator_provider
)
```

## Support Components

### `PositionCache`
Thread-safe caching layer for position data.
- TTL-based expiration
- Atomic get/set operations

### `ProviderHealthMonitor`
Tracks provider health status.
- Success/failure counting
- Circuit breaker pattern
- Recovery detection

### `ProviderFactory`
Plugin system for provider registration.
- Register custom providers
- Create by type name
- Configuration-driven instantiation

## Creating Custom Providers

Implement the `PositionProvider` abstract base class:

```python
from src.providers.base import PositionProvider
from src.models import Position

class MyCustomProvider(PositionProvider):
    def fetch_positions(self) -> List[Position]:
        # Your implementation
        return positions

    def is_available(self) -> bool:
        return True
```

## Thread Safety

All providers are designed to be thread-safe for concurrent access.

