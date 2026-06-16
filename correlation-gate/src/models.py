"""
Data Models for Correlation Gate
=================================

Immutable data structures using dataclasses.
All monetary values use Decimal for precision.
"""

from dataclasses import dataclass, field
from datetime import datetime, timezone
from decimal import Decimal
from enum import Enum
from typing import Dict, List, Optional, Any
import uuid


class GateState(Enum):
    """
    Gate operational states.

    DIAMOND STANDARD: Fail-safe startup sequence.
    Gate blocks all trades until READY state.

    States:
        INITIALIZING: Startup, no position data yet
        SYNCHRONIZING: Fetching initial positions
        READY: Normal operation
        DEGRADED: Operating with stale data
        FAILED: Critical failure, all trades blocked
    """

    INITIALIZING = "INITIALIZING"
    SYNCHRONIZING = "SYNCHRONIZING"
    READY = "READY"
    DEGRADED = "DEGRADED"
    FAILED = "FAILED"


@dataclass(frozen=True)
class Position:
    """
    Represents an open trading position.

    Immutable to ensure thread safety when passed between components.

    Attributes:
        instrument: Trading pair (e.g., "EUR_USD")
        direction: Trade direction ("LONG" or "SHORT")
        units: Actual filled units (not order size)
        entry_price: Entry price for notional calculation
        entry_time: When position was opened
        position_id: Unique identifier from broker

    Example:
        position = Position(
            instrument="EUR_USD",
            direction="LONG",
            units=10000,
            entry_price=Decimal("1.0850"),
            entry_time=datetime.now(timezone.utc),
            position_id="pos_001"
        )
    """

    instrument: str
    direction: str
    units: int
    entry_price: Decimal
    entry_time: datetime
    position_id: str

    def __post_init__(self) -> None:
        """Validate position after initialization."""
        if not self.instrument:
            raise ValueError("instrument cannot be empty")
        if self.direction not in ("LONG", "SHORT"):
            raise ValueError(f"direction must be LONG or SHORT, got {self.direction}")
        if self.units < 0:
            raise ValueError(f"units must be non-negative, got {self.units}")


@dataclass
class TradeSignal:
    """
    Represents an incoming trade signal for evaluation.

    Attributes:
        instrument: Trading pair to trade
        direction: Proposed direction ("LONG" or "SHORT")
        units: Proposed position size in base currency units
        signal_id: Optional unique identifier
        timestamp: When signal was generated
        metadata: Optional additional context

    Example:
        signal = TradeSignal(
            instrument="EUR_USD",
            direction="LONG",
            units=10000
        )
    """

    instrument: str
    direction: str
    units: int
    signal_id: Optional[str] = None
    timestamp: Optional[datetime] = None
    metadata: Optional[Dict[str, Any]] = None

    def __post_init__(self) -> None:
        """Set defaults and validate after initialization."""
        if self.signal_id is None:
            object.__setattr__(self, "signal_id", f"sig_{uuid.uuid4().hex[:12]}")
        if self.timestamp is None:
            object.__setattr__(self, "timestamp", datetime.now(timezone.utc))

    def to_position(self, position_id: str, entry_price: Decimal) -> Position:
        """
        Convert signal to position for exposure calculation.

        Args:
            position_id: ID for the new position
            entry_price: Entry price for the position

        Returns:
            Position object representing the signal as executed
        """
        return Position(
            instrument=self.instrument,
            direction=self.direction,
            units=self.units,
            entry_price=entry_price,
            entry_time=self.timestamp or datetime.now(timezone.utc),
            position_id=position_id,
        )


@dataclass
class PositionContribution:
    """
    Details of how a single position contributes to a basket.

    Used for audit trail and partial fill tracking.

    Attributes:
        position_id: Unique position identifier
        instrument: Trading pair
        direction: LONG or SHORT for this currency
        units: Position units
        usd_notional: USD-equivalent notional value
    """

    position_id: str
    instrument: str
    direction: str
    units: int
    usd_notional: Decimal


