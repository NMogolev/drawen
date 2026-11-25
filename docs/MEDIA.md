# Media

Media support in Drawen is split into three layers: a timing and scheduling
contract, a decode/playback session behind that contract, and widgets that only
present frames and forward user intent. This document defines the contract and
the current backends: the out-of-process `FFmpeg` command-line backend, the
in-process Paleo backend (H.264, HEVC, VP9, and AV1 in pure Rust), and the
audio-mastered Paleo backend with real audio output. It is the reference to read
before adding a streaming source or a new decoder.

## Layers

```text
MediaClock          presentation time, rate, state
    │
MediaSession        decode, transport, buffering, failure
    │
Video / AudioPlayer views and controls
```

A view never owns the decoder. `Video` and `AudioPlayer` hold an optional
`Box<dyn MediaSession>` plus a `MediaClock` that is used when the application
supplies frames manually. All position, duration, rate, and playing-state queries
resolve through the session when one is attached.

## Clock

`MediaClock` is the single playback timeline. It stores a position, an optional
duration, a rate, and a `MediaState`:

| State | Meaning |
| --- | --- |
| `Idle` | Nothing has been opened. |
| `Buffering` | The session is filling its buffer. |
| `Playing` | Position advances. |
| `Paused` | Position is held. |
| `Ended` | Position reached the duration. |
| `Failed` | A fatal session error stopped playback. |

The clock is a value type. The view or session advances it in one of two ways:

- **Decoder-only sessions** call `MediaClock::advance(delta)` (or
  `advance_looping`) with the time since the previous update.
- **Sessions with audio output** call `MediaClock::synchronize(position)` with
  the audio device position. Audio is the master clock because resampling and
  device latency make the output position authoritative; the wall clock is not.

`advance` applies `rate`, clamps at the duration, and moves to `Ended` unless the
clock is looping. `advance_looping` preserves the overshoot modulo the duration.
`seek` clamps to the known duration and leaves an idle, ended, or failed clock
paused at the new position. `set_rate` clamps to `0.05..=16.0` and rejects
non-finite values. `finish` and `fail` record terminal states without inventing
a position.

Rates above `1.0` change presentation speed only. A backend that cannot decode
faster than real time must either drop frames or report `Buffering`; it must not
advance the clock ahead of the frames it can deliver.

## Frame scheduling

`MediaFrame` couples an RGBA `RasterImage` with a presentation timestamp and the
duration for which the frame stays visible. `MediaFrameScheduler` owns a small
bounded queue (four frames) and decides what to paint:

- `push` rejects frames that already expired or arrive out of order.
- `present(position)` discards expired frames and selects the newest frame whose
  timestamp has been reached. Frames that are early are never shown early,
  except as an initial preview when nothing has been presented yet.
- `seek(position)` drops queued frames but keeps the visible image, so a seek
  does not flash an empty surface while the decoder restarts.
- `replace(frame)` installs an application-supplied frame immediately.
- `current()` is what a view paints.

The scheduler is deliberately not a decoder buffer. It holds no encoded data,
performs no I/O, and does not decide when to block. Backends own their own
buffering and deliver decoded frames through `MediaUpdate`.

## Sessions

`MediaSession` is the transport and decode contract:

```rust
trait MediaSession {
    fn metadata(&self) -> Option<&MediaMetadata>;
    fn clock(&self) -> &MediaClock;
    fn clock_mut(&mut self) -> &mut MediaClock;
    fn play(&mut self) -> Result<(), MediaError>;
    fn pause(&mut self);
    fn seek(&mut self, position: Duration) -> Result<(), MediaError>;
    fn set_rate(&mut self, rate: f64);
    fn update(&mut self, delta: Duration) -> MediaUpdate;
    fn is_active(&self) -> bool;
}
```

`update` is called once per animation tick. It advances or synchronizes the
clock, returns at most the newest frame, and may report an error or a state
change. A session must be cheap when nothing changed: a paused session returns an
empty update instead of waking a decoder.

Two methods are defaulted for sessions whose video can be suspended without
stopping the media:

- `supports_video_pause()` reports whether the session has an independent audio
  path, so audio can continue while video decoding stops.
- `set_video_enabled(bool)` stops or resumes video decoding and returns whether
  the state changed. The audio clock remains authoritative, so resuming drops
  pictures behind the clock and waits for the next random-access point instead of
  replaying the hidden interval.

`Video` forwards both methods and adds `release_session()`, which drops the
session while keeping the last painted frame. Applications use them through a
policy layer such as `drawen-director`; a widget never decides on its own what a
hidden window means.

