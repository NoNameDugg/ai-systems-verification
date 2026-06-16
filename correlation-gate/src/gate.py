"""
Gate Engine for Correlation Gate
=================================

Core gate logic with thread safety and fail-closed behavior.
Implements all Diamond Standard requirements.
"""

import logging
import threading
import time
import uuid
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from typing import Any, Callable, Dict, List, Optional, Set, Tuple

from src.models import (
    Position,
    TradeSignal,
    BasketExposure,
    BasketSnapshot,
    GateDecision,
    GateConfig,
    GateState,
    PendingSignal,
    SyncReport,
    SyncResult,
)
from src.basket import (
    calculate_basket_exposure,
    create_basket_snapshot,
    project_exposure_with_signal,
)
from src.directional import parse_directional_exposure
from src.exceptions import (
    GateError,
    InvalidSignalError,
    InvalidInstrumentError,
    InvalidDirectionError,
    GateTimeoutError,
    ProviderError,
)
from src.security.audit import AuditLogger


logger = logging.getLogger(__name__)


# =============================================================================
# POSITION PROVIDER INTERFACE
# =============================================================================


class PositionProvider:
    """
    Abstract interface for position providers.

    Implementations fetch positions from various sources
    (OANDA API, MarketSim, mock for testing).
    """

    def fetch_positions(self) -> List[Position]:
        """Fetch current open positions."""
        raise NotImplementedError

    def is_available(self) -> bool:
        """Check if provider is available."""
        raise NotImplementedError

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Get time of last successful fetch."""
        raise NotImplementedError


# =============================================================================
# CORRELATION GATE CLASS
# =============================================================================


class CorrelationGate:
    """
    Core gate engine for evaluating trade signals.

    Thread-safe with atomic operations.
    Implements Diamond Standard requirements:
    - Dual-metric gating (count + notional)
    - Initial sync lock (HARD_BLOCK during INITIALIZING)
    - Dynamic XAU scaling (real-time price)
    - Fail-closed mandate (all errors -> HARD_BLOCK)

    Attributes:
        config: GateConfig instance
        state: Current GateState

    Example:
        >>> config = GateConfig(soft_warning_count=2, hard_block_count=3)
        >>> provider = OandaPositionProvider(credentials)
        >>> gate = CorrelationGate(config, provider)
        >>> gate.initialize()
        True
        >>> decision = gate.evaluate(signal)
    """

    def __init__(
        self,
        config: GateConfig,
        provider: PositionProvider,
        audit_logger: Optional[AuditLogger] = None,
    ) -> None:
        """
        Initialize gate with config and provider.

        Gate starts in INITIALIZING state and blocks all signals
        until initialize() is called successfully.

        Args:
            config: Gate configuration
            provider: Position data provider
            audit_logger: Optional audit logger for decision/state tracking
        """
        self._config = config
        self._provider = provider
        self._audit_logger = audit_logger

        # DIAMOND POINT #2: Start in INITIALIZING state
        self._state = GateState.INITIALIZING

        # Thread safety
        self._lock = threading.RLock()

        # Position cache
        self._positions: List[Position] = []
        self._last_fetch: Optional[datetime] = None

        # Pending signals
        self._pending: Dict[str, PendingSignal] = {}

        # State version counter
        self._state_version = 0

        # Spot prices cache (for XAU)
        self._spot_prices: Dict[str, Decimal] = {}

        logger.info("CorrelationGate initialized in INITIALIZING state")

    def _log_state_change(
        self, old_state: GateState, new_state: GateState, reason: str
    ) -> None:
        """Log state transition via audit logger if present."""
        if self._audit_logger:
            try:
                self._audit_logger.log_state_change(old_state, new_state, reason)
            except Exception:
                # Audit logging failure should not affect gate operation
                pass

    def _log_decision(
        self,
        signal: TradeSignal,
        decision: GateDecision,
        context: Optional[Dict[str, Any]] = None,
    ) -> None:
        """Log gate decision via audit logger if present."""
        if self._audit_logger:
            try:
                self._audit_logger.log_decision(signal, decision, context)
            except Exception:
                # Audit logging failure should not affect gate operation
                pass

    def _log_error(
        self, error: Exception, context: Optional[Dict[str, Any]] = None
    ) -> None:
        """Log error via audit logger if present."""
        if self._audit_logger:
            try:
                self._audit_logger.log_error(error, context)
            except Exception:
                # Audit logging failure should not affect gate operation
                pass

    @property
    def state(self) -> GateState:
        """Get current gate state."""
        return self._state

    def _state_override_for_test(self, state_name: str) -> None:
        """
        Override state for testing purposes only.

        WARNING: Only for test fixtures. Do not use in production.
        """
        self._state = GateState(state_name)

    # =========================================================================
    # INITIALIZATION
    # =========================================================================

    def initialize(self) -> bool:
        """
        Perform initial synchronization.

        Must be called before evaluate(). Transitions gate from
        INITIALIZING to READY (success) or FAILED (error).

        Returns:
            True if initialization successful, False otherwise.

        Example:
            >>> gate = CorrelationGate(config, provider)
            >>> if gate.initialize():
            ...     # Gate is ready for use
            ...     decision = gate.evaluate(signal)
        """
        with self._lock:
            old_state = self._state
            self._state = GateState.SYNCHRONIZING
            self._log_state_change(
                old_state, GateState.SYNCHRONIZING, "Initialization started"
            )
            logger.info("Gate entering SYNCHRONIZING state")

            try:
                positions = self._provider.fetch_positions()
                self._positions = positions
                self._last_fetch = datetime.now(timezone.utc)
                self._state_version += 1
                old_state = self._state
                self._state = GateState.READY
                self._log_state_change(
                    old_state, GateState.READY, "Initialization successful"
                )
                logger.info(
                    f"Gate initialized successfully with {len(positions)} positions"
                )
                return True

            except Exception as e:
                old_state = self._state
                self._state = GateState.FAILED
                self._log_state_change(
                    old_state, GateState.FAILED, f"Initialization failed: {e}"
                )
                self._log_error(e, {"phase": "initialization"})
                logger.error(f"Gate initialization failed: {e}")
                return False

    # =========================================================================
    # EVALUATION
    # =========================================================================

    def evaluate(self, signal: TradeSignal) -> GateDecision:
        """
        Evaluate trade signal against current exposure.

        Thread-safe, atomic operation with fail-closed behavior.

        Args:
            signal: Trade signal to evaluate

        Returns:
            GateDecision with ALLOW, SOFT_WARNING, or HARD_BLOCK

        Performance:
            Target: < 5ms for 100 positions

        Example:
            >>> signal = TradeSignal("EUR_USD", "LONG", units=10000)
            >>> decision = gate.evaluate(signal)
            >>> if decision.decision == "ALLOW":
            ...     execute_trade(signal)
            ...     gate.confirm_execution(decision.pending_id)
        """
        start_time = time.perf_counter_ns()

        try:
            # DIAMOND POINT #2: Check state first
            if self._state == GateState.INITIALIZING:
                return self._fail_closed_decision(
                    reason="Gate is initializing - no position data available",
                    start_time=start_time,
                )

            if self._state == GateState.SYNCHRONIZING:
                return self._fail_closed_decision(
                    reason="Gate is synchronizing initial positions",
                    start_time=start_time,
                )

            if self._state == GateState.FAILED:
                return self._fail_closed_decision(
                    reason="Gate is in FAILED state", start_time=start_time
                )

            # Validate signal
            try:
                self._validate_signal(signal)
            except (InvalidInstrumentError, InvalidDirectionError) as e:
                return self._fail_closed_decision(
                    reason=f"Invalid signal: {e}", start_time=start_time
                )

            # DIAMOND POINT #4: Lock acquisition with timeout
            acquired = self._lock.acquire(timeout=self._config.lock_timeout_seconds)
            if not acquired:
                return self._fail_closed_decision(
                    reason="Lock acquisition timeout", start_time=start_time
                )

            try:
                # Fetch positions (with timeout handling)
                try:
                    positions = self._get_positions_safe()
                except GateTimeoutError as e:
                    return self._fail_closed_decision(
                        reason=f"Position fetch timeout: {e}", start_time=start_time
                    )
                except ProviderError as e:
                    return self._fail_closed_decision(
                        reason=f"Position provider error: {e}", start_time=start_time
                    )

                # Calculate current exposure
                current_snapshot = self._calculate_snapshot(positions)

                # Parse signal to get affected currencies
                parse_result = parse_directional_exposure(
                    signal.instrument, signal.direction
                )

                # Project exposure with signal
                signal_position = signal.to_position(
                    position_id=f"pending_{signal.signal_id}",
                    entry_price=Decimal("1.0"),  # Approximation for projection
                )
                projected_snapshot = project_exposure_with_signal(
                    positions, signal_position, self._state_version, self._spot_prices
                )

                # Include pending signals in projection
                projected_snapshot = self._include_pending_in_projection(
                    projected_snapshot, positions
                )

                # Evaluate thresholds
                decision, affected_currencies, reasons = self._evaluate_thresholds(
                    projected_snapshot.baskets, parse_result.affected_baskets
                )

                # Generate recommendations
                recommendations = self._generate_recommendations(
                    decision, affected_currencies, projected_snapshot.baskets
                )

                # Calculate evaluation time
                evaluation_time_ms = (time.perf_counter_ns() - start_time) / 1_000_000

                # Register pending if not HARD_BLOCK
                pending_id = None
                if decision != "HARD_BLOCK":
                    pending_id = self._register_pending(signal, decision)

                gate_decision = GateDecision(
                    decision=decision,
                    pending_id=pending_id,
                    affected_currencies=affected_currencies,
                    current_exposure=current_snapshot,
                    projected_exposure=projected_snapshot,
                    reason="; ".join(reasons) if reasons else "Within limits",
                    recommendations=recommendations,
                    evaluation_time_ms=evaluation_time_ms,
                )

                # Log decision via audit logger
                self._log_decision(signal, gate_decision)
                return gate_decision

            finally:
                self._lock.release()

        except Exception as e:
            # DIAMOND POINT #4: Unknown exception -> HARD_BLOCK
            logger.exception(f"Unexpected error in evaluate: {e}")
            signal_id = getattr(signal, "signal_id", None) if signal else None
            self._log_error(e, {"phase": "evaluation", "signal_id": signal_id})
            decision = self._fail_closed_decision(
                reason=f"Unexpected error: {type(e).__name__}: {str(e)}",
                start_time=start_time,
            )
            if signal:
                self._log_decision(signal, decision)
            return decision

    # =========================================================================
    # PENDING MANAGEMENT
    # =========================================================================

    def confirm_execution(self, pending_id: str) -> bool:
        """
        Confirm pending signal was executed.

        Args:
            pending_id: ID from GateDecision

        Returns:
            True if confirmed, False if not found

        Example:
            >>> decision = gate.evaluate(signal)
            >>> execute_trade(signal)
            >>> gate.confirm_execution(decision.pending_id)
            True
        """
        with self._lock:
            if pending_id in self._pending:
                del self._pending[pending_id]
                logger.info(f"Pending signal {pending_id} confirmed")
                return True
            return False

    def cancel_pending(self, pending_id: str) -> bool:
        """
        Cancel pending signal.

        Args:
            pending_id: ID from GateDecision

        Returns:
            True if cancelled, False if not found
        """
        with self._lock:
            if pending_id in self._pending:
                del self._pending[pending_id]
                logger.info(f"Pending signal {pending_id} cancelled")
                return True
            return False

    def get_pending_ids(self) -> Set[str]:
        """Get all current pending IDs."""
        with self._lock:
            return set(self._pending.keys())

    def synchronize_with_engine(self, active_pending_ids: List[str]) -> SyncResult:
        """
        DEC-062: Synchronize gate pending state with engine's known active IDs.

        This handshake eliminates "Zombie Slots" - pending signals that the gate
        tracks but the engine has forgotten (e.g., after a crash/restart).

        Args:
            active_pending_ids: List of pending IDs the engine knows about

        Returns:
            SyncResult with SUCCESS/FAILED status and detailed SyncReport

        Example:
            >>> # At engine startup, load persisted pending IDs
            >>> engine_pending_ids = load_from_state_file()
            >>> result = gate.synchronize_with_engine(engine_pending_ids)
            >>> if result.status == "FAILED":
            ...     sys.exit(1)  # Fail-closed: don't trade with unverified state
            >>> logger.info(f"Purged {len(result.report.purged_ids)} zombies")
        """
        try:
            with self._lock:
                # Convert to set for O(1) lookups
                active_set = set(active_pending_ids)
                current_gate_ids = set(self._pending.keys())

                # Identify zombie IDs (in gate but not in engine's active list)
                zombie_ids = current_gate_ids - active_set
                retained_ids = current_gate_ids & active_set

                # Purge zombies
                for zombie_id in zombie_ids:
                    del self._pending[zombie_id]
                    logger.info(f"[HANDSHAKE] Purged zombie pending ID: {zombie_id}")

                # Create report
                report = SyncReport(
                    purged_ids=list(zombie_ids),
                    retained_ids=list(retained_ids),
                )

                logger.info(
                    f"[HANDSHAKE] Synchronization complete: "
                    f"{len(zombie_ids)} zombies purged, {len(retained_ids)} slots retained"
                )

                return SyncResult(status="SUCCESS", report=report)

        except Exception as e:
            # Fail-safe: Set gate to FAILED state on any exception
            logger.error(f"[HANDSHAKE] Synchronization failed: {e}")
            old_state = self._state
            self._state = GateState.FAILED
            self._log_state_change(old_state, GateState.FAILED, f"Sync failed: {e}")
            self._log_error(e, {"phase": "synchronize_with_engine"})

            # Return FAILED result with empty report
            return SyncResult(
                status="FAILED", report=SyncReport(purged_ids=[], retained_ids=[])
            )

    # =========================================================================
    # EXPOSURE ACCESS
    # =========================================================================

    def get_exposure_snapshot(self) -> BasketSnapshot:
        """
        Get current exposure snapshot.

        Returns:
            BasketSnapshot with current exposures
        """
        with self._lock:
            return create_basket_snapshot(
                self._positions, self._state_version, self._spot_prices
            )

    def refresh_positions(self) -> int:
        """
        Force position cache refresh.

        Returns:
            Number of positions after refresh
        """
        with self._lock:
            try:
                self._positions = self._provider.fetch_positions()
                self._last_fetch = datetime.now(timezone.utc)
                self._state_version += 1
                return len(self._positions)
            except Exception as e:
                logger.error(f"Position refresh failed: {e}")
                self._log_error(e, {"phase": "refresh_positions"})
                if self._state == GateState.READY:
                    old_state = self._state
                    self._state = GateState.DEGRADED
                    self._log_state_change(
                        old_state, GateState.DEGRADED, f"Refresh failed: {e}"
                    )
                return len(self._positions)

    def get_statistics(self) -> Dict[str, Any]:
        """
        Get gate statistics.

        Returns:
            Dict with statistics
        """
        with self._lock:
            return {
                "state": self._state.value,
                "state_version": self._state_version,
                "position_count": len(self._positions),
                "pending_count": len(self._pending),
                "last_fetch": self._last_fetch.isoformat()
                if self._last_fetch
                else None,
                "config": {
                    "soft_warning_count": self._config.soft_warning_count,
                    "hard_block_count": self._config.hard_block_count,
                },
            }

    # =========================================================================
    # PRIVATE METHODS
    # =========================================================================

    def _validate_signal(self, signal: TradeSignal) -> None:
        """Validate signal format and values."""
        if not signal:
            raise InvalidSignalError(None, "signal is None")

        if not signal.instrument:
            raise InvalidSignalError(signal, "instrument is empty")

        # This will raise InvalidInstrumentError or InvalidDirectionError
        parse_directional_exposure(signal.instrument, signal.direction)

    def _get_positions_safe(self) -> List[Position]:
        """Get positions with timeout handling."""
        import concurrent.futures

        # Check if we need fresh data
        if self._last_fetch is None or self._should_refresh():
            try:
                # Use ThreadPoolExecutor for timeout
                with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
                    future = executor.submit(self._provider.fetch_positions)
                    try:
                        positions = future.result(
                            timeout=self._config.position_fetch_timeout_seconds
                        )
                        self._positions = positions
                        self._last_fetch = datetime.now(timezone.utc)
                        self._state_version += 1
                    except concurrent.futures.TimeoutError:
                        raise GateTimeoutError(
                            "position_fetch",
                            self._config.position_fetch_timeout_seconds,
                        )
            except GateTimeoutError:
                raise
            except Exception as e:
                raise ProviderError("position_provider", str(e))

        if self._positions is None:
            self._positions = []
        return self._positions

    def _should_refresh(self) -> bool:
        """Check if position cache should be refreshed."""
        if self._last_fetch is None:
            return True
        age = (datetime.now(timezone.utc) - self._last_fetch).total_seconds()
        return age > 5.0  # Refresh if older than 5 seconds

    def _evaluate_thresholds(
        self, projected: Dict[str, BasketExposure], affected_baskets: List[str]
    ) -> Tuple[str, List[str], List[str]]:
        """
        Evaluate projected exposure against thresholds.

        DIAMOND POINT #1: Checks count AND notional thresholds.

        Returns:
            Tuple of (decision, affected_currencies, reasons)
        """
        affected_currencies = []
        reasons = []

        # Check HARD_BLOCK thresholds first (most restrictive)
        for currency in affected_baskets:
            if currency not in projected:
                continue

            exposure = projected[currency]
            thresholds = self._config.get_thresholds_for_currency(currency)

            # Count threshold
            if exposure.net_count >= thresholds["hard_block_count"]:
                affected_currencies.append(currency)
                reasons.append(
                    f"{currency} count {exposure.net_count} >= "
                    f"{thresholds['hard_block_count']}"
                )
                return "HARD_BLOCK", affected_currencies, reasons

            # Net notional threshold
            if exposure.net_notional >= thresholds["hard_block_net_notional"]:
                affected_currencies.append(currency)
                reasons.append(
                    f"{currency} net notional ${exposure.net_notional:,.2f} >= "
                    f"${thresholds['hard_block_net_notional']:,.2f}"
                )
                return "HARD_BLOCK", affected_currencies, reasons

            # Gross notional threshold
            if exposure.gross_notional >= thresholds["hard_block_gross_notional"]:
                affected_currencies.append(currency)
                reasons.append(
                    f"{currency} gross notional ${exposure.gross_notional:,.2f} >= "
                    f"${thresholds['hard_block_gross_notional']:,.2f}"
                )
                return "HARD_BLOCK", affected_currencies, reasons

        # Check SOFT_WARNING thresholds
        warning_currencies = []
        warning_reasons = []

        for currency in affected_baskets:
            if currency not in projected:
                continue

            exposure = projected[currency]
            thresholds = self._config.get_thresholds_for_currency(currency)

            if exposure.net_count >= thresholds["soft_warning_count"]:
                warning_currencies.append(currency)
                warning_reasons.append(
                    f"{currency} count {exposure.net_count} approaching limit"
                )

            if exposure.net_notional >= thresholds["soft_warning_net_notional"]:
                if currency not in warning_currencies:
                    warning_currencies.append(currency)
                warning_reasons.append(f"{currency} net notional elevated")

            if exposure.gross_notional >= thresholds["soft_warning_gross_notional"]:
                if currency not in warning_currencies:
                    warning_currencies.append(currency)
                warning_reasons.append(f"{currency} gross notional elevated")

        if warning_currencies:
            return "SOFT_WARNING", warning_currencies, warning_reasons

        return "ALLOW", [], []

    def _generate_recommendations(
        self,
        decision: str,
        affected_currencies: List[str],
        projected: Dict[str, BasketExposure],
    ) -> List[str]:
        """Generate actionable recommendations."""
        recommendations = []

        if decision == "HARD_BLOCK":
            recommendations.append(
                "Consider closing existing positions in affected currencies"
            )
            recommendations.append("Review portfolio correlation before retry")
            for currency in affected_currencies:
                if currency in projected:
                    exp = projected[currency]
                    recommendations.append(
                        f"Reduce {currency} {exp.net_direction} exposure "
                        f"(currently {exp.net_count} positions)"
                    )

        elif decision == "SOFT_WARNING":
            recommendations.append("Proceeding will increase concentration risk")
            recommendations.append("Consider smaller position size")

        return recommendations

    def _register_pending(self, signal: TradeSignal, decision: str) -> str:
        """Register pending signal and return ID."""
        pending_id = f"pending_{uuid.uuid4().hex[:12]}"

        expires_at = datetime.now(timezone.utc) + timedelta(
            seconds=self._config.pending_timeout_seconds
        )

        self._pending[pending_id] = PendingSignal(
            pending_id=pending_id,
            signal=signal,
            decision=decision,
            registered_at=datetime.now(timezone.utc),
            expires_at=expires_at,
        )

        return pending_id

    def _include_pending_in_projection(
        self, snapshot: BasketSnapshot, current_positions: List[Position]
    ) -> BasketSnapshot:
        """Include pending signals in exposure projection."""
        # Convert pending signals to positions and recalculate
        if not self._pending:
            return snapshot

        # Purge expired pending first
        self._purge_expired_pending()

        # Create positions from pending signals
        pending_positions = []
        for pending in self._pending.values():
            if not pending.is_expired:
                pos = pending.signal.to_position(
                    position_id=pending.pending_id, entry_price=Decimal("1.0")
                )
                pending_positions.append(pos)

        if not pending_positions:
            return snapshot

        # Recalculate with pending included
        all_positions = current_positions + pending_positions
        return create_basket_snapshot(
            all_positions, self._state_version, self._spot_prices
        )

    def _purge_expired_pending(self) -> int:
        """Purge expired pending signals."""
        expired = [pid for pid, pending in self._pending.items() if pending.is_expired]
        for pid in expired:
            del self._pending[pid]
        return len(expired)

    def _fail_closed_decision(self, reason: str, start_time: int) -> GateDecision:
        """
        Generate a fail-closed HARD_BLOCK decision.

        DIAMOND POINT #4: Default response for ANY error condition.
        """
        evaluation_time_ms = (time.perf_counter_ns() - start_time) / 1_000_000

        logger.warning(f"Fail-closed HARD_BLOCK: {reason}")

        return GateDecision(
            decision="HARD_BLOCK",
            pending_id=None,
            affected_currencies=[],
            current_exposure=BasketSnapshot.empty(self._state_version),
            projected_exposure=BasketSnapshot.empty(self._state_version),
            reason=f"[FAIL-CLOSED] {reason}",
            recommendations=["Investigate error before retrying"],
            evaluation_time_ms=evaluation_time_ms,
        )

    def _calculate_snapshot(self, positions: List[Position]) -> BasketSnapshot:
        """Calculate basket snapshot from positions."""
        return create_basket_snapshot(positions, self._state_version, self._spot_prices)
