"""
Custom Exceptions for Correlation Gate
======================================

All exceptions inherit from GateError base class.
Each exception includes context-rich information for debugging.
"""

from typing import Optional, Any


class GateError(Exception):
    """
    Base exception for all Correlation Gate errors.

    All gate-related exceptions inherit from this class,
    allowing callers to catch all gate errors with a single handler.

    Attributes:
        message: Human-readable error description
        context: Optional dict of contextual information

    Example:
        try:
            gate.evaluate(signal)
        except GateError as e:
            logger.error(f"Gate error: {e}")
    """

    def __init__(self, message: str, context: Optional[dict] = None) -> None:
        """
        Initialize GateError with message and optional context.

        Args:
            message: Human-readable error description
            context: Optional dict of additional context
        """
        self.message = message
        self.context = context or {}
        super().__init__(message)


class InvalidInstrumentError(GateError):
    """
    Raised when an instrument string cannot be parsed.

    Causes:
        - Empty instrument string
        - Unrecognized currency codes
        - Invalid separator format
        - Unsupported trading pair

    Attributes:
        instrument: The invalid instrument string
        reason: Specific reason for invalidity

    Example:
        raise InvalidInstrumentError("INVALID", "unrecognized format")
    """

    def __init__(self, instrument: str, reason: str) -> None:
        """
        Initialize InvalidInstrumentError.

        Args:
            instrument: The invalid instrument string
            reason: Why the instrument is invalid
        """
        self.instrument = instrument
        self.reason = reason
        message = f"Invalid instrument '{instrument}': {reason}"
        super().__init__(message, {"instrument": instrument, "reason": reason})


class InvalidDirectionError(GateError):
    """
    Raised when a direction string is not LONG or SHORT.

    Attributes:
        direction: The invalid direction string
        valid_directions: List of valid direction values

    Example:
        raise InvalidDirectionError("SIDEWAYS")
    """

    VALID_DIRECTIONS = {"LONG", "SHORT"}

    def __init__(self, direction: str) -> None:
        """
        Initialize InvalidDirectionError.

        Args:
            direction: The invalid direction string
        """
        self.direction = direction
        self.valid_directions = list(self.VALID_DIRECTIONS)
        message = f"Invalid direction '{direction}': must be LONG or SHORT"
        super().__init__(message, {"direction": direction})


class InvalidSignalError(GateError):
    """
    Raised when a trade signal has invalid format or values.

    This is a wrapper exception that may contain more specific
    errors (InvalidInstrumentError, InvalidDirectionError).

    Attributes:
        signal: The invalid signal (if available)
        reason: Specific reason for invalidity

    Example:
        raise InvalidSignalError(signal, "units must be positive")
    """

    def __init__(self, signal: Any, reason: str) -> None:
        """
        Initialize InvalidSignalError.

        Args:
            signal: The invalid signal object
            reason: Why the signal is invalid
        """
        self.signal = signal
        self.reason = reason
        instrument = getattr(signal, "instrument", "unknown") if signal else "unknown"
        message = f"Invalid signal for {instrument}: {reason}"
        super().__init__(message, {"signal": str(signal), "reason": reason})


class ConfigError(GateError):
    """
    Raised when gate configuration is invalid.

    Causes:
        - Missing required fields
        - Invalid threshold values
        - Inconsistent settings

    Attributes:
        field: The problematic configuration field
        value: The invalid value
        reason: Why it's invalid

    Example:
        raise ConfigError("hard_block_threshold", -1, "must be positive")
    """

    def __init__(self, field: str, value: Any, reason: str) -> None:
        """
        Initialize ConfigError.

        Args:
            field: Configuration field name
            value: The invalid value
            reason: Why it's invalid
        """
        self.field = field
        self.value = value
        self.reason = reason
        message = f"Configuration error in '{field}': {reason} (got {value})"
        super().__init__(message, {"field": field, "value": value, "reason": reason})


class ProviderError(GateError):
    """
    Base exception for position provider errors.

    Attributes:
        provider_name: Name of the failing provider

    Example:
        raise ProviderError("OANDA", "API connection failed")
    """

    def __init__(self, provider_name: str, message: str) -> None:
        """
        Initialize ProviderError.

        Args:
            provider_name: Name of the provider
            message: Error description
        """
        self.provider_name = provider_name
        full_message = f"Provider '{provider_name}' error: {message}"
        super().__init__(full_message, {"provider": provider_name})


class ProviderUnavailableError(ProviderError):
    """
    Raised when position provider is unavailable.

    This may be due to network issues, authentication failure,
    or the provider being in maintenance mode.

    Example:
        raise ProviderUnavailableError("OANDA")
    """

    def __init__(self, provider_name: str) -> None:
        """
        Initialize ProviderUnavailableError.

        Args:
            provider_name: Name of the unavailable provider
        """
        super().__init__(provider_name, "provider is unavailable")