`MediaMetadata` reports the display width and height after rotation, plus the
container duration (zero when unknown). `MediaError` is a displayable fatal
error. Recoverable conditions belong in `MediaState::Buffering`, not in an error.

A session may restart its decoder internally on seek. That is an implementation
detail; the contract only requires that the clock and subsequent frames agree on
the requested position.

## Widget rules

- `Video` paints `MediaFrameScheduler::current()` and forwards play, pause, seek,
  rate, and loop requests to the session. `VideoPlayer` is an alias that names the
  widget's player role.
- `Video::set_frame` is the manual path for applications that already own a
  decoder. It installs the frame through the scheduler; the widget advances its
  own `MediaClock` so progress and time labels stay live.
- `AudioPlayer` drives the same `MediaSession`/`MediaClock` contract without a
  picture surface. It paints cover art, metadata, a seek bar, and transport
  controls, and reports track changes through signals instead of guessing a
  playlist.
- `AnimatedImage` and `Image` are not sessions. A GIF is a small, fully decoded
  timeline, not a media pipeline, and it keeps its own frame index. The frame
  clock is purely visual: the runtime freezes it while the widget is scrolled
  offscreen, clipped away, inside a collapsed subtree, or under a modal scrim,
  and resumes from the same frame when it becomes visible again.
- Neither player owns a timer, spawns a thread, or calls a platform API. They
  paint, tick through `Widget::advance_animation`, and emit typed signals. Audio
  output still lives in `paleo-audio` and `AudioPaleoSession`.

## Player configuration

`Video` and `AudioPlayer` share configuration types from
`drawen_widgets::player` (re-exported by `drawen`):

- `PlayerTheme` sets every color, radius, padding, spacing, button size, track
  height, and font size the players paint. It falls back to the active theme for
  anything the application does not override.
- `PlayerBehavior` sets looping, initial volume and mute state, playback-rate
  choices, seek and volume steps, keyboard and wheel handling, and the control
  auto-hide delay. A `None` delay keeps controls visible; video defaults to
  auto-hide, audio defaults to permanent controls.
- `VideoControls` and `AudioControls` are plain bool sets. `none()` hides
  everything and `minimal()` starts from a small, opinionated set.
- `AudioPlayerLayout` selects `Inline` (metadata and a full transport row) or
  `Compact` (one row for narrow surfaces).
- `PlayerTimeDisplay` selects elapsed, remaining, or both time labels;
  `RepeatMode` cycles off, all, and one.

Both players emit signals rather than mutating application state:
`play_toggled`, `seek_requested`, `volume_changed`, `mute_toggled`,
`rate_changed`, and layout-specific extras. Video adds `fullscreen_requested` and
`loop_toggled`; audio adds `repeat_changed`, `shuffle_toggled`,
`previous_requested`, and `next_requested`. Subscribing to a
signal keeps the widget free of application policy.

Styling uses the `video` and `audio-player` type selectors. Both widgets accept
DSS borders, hover/focus state, and the standard text and accent properties
through `paint_styled`; the players' own theme values fill in whatever the
stylesheet leaves unset.

## FFmpeg command-line backend

The first backend is intentionally out of process:

- `ffprobe` reads width, height, rotation, and duration.
- `ffmpeg` decodes to raw RGBA at a requested frame rate and maximum dimension.
- A worker thread reads fixed-size frames and posts the newest one; a slow UI
  thread drops frames instead of growing an unbounded backlog.
- Seeking kills the process and restarts it at the requested position.
- The decoded size is capped at 1280 pixels by default; rotation swaps the
  reported width and height before scaling.
- `DRAWEN_FFMPEG` and `DRAWEN_FFPROBE` select the executables. `FfmpegConfig` can
  point at a portable build shipped next to the application.

Frame timestamps assigned by the reader are `start + index / frame_rate`. That is
accurate for constant-frame-rate output and is why the backend is suitable for
local clips, previews, onboarding media, and media cards, but not for exact frame
stepping.

Known limits:

- raw frames cross the CPU boundary and are copied into `RasterImage`, the
  display list, and then a GPU texture;
- no audio track, so there is no audio/video clock relationship yet;
- no hardware decode or zero-copy external textures;
- no buffering policy, network source, adaptive bitrate, or subtitle track;
- seek precision depends on `ffmpeg` and keyframe placement.

These are backend limits, not contract limits. A new backend implements
`MediaSession`; widget code does not change.

## Paleo backend

`PaleoSession` (in `drawen-widgets`, behind the `media` feature) is the
in-process backend. It wraps `paleo_player::Player`, which owns the container
reader, the decoder chosen by `paleo-video`, and the bounded picture queue:

- `Video::open_paleo(path)` is the convenience entry point; `Video::with_session`
  attaches any other `MediaSession` implementation.
- The default build decodes H.264, HEVC, VP9, and AV1. A track whose codec has no
  backend fails the video-only session with an error that names the codec and the
  backends this build contains.
- Files open video-only, so an audio codec that this build cannot decode does
  not prevent playback and no audio is decoded before an output device exists.
- Decoding is synchronous and bounded on the calling thread. Each `update`
  reads at most 96 packets and keeps three pictures ahead of the playhead
  instead of building an unbounded queue. A picture is released only when the
  clock reaches its timestamp, so a high update rate cannot outrun the stream.
  There is no worker thread yet, so a large frame can briefly block the UI
  thread.
- Seeking finds the keyframe at or before the target (scanning back up to 10 s),
  moves the reader there, resets the decoder, and decodes a preview even while
  paused, so the view repaints at or before the requested position instead of
  jumping forward to the next keyframe. Each backend recognizes its own
  random-access points (`VideoDecoder::is_keyframe_data`), because Symphonia 0.6
  does not expose packet keyframe flags.
- `Player` reopens the source when seeking after end-of-stream. Symphonia 0.6's
  ISO/MP4 reader cannot resume once its atom iterator has reached EOF;
  `paleo::Media` does the same for direct callers.
- H.264 B-pictures are ordered by the decoder's `PicOrderCnt`, because Symphonia
  0.6 does not implement the ISO/MP4 `ctts` composition offsets. See
  [PALEO.md](PALEO.md) for the framing and timestamp details.

Known limits:

- decoding is software-only on the calling thread; there is no hardware decode
  or zero-copy texture path;
- H.264 supports 4:2:0 frame pictures, HEVC and VP9 cover the profiles their
  pure-Rust decoders implement, and AV1 uses `rusty_av1d` without its assembly
  kernels, so high-bitrate 4K playback is CPU-bound;
- VP8 and MPEG-4 Part 2 still need a backend.

## Audio-mastered Paleo backend

`AudioPaleoSession` (in `drawen-widgets`, behind the `audio` feature) adds the
other half of playback. It is the same `paleo_player::Player` with audio output
enabled: the player decodes audio with Paleo, converts it to the output device
with `paleo_audio::PcmConverter`, and uses the device position as the master
clock, so video follows it:

```text
Paleo audio ──► PcmConverter ──► AudioOutput ring ──► device
                                        │
                                        └── position ──► player timeline ──► Video frames
```

- `Video::open_paleo_audio(path)` is the convenience entry point; the
  `drawen` facade forwards the same feature as `drawen/audio`.
- The device is opened with its default output configuration and the media is
  adapted to it. A file with no audio track, an unsupported audio codec, or no
  usable output device falls back to the wall-clock behavior of `PaleoSession`;
  `PaleoSession::warning` reports why audio was skipped.
- Audio is queued ahead of the device up to 300 ms, bounded by the device ring.
  When a converted PCM block does not fit, the player keeps its unqueued tail
  and resumes it on the next update instead of dropping samples. Video is
  limited to three queued pictures, so the demuxer cannot race ahead of the
  device and reach end-of-stream early.
- Seeking searches back for the preceding keyframe, repositions audio and video
  together, resets the converter and output clock to the requested position, and
  trims audio and video decoded before the target. The decoder resumes at a
  random-access point, then decodes forward until it has a frame at or after the
  target; stale pre-seek pictures cannot fill the bounded video queue. The last
  presented picture stays visible as a placeholder until that first new picture
  is ready, including after a backward seek.
- Video is decoded for every codec with a backend. An unsupported video codec is
  skipped so that audio still plays, instead of failing the session.
- `set_video_enabled(false)` stops video decoding and presentation while audio
  keeps playing; `set_video_enabled(true)` resynchronizes at the audio clock.
  Minimizing or occluding a window to trigger this is an application policy. The
  platform layer reports `WindowEvent::Visible`, `Minimized`, and `Occluded` so
  an application or director can stop decoding without stopping audio. The
  adapter itself already stops presenting hidden frames and keeps the background
  tick running; decoding is the part that needs a policy.
- `set_rate` scales the timeline and suspends audio outside `1.0`, because the
  device has no rate control. Returning to `1.0` seeks to the current position
  so audio resumes in sync.

Known limits:

- decoding still happens on the calling thread. Audio decoding, conversion, and
  queueing are bounded per update, but a large video frame can briefly block the
  UI thread;
- no device hot-plug handling: a device that disappears surfaces as a stream
  error and a failed clock rather than a silent switch to another output;
