# Installation Guide

`rust-plex-mcp` provides both a native GPU desktop dashboard (`plex-app.exe`) and a lightweight MCP stdio server / CLI (`rust-plex-mcp.exe`).

## Installation Options

### Option 1: Windows Installer (Recommended)

1. Download `Plex-Rust-0.4.0-setup.exe` from GitHub Releases.
2. Run the installer. It installs binaries to `%LOCALAPPDATA%\Programs\rust-plex-mcp` (per-user installation, no administrator privileges or UAC required).
3. Shortcuts are created in the Start Menu under **Plex (Rust)**.

### Option 2: Portable Release Binaries

Download `plex-app.exe` and `rust-plex-mcp.exe` directly from the release assets and place them in any folder on your `PATH`.

Verify checksums against `SHA256SUMS.txt`:
```powershell
Get-FileHash plex-app.exe, rust-plex-mcp.exe
```

### Option 3: Building from Source

Prerequisites:
- Rust 1.80+ (`cargo`, `rustc`)
- Just command runner (`cargo install just`)
- NSIS 3.0+ (optional, for building the installer)
- `protoc` (required: LanceDB storage builds on it — `winget install --id Google.Protobuf`)
- First `rag-sync` downloads the MiniLM embedding model once (~90 MB, minutes), then embeds on CPU

```powershell
git clone https://github.com/sandraschi/rust-plex-mcp.git
cd rust-plex-mcp
cargo build --workspace --release
```

Release binaries will be generated in `target/release/`.

## Configuring MCP Clients

### Claude Desktop
Edit `%APPDATA%\Claude\claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "plex": {
      "command": "C:\\Users\\<USER>\\AppData\\Local\\Programs\\rust-plex-mcp\\rust-plex-mcp.exe",
      "args": ["mcp"]
    }
  }
}
```

### Antigravity
Edit `mcp_servers.json`:
```json
{
  "plex": {
    "command": "C:\\Users\\<USER>\\AppData\\Local\\Programs\\rust-plex-mcp\\rust-plex-mcp.exe",
    "args": ["mcp"]
  }
}
```

### Environment Variables
Optionally set credentials via environment variables:
- `PLEX_URL`: Base URL to your Plex Media Server (default: `http://127.0.0.1:32400`)
- `PLEX_TOKEN`: Your Plex authentication token (also securely stored in Windows Credential Manager via the desktop app Settings)