class ProviderTimeoutError(ProviderError):
    """
    Raised when position provider times out.

    Attributes:
        timeout_seconds: The timeout that was exceeded

    Example:
        raise ProviderTimeoutError("OANDA", 5.0)
    """

    def __init__(self, provider_name: str, timeout_seconds: float) -> None:
        """
        Initialize ProviderTimeoutError.

        Args:
            provider_name: Name of the provider
            timeout_seconds: Timeout that was exceeded
        """
        self.timeout_seconds = timeout_seconds
        super().__init__(provider_name, f"operation timed out after {timeout_seconds}s")


class GateTimeoutError(GateError):
    """
    Raised when a gate operation times out.

    This could be position fetch, lock acquisition, or overall evaluation.

    Attributes:
        operation: What operation timed out
        timeout_seconds: The timeout that was exceeded

    Example:
        raise GateTimeoutError("lock_acquisition", 1.0)
    """

    def __init__(self, operation: str, timeout_seconds: float) -> None:
        """
        Initialize GateTimeoutError.

        Args:
            operation: Name of the operation that timed out
            timeout_seconds: Timeout that was exceeded
        """
        self.operation = operation
        self.timeout_seconds = timeout_seconds
        message = f"Gate operation '{operation}' timed out after {timeout_seconds}s"
        super().__init__(message, {"operation": operation, "timeout": timeout_seconds})


class GateStateError(GateError):
    """
    Raised when gate is in an invalid state for the requested operation.

    Attributes:
        current_state: Current gate state
        required_state: State required for the operation
        operation: What was attempted

    Example:
        raise GateStateError("INITIALIZING", "READY", "evaluate")
    """

    def __init__(self, current_state: str, required_state: str, operation: str) -> None:
        """
        Initialize GateStateError.

        Args:
            current_state: Current gate state
            required_state: Required state for operation
            operation: What was attempted
        """
        self.current_state = current_state
        self.required_state = required_state
        self.operation = operation
        message = (
            f"Cannot perform '{operation}' in state '{current_state}': "
            f"requires '{required_state}'"
        )
        super().__init__(
            message,
            {
                "current_state": current_state,
                "required_state": required_state,
                "operation": operation,
            },
        )


class PendingNotFoundError(GateError):
    """
    Raised when a pending signal ID is not found.

    This may occur when trying to confirm or cancel a pending
    that has already been processed or has expired.

    Attributes:
        pending_id: The ID that was not found

    Example:
        raise PendingNotFoundError("pending_12345")
    """

    def __init__(self, pending_id: str) -> None:
        """
        Initialize PendingNotFoundError.

        Args:
            pending_id: The pending ID that was not found
        """
        self.pending_id = pending_id
        message = f"Pending signal '{pending_id}' not found"
        super().__init__(message, {"pending_id": pending_id})


# =============================================================================
# PHASE 2: PROVIDER-SPECIFIC EXCEPTIONS
# =============================================================================


class ProviderAuthError(ProviderError):
    """
    Raised when provider authentication or authorization fails.

    Causes:
        - Invalid API token
        - Expired credentials
        - Insufficient permissions (403)

    Attributes:
        reason: Specific authentication failure reason

    Example:
        raise ProviderAuthError("OANDA", "Invalid API token")
    """

    def __init__(self, provider_name: str, reason: str) -> None:
        """
        Initialize ProviderAuthError.

        Args:
            provider_name: Name of the provider
            reason: Why authentication failed
        """
        self.reason = reason
        super().__init__(provider_name, f"authentication failed: {reason}")


class ProviderRateLimitError(ProviderError):
    """
    Raised when provider rate limit is exceeded.

    Attributes:
        retry_after_seconds: Suggested wait time before retry

    Example:
        raise ProviderRateLimitError("OANDA", 60.0)
    """

    def __init__(
        self, provider_name: str, retry_after_seconds: Optional[float] = None
    ) -> None:
        """
        Initialize ProviderRateLimitError.

        Args:
            provider_name: Name of the provider
            retry_after_seconds: How long to wait before retry
        """
        self.retry_after_seconds = retry_after_seconds
        msg = "rate limit exceeded"
        if retry_after_seconds is not None:
            msg += f", retry after {retry_after_seconds}s"
        super().__init__(provider_name, msg)


class CacheError(GateError):
    """
    Raised when a cache operation fails.

    Causes:
        - Cache corruption
        - Serialization failure
        - Memory exhaustion

    Attributes:
        operation: What cache operation failed
        reason: Why it failed

    Example:
        raise CacheError("set", "memory exhausted")
    """

    def __init__(self, operation: str, reason: str) -> None:
        """
        Initialize CacheError.

        Args:
            operation: Cache operation that failed
            reason: Why it failed
        """
        self.operation = operation
        self.reason = reason
        message = f"Cache {operation} failed: {reason}"
        super().__init__(message, {"operation": operation, "reason": reason})