- audio is refilled from `MediaSession::update`, so sound continues only while
  the application keeps ticking the session. The native adapter calls
  `WindowApplication::tick` at the background interval while the window is
  minimized or occluded, so a `UiRuntime` application keeps media alive by
  forwarding that call to `UiRuntime::tick`. `drawen-director` supplies the
  policy and window actions, but the application still owns the tick hook.

## Lifecycle and presentation policy

The platform layer reports normalized states instead of prescribing what to do:

| Event | Meaning |
| --- | --- |
| `Visible(bool)` | the window is shown or hidden |
| `Minimized(bool)` | the user minimized or restored the window |
| `Occluded(bool)` | another window or the desktop fully covers the window |
| `CloseRequested` | the user asked to close the window |

Drawen does not assume that audio must stop when video is not visible, and it does
not assume that closing the last window means the application continues. Those
are application decisions. The intended defaults are:

- minimize or occlude: keep audio playing, stop presenting video, and skip GPU
  redraws; pause video decoding or keep only a tiny preview buffer. On restore,
  the audio clock already gives the position the video must catch up to.
- lose focus while still visible: keep presenting, but cap the frame cadence at
  30 frames per second so a sidelined player does not compete with a game or
  another foreground application. `WindowApplication::inactive_frame_interval`
  can opt out; `winit` does not report full occlusion on Windows, so focus loss
  is the practical signal there.
- close the last window: exit the application, unless background playback was
  requested explicitly.

`drawen-director` packages those policies as an optional crate so applications do
not reimplement them. It is not part of `drawen-widgets`: the widgets stay
presentation and control only. Enable it through the `drawen` facade with the
`director` feature, or depend on `drawen-director` directly:

```text
MediaDirector    owns lifecycle state and converts WindowEvents into actions
VisibilityPolicy Continue | PauseVideoKeepAudio | PauseAll | StopAndRelease
AppLifecycle     WindowVisible | WindowOccluded | WindowMinimized |
                 LastWindowClosed | ApplicationExiting
DirectorAction   AppLifecycle + PlaybackAction + WindowAction + redraw flag
```

`MediaDirector::window_event` ignores input and resize events and turns
visibility, minimize, occlude, and close events into a `DirectorAction`.
`MediaDirector::apply` applies it to a `Video`: `PauseVideo` calls
`Video::set_video_enabled(false)` when the session supports it and degrades to
`PauseAll` when it does not; `StopAndRelease` pauses and calls
`Video::release_session`. The director remembers whether it paused playback, so
a policy pause resumes on restore while a user pause stays paused.

The default policy is `PauseVideoKeepAudio`. The last window closing exits unless
the application opts into background playback with
`with_background_playback(true)`. A director action requests a redraw when a
window becomes visible again, so a presenter that skipped frames while hidden
repaints immediately.

### Wiring background playback

The snippet below requires the `audio` and `director` facade features. Both
imply `media`, which provides `Video` and the session contracts. Enable the
features that provide the session and the policy:

```text
cargo add drawen --features audio,director
```

Keep one director per presented window and forward lifecycle events before the
application's own handling:

```rust
use drawen::{
    MediaDirector, Video, VisibilityPolicy,
    platform::{WindowAction, WindowEvent},
};

let mut director = MediaDirector::new()
    .with_policy(VisibilityPolicy::PauseVideoKeepAudio)
    .with_background_playback(true);

fn handle(
    director: &mut MediaDirector,
    video: &mut Video,
    event: &WindowEvent,
) -> WindowAction {
    let Some(action) = director.window_event(event) else {
        return WindowAction::None;
    };
    director.apply(action, video);
    action.window()
}
```

`with_background_playback(true)` is the explicit opt-in that keeps the process
alive after the last window closes. Without it, the director returns
`WindowAction::Exit`. The director does not tick audio itself: the native adapter
calls `WindowApplication::tick` while the window is hidden, and the application
forwards it to `UiRuntime::tick` (or calls `MediaSession::update` directly) so
background audio keeps running.

## Adding a backend

1. Probe or negotiate metadata before playback and build a `MediaClock` with the
   duration.
2. Decode on a worker or platform callback, timestamp frames with presentation
   time, and deliver only the newest frame through `MediaUpdate`.
3. Restart or flush on `seek`, and keep the clock position equal to the first
   frame the backend will deliver.
4. If the backend has audio, synchronize the clock from the output position and
   let video frames follow the clock rather than the wall clock.
5. Report fatal failures through `MediaError` and the clock's `Failed` state.

In-process and hardware backends are expected to keep the same contract. Their
value is removing copies and improving seek accuracy, not changing the view API.
