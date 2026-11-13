# Third-party source notices

## Edge TTS Sec-MS-GEC v2 protocol implementation

`src/auth.rs` contains a Rust adaptation of the token algorithm in
`src/edge_tts/drm.py` from **edge-tts 7.3.1**, maintained by **rany and the
edge-tts contributors**:

- Project: <https://github.com/rany2/edge-tts>
- Release used: <https://pypi.org/project/edge-tts/7.3.1/>
- Source file: <https://github.com/rany2/edge-tts/blob/master/src/edge_tts/drm.py>
- SHA-256 of the source file in that release:
  `b29c37e2a0c11284adb2a1ffc04f0f231e1ba2d921494a912797218b05d64c4a`

The upstream license states that files other than `srt_composer.py` are
licensed under **GNU LGPL version 3**. The original upstream license notice,
including its component attribution, is preserved in
[licenses/edge-tts-LICENSE.txt](licenses/edge-tts-LICENSE.txt).
The incorporated GNU GPL version 3 text is included in
[licenses/GPL-3.0.txt](licenses/GPL-3.0.txt).
No `srt_composer.py` code is included in this crate.

The adapted `src/auth.rs` is marked `SPDX-License-Identifier: LGPL-3.0-only`.
Existing files retain their original `MIT OR Apache-2.0` licensing.
The crate's package metadata therefore records
`(MIT OR Apache-2.0) AND LGPL-3.0-only`.

### Changes in this adaptation

- Replaces Python's unrolled SHA-256 rounds with compact Rust loops and
  explicit `u32` wrapping arithmetic.
- Retains the protocol's prefix state, schedule adjustments and CS64
  coefficients.
- Uses integer Unix-to-Windows time conversion and five-minute quantization.
- Omits upstream HTTP handling, clock-skew adjustment, cookies and Python
  caching. The existing Rust networking code supplies requests and time limits.
- Includes fixed-time vectors generated with
  `edge_tts.drm.generate_sec_ms_gec_v2(timestamp)` from release 7.3.1.

This source adaptation does not add a Python runtime or a Python package
dependency to the Rust crate.
