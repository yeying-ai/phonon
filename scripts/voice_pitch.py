#!/usr/bin/env python3
"""Check that each TTS preset's default host / guest voices differ in pitch.

`phonon podcast script` gives the host and guest the first two voices of the
TTS preset. Vendors don't document voice gender (GLM's docs list names only),
so this synthesizes one sentence per voice and compares median F0 (YIN).
Measured 2026-10-23: male presets 120-203 Hz, female 222-276 Hz.

Needs the vendors' API keys, ffmpeg and numpy. Exits 1 when a pair is closer
than --min-gap Hz.

Usage:
  scripts/voice_pitch.py                          # minimax qwen glm stepfun edge
  scripts/voice_pitch.py glm --voices tongtong,chuichui   # a known same-pitch pair: fails
"""

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

SR = 16000
TEXT = "大家好，欢迎收听本期节目，今天我们聊一聊怎么开始跑步。"
PHONON = ["cargo", "run", "-q", "-p", "phonon-cli", "--"]


def first_two_voices(provider: str, tmp: Path) -> list[str]:
  config = tmp / f"{provider}.toml"
  config.write_text(f'[tts]\nprovider = "{provider}"\n')
  out = subprocess.run(PHONON + ["providers", "--config", str(config)],
                       capture_output=True, text=True, check=True).stdout
  match = re.search(r"^\s*voices: (.+)$", out, re.MULTILINE)
  if not match:
    sys.exit(f"{provider}: no voice list in `phonon providers` output")
  return [v.strip() for v in match.group(1).split(",")][:2]


def synthesize(provider: str, voice: str, tmp: Path) -> np.ndarray:
  path = tmp / f"{provider}_{voice}.wav"
  subprocess.run(PHONON + ["tts", "--provider", provider, "--voice", voice,
                           "--text", TEXT, "-o", str(path)],
                 capture_output=True, check=True)
  raw = subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-ac", "1",
                        "-ar", str(SR), "-f", "f32le", "-"],
                       capture_output=True, check=True).stdout
  return np.frombuffer(raw, dtype=np.float32)


def median_f0(x: np.ndarray) -> float:
  """YIN: first dip of the cumulative mean normalized difference below 0.15.

  Unlike an autocorrelation peak this doesn't jump an octave on voices with a
  strong second harmonic or a missing fundamental (see --selftest).
  """
  frame, hop = 800, 160  # 50 ms windows, 10 ms hop
  lo, hi = SR // 450, SR // 60
  floor = (np.max(np.abs(x)) + 1e-9) * 0.05
  f0s = []
  for start in range(0, len(x) - frame - hi, hop):
    w = x[start:start + frame + hi]
    if np.sqrt(np.mean(w[:frame] ** 2)) < floor:
      continue
    diff = np.array([np.sum((w[:frame] - w[t:t + frame]) ** 2) for t in range(hi)])
    cmnd = np.ones(hi)
    cmnd[1:] = diff[1:] * np.arange(1, hi) / (np.cumsum(diff[1:]) + 1e-12)
    below = np.where(cmnd[lo:] < 0.15)[0]
    if len(below) == 0:
      continue
    lag = lo + below[0]
    while lag + 1 < hi and cmnd[lag + 1] < cmnd[lag]:
      lag += 1
    f0s.append(SR / lag)
  return float(np.median(f0s)) if f0s else 0.0


def selftest() -> None:
  t = np.arange(SR) / SR
  cases = [(110, [1, .5, .3]), (110, [.3, 1, .6, .4]), (110, [0, 1, .8, .6]), (230, [1, .4])]
  for f, weights in cases:
    x = np.zeros(SR)
    for k, a in enumerate(weights):
      x += a * np.sin(2 * np.pi * f * (k + 1) * t)
    got = median_f0(x.astype(np.float32))
    print(f"{f} Hz harmonics {weights} -> {got:.1f} Hz")
    if abs(got - f) > f * 0.03:
      sys.exit("selftest failed")


def main() -> None:
  parser = argparse.ArgumentParser()
  parser.add_argument("providers", nargs="*", default=["minimax", "qwen", "glm", "stepfun", "edge"])
  parser.add_argument("--voices", help="comma-separated pair instead of the preset's first two")
  parser.add_argument("--min-gap", type=float, default=40.0)
  parser.add_argument("--selftest", action="store_true")
  args = parser.parse_args()
  if args.selftest:
    selftest()
    return

  failed = False
  with tempfile.TemporaryDirectory() as d:
    tmp = Path(d)
    for provider in args.providers:
      voices = args.voices.split(",") if args.voices else first_two_voices(provider, tmp)
      f0 = [median_f0(synthesize(provider, v, tmp)) for v in voices]
      gap = abs(f0[0] - f0[1])
      ok = gap >= args.min_gap
      failed |= not ok
      pairs = ", ".join(f"{v} {f:.0f} Hz" for v, f in zip(voices, f0))
      print(f"{'ok  ' if ok else 'FAIL'} {provider:<8} {pairs}  gap {gap:.0f} Hz")
  sys.exit(1 if failed else 0)


if __name__ == "__main__":
  main()
