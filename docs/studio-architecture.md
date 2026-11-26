# Phonon Studio Desktop Architecture (v0.9)

## 1. Product Positioning & Scope

Phonon Studio is positioned as a **local-first, multi-vendor BYOK AI Podcast & Voice Studio** — not a traditional DAW or general-purpose waveform editor.

### Dual-Workspace Layout
- **Primary Workspace — AI Podcast Studio**: Script-driven podcast production powered directly by `phonon-ai` (`Config`, `AnyTts`, `CachingTts`, `ChatEndpoint`), `phonon-podcast` (`PodcastScript`, `PodcastPipeline`, `OpenAiScriptGenerator`, `Episode`), `phonon-core` (BS.1770-4 LUFS + true-peak limiting), `phonon-engine` (real-time playback + seek), and `phonon-io` (tagged MP3/FLAC/OGG/WAV/AAC export).
- **Secondary Workspace — Audio Lab**: Preserves single-file audio inspection & DSP processing (Waveform, FFT Spectrum, LUFS/RMS/Peak meters, Effects, 5-band Parametric EQ, Real-time Plugin Chain with live parameter sync and plugin-baked export).

### Phase Split (Current Version Boundary)
- **Phase 1 (MVP — this milestone)**:
  1. Project/Script editor with Undo/Redo (`StudioProject` wrapping `PodcastScript`, compatible with existing `.json` podcast scripts).
  2. Multi-vendor AI provider selector (`minimax`, `qwen`, `glm`, `stepfun`, `deepseek`, `openai`, `elevenlabs`, `edge`, plus custom `phonon.toml`), environment key presence indicator, and verified voice dropdowns per provider.
  3. Optional AI Script Drafting from topic via configured LLM (`OpenAiScriptGenerator`).
  4. Per-segment content-addressed cached synthesis (`CachingTts`) with instant single-segment `Preview` and cache-bypassing `Regenerate`.
  5. Show & Broadcast Mastering controls: segment pause, crossfade, BGM/Intro/Outro file selection, relative BGM level, fade in/out, and optional BS.1770-4 `target_lufs` mastering (`-16` Apple Podcasts / `-14` Spotify / `-18` Mono / `-23` EBU R128).
  6. Episode Inspector: Waveform + BS.1770-4 Loudness & True-Peak meter, click-to-seek Chapters, live SRT Subtitle preview, PodcastTags + Cover art editor, and one-click export (MP3 w/ ID3 chapters & cover, WAV, FLAC, OGG, SRT, Chapters TXT).
  7. Audio Lab bug fixes: live plugin parameter sync to `AudioPlayer`, plugin sync on lazy player creation, real-time volume slider & seek bar, fix paused-track switch resume bug, and optional baking of active plugin chain during export.
- **Phase 2 (Implemented — v0.9.1)**:
  - **Recorded Voice + Text-Driven Audio Cutting ("改文本即剪音频")**: Attach real voice recordings to any segment (`SegmentMediaOverride`), align into clickable word/clause/clip spans via `phonon-ai` STT (with automatic offline 20ms RMS VAD fallback `detect_speech_regions`), auto-tag Chinese/English fillers, click any chip to cut/restore non-destructively via `phonon_core::splice_ranges` (15ms raised-cosine micro-crossfade), and mix human + AI TTS segments in the same episode.
  - **Local Broadcast Voice Enhancement**: Pure-Rust `phonon_core::enhance_voice` (80Hz high-pass + minimum-statistics STFT Wiener spectral denoiser + broadcast vocal presence EQ + compression + BS.1770 LUFS mastering) with zero external C++ runtime or model downloads. Re-evaluation trigger for optional `ort` DeepFilterNet3 feature: if non-stationary noise (café chatter / keyboard clicks) leaves SNR degradation > 6 dB on real recordings.
  - **Local MCP Live Co-Editing**: 7 new `phonon-mcp` tools (`audio_enhance_voice`, `audio_splice`, `studio_get_project`, `studio_edit_segment`, `studio_add_segment`, `studio_delete_segment`, `studio_set_mastering`) using atomic temp+rename writes inside `PHONON_MCP_ROOT`, paired with Studio's 500ms live-sync watcher that hot-reloads external Agent edits onto Undo history.

## 2. Minimal Backend Seams

