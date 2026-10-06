# Network Layer

`oanda_stream.rs` is the module the binary uses: OANDA HTTP chunked-stream consumption with an idle timeout and the reconnect policy (added in v1.2 when an outside review found a silent connection was never detected and the reconnect delay could reset to zero).

The rest of this directory — the WebSocket connector, heartbeat monitoring, the reconnection manager and the exchange-specific adapters — is **prototype** code: tested, but not used by the binary, whose transport is HTTP streaming, not WebSocket.