@dataclass
class BasketExposure:
    """
    Aggregate directional exposure for a single currency.

    DIAMOND STANDARD: Includes both count and weighted notional.

    Attributes:
        currency: Currency code (e.g., "USD")
        long_count: Number of LONG positions
        short_count: Number of SHORT positions
        long_notional: Total USD notional of LONG positions
        short_notional: Total USD notional of SHORT positions
        net_direction: "LONG", "SHORT", or "NEUTRAL"
        net_count: Absolute count of dominant direction
        net_notional: Absolute USD notional difference (directional risk)
        gross_notional: Total USD notional (LONG + SHORT) (volume risk)
        exposure_level: "LOW", "MEDIUM", "HIGH", "CRITICAL"
        contributing_positions: Detailed position contributions
    """

    currency: str
    long_count: int = 0
    short_count: int = 0
    long_notional: Decimal = Decimal("0")
    short_notional: Decimal = Decimal("0")
    net_direction: str = "NEUTRAL"
    net_count: int = 0
    net_notional: Decimal = Decimal("0")
    gross_notional: Decimal = Decimal("0")
    exposure_level: str = "LOW"
    contributing_positions: List[PositionContribution] = field(default_factory=list)

    @classmethod
    def empty(cls, currency: str) -> "BasketExposure":
        """Create empty basket exposure for a currency."""
        return cls(currency=currency)

    def finalize(self) -> None:
        """
        Calculate derived fields after all positions are added.

        Must be called after adding all position contributions.
        """
        # Net direction
        if self.long_count > self.short_count:
            self.net_direction = "LONG"
        elif self.short_count > self.long_count:
            self.net_direction = "SHORT"
        else:
            self.net_direction = "NEUTRAL"

        # Net count
        self.net_count = abs(self.long_count - self.short_count)

        # Net notional (directional risk)
        self.net_notional = abs(self.long_notional - self.short_notional)

        # Gross notional (volume risk)
        self.gross_notional = self.long_notional + self.short_notional

        # Exposure level classification
        self.exposure_level = self._classify_level()

    def _classify_level(self) -> str:
        """Classify exposure level based on net count."""
        if self.net_count <= 1:
            return "LOW"
        elif self.net_count == 2:
            return "MEDIUM"
        elif self.net_count == 3:
            return "HIGH"
        else:
            return "CRITICAL"


@dataclass
class BasketSnapshot:
    """
    Complete snapshot of all basket exposures.

    Thread-safe point-in-time capture of exposure state.

    Attributes:
        timestamp: When snapshot was taken
        baskets: Dict of currency -> BasketExposure
        total_positions: Total open positions
        highest_exposure_currency: Currency with highest exposure
        highest_exposure_level: Level of highest exposure
        state_version: Monotonic state counter
    """

    timestamp: datetime
    baskets: Dict[str, BasketExposure]
    total_positions: int
    highest_exposure_currency: str
    highest_exposure_level: str
    state_version: int

    @classmethod
    def empty(cls, state_version: int = 0) -> "BasketSnapshot":
        """Create empty basket snapshot."""
        return cls(
            timestamp=datetime.now(timezone.utc),
            baskets={},
            total_positions=0,
            highest_exposure_currency="",
            highest_exposure_level="LOW",
            state_version=state_version,
        )


@dataclass
class GateDecision:
    """
    Result of gate evaluation.

    Immutable record of the gate's decision for audit trail.

    Attributes:
        decision: "ALLOW", "SOFT_WARNING", or "HARD_BLOCK"
        pending_id: ID for confirmation (if ALLOW/WARNING)
        affected_currencies: Currencies triggering decision
        current_exposure: Exposure before signal
        projected_exposure: Exposure if signal executes
        reason: Human-readable explanation
        recommendations: List of suggested actions
        evaluation_time_ms: Processing time in milliseconds
    """

    decision: str
    pending_id: Optional[str]
    affected_currencies: List[str]
    current_exposure: BasketSnapshot
    projected_exposure: BasketSnapshot
    reason: str
    recommendations: List[str]
    evaluation_time_ms: float


@dataclass
class PriceSnapshot:
    """
    Point-in-time price capture for dynamic notional calculation.

    DIAMOND STANDARD: No hardcoded prices.

    Attributes:
        instrument: Trading pair
        bid: Bid price
        ask: Ask price
        mid_price: Mid-market price
        timestamp: When price was captured
    """

    instrument: str
    bid: Decimal
    ask: Decimal
    mid_price: Decimal
    timestamp: datetime

    @property
    def age_ms(self) -> int:
        """Milliseconds since price was captured."""
        delta = datetime.now(timezone.utc) - self.timestamp
        return int(delta.total_seconds() * 1000)

    @property
    def is_stale(self) -> bool:
        """Price is stale if > 5 seconds old."""
        return self.age_ms > 5000


