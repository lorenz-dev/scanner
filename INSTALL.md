# Solana Scanner - Quick Start Guide

A high-performance Solana blockchain scanner that monitors transactions from specific programs via Yellowstone gRPC.

## Installation

### macOS
```bash
# Extract the archive
unzip scanner-*-macos.zip
cd scanner-*-macos/

# Make the binary executable (if needed)
chmod +x scanner

# Optional: Install system-wide
sudo cp scanner /usr/local/bin/
```

### Linux
```bash
# Extract the archive
unzip scanner-*-linux.zip
cd scanner-*-linux/

# Make the binary executable (if needed)
chmod +x scanner

# Optional: Install system-wide
sudo cp scanner /usr/local/bin/
```

## Configuration

Before running, create a `config.toml` file in the same directory as the scanner binary:

```toml
# List of Solana programs to monitor
arb_programs = [
    "CroWg74XNDF8UMnAZVbXx49iVj7iJ7b4CsqTCVWF7aK",
    "rvgxRnWNAWoLJkAc6S7y1zSNW9eAeFvpjV65YMHYxow",
]

# Yellowstone gRPC endpoint(s)
[[grpc]]
endpoint = "http://your-rpc-endpoint:10000/"
```

Alternatively, you can use JSON format (`config.json`):

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

## Running the Scanner

### Basic Usage

```bash
# Run with default config.toml
./scanner

# Run with custom config file
./scanner my-config.toml

# Run with debug logging
RUST_LOG=debug ./scanner

# Show help
./scanner --help
```

## Dashboard Interface

Once running, the scanner displays a real-time TUI (Terminal User Interface) dashboard with:

- **Statistics Panel**: Shows uptime, connection status, transaction count, and current slot
- **gRPC Connections**: Status of each endpoint (🟢 green = connected, 🔴 red = disconnected)
- **Recent Transactions**: List of last 20 transaction signatures
- **Logs**: Recent log messages with timestamps

### Dashboard Controls

- **q** or **ESC**: Quit the scanner gracefully
- **Ctrl+C**: Emergency shutdown

## Features

✅ Real-time monitoring of Solana transactions
✅ Automatic reconnection with exponential backoff
✅ Graceful shutdown handling
✅ Multiple gRPC endpoint support
✅ Beautiful terminal dashboard
✅ Structured logging

## Troubleshooting

### Connection Issues

If you see connection errors:
1. Verify your gRPC endpoint is correct and accessible
2. Check if the endpoint requires authentication
3. Ensure your network allows connections to the gRPC port

### Permission Denied

If you get "permission denied" errors:
```bash
chmod +x scanner
```

### Config Not Found

Make sure `config.toml` is in the same directory where you run the scanner, or specify the full path:
```bash
./scanner /path/to/config.toml
```

## Environment Variables

- `RUST_LOG`: Set logging level (`error`, `warn`, `info`, `debug`, `trace`)
  ```bash
  RUST_LOG=debug ./scanner
  ```

## Support

For issues and questions, please visit the project repository.
