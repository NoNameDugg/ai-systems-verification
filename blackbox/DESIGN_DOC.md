# BlackBox: The Flight Recorder

## 1. Executive Summary
**blackbox** is a deterministic recording and replay system. It captures the entire state of the market and internal system events to a high-fidelity log, allowing developers to replay exact market conditions "offline" to debug complex, transient issues ("Heisenbugs").

## 2. Problem Statement
In high-frequency and complex algorithmic trading, bugs often occur due to specific, millisecond-level sequences of events (e.g., a quote arrives exactly when a signal is emitted). Standard logs (`INFO: Trade executed`) are insufficient to reproduce these states.

## 3. Solution Architecture

### 3.1. The Recorder (Write-Path)
*   **Tap Points**: Placed at the ingress (Market Data) and egress (Orders) of the system.
*   **Format**: **Simple Binary Encoding (SBE)** or a flat binary struct format for zero-allocation logging. Text/JSON is too slow and large.
*   **Strategy**: "Journaling". Every incoming packet from the exchange is written to the journal *before* being processed.

### 3.2. The Replayer (Read-Path)
*   **Mock Exchange**: A mode where the system disconnects from the real WebSocket and attaches to the `BlackBox` reader.
*   **Clock Simulation**: The system clock is mocked. Time advances only when the Replayer feeds the next "tick". This allows:
    *   **Step-Through Debugging**: Pause time, inspect state.
    *   **Fast-Forward**: Replay a day of trading in minutes.

### 3.3. Determinism Enforcers
*   Remove `datetime.now()` calls; replace with `context.now()` provided by the engine.
*   Fixed seeds for random number generators.

## 4. Technical Stack
*   **Language**: **Rust**. Speed and memory safety are paramount for the recorder to add near-zero latency.
*   **Storage**: Memory-mapped files (MMAP) for high-throughput writing.
*   **Compression**: Zstd (optional, post-process).

## 5. Development Phases
1.  **Phase 1: The Journal**: Build the high-speed binary writer.
2.  **Phase 2: The Tap**: Instrument the host `trading-engine` to journal all incoming ticks.
3.  **Phase 3: The Player**: Build the playback engine (Mock Exchange).
4.  **Phase 4: Integration**: Refactor the host `trading-engine` to accept an injected "Clock" and "Data Source".

## 6. Integration Points
*   **Core**: the host `trading-engine` (Market Data Source).
*   **Target**: the host `trading-engine` / `execution-engine` (Systems under test).
