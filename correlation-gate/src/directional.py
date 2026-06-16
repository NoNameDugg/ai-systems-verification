"""
Directional Mapper for Correlation Gate
========================================

Maps instrument + direction to currency exposures.
Supports all 7 USD majors plus XAU (Gold).
"""

import re
from decimal import Decimal
from typing import Dict, Optional, Tuple

from src.models import ParseResult, ParsedInstrument
from src.exceptions import InvalidInstrumentError, InvalidDirectionError


# =============================================================================
# CONSTANTS
# =============================================================================

MAJOR_CURRENCIES = frozenset(
    {"USD", "EUR", "GBP", "JPY", "CHF", "AUD", "CAD", "NZD", "XAU"}
)

SUPPORTED_PAIRS = frozenset(
    {
        "EUR_USD",
        "USD_JPY",
        "GBP_USD",
        "USD_CHF",
        "AUD_USD",
        "USD_CAD",
        "NZD_USD",
        "XAU_USD",
    }
)

VALID_DIRECTIONS = frozenset({"LONG", "SHORT"})

# Pattern for parsing instruments with separators
INSTRUMENT_PATTERN = re.compile(r"^([A-Z]{3})[-_/]?([A-Z]{3})$", re.IGNORECASE)

# Pattern for 6-character instruments without separator
INSTRUMENT_NO_SEP_PATTERN = re.compile(r"^([A-Z]{3})([A-Z]{3})$", re.IGNORECASE)


# =============================================================================
# PUBLIC FUNCTIONS
# =============================================================================


def parse_directional_exposure(instrument: str, direction: str) -> ParseResult:
    """
    Parse instrument and direction into currency exposures.

    Maps a trading pair and direction to the resulting directional
    exposure for each affected currency basket.

    Args:
        instrument: Forex pair (e.g., "EUR_USD", "EUR/USD", "EURUSD")
        direction: Trade direction ("LONG" or "SHORT")

    Returns:
        ParseResult containing:
        - exposures: Dict mapping currency -> direction
        - is_cross_pair: True if neither currency is USD
        - affected_baskets: List of affected currency codes
        - warnings: Any parsing warnings

    Raises:
        InvalidInstrumentError: If instrument format invalid
        InvalidDirectionError: If direction not LONG or SHORT

    Example:
        >>> result = parse_directional_exposure("EUR_USD", "LONG")
        >>> result.exposures
        {'EUR': 'LONG', 'USD': 'SHORT'}

    Performance:
        Target: < 0.1ms per parse
    """
    # Validate inputs
    if not instrument:
        raise InvalidInstrumentError("", "instrument cannot be empty")

    direction_upper = direction.upper().strip() if direction else ""
    if direction_upper not in VALID_DIRECTIONS:
        raise InvalidDirectionError(direction)

    # Parse instrument
    parsed = _parse_instrument_robust(instrument)
    if parsed is None:
        raise InvalidInstrumentError(instrument, "unrecognized format")

    # Validate currencies
    base_upper = parsed.base.upper()
    quote_upper = parsed.quote.upper()

    if base_upper not in MAJOR_CURRENCIES:
        raise InvalidInstrumentError(
            instrument, f"unrecognized base currency '{base_upper}'"
        )
    if quote_upper not in MAJOR_CURRENCIES:
        raise InvalidInstrumentError(
            instrument, f"unrecognized quote currency '{quote_upper}'"
        )

    # Build exposures
    # LONG pair = LONG base, SHORT quote
    # SHORT pair = SHORT base, LONG quote
    if direction_upper == "LONG":
        base_direction = "LONG"
        quote_direction = "SHORT"
    else:
        base_direction = "SHORT"
        quote_direction = "LONG"

    exposures = {base_upper: base_direction, quote_upper: quote_direction}

    # Determine if cross-pair
    is_cross = "USD" not in exposures

    # Warnings
    warnings = []
    if is_cross:
        warnings.append(
            f"Cross-pair {instrument} affects {base_upper} and {quote_upper} baskets"
        )

    return ParseResult(
        exposures=exposures,
        is_cross_pair=is_cross,
        affected_baskets=list(exposures.keys()),
        warnings=warnings,
    )


