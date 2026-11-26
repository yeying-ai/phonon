# Phonon MCP Tool Reference

Complete reference for the 23 tools exposed by `phonon-mcp`. For
install + client configuration, see
[`phonon-mcp/README.md`](../phonon-mcp/README.md).

## Protocol overview

`phonon-mcp` speaks JSON-RPC 2.0 over stdio (newline-delimited).
Messages mirror the [MCP `2025-06-18`](https://modelcontextprotocol.io/)
spec.

Methods supported by the server:

| Method | Purpose |
|---|---|
| `initialize` | Handshake. Returns `protocolVersion`, `serverInfo`, and `capabilities.tools = {}`. |
| `notifications/initialized` | No-op acknowledgement (no response). |
| `ping` | Keep-alive — returns `{}`. |
| `tools/list` | Returns the 23 tools with their JSON Schemas. |
| `tools/call` | Invokes a tool. Params: `{ name, arguments }`. |

Anything else returns `error.code = -32601` (Method not found).

## AudioHandle

Audio buffers never travel through the JSON-RPC channel. Instead each
buffer is stored in an in-process `AssetRegistry` and referenced by an
`AudioHandle = UUID v4`. Tools that **produce** audio return:

```json
{ "handle": "9b7e…", "duration_secs": 3.21, "sample_rate": 48000, "channels": 1 }
```

Tools that **consume** audio take `"handle": "9b7e…"` (and possibly
`"a"` / `"b"` for binary ops). When you're done with a buffer call
`audio_drop` to free the memory.

Every producing tool creates a **new** handle, so the registry caps the PCM
it holds (1 GiB of `f32` samples by default, `AssetRegistry::with_max_bytes`
for embedders). Past the cap, producing tools fail with
`Asset registry full: … free memory with audio_drop` until handles are dropped.

## Errors

Tool handlers return one of:

| Variant | When |
|---|---|
| `InvalidArgs(msg)` | Bad JSON shape, out-of-range values, unknown enum variant. |
| `HandleNotFound(uuid)` | The requested handle isn't in the registry (or already dropped). |
| `Audio(msg)` | A `phonon-core` / `phonon-io` / `phonon-ai` operation failed. |
| `PathDenied(msg)` | Path escapes the sandbox root, or `audio_save` target exists without `overwrite`. |
| `Io(msg)` | File system I/O failure (e.g. `audio_load` on a missing path). |
| `ToolNotFound(name)` | `tools/call` with an unknown tool name. |
| `Serde(msg)` | JSON deserialization failure inside a handler. |

Tool execution errors are surfaced in the MCP `tools/call` result payload with
`result.isError = true` and a human-readable message in `result.content[0].text`.
Protocol-level errors (malformed JSON, unknown method, missing `tools/call`
`name`) return standard JSON-RPC error codes (`-32700`, `-32600`, `-32601`,
`-32602`).

---

## Basic — asset registry

> **Sandbox.** `audio_load` / `audio_save` only touch files under the
> sandbox root: `$PHONON_MCP_ROOT`, or the directory `phonon-mcp` was
> launched from. Relative paths resolve against the root; absolute paths,
> `..` and symlinks that land outside it are rejected with `PathDenied`.

### `audio_load`

Load an audio file (WAV / MP3 / FLAC / OGG / AAC) and return a handle.
Format is inferred from the extension via Symphonia.

| Field | Type | Required | Notes |
|---|---|---|---|
| `path` | string | yes | Root-relative path (absolute paths must lie inside the root). |

Request:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call",
 "params":{"name":"audio_load","arguments":{"path":"./song.wav"}}}