To avoid wrapper layers or duplicated logic inside the GUI, three small, high-cohesion seams are added to existing crates:

| Crate | Addition | Purpose |
| --- | --- | --- |
| `phonon-engine` | `AudioPlayer::set_plugin_param(&self, idx, name, val)` and `AudioPlayer::replace_plugins(&self, plugins)` | Live parameter updates from UI sliders and deterministic chain sync when the player is initialized lazily |
| `phonon-ai` | `builtin_presets() -> Vec<ProviderPresetInfo>` and `Config::tts_provider_name()` / `Config::llm_provider_name()` | UI lists real built-in vendors, env var status, and verified voice presets without duplicating tables |
| `phonon-podcast` | `PipelineConfig.target_lufs: Option<f32>` and `PodcastPipeline::assemble_episode(&self, script, segment_buffers)` + `render_episode_from_buffers(...)` | Broadcast BS.1770-4 LUFS mastering in the pipeline and zero-network re-assembly from cached in-memory segment buffers |

## 3. Studio Core Architecture (`phonon-studio`)

The crate is split along the UI/logic boundary so all state transitions, project serialization, undo/redo, cache key invalidation, and export DSP baking are unit-tested headlessly without starting a window:

```
phonon-studio/src/
├── main.rs                 # Dioxus 0.7 shell, workspace switcher, drag-and-drop, global transport timer
├── project.rs              # Pure Rust StudioProject IR, ProjectHistory (Undo/Redo), SegmentAudioState, EpisodeSummary
├── studio_ops.rs           # Pure/async business operations: provider resolution, CachingTts preview/regen, episode assembly, export & plugin baking
├── state.rs                # Dioxus Signals wiring StudioProject + Audio Lab + shared AudioPlayer transport
└── components/
    ├── header.rs           # Top console bar: workspace tabs, project I/O, AI Draft trigger, Undo/Redo, provider badge
    ├── podcast_studio.rs   # 3-column Broadcast Desk (Left: Providers/Cast/Mastering, Center: Segment Editor, Right: Episode Inspector)
    ├── transport.rs        # Shared bottom transport: Play/Pause/Stop, interactive Seek bar, real-time Volume, live timecode
    ├── audio_lab.rs        # Audio Lab tab container (Waveform/Spectrum + Info + Effects/EQ/Plugins/Export drawer)
    ├── waveform.rs         # Waveform SVG + Spectrum toggle
    ├── spectrum.rs         # 64-bar FFT spectrum SVG
    ├── info_panel.rs       # Technical metrics including BS.1770-4 LUFS & True Peak
    ├── effects_panel.rs    # Normalize, LUFS target, Fade, Speed, Compressor
    ├── eq_panel.rs         # 5-band Parametric EQ
    ├── plugin_browser.rs   # Real-time DspPlugin chain with live parameter sync
    └── export_panel.rs     # Single-file Audio Lab export with optional plugin chain baking
```

### Phase 2 Extension Points Reserved in `StudioProject`
- `version: u32` (schema version `1`) for forward migrations.
- `SegmentMediaOverride` (`#[serde(default)]` optional map keyed by segment index or ID) allowing future Phase 2 recorded audio paths, non-destructive trim ranges, and enhancement flags without breaking v1 projects.
- `StudioProject` round-trips cleanly to/from plain `PodcastScript` JSON: opening a legacy CLI `episode.json` automatically imports it as a `StudioProject`.

## 4. Visual Design System (Broadcast Mastering Console)

- **Palette**:
  - Canvas `#141619` | Primary Surface `#1C1F24` | Elevated Control `#252A31` | Hairline Border `#2D333C`
  - Primary Action / Active Transport Accent: Broadcast Amber `#E8A238`
  - Audio / LUFS / Waveform Accent: Studio Teal `#38B2AC`
  - Clipping / Destructive Warning: Signal Red `#E05252`
  - Text Primary `#ECEFF4` | Text Secondary `#9AA4B2` | Text Muted `#647080`
- **Typography**: System humanist sans (`SF Pro Text` / `PingFang SC` / `Inter`) for dialogue scripts; tabular monospace (`font-mono tabular-nums`) exclusively for timecodes, LUFS, dBTP, and sample rates.
- **Restraint**: One clear visual hierarchy per column, zero decorative gradients, sentence-case active-voice labels, and direct actionable error messages.