def extract_currencies(instrument: str) -> Tuple[str, str]:
    """
    Extract base and quote currencies from instrument.

    Args:
        instrument: Forex pair (e.g., "EUR_USD")

    Returns:
        Tuple of (base_currency, quote_currency)

    Raises:
        InvalidInstrumentError: If format invalid

    Example:
        >>> extract_currencies("EUR_USD")
        ('EUR', 'USD')
    """
    parsed = _parse_instrument_robust(instrument)
    if parsed is None:
        raise InvalidInstrumentError(instrument, "unrecognized format")
    return parsed.base.upper(), parsed.quote.upper()


def get_opposite_direction(direction: str) -> str:
    """
    Get opposite direction.

    Args:
        direction: "LONG" or "SHORT"

    Returns:
        "SHORT" if "LONG", "LONG" if "SHORT"

    Raises:
        InvalidDirectionError: If direction not valid

    Example:
        >>> get_opposite_direction("LONG")
        'SHORT'
    """
    direction_upper = direction.upper().strip() if direction else ""
    if direction_upper == "LONG":
        return "SHORT"
    elif direction_upper == "SHORT":
        return "LONG"
    else:
        raise InvalidDirectionError(direction)


def is_cross_pair(instrument: str) -> bool:
    """
    Check if instrument is a cross-pair (no USD).

    Args:
        instrument: Forex pair

    Returns:
        True if neither currency is USD

    Raises:
        InvalidInstrumentError: If format invalid

    Example:
        >>> is_cross_pair("EUR_JPY")
        True
        >>> is_cross_pair("EUR_USD")
        False
    """
    base, quote = extract_currencies(instrument)
    return base != "USD" and quote != "USD"


def calculate_xau_notional(units: int, spot_price: Decimal) -> Decimal:
    """
    Calculate XAU notional using real-time spot price.

    DIAMOND STANDARD: Dynamic pricing, no hardcoded values.

    Args:
        units: Number of ounces (positive)
        spot_price: Current XAU/USD mid price

    Returns:
        USD notional value

    Example:
        >>> calculate_xau_notional(10, Decimal("2050.00"))
        Decimal('20500.00')
    """
    return Decimal(str(units)) * spot_price


def calculate_usd_notional(
    instrument: str,
    units: int,
    entry_price: Decimal,
    spot_prices: Optional[Dict[str, Decimal]] = None,
) -> Decimal:
    """
    Calculate USD-equivalent notional for any position.

    Handles different instrument types:
    - USD base: notional = units
    - USD quote: notional = units * entry_price
    - XAU: notional = units * spot_price (dynamic)
    - Cross-pairs: requires conversion rate

    Args:
        instrument: Trading pair
        units: Position size
        entry_price: Entry price of position
        spot_prices: Current spot prices for XAU/cross-pair conversion

    Returns:
        USD-equivalent notional value

    Example:
        >>> calculate_usd_notional("EUR_USD", 10000, Decimal("1.0850"))
        Decimal('10850.00')
    """
    base, quote = extract_currencies(instrument)
    units_decimal = Decimal(str(abs(units)))

    # XAU handling - DIAMOND POINT #3
    if base == "XAU":
        if spot_prices and "XAU_USD" in spot_prices:
            return units_decimal * spot_prices["XAU_USD"]
        # Fallback to entry price if no spot
        return units_decimal * entry_price

    # USD is base currency (e.g., USD_JPY)
    if base == "USD":
        return units_decimal

    # USD is quote currency (e.g., EUR_USD)
    if quote == "USD":
        return units_decimal * entry_price

    # Cross-pair - need conversion through USD
    # For now, use entry price as approximation
    # In production, would use spot_prices for proper conversion
    return units_decimal * entry_price


# =============================================================================
# PRIVATE FUNCTIONS
# =============================================================================


def _parse_instrument_robust(instrument: str) -> Optional[ParsedInstrument]:
    """
    Robustly parse instrument handling various formats.

    Supported formats:
    - "EUR_USD" (underscore)
    - "EUR/USD" (slash)
    - "EUR-USD" (dash)
    - "EURUSD" (no separator, 6-char)
    - "XAUUSD" (commodity, no separator)

    Args:
        instrument: Raw instrument string

    Returns:
        ParsedInstrument or None if unparseable
    """
    if not instrument:
        return None

    instrument = instrument.strip()

    # Pattern with optional separator handles all valid formats:
    # - "EUR_USD" (underscore), "EUR/USD" (slash), "EUR-USD" (dash)
    # - "EURUSD" (6-char, no separator)
    match = INSTRUMENT_PATTERN.match(instrument)
    if match:
        return ParsedInstrument(
            base=match.group(1).upper(),
            quote=match.group(2).upper(),
            original=instrument,
        )

    return None
