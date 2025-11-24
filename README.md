# Solana Scanner

A high-performance Solana blockchain scanner that monitors transactions from specific programs via Yellowstone gRPC.

## Features

- ✅ **Real-time TUI Dashboard**: Beautiful terminal dashboard built with ratatui
- ✅ **Config-based setup**: Load configuration from TOML or JSON files
- ✅ **Automatic reconnection**: Exponential backoff retry logic for failed connections
- ✅ **Graceful shutdown**: Proper signal handling (Ctrl+C or ESC/q in dashboard)
- ✅ **Structured logging**: Using `tracing` for observability
- ✅ **Multiple gRPC endpoints**: Support for multiple Yellowstone gRPC connections
- ✅ **Live metrics**: Track uptime, transaction count, current slot, and connection status

## Configuration

Create a `config.toml` file in the project root:

```toml
arb_programs = [
    "CroWg74XNDF8UMnAZVbXx49iVj7iJ7b4CsqTCVWF7aK",
    "rvgxRnWNAWoLJkAc6S7y1zSNW9eAeFvpjV65YMHYxow",
]

[[grpc]]
endpoint = "http://your-rpc-endpoint:10000/"
```

Or use JSON format (config.json):

```json
{
  "grpc": [
    {
      "endpoint": "http://your-rpc-endpoint:10000/"
    }
  ],
  "arb_programs": [
    "CroWg74XNDF8UMnAZVbXx49iVj7iJ7b4CsqTCVWF7aK",
    "rvgxRnWNAWoLJkAc6S7y1zSNW9eAeFvpjV65YMHYxow"
  ]
}
```

### Environment Variables

- `RUST_LOG`: Logging level (e.g., `info`, `debug`, `trace`)

## Usage

```bash
# Build the project
cargo build --release

# Run with default config.toml (launches TUI dashboard)
cargo run --release

# Or specify a custom config file
cargo run --release -- my-config.toml

# Use the built binary directly
./target/release/scanner
./target/release/scanner config.toml

# Run with debug logging
RUST_LOG=debug cargo run --release

# Show help
cargo run --release -- --help
```

### Dashboard Controls

- **q** or **ESC**: Quit the scanner
- The dashboard automatically updates in real-time with:
  - **Header**: Application title
  - **Statistics**: Uptime, connection status, transaction count, current slot
  - **gRPC Connections**: Status of each endpoint (green = connected, red = disconnected)
  - **Recent Transactions**: Last 20 transaction signatures
  - **Logs**: Last 8 log messages with timestamps and severity levels

## Architecture

### Reconnection Logic

The scanner implements automatic reconnection with exponential backoff:
- Initial retry delay: 1 second
- Maximum retry delay: 60 seconds
- Backoff multiplier: 2x per retry
- Resets to 1 second on successful connection

### Graceful Shutdown

Press `Ctrl+C` to trigger graceful shutdown. The scanner will:
1. Cancel all active tasks
2. Close gRPC connections
3. Flush remaining messages
4. Exit cleanly

## Development

```bash
# Check code
cargo check

# Run tests
cargo test

# Format code
cargo fmt

# Run linter
cargo clippy
```

## Dependencies

- `tokio`: Async runtime
- `yellowstone-grpc-client`: Solana gRPC client
- `tracing`: Structured logging
- `anyhow`: Error handling
- `dashmap`: Concurrent hashmap
- `serde`: Serialization/deserialization