@dataclass
class GateConfig:
    """
    Gate configuration settings.

    DIAMOND STANDARD: Dual-metric thresholds (count + notional).

    Attributes:
        enabled: Whether gate is active
        soft_warning_count: Position count for soft warning
        hard_block_count: Position count for hard block
        soft_warning_net_notional: Net USD notional for warning
        hard_block_net_notional: Net USD notional for block
        soft_warning_gross_notional: Gross USD notional for warning
        hard_block_gross_notional: Gross USD notional for block
        currency_overrides: Per-currency threshold overrides
        position_fetch_timeout_seconds: Max time for position fetch
        lock_timeout_seconds: Max time to acquire lock
        pending_timeout_seconds: Pending signal expiry
    """

    enabled: bool = True
    soft_warning_count: int = 2
    hard_block_count: int = 3
    soft_warning_net_notional: Decimal = Decimal("100000")
    hard_block_net_notional: Decimal = Decimal("200000")
    soft_warning_gross_notional: Decimal = Decimal("150000")
    hard_block_gross_notional: Decimal = Decimal("300000")
    currency_overrides: Optional[Dict[str, Dict[str, Any]]] = None
    position_fetch_timeout_seconds: float = 5.0
    lock_timeout_seconds: float = 1.0
    pending_timeout_seconds: float = 30.0

    def get_thresholds_for_currency(self, currency: str) -> Dict[str, Any]:
        """
        Get thresholds for a specific currency.

        Uses override if defined, otherwise returns defaults.

        Args:
            currency: Currency code

        Returns:
            Dict with all threshold values
        """
        defaults = {
            "soft_warning_count": self.soft_warning_count,
            "hard_block_count": self.hard_block_count,
            "soft_warning_net_notional": self.soft_warning_net_notional,
            "hard_block_net_notional": self.hard_block_net_notional,
            "soft_warning_gross_notional": self.soft_warning_gross_notional,
            "hard_block_gross_notional": self.hard_block_gross_notional,
        }

        if self.currency_overrides and currency in self.currency_overrides:
            defaults.update(self.currency_overrides[currency])

        return defaults


@dataclass
class PendingSignal:
    """
    Represents a pending (allowed but not confirmed) signal.

    Used for tracking signals between evaluation and execution.

    Attributes:
        pending_id: Unique identifier
        signal: The trade signal
        decision: Gate decision (ALLOW or SOFT_WARNING)
        registered_at: When signal was registered
        expires_at: When signal expires
    """

    pending_id: str
    signal: TradeSignal
    decision: str
    registered_at: datetime
    expires_at: datetime

    @property
    def is_expired(self) -> bool:
        """Check if pending signal has expired."""
        return datetime.now(timezone.utc) > self.expires_at


@dataclass
class ParseResult:
    """
    Result of parsing an instrument into currency exposures.

    Attributes:
        exposures: Dict of currency -> direction
        is_cross_pair: True if neither currency is USD
        affected_baskets: List of currency codes affected
        warnings: Any parsing warnings
    """

    exposures: Dict[str, str]
    is_cross_pair: bool
    affected_baskets: List[str]
    warnings: List[str] = field(default_factory=list)


@dataclass
class ParsedInstrument:
    """
    Parsed instrument components.

    Attributes:
        base: Base currency code
        quote: Quote currency code
        original: Original instrument string
    """

    base: str
    quote: str
    original: str = ""


@dataclass
class SyncReport:
    """
    DEC-062: Report of synchronization between engine and gate.

    Details which pending IDs were purged (zombies) and which were retained.

    Attributes:
        purged_ids: List of pending IDs that were removed (zombies)
        retained_ids: List of pending IDs that were kept (active)
        sync_timestamp: When synchronization occurred
    """

    purged_ids: List[str]
    retained_ids: List[str]
    sync_timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))


@dataclass
class SyncResult:
    """
    DEC-062: Result of engine-gate synchronization handshake.

    Attributes:
        status: "SUCCESS" or "FAILED"
        report: SyncReport with details of the synchronization
    """

    status: str
    report: SyncReport

    def __post_init__(self) -> None:
        """Validate status after initialization."""
        if self.status not in ("SUCCESS", "FAILED"):
            raise ValueError(f"status must be SUCCESS or FAILED, got {self.status}")
