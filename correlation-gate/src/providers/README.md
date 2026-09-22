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
from src.providers import OandaConfig, OandaPositionProvider

provider = OandaPositionProvider(
    OandaConfig(account_id="your_account", api_token="your_token", environment="practice"),
    cache_ttl_seconds=5,
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
from datetime import datetime, timezone
from decimal import Decimal

from src.providers import InMemoryPortfolioAccessor, SimulatorPosition, SimulatorPositionProvider

# The provider reads positions through a PortfolioAccessor; the in-memory one is for tests.
accessor = InMemoryPortfolioAccessor()
provider = SimulatorPositionProvider(accessor)
accessor.add_position(
    SimulatorPosition(
        position_id="pos_001",
        instrument="EUR_USD",
        direction="LONG",
        units=100000,
        entry_price=Decimal("1.0850"),
        entry_time=datetime.now(timezone.utc),
    )
)
accessor.remove_position("pos_001")
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
from src.providers import FallbackPositionProvider

fallback = FallbackPositionProvider(primary=oanda_provider, fallback=simulator_provider)
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
from datetime import datetime
from typing import List, Optional

from src.models import Position
from src.providers.base import PositionProvider

class MyCustomProvider(PositionProvider):
    def fetch_positions(self) -> List[Position]:
        return []  # your implementation

    def is_available(self) -> bool:
        return True

    def get_last_fetch_time(self) -> Optional[datetime]:
        return None  # all three abstract methods must be implemented
```

## Thread Safety

All providers are designed to be thread-safe for concurrent access.

