# Paleo

Paleo is Drawen's media layer. It is not a codec library and not a container
parser: it assembles focused crates behind one streaming API, so Drawen can pick
the best implementation for each job instead of depending on a single
monolithic framework.

The first implementation is built on [Symphonia](https://docs.rs/symphonia):

```text
file ──► Symphonia demuxer ──┬──► Symphonia audio decoder ──► AudioBuffer (f32, interleaved)
                             │                                        │
                             │                          paleo-audio ──► PcmConverter ──► device
                             │
                             └──► VideoPacket (compressed) ──► paleo-video ──► codec backend
                                                                                │
                                   paleo-h264 / paleo-hevc / paleo-vp9 / paleo-av1
                                                                                │
                                                                        YUV ──► RGBA VideoFrame
```

Paleo lives in `crates/paleo`. The codec backends are `paleo-h264`,
`paleo-hevc`, `paleo-vp9`, and `paleo-av1`; `paleo-video` selects one for a
track, `paleo-player` combines demuxing, decoding, audio output, and timing, and
the optional audio runtime lives in `crates/paleo-audio`. The `MediaSession`
contract, the `PaleoSession` and `AudioPaleoSession` adapters, and the widgets
that present frames live in `drawen-widgets`; see [MEDIA.md](MEDIA.md).

## Why not one framework

The media layer needs four independent things:

1. container demuxing that streams and seeks;
2. audio decoding for many codecs;
3. video decoding for H.264, HEVC, AV1, and VP8/VP9;
4. encoding when Drawen needs to export media.

No single pure-Rust crate is currently best at all four. Symphonia is the de
facto standard for demuxing and audio decoding: it is widely used, permissively
licensed (`MPL-2.0`), `#![forbid(unsafe_code)]`, and reads MP4, Matroska/WebM,
Ogg, WAV, AIFF, CAF, FLAC, and MPEG audio without loading the file into memory.
Its video support is experimental and has no production decoders, so Paleo takes
video packets from the demuxer and leaves decoding to dedicated crates.

This was also why Rumpeg was rejected as the foundation. It is a complete
FFmpeg-shaped framework, but its MP4 demuxer reads the whole file, drops AAC
tracks written by FFmpeg, and seeks to non-keyframe samples; its scale layer
cannot convert YUV 4:2:0 to RGBA directly, and it has no audio output. Fixing
those defects would mean owning and maintaining a fork of a one-month-old
project. Paleo instead composes crates that already solve those problems.

## Memory model

Paleo deliberately keeps no internal queues and never reads an entire file:

| Allocation | Bound |
| --- | --- |
| Source buffer | fixed 64 KiB (`MediaSourceStream`) |
| Container indexes | owned by the demuxer, bounded by the container |
| Decoder state | one audio decoder and one video decoder per player |
| Returned data | one `AudioBuffer` or `VideoPacket` per `next_event` |
| Video demux | none: packets are handed to the caller |
| `paleo-h264` reorder | at most `reorder_depth` YUV pictures (default 4) |
| `paleo-hevc` reorder | pictures the decoder holds for presentation order |
| `paleo-player` video queue | at most 3 decoded RGBA pictures |
| `paleo-player` audio queue | at most 300 ms, further bounded by the device ring |
| `paleo-audio` ring | fixed at open: 250 ms by default, 20 ms–2 s requested, 1024-frame floor |

`Media::next_event` returns owned values, so a frame or packet is freed as soon
as the session has consumed it. The demuxer has no decode-ahead buffer by
design; a `MediaSession` implementation decides how many frames to keep for
scheduling. `paleo-h264` keeps only the decoded pictures that cannot yet be
emitted in display order, and recycles each YUV frame into the next decode.
`paleo-hevc` converts the decoder's `u16` planes without an intermediate copy.
`paleo-player` keeps at most three RGBA pictures and a device-sized audio queue,
and stops demuxing when both are full. `paleo-audio` owns one fixed-size ring
between the player and the device callback; a full ring drops the overflow
instead of blocking, so audio buffering cannot grow with a slow consumer.

Measured with the `scan` example (release build, Windows, x86-64). Peak working
set is sampled while the process runs:

| Input | Size | Work | Peak working set | Throughput |
| --- | --- | --- | --- | --- |
| PCM WAV, 48 kHz stereo, 600 s | 110 MB | decode to `f32` | 4.8 MB | 6201x realtime |
| AAC in MP4, 48 kHz stereo, 120 s | 1.9 MB | decode to `f32` | 4.4 MB | 2024x realtime |
| H.264 720p30 + AAC in MP4, 30 s | 10.7 MB | demux video, decode audio | 4.4 MB | 848x realtime |

Throughput covers demuxing and audio decoding only; the 720p row moves 900
H.264 packets (10.4 MB) without buffering them.

Reproduce with:

```text
cargo run -p paleo --release --example scan -- big.wav
```

## Supported media

Containers: ISO/MP4, Matroska/WebM, Ogg, Wave, AIFF, CAF, FLAC, and MPEG audio.

Audio codecs: AAC-LC, ALAC, FLAC, MP1/MP2/MP3, Vorbis, Opus (through
`symphonia-opus`), PCM, and ADPCM.

Not yet decodable: HE-AAC and HE-AACv2 (Symphonia marks them unfinished). A file
whose selected audio track has no decoder fails to open with
`Error::UnsupportedCodec`; the file can still be inspected by a video-only
session that tolerates missing audio.

Video: Paleo demuxes H.264, HEVC, AV1, VP8, VP9, MPEG-4 Part 2, and Motion JPEG
packets. H.264, HEVC, VP9, and AV1 are decodable through the pure-Rust backends
below; VP8 and MPEG-4 Part 2 still need one. Track metadata exposes the
container's codec configuration records (`VideoTrackInfo::extra_data`),
including the H.264 `avcC` record that carries SPS/PPS and the NAL length size,
and the HEVC/AV1 records the corresponding backends parse.

## API

```rust
use std::time::Duration;
use paleo::{Event, Media};

let mut media = Media::open("clip.mp4")?;
if let Some(video) = media.info().video() {
    println!("{} {}x{}", video.codec(), video.width(), video.height());
}

loop {
    match media.next_event()? {
        Event::Audio(audio) => { /* feed the audio device */ }
        Event::Video(packet) => { /* feed a video decoder */ }
        Event::EndOfStream => break,
    }
}

media.seek(Duration::from_secs(10))?;
```

`Media::open_with(path, MediaSelection::VideoOnly)` selects only the video
track, which is useful for a video session that cannot decode or output audio.
`Media::open` selects the default audio and video tracks.

`seek` is valid after end-of-stream. Symphonia 0.6's ISO/MP4 reader cannot
resume once its atom iterator has reached EOF, so `Media` reopens the source in
that case; the cost is one probe and no re-decoding.

`seek_keyframe(position, lookback, is_keyframe)` is the video-aware form. A
container seek lands on the packet that *contains* the requested time, which is
usually an inter picture, and a decoder that resets there can only resume at the
next random-access point. `seek_keyframe` instead scans a bounded window before
`position`, keeps the last packet that `is_keyframe` accepts, and seeks exactly
there. Audio packets in the window are skipped without decoding. The caller
supplies the predicate because Symphonia 0.6 does not expose packet keyframe
flags; every backend implements `VideoDecoder::is_keyframe_data` (H.264 IDR
slices, HEVC IRAP pictures, VP9 keyframes, AV1 keyframe OBUs).

`AudioBuffer` contains interleaved `f32` samples normalized to `-1.0..=1.0`,
with the presentation timestamp, sample rate, and channel count of the block.
`VideoPacket` contains the compressed payload and both timestamps; it carries no
keyframe flag because Symphonia 0.6 does not expose packet flags, so a video
decoder must recognize its own random-access points from the bitstream.
Symphonia 0.6's ISO/MP4 reader does not implement `ctts`, so it reports
`pts == dts` in decode order. `paleo-h264` therefore orders B-pictures by their
decoded `PicOrderCnt` and assigns presentation timestamps from the packet
timestamps rather than trusting `pts` directly.

Only the default audio and video tracks are selected, matching the container's
default-track flag and falling back to the first track with a known codec.

## Player

`crates/paleo-player` is the high-level API. It owns the container reader, the
decoder chosen by `paleo-video`, the audio device, the buffers, and the clock, so
an application does not wire them together:

```rust
use std::time::Duration;
use paleo_player::{Player, PlayerState};

let mut player = Player::open("clip.mp4")?;
player.play()?;

let mut now = Duration::ZERO;
loop {
    let update = player.update(now)?;
    if update.state() == PlayerState::Ended {
        break;
    }
    if update.frame_changed() {
        let rgba: &[u8] = player.frame().expect("a frame was produced").pixels();
        let _ = rgba;
    }
    now += Duration::from_millis(16);
}
```

`Player` exposes:

| Item | Meaning |
| --- | --- |
| `PlayerOptions` | audio/video enable, autoplay, looping, rate, volume, mute, device, latency |
| `PlayerInfo` | container tracks, `has_video`/`has_audio`, and `video_error`/`audio_error` |
| `play`, `pause`, `seek` | transport; `seek` finds the keyframe at or before the target |
| `set_rate`, `set_volume`, `set_muted`, `set_looping`, `set_video_enabled` | runtime controls |
| `update(now)` | one bounded demux/decode step; returns `PlayerUpdate` |
| `frame()` | borrows the current RGBA picture, so a view draws without copying |

`PlayerUpdate` reports `position`, `state`, `state_changed`, `frame_changed`, and
a recoverable `error` (for example audio frames dropped by a full device ring).
`state_changed` and `frame_changed` compare against the previous `update` call,
so a transition made by `play`/`pause`/`seek` is still reported once.

Two policy choices keep a codec gap from being fatal:

- a track whose codec has no backend is skipped, and the reason is stored in
  `PlayerInfo::video_error` or `PlayerInfo::audio_error`; opening fails only when
  neither track can be played;
- with `PlayerOptions::with_audio(false)` the file opens video-only, so an
  unsupported audio codec does not block a silent view.

The device is the master clock at rate `1.0`; without audio, or at another rate,
the `now` values passed to `update` drive the playhead. Audio is not queued
outside `1.0`, and returning to `1.0` seeks to the current position so audio and
video restart in step. `Player` never spawns a thread and never calls a platform
API on its own: `update` does a bounded amount of work (96 packets, three queued
pictures, 300 ms of audio) on the caller's thread.

## Video backends

The packet API exists so video decoding can be added without changing the
container layer. `paleo-video` maps a track's codec to the backend this build
contains; every backend is an optional feature, and the default build enables
all four:

| Codec | Crate | Decoder | Notes |
| --- | --- | --- | --- |
| H.264 | `paleo-h264` | `rusty_h264-decoder` (`=0.17.2`) | `avcC` bridge, `PicOrderCnt` reorder, 4:2:0 |
| HEVC | `paleo-hevc` | `rusty_h265` (`=0.7.1`) | `hvcC` or Annex-B, 8/10-bit 4:2:0/4:2:2 |
| VP9 | `paleo-vp9` | `rusty_vp9` 0.2.1 | WebM/Matroska, 8/10/12-bit 4:2:0/4:2:2/4:4:4 |
| AV1 | `paleo-av1` | `rusty_av1d` (`=1.3.0`, no asm) | keyframe OBU scan, without assembly kernels |

Backends are separate crates that convert `VideoPacket` plus `VideoTrackInfo`
into a shared `paleo::VideoFrame` (tightly packed RGBA). Paleo's `yuv` module
does the conversion for the pure-Rust backends, so each one only has to name its
pixel layout and matrix. A build that needs one codec can drop the rest:

```toml
paleo-video = { version = "0.2", default-features = false, features = ["h264"] }
```

`paleo-video::codecs` and `supports` report what the build can decode, and an
unsupported codec produces `Error::UnsupportedCodec` naming both the codec and
the backends that are present.

`paleo-h264` is the most complete backend. It:

- parses the `avcC` record for SPS/PPS and the NAL length size, and converts
  length-prefixed packets to Annex-B access units, prepending parameter sets at
  every IDR. A packet is accepted as Annex-B only when it does not form a
  consistent sequence of length-prefixed NAL units, so a four-byte length such
  as `00 00 01 b0` is not mistaken for a three-byte start code;
- decodes with one streaming `rusty_h264_decoder::Decoder`, with no packet
  queue and no whole-GOP buffering;
- reorders B-pictures by `PicOrderCnt` with a bounded window
  (`DEFAULT_REORDER_DEPTH`, 4 pictures; adjustable with `set_reorder_depth`);
- converts YUV 4:2:0 to tightly packed RGBA with fixed-point limited-range
  BT.601 for SD and BT.709 for 720 lines and above (`ColorMatrix::Auto`);
- drops inter pictures after a seek until the next IDR, so resuming is
  repredictable rather than corrupt.

`rusty_h264-decoder` is pinned exactly (`=0.17.2`) because the packet bridge is
written against its streaming API; `rusty_h265` and `rusty_av1d` are pinned as
well, for the same reason.

The HEVC, VP9, and AV1 backends share the same shape: they wait for a
random-access point after `reset`, recreate the underlying decoder lazily when
the decoder reports it can no longer continue, convert native planes through
`paleo::rgba_from_yuv`/`rgba_from_yuv16`, and recognize their own keyframes in
`is_keyframe_data`, which is what `paleo::Media::seek_keyframe` scans for.

Colour handling differs by what the decoder exposes. AV1 reads the matrix and
range from the picture's own metadata; HEVC starts with the height-based matrix
and limited range and lets a caller override the matrix with
`set_color_matrix`; `rusty_vp9` reports neither, so the VP9 backend assumes
limited range and the height-based matrix.

## Audio output

`crates/paleo-audio` is the platform-dependent half of playback. It keeps the
same separation as the video side: Paleo decodes, `paleo-audio` adapts and
outputs, and a session decides policy.

| Type | Role |
| --- | --- |
| `PcmFormat`, `PcmBuffer` | sample rate, channel count, interleaved `f32` frames, timestamps |
| `ChannelMap` | mono/stereo/surround mapping, including a `-3 dB` centre and surround downmix |
| `LinearResampler` | streaming sample-rate conversion with one frame of history |
| `PcmConverter` | channel map, then resample, then output timestamps; `reset_at` trims pre-seek audio |
| `AudioOutput` | cpal device, bounded SPSC queue, volume/mute, pause, and a playable position |

The output device is always opened with the host's default output
configuration; media is adapted to it rather than the reverse. `AudioOutput`
owns a fixed ring buffer (250 ms by default, clamped to 20 ms–2 s with a
1024-frame floor). The application pushes converted frames; the real-time
callback owns the consumer, pops them, and converts to the device's sample
format. The render path takes no locks and allocates nothing. A full ring drops
the remainder instead of blocking the decoding thread, and `QueueReport` reports
both counts so a session can throttle.

`AudioOutput::position` is `base + rendered_frames / sample_rate`. It counts
frames the device actually consumed, including silence written after an
underrun, which is why it can drive the media clock while decorated mode is
paused or occluded. `reset(position)` publishes an atomic flush generation and
sample marker instead of locking the callback; the callback discards audio
queued before the marker on its next invocation, and `buffered()` already
excludes it. Call `reset(position)` and `PcmConverter::reset_at(position)`
together after a seek so the converter drops decoded audio that precedes the
target keyframe.

The `device` feature is enabled by default and pulls in `cpal` and `ringbuf`.
`paleo-audio` with `default-features = false` keeps the PCM utilities and links
no native audio API, so a metadata, preview, or server build stays headless.
`paleo-player` wraps these types for audio-mastered playback, and
`drawen-widgets` exposes that through its own `audio` feature; see
[MEDIA.md](MEDIA.md).

## Testing

- `cargo test -p paleo` runs PCM tests that build a WAV fixture in memory and
  verify decode counts, timestamps, normalization, and seek behavior.
- `cargo test -p paleo-h264` runs unit tests for the `avcC` parser, NAL framing,
  and RGBA conversion.
- `cargo test -p paleo-hevc -p paleo-av1` runs unit tests for the `hvcC` and AV1
  OBU parsers, including keyframe detection.
- `cargo test -p paleo-video` checks that the codec list matches the enabled
  features and that an unsupported codec names the build.
- `cargo test -p paleo-player` checks the timeline: rate clamping, advance
  scaling, position clamping, and change reporting. Its doctest compiles the
  whole `Player` loop.
- `cargo test -p paleo-audio` runs PCM, channel-map, resampler, and converter
  tests without touching a device. Setting `DRAWEN_AUDIO_DEVICE_TEST` adds a
  short default-output smoke test that starts a stream and checks that the
  position advances.
- `cargo test -p drawen-widgets --features media,audio` covers
  `PaleoSession` and `AudioPaleoSession`. Setting `DRAWEN_AUDIO_DEVICE_TEST` as
  well as `DRAWEN_FFMPEG` adds an end-to-end test that encodes a one-second
  clip, plays it through the device, checks that the audio clock drives
  playback, that a hidden window keeps audio running while video stops, and that
  seeking resumes video from the audio clock.
- `cargo test -p drawen-director` covers lifecycle transitions, every visibility
  policy, last-window close behavior, and applying actions to a `Video`.
- Setting `DRAWEN_FFMPEG` additionally runs `paleo/tests/ffmpeg_formats.rs` and
  `paleo-h264/tests/decode.rs`. They generate AAC, MP3, FLAC, Vorbis, MP4/H.264,
  and Matroska fixtures with a real FFmpeg, then check track selection, audio
  decoding, packet timestamps, A/V seek, and H.264 frame-for-frame agreement with
  FFmpeg's decode, including a strict B-pyramid and a 160×90 clip whose NAL
  prefixes resemble start codes. Opus is decoded through `symphonia-opus` but is
  not part of that fixture set yet.
  The H.264 comparison allows a mean per-channel difference below 1 after
  matching FFmpeg's nearest-neighbour chroma upsampling.

The optional tests follow the existing `drawen-widgets` FFmpeg smoke test: no
system FFmpeg is required for the normal suite.

## Limits

Not implemented:

- VP8, MPEG-4 Part 2, and Motion JPEG video decoding;
- HE-AAC and HE-AACv2 audio decoding;
- multiple audio or video tracks, subtitles, chapters, and attachments;
- network sources and adaptive buffering;
- variable-speed audio playback (resampling at a scaled ratio);
- encoding.

Known limits of the H.264 backend: `rusty_h264-decoder` decodes 4:2:0 frame
pictures only (no field/MBAFF or 4:2:2/4:4:4), and the reorder window uses a
fixed depth rather than reading `max_num_reorder_frames` from the SPS VUI.
Streams with a deeper reorder pattern can raise the depth explicitly.

Known limits of the other backends: HEVC and VP9 cover the profiles their
pure-Rust decoders implement, and AV1 is built without `rusty_av1d`'s assembly
kernels, so high-bitrate 4K playback is CPU-bound. Every backend decodes in
software on the calling thread; there is no hardware decode or zero-copy texture
path yet.

Known limits of the audio runtime: decoded audio is not resampled for playback
rates other than `1.0` (the player pauses audio and follows the wall clock
instead), decoding, conversion, and queueing happen on the calling thread, and
device loss is reported as an error instead of switching outputs automatically.
