"""
Test fixtures for Directional Mapper tests.

ASTRA Correlation Gate - TDI Phase 1, Part 1.1
Version: 2.0.0 (Diamond Standard)
"""

from decimal import Decimal
from typing import Dict, List, Tuple, Any


# =============================================================================
# VALID PAIRS WITH EXPECTED EXPOSURES
# =============================================================================

VALID_PAIRS_WITH_EXPECTED: List[Tuple[str, str, Dict[str, str], bool]] = [
    # (instrument, direction, expected_exposures, is_cross_pair)
    # Major USD Pairs - LONG
    ("EUR_USD", "LONG", {"EUR": "LONG", "USD": "SHORT"}, False),
    ("GBP_USD", "LONG", {"GBP": "LONG", "USD": "SHORT"}, False),
    ("AUD_USD", "LONG", {"AUD": "LONG", "USD": "SHORT"}, False),
    ("NZD_USD", "LONG", {"NZD": "LONG", "USD": "SHORT"}, False),
    # Major USD Pairs - SHORT
    ("EUR_USD", "SHORT", {"EUR": "SHORT", "USD": "LONG"}, False),
    ("GBP_USD", "SHORT", {"GBP": "SHORT", "USD": "LONG"}, False),
    # USD as Base Currency
    ("USD_JPY", "LONG", {"USD": "LONG", "JPY": "SHORT"}, False),
    ("USD_JPY", "SHORT", {"USD": "SHORT", "JPY": "LONG"}, False),
    ("USD_CHF", "LONG", {"USD": "LONG", "CHF": "SHORT"}, False),
    ("USD_CAD", "LONG", {"USD": "LONG", "CAD": "SHORT"}, False),
    # Gold (XAU)
    ("XAU_USD", "LONG", {"XAU": "LONG", "USD": "SHORT"}, False),
    ("XAU_USD", "SHORT", {"XAU": "SHORT", "USD": "LONG"}, False),
]


# =============================================================================
# CROSS-PAIRS (No USD)
# =============================================================================

CROSS_PAIRS_WITH_EXPECTED: List[Tuple[str, str, Dict[str, str], bool]] = [
    # (instrument, direction, expected_exposures, is_cross_pair)
    ("EUR_JPY", "LONG", {"EUR": "LONG", "JPY": "SHORT"}, True),
    ("EUR_JPY", "SHORT", {"EUR": "SHORT", "JPY": "LONG"}, True),
    ("EUR_GBP", "LONG", {"EUR": "LONG", "GBP": "SHORT"}, True),
    ("GBP_JPY", "LONG", {"GBP": "LONG", "JPY": "SHORT"}, True),
    ("AUD_JPY", "LONG", {"AUD": "LONG", "JPY": "SHORT"}, True),
    ("EUR_CHF", "LONG", {"EUR": "LONG", "CHF": "SHORT"}, True),
    ("GBP_CHF", "LONG", {"GBP": "LONG", "CHF": "SHORT"}, True),
    ("AUD_NZD", "LONG", {"AUD": "LONG", "NZD": "SHORT"}, True),
]


# =============================================================================
# ALTERNATE FORMAT PAIRS
# =============================================================================

ALTERNATE_FORMATS: List[Tuple[str, str]] = [
    # (input_format, expected_normalized)
    ("EUR/USD", "EUR_USD"),  # Slash separator
    ("EUR-USD", "EUR_USD"),  # Dash separator
    ("EURUSD", "EUR_USD"),  # No separator (6-char)
    ("eur_usd", "EUR_USD"),  # Lowercase
    ("Eur_Usd", "EUR_USD"),  # Mixed case
    (" EUR_USD ", "EUR_USD"),  # Whitespace
    ("  EUR_USD", "EUR_USD"),  # Leading whitespace
    ("EUR_USD  ", "EUR_USD"),  # Trailing whitespace
]


COMMODITY_ALTERNATE_FORMATS: List[Tuple[str, str]] = [
    # (input_format, expected_normalized)
    ("XAU/USD", "XAU_USD"),  # Slash
    ("XAU-USD", "XAU_USD"),  # Dash
    ("XAUUSD", "XAU_USD"),  # No separator (7-char commodity)
    ("xau_usd", "XAU_USD"),  # Lowercase
]


# =============================================================================
# INVALID INPUTS
# =============================================================================

INVALID_INSTRUMENTS: List[Tuple[str, str]] = [
    # (invalid_input, expected_error_type)
    ("INVALID", "InvalidInstrumentError"),
    ("XXX_USD", "UnknownCurrencyError"),
    ("EUR_XXX", "UnknownCurrencyError"),
    ("", "InvalidInstrumentError"),
    ("EUR", "InvalidInstrumentError"),  # Single currency
    ("EUR__USD", "InvalidInstrumentError"),  # Double underscore
    ("E_USD", "InvalidInstrumentError"),  # Too short base
    ("EUR_U", "InvalidInstrumentError"),  # Too short quote
    ("123_456", "InvalidInstrumentError"),  # Numeric
    ("EUR_USD_JPY", "InvalidInstrumentError"),  # Three currencies
    ("_EUR_USD", "InvalidInstrumentError"),  # Leading underscore
    ("EUR_USD_", "InvalidInstrumentError"),  # Trailing underscore
]


INVALID_DIRECTIONS: List[str] = [
    "BUY",
    "SELL",
    "HOLD",
    "",
    "LONGG",
    "SHORTT",
    "L",
    "S",
    "1",
    "TRUE",
]


# =============================================================================
# KNOWN CURRENCIES
# =============================================================================

KNOWN_CURRENCIES = {"USD", "EUR", "GBP", "JPY", "CHF", "AUD", "CAD", "NZD", "XAU"}


SUPPORTED_PAIRS = {
    "EUR_USD",
    "USD_JPY",
    "GBP_USD",
    "USD_CHF",
    "AUD_USD",
    "USD_CAD",
    "NZD_USD",
    "XAU_USD",
}


# =============================================================================
# NOTIONAL CALCULATION TEST DATA
# =============================================================================

NOTIONAL_TEST_CASES: List[Dict[str, Any]] = [
    {
        "instrument": "EUR_USD",
        "units": 100000,
        "spot_price": Decimal("1.0850"),
        "expected_usd_notional": Decimal("108500.00"),
        "description": "Standard EUR lot at 1.0850",
    },
    {
        "instrument": "XAU_USD",
        "units": 10,
        "spot_price": Decimal("2350.50"),
        "expected_usd_notional": Decimal("23505.00"),
        "description": "10 oz Gold at $2350.50",
    },
    {
        "instrument": "USD_JPY",
        "units": 100000,
        "spot_price": Decimal("154.50"),
        "expected_usd_notional": Decimal("100000.00"),
        "description": "USD/JPY - units already in USD",
    },
    {
        "instrument": "GBP_USD",
        "units": 50000,
        "spot_price": Decimal("1.2650"),
        "expected_usd_notional": Decimal("63250.00"),
        "description": "Half lot GBP at 1.2650",
    },
]


# =============================================================================
# PERFORMANCE TEST PARAMETERS
# =============================================================================

PERFORMANCE_ITERATIONS = 10000
PERFORMANCE_TARGET_MS = 0.1  # 100 microseconds per parse
BATCH_PERFORMANCE_SIZE = 1000
BATCH_PERFORMANCE_TARGET_MS = 50  # 50ms for 1000 parses
