# rust-plex-mcp

Pure-Rust Plex Media Server interface, 36-tool Model Context Protocol (MCP) server, and native GPU-rendered desktop dashboard.

## What this wraps

This project interfaces with **[Plex Media Server](https://www.plex.tv/)**, the leading media platform for personal video, music, and photo streaming. It connects to local or remote Plex servers via REST API with token authentication, or runs in self-contained demo mode.

## What You Can Do

- **Full Library Exploration**: Browse sections, movies, television series, seasons, and episodes across your entire library.
- **Media Inspector**: View detailed audio/video codec telemetry, stream bitrates, file container paths, resolutions, and runtimes.
- **Active Streaming & Telemetry**: Inspect active transcode and direct play sessions, streaming bandwidth, user sessions, and connected player clients.
- **Remote Playback Control**: Send safe playback commands (play, pause, stop, step) to active Plex player clients with confirmation fences.
- **Media Repair & Integrity**: Detect unplayable containers, missing audio streams, corrupt codecs, and missing poster metadata.
- **Universal & Semantic Search**: Search titles and artists, or perform natural-language semantic similarity searches over synopsis embeddings.
- **Curated Canon Coverage**: Track collection completeness against IMDB Top 250, AFI 100, and custom franchise collections.
- **Taste Analytics & Recommendations**: View watch time distributions, top genres, directors, and AI-driven recommendations.
- **AI Media Assistant**: Interactive conversational copilot with local Ollama (`gemma4:12b`), LM Studio, or cloud models.
- **Fleet Crossconnect**: Speak media alerts via `speech-mcp` or push telemetry events to `aiwatcher`.

## Install

### 1. Windows Installer (Recommended)
1. Download **`Plex-Rust-0.4.0-setup.exe`** from the [latest release](https://github.com/sandraschi/rust-plex-mcp/releases/latest).
2. Run the installer. Windows may show a SmartScreen warning for unsigned binaries: choose **More info > Run anyway**.
3. Open **Plex (Rust)** from your Start menu.

### 2. For Claude Desktop / Antigravity / Cursor (MCP Server Only)
Add `rust-plex-mcp` to your MCP client configuration (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "plex": {
      "command": "C:\\path\\to\\rust-plex-mcp.exe",
      "args": ["mcp"]
    }
  }
}
```

For full installation options, see [INSTALL.md](INSTALL.md).  
Developers: build instructions and test recipes are in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Example Prompts

Once configured in Claude or your AI agent, you can ask natural-language questions:

- *"What movies in my 4K library have Dolby Atmos audio tracks?"*
- *"Who is currently streaming on my Plex server and are they transcoding?"*
- *"Find any cyberpunk sci-fi movies in my collection under 100 minutes that I haven't watched yet."*
- *"What is my coverage percentage for the IMDB Top 250 movies?"*

## Documentation

| Document | Description |
|---|---|
| [Installation Guide](INSTALL.md) | Full setup options (installer, portable exe, build from source) |
| [Product Requirements Document](docs/PRD.md) | PRD, scope, architecture, and feature verification |
| [UI Parity Specification](docs/UI_PARITY.md) | 19-route UI/UX clone parity with legacy Next.js webapp |
| [Architecture](docs/ARCHITECTURE.md) | 3-crate ArtCraft architecture and async command bus |
| [Conversion Dossier](docs/CONVERSION_DOSSIER.md) | Forensic migration audit from Next.js to pure Rust |
| [Conversion Log](docs/CONVERSION_LOG.md) | Chronological migration history and milestones |

## Requirements

- **Operating System**: Windows 10/11, macOS 13+, or Linux x64
- **Plex Media Server**: Plex Media Server 1.30+ running locally or remotely (or run in Demo mode)
- **Claude / Cursor / Antigravity**: Any MCP-compatible AI client

## License

Dual-licensed under MIT OR Apache-2.0.
