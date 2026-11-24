# Batch Audio

Batch Audio is a desktop app that saves audio from many YouTube links at once. It is written in Rust with [egui](https://github.com/emilk/egui) and uses [yt-dlp](https://github.com/yt-dlp/yt-dlp) for downloads.

![Batch Audio main window](docs/screenshot.png)

## Features

- Paste many links at once, separated by spaces, new lines, or commas.
- Counts invalid links and skips duplicates.
- Saves as MP3, WAV, M4A, FLAC, Opus, or AAC.
- Shows the title, progress, and result of each download.
- Cancels a running batch. Files that are already saved stay in the folder.
- Lets you choose the output folder. The default is `Downloads/Batch Audio` in your home folder.
- Light, dark, and system appearance.

It accepts `youtube.com/watch`, `youtu.be`, `shorts`, `live`, `embed`, and `music.youtube.com` links.

## Requirements

- [Rust](https://rustup.rs/), to build the app
- [yt-dlp](https://github.com/yt-dlp/yt-dlp/wiki/Installation)
- [FFmpeg](https://ffmpeg.org/download.html), including `ffprobe`
- A JavaScript runtime: [Deno](https://docs.deno.com/runtime/getting_started/installation/), Node.js, or Bun

On macOS with Homebrew:

```sh
brew install yt-dlp ffmpeg deno
```

## Run

```sh
git clone https://github.com/BothSann/bulk-yt.git
cd bulk-yt/app
cargo run --release
```

## Limitations

- At most three downloads run at once.
- The window stays open until a running batch finishes or is cancelled.
- Playlists are not expanded.

## Project layout

| Path | Purpose |
| --- | --- |
| `app/src/main.rs` | Command-line arguments and startup |
| `app/src/app.rs` | App state and actions |
| `app/src/ui` | Window layout, panels, and shared widgets |
| `app/src/theme.rs` | Colors, fonts, and egui styling |
| `app/src/screenshot.rs` | The `--screenshot` option |
| `app/src/queue.rs` | Download queue and background workers |
| `app/src/cancel.rs` | Stopping a batch and its processes |
| `app/src/downloader.rs` | Tool detection and yt-dlp |
| `app/src/links.rs` | Link parsing |
| `app/src/format.rs` | Audio formats |
| `app/assets/fonts` | Bundled fonts. Inter is under the SIL Open Font License. |

## Responsible use

Download only content that you own or have permission to save. You are responsible for following YouTube's terms and the copyright law that applies to you.
