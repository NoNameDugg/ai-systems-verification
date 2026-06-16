"""
Basket Calculator for Correlation Gate
=======================================

Aggregates positions into currency basket exposures.
Supports dual-metric tracking (count + notional).
"""

from datetime import datetime, timezone
from decimal import Decimal
from typing import Dict, List, Optional, Tuple

from src.models import (
    Position,
    BasketExposure,
    BasketSnapshot,
    PositionContribution,
    PriceSnapshot,
)
from src.directional import (
    parse_directional_exposure,
    calculate_usd_notional,
    calculate_xau_notional,
)


# =============================================================================
# PUBLIC FUNCTIONS
# =============================================================================


def calculate_basket_exposure(
    positions: List[Position], spot_prices: Optional[Dict[str, Decimal]] = None
) -> Dict[str, BasketExposure]:
    """
    Calculate aggregate exposure for all currencies.

    DIAMOND STANDARD: Uses both count AND weighted notional.

    Args:
        positions: List of open positions
        spot_prices: Optional current spot prices for XAU/cross-pair notional

    Returns:
        Dict mapping currency -> BasketExposure (with notional)

    Performance:
        Target: < 1ms for 10K positions

    Example:
        >>> positions = [Position("EUR_USD", "LONG", 10000, ...)]
        >>> baskets = calculate_basket_exposure(positions)
        >>> baskets["EUR"].long_count
        1
        >>> baskets["USD"].short_count
        1
    """
    baskets: Dict[str, BasketExposure] = {}

    for position in positions:
        # Parse directional exposure
        parse_result = parse_directional_exposure(
            position.instrument, position.direction
        )

        # Calculate USD notional for this position
        notional = calculate_position_notional(position, spot_prices)

        # Add to each affected basket
        for currency, direction in parse_result.exposures.items():
            if currency not in baskets:
                baskets[currency] = BasketExposure.empty(currency)

            basket = baskets[currency]

            # Create contribution record
            contribution = PositionContribution(
                position_id=position.position_id,
                instrument=position.instrument,
                direction=direction,
                units=position.units,
                usd_notional=notional,
            )
            basket.contributing_positions.append(contribution)

            # Update counts and notionals
            if direction == "LONG":
                basket.long_count += 1
                basket.long_notional += notional
            else:
                basket.short_count += 1
                basket.short_notional += notional

    # Finalize all baskets
    for basket in baskets.values():
        basket.finalize()

    return baskets


def calculate_position_notional(
    position: Position, spot_prices: Optional[Dict[str, Decimal]] = None
) -> Decimal:
    """
    Calculate USD notional for a single position.

    Args:
        position: Position to calculate notional for
        spot_prices: Optional spot prices for XAU

    Returns:
        USD-equivalent notional value

    Example:
        >>> pos = Position("EUR_USD", "LONG", 10000, Decimal("1.0850"), ...)
        >>> calculate_position_notional(pos)
        Decimal('10850.00')
    """
    return calculate_usd_notional(
        position.instrument, position.units, position.entry_price, spot_prices
    )


def classify_exposure_level(net_count: int) -> str:
    """
    Classify exposure level from net count.

    Args:
        net_count: Net directional position count

    Returns:
        "LOW", "MEDIUM", "HIGH", or "CRITICAL"

    Thresholds:
        - LOW: 0-1
        - MEDIUM: 2
        - HIGH: 3
        - CRITICAL: 4+

    Example:
        >>> classify_exposure_level(0)
        'LOW'
        >>> classify_exposure_level(3)
        'HIGH'
    """
    if net_count <= 1:
        return "LOW"
    elif net_count == 2:
        return "MEDIUM"
    elif net_count == 3:
        return "HIGH"
    else:
        return "CRITICAL"


def find_highest_exposure(baskets: Dict[str, BasketExposure]) -> Tuple[str, str]:
    """
    Find currency with highest exposure.

    Args:
        baskets: Dict of currency -> BasketExposure

    Returns:
        Tuple of (currency, exposure_level)

    Example:
        >>> baskets = {"EUR": BasketExposure(...), "USD": BasketExposure(...)}
        >>> find_highest_exposure(baskets)
        ('USD', 'HIGH')
    """
    if not baskets:
        return "", "LOW"

    highest_currency = ""
    highest_count = -1
    highest_level = "LOW"

    level_order = {"LOW": 0, "MEDIUM": 1, "HIGH": 2, "CRITICAL": 3}

    for currency, exposure in baskets.items():
        exposure_order = level_order.get(exposure.exposure_level, 0)
        if exposure.net_count > highest_count or (
            exposure.net_count == highest_count
            and exposure_order > level_order.get(highest_level, 0)
        ):
            highest_currency = currency
            highest_count = exposure.net_count
            highest_level = exposure.exposure_level

    return highest_currency, highest_level


def create_basket_snapshot(
    positions: List[Position],
    state_version: int,
    spot_prices: Optional[Dict[str, Decimal]] = None,
) -> BasketSnapshot:
    """
    Create complete basket snapshot.

    Thread-safe point-in-time capture of all exposures.

    Args:
        positions: Current open positions
        state_version: Current state version
        spot_prices: Optional spot prices for XAU

    Returns:
        BasketSnapshot with all exposures

    Example:
        >>> snapshot = create_basket_snapshot(positions, state_version=1)
        >>> snapshot.total_positions
        10
        >>> snapshot.highest_exposure_currency
        'USD'
    """
    baskets = calculate_basket_exposure(positions, spot_prices)
    highest_currency, highest_level = find_highest_exposure(baskets)

    return BasketSnapshot(
        timestamp=datetime.now(timezone.utc),
        baskets=baskets,
        total_positions=len(positions),
        highest_exposure_currency=highest_currency,
        highest_exposure_level=highest_level,
        state_version=state_version,
    )


def project_exposure_with_signal(
    current_positions: List[Position],
    signal_position: Position,
    state_version: int,
    spot_prices: Optional[Dict[str, Decimal]] = None,
) -> BasketSnapshot:
    """
    Project exposure if signal were executed.

    Creates a hypothetical snapshot including the proposed position.

    Args:
        current_positions: Current open positions
        signal_position: Proposed position from signal
        state_version: Current state version
        spot_prices: Optional spot prices

    Returns:
        BasketSnapshot with projected exposures
    """
    projected_positions = current_positions + [signal_position]
    return create_basket_snapshot(projected_positions, state_version, spot_prices)