```

Response:

```json
{"handle":"9b7e3a5e-…","duration_secs":12.3,"sample_rate":48000,"channels":2}
```

### `audio_save`

Save a handle to a file. Output format is inferred from the
destination extension (`.wav` / `.mp3` / `.flac` / `.ogg` / `.aac`).

| Field | Type | Required | Notes |
|---|---|---|---|
| `handle` | UUID | yes | |
| `path` | string | yes | Root-relative; parent directory must exist. |
| `overwrite` | bool | no | Default `false`: an existing file is not replaced. |

Returns `{ "path": "<resolved absolute path>" }`.

### `audio_info`

Inspect a handle's metadata + cheap measurements.

Input: `{ "handle": "<uuid>" }`.
Output:

```json
{
  "handle": "9b7e…",
  "duration_secs": 12.3, "sample_rate": 48000, "channels": 2, "samples": 1180800,
  "peak": 0.94, "rms": 0.21, "dbfs": -13.6
}
```

### `audio_list`

Returns every handle currently in memory.

Input: `{}`.
Output: `{ "handles": ["uuid-1", "uuid-2", …], "count": 2 }`.

### `audio_drop`

Free a handle.

Input: `{ "handle": "<uuid>" }`.
Output: `{ "removed": true }` (false if the handle wasn't present).

---

## DSP — produces a new handle

Every DSP tool is non-destructive: the input handle is left intact
and a fresh handle for the result is returned.

### `audio_mix`

Sum two handles sample-for-sample, clamped to `[-1, 1]`. Both inputs
must share `sample_rate` and `channels`.

| Field | Type | Required |
|---|---|---|
| `a` | UUID | yes |
| `b` | UUID | yes |

Returns a `HandleResult`.

### `audio_trim`

Extract `[start_secs, end_secs]` from a buffer.

| Field | Type | Required |
|---|---|---|
| `handle` | UUID | yes |
| `start_secs` | number | yes |
| `end_secs` | number | yes |

### `audio_fade`

Apply a linear fade-in or fade-out.

| Field | Type | Required | Notes |
|---|---|---|---|
| `handle` | UUID | yes | |
| `kind` | string | yes | `"in"` or `"out"`. |
| `duration_secs` | number | yes | Length of the fade region. |

### `audio_normalize`

Peak-normalize so the loudest sample sits at `target_peak`.

| Field | Type | Required | Default |
|---|---|---|---|
| `handle` | UUID | yes | |
| `target_peak` | number 0–1 | no | `0.95` |

### `audio_normalize_lufs`

Loudness-normalize per ITU-R BS.1770-4 / EBU R128 with a true-peak
ceiling. Use `target_lufs = -16, true_peak_max_dbtp = -1` for the
podcast publishing standard.

| Field | Type | Required | Default |
|---|---|---|---|
| `handle` | UUID | yes | |
| `target_lufs` | number | yes | e.g. `-16` |
| `true_peak_max_dbtp` | number | no | `-1.0` |

### `audio_volume_db`

Apply a gain in decibels (positive = louder, negative = quieter).
Output is clamped to `[-1, 1]`.

| Field | Type | Required |
|---|---|---|
| `handle` | UUID | yes |
| `db` | number | yes |

### `audio_pan`

Constant-power stereo panning. Mono input is upmixed to stereo;
stereo input is collapsed to its mid signal then re-distributed.

| Field | Type | Required | Range |
|---|---|---|---|
| `handle` | UUID | yes | |
| `position` | number | yes | `-1.0` (full L) … `+1.0` (full R) |

### `audio_resample`

Lanczos-windowed sinc resample to `target_rate` Hz.

| Field | Type | Required |
|---|---|---|
| `handle` | UUID | yes |
| `target_rate` | integer | yes |

### `audio_enhance_voice`

Pure-Rust local broadcast voice enhancement (80 Hz high-pass, adaptive STFT spectral denoise, broadcast vocal presence EQ, compression, and BS.1770-4 LUFS mastering).

| Field | Type | Required | Default |
|---|---|---|---|
| `handle` | UUID | yes | |
| `preset` | `"podcast_studio"` \| `"noisy_room"` \| `"clean_mic"` | no | `"podcast_studio"` |
| `denoise_strength` | number 0–1 | no | preset default |
| `target_lufs` | number `-60..-5` | no | `-16.0` |

### `audio_splice`

Non-destructively keep only the specified `[start_secs, end_secs]` time ranges and join them with raised-cosine micro-crossfades.

| Field | Type | Required | Default |
|---|---|---|---|
| `handle` | UUID | yes | |
| `keep_ranges` | array of `[start, end]` pairs | yes | |
| `crossfade_secs` | number `>= 0` | no | `0.015` |

### `audio_eq`

Apply a multi-band parametric equalizer preset (`"voice"`, `"bass_heavy"`, `"bright"`, `"flat"`) and/or up to 32 custom biquad bands.

| Field | Type | Required | Default |
|---|---|---|---|
| `handle` | UUID | yes | |
| `preset` | `"voice"` \| `"bass_heavy"` \| `"bright"` \| `"flat"` | no | `"flat"` |
| `bands` | array of `{ filter_type, freq_hz, q?, gain_db? }` | no | `[]` |

### `audio_compress`

Apply channel-linked dynamic range compression using a preset (`"voice"`, `"gentle"`, `"limiter"`, `"default"`) with optional parameter overrides.

| Field | Type | Required | Default |
|---|---|---|---|
| `handle` | UUID | yes | |
| `preset` | `"voice"` \| `"gentle"` \| `"limiter"` \| `"default"` | no | `"voice"` |
| `threshold_db` | number `-60..0` | no | preset default |
| `ratio` | number `1..50` | no | preset default |
| `attack_secs` | number `0.0001..1.0` | no | preset default |
| `release_secs` | number `0.005..5.0` | no | preset default |
| `makeup_gain_db` | number `-24..36` | no | preset default |

---

## Studio Co-Editing — atomic `.phonon.json` tools (confined to `PHONON_MCP_ROOT`)

All `studio_*` tools validate paths against `PHONON_MCP_ROOT` and persist via atomic same-directory temp file + `rename` so a running **Phonon Studio** instance with MCP Live Sync enabled hot-reloads changes automatically (with Undo history).

| Tool | Purpose | Key Arguments |
|---|---|---|
| `studio_get_project` | Inspect title, speakers, numbered segments, overrides & mastering config | `path` |
| `studio_edit_segment` | Edit segment text/speaker/chapter/language, toggle `enhance_voice`, or cut filler spans | `path`, `index`, `text?`, `speaker?`, `chapter?`, `language?`, `enhance_voice?`, `delete_filler_spans?` |
| `studio_add_segment` | Insert a new dialogue segment | `path`, `speaker`, `text`, `after_index?`, `chapter?`, `language?` |
| `studio_delete_segment` | Delete a segment (keeps at least 1) | `path`, `index` |
| `studio_set_mastering` | Adjust `target_lufs`, `bgm_volume`, pause, crossfade, fade in/out | `path`, `target_lufs?`, `bgm_volume?`, `pause_between_segments_secs?`, `crossfade_secs?`, `fade_in_secs?`, `fade_out_secs?` |

---

## Measure — read-only

### `audio_loudness`

Run the full loudness measurement bundle (ITU-R BS.1770-4 / EBU R128
plus dBFS / RMS / peak).

Input: `{ "handle": "<uuid>" }`.
Output:

```json
{
  "integrated_lufs":     -16.3,
  "momentary_max_lufs":  -10.1,
  "short_term_max_lufs": -12.4,
  "range_lu":            5.1,
  "true_peak_dbtp":      -1.0,
  "peak":                0.89,
  "rms":                 0.22,
  "dbfs":                -13.0
}
```

Any field that would otherwise be `-inf` (silent buffer, etc.) is
serialized as JSON `null`.

---

## End-to-end example

A typical chain — load → loudness-normalize → save — looks like
this on the wire:

```jsonl
{"jsonrpc":"2.0","id":1,"method":"initialize"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"audio_load","arguments":{"path":"voice.wav"}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"audio_normalize_lufs","arguments":{"handle":"<id-from-2>","target_lufs":-16,"true_peak_max_dbtp":-1}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"audio_save","arguments":{"handle":"<id-from-3>","path":"voice_loud.mp3"}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"audio_drop","arguments":{"handle":"<id-from-2>"}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"audio_drop","arguments":{"handle":"<id-from-3>"}}}
```

## Extending the registry

`ToolRegistry::register(tool: Tool)` adds a custom tool at runtime.
Use this from a downstream binary that wraps `phonon-mcp` to add
project-specific tools (e.g. a `transcribe` tool that calls
`phonon_ai::WhisperStt`):

```rust
use phonon_mcp::{Server, tools::{Tool, ToolRegistry}};
use serde_json::json;
use std::sync::Arc;

let mut tools = ToolRegistry::new("/path/to/sandbox/root");
tools.register(Tool {
  name: "my_tool".into(),
  description: "…".into(),
  input_schema: json!({"type":"object","properties":{}}),
  handler: Arc::new(|_args| Box::pin(async move { Ok(json!({"ok": true})) })),
});

Server::with_tools(tools).run_stdio().await?;
```

This keeps the published `phonon-mcp` binary tight while letting
each downstream project layer in its own AI / pipeline tools.
