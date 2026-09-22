# Integration Tests

Tests for component interaction and end-to-end scenarios.

## Test Files

| File | Focus |
| ------ | ------- |
| `test_integration.py` | Core gate integration |
| `test_provider_integration.py` | Provider stack |
| `test_security_integration.py` | Security components |

## Key Scenarios

### Core Integration (`test_integration.py`)
- Gate + Provider full workflow
- Multi-signal evaluation sequences
- State consistency across operations
- Pending signal lifecycle

### Provider Integration (`test_provider_integration.py`)
- OANDA + Cache integration
- Fallback provider chain
- Provider factory instantiation
- Health monitor integration

### Security Integration (`test_security_integration.py`)
- Atomic guard + Gate evaluation
- Audit logger + Gate decisions
- Full security stack validation
- Concurrent access with audit

## Running

```bash
pytest tests/integration/ -v
```

## End-to-End Test Example

```python
def test_full_evaluation_workflow():
    """Complete evaluation from signal to confirmation."""
    # Setup
    provider = SimulatorPositionProvider()
    gate = CorrelationGate(config, provider)
    gate.initialize()

    # Evaluate
    decision = gate.evaluate(signal)

    # Confirm
    gate.confirm_execution(decision.pending_id)

    # Verify state
    assert gate.get_statistics().evaluation_count == 1
```

