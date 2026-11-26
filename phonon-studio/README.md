# Phonon Studio

Desktop **AI Podcast & Voice Studio** + **Audio Lab** built with Rust and Dioxus 0.7 directly on top of the Phonon workspace crates.

See [docs/studio-architecture.md](../docs/studio-architecture.md) for the full architecture and Phase 2 roadmap.

## Capabilities

### AI Podcast Studio (Primary Workspace)
- **Script & Project Editor**: Multi-speaker dialogue editor with chapter markers, per-segment language overrides, drag-and-drop import (`.phonon.json` or legacy CLI `PodcastScript` JSON), and bounded Undo/Redo.
- **Multi-Vendor BYOK Providers**: Switch freely across `minimax`, `qwen`, `glm`, `stepfun`, `deepseek`, `openai`, `elevenlabs`, `edge` (keyless out-of-the-box), or custom `phonon.toml` endpoints with live environment-key status and verified preset voice selectors.
- **AI Script Drafting**: Draft structured chaptered podcast scripts from a topic via the configured LLM provider.
- **Per-Segment Cached Preview & Local Regeneration**: Content-addressed fingerprinting + `CachingTts` filesystem cache — edit any single line and re-synthesize only that line.
- **Recorded Voice & Text-Driven Audio Cutting ("改文本即剪音频")**: Attach real voice recordings to any segment, transcribe & align into clickable word/clause chips via STT (or offline 20ms VAD speech-clip detection without an API key), auto-highlight & cut fillers in 1 click, stitch seamlessly with 15ms cosine micro-crossfades, and enable local pure-Rust Broadcast Voice Enhancement (`enhance_voice`).
- **Local MCP Live Co-Editing**: Bind a `.phonon.json` project file and enable 500ms MCP Live Sync to let external Agents (`phonon-mcp`) co-edit segments, cut fillers, or adjust mastering live with full Undo safety.
- **Broadcast Mastering & Publishing**: Relative-LUFS BGM mixing, Intro/Outro stitching, BS.1770-4 loudness mastering (`-16` / `-14` / `-18` / `-23` LUFS with `-1.0 dBTP` true-peak ceiling), live LUFS/dBTP/LRA telemetry, click-to-seek Chapters, synchronized SRT preview, ID3 metadata + cover art, and one-click export (`MP3`, `WAV`, `FLAC`, `OGG`, `SRT`, `Chapters TXT`).

### Audio Lab (Secondary Workspace)
- Waveform & 64-band FFT spectrum visualization with BS.1770-4 LUFS & True Peak metrics.
- Offline DSP (Local Broadcast Voice Enhance presets, Auto Strip Silence, BS.1770 LUFS normalize, peak normalize, fade in/out, speed, compressor presets) and 5-band Parametric EQ.
- Real-time `DspPlugin` effect chain with live slider parameter sync and optional plugin-chain baking on export.
- Shared real-time transport bar with interactive seek slider, live playhead timecode, and instant volume control.

## Running & Building

```bash
# Rebuild Tailwind v4 stylesheet after changing UI classes
cd phonon-studio && npm run build && cd ..

# Run desktop application
cargo run -p phonon-studio

# Run headless unit tests for Studio core & operations
cargo test -p phonon-studio
```
