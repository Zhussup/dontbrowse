# dontbrowse

A browser for your terminal: YouTube feeds and search in a responsive video-card grid, plus a
reader that turns any article into clean, typographic terminal content.

```
╭──────────────────────────────────╮
│ ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄ │  thumbnail as half-block pixels
│ ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀ │  duration badge, bottom right
│                                  │
│ The Title Wraps To At Most Two   │  bold, accent when selected
│ Lines                            │
│ Channel · 3 days ago · 2.7M      │  dim metadata line
╰──────────────────────────────────╯
```

## What it does

- **YouTube grid** — search, Discover, channel and playlist feeds, subscriptions. Cards show a
  thumbnail (rendered as colored half-blocks, no graphics protocol required), title, channel,
  relative publish date, duration, views, and a LIVE badge.
- **Article reader** — point it at a URL and it extracts the main content and renders headings,
  paragraphs, emphasis, links, lists, quotes, code blocks, tables, figures, and images. Includes a
  table of contents, reading progress, and clickable links.
- **Playback** — audio plays through mpv in the background while the TUI shows a player bar;
  `v` switches to a full video window at the same position. The TUI controls playback over mpv's
  JSON IPC socket.
- **Persistence** — subscriptions, library, watch position, article progress, and cached feeds live
  in SQLite; the cookie jar lives in a `0600` file and is never logged.
- **Graceful degradation** — works without mpv, without yt-dlp, over SSH, in 256-color terminals,
  and with `NO_COLOR`. Links use OSC 8 where supported; the `o` picker always works.

## Requirements

- Rust 1.97+ (edition 2024)
- `yt-dlp` for YouTube fallback and playback resolution
- `mpv` (optional) for playback; `ffmpeg` recommended for format merging
- A terminal with 256 colors; truecolor and OSC 8 are used when available

Install the tools (the exact command depends on your distro):

```bash
sudo apt install mpv ffmpeg
pipx install yt-dlp        # distro packages are often stale
```

## Build and install

```bash
cargo build --release
install -m755 target/release/dontbrowse ~/.local/bin/dontbrowse
```

Or `cargo install --path .`.

## Usage

```bash
dontbrowse                              # Discover feed
dontbrowse --search "rust programming"  # start in search
dontbrowse "https://example.com/article"
dontbrowse "https://www.youtube.com/@3blue1brown/videos"
dontbrowse --offline --search "rust"    # serve from cache only
```

Useful flags:

| Flag | Meaning |
|---|---|
| `--search QUERY` | Run a YouTube search on launch |
| `--offline` | Never touch the network; serve cached feeds/articles |
| `--cookies FILE` | Import a Netscape `cookies.txt` and pass it to yt-dlp |
| `--cookies-from-browser BROWSER` | Let yt-dlp use browser cookies (`firefox`, `chromium:Default`, …) |
| `--import-subscriptions FILE` | Import channels from CSV/JSON and exit |
| `--theme auto\|dark\|light` | Theme choice |
| `--no-images` | Disable thumbnails and article images |
| `--yt-dlp PATH`, `--mpv PATH` | Override tool paths |
| `--debug` | Verbose logging to `$XDG_CACHE_HOME/dontbrowse/log/dontbrowse.log` |

Inside the app, `Ctrl+L` (or `:`) opens the omnibox: URLs go to the reader, plain text searches
YouTube. `Ctrl+P` is the command palette, `?` shows all shortcuts.

## Key bindings

**Global** — `q`/`Ctrl+C` quit · `?` help · `Ctrl+P` palette · `Ctrl+L`/`:` open URL or search ·
`1`–`4` tabs · `Tab`/`Shift+Tab` cycle tabs · `Esc` back/close · `gg`/`G` top/bottom.

**Feeds** — `h j k l`/arrows move · `Enter` play audio · `Shift+Enter` open video window · `p` play ·
`s` subscribe/unsubscribe · `r` refresh · `c` copy URL · `L` save to library · `i` details ·
`/` search.

**Reader** — `j`/`k` scroll · `d`/`u` half page · `Ctrl+D`/`Ctrl+U` full page · `[`/`]` previous/next
heading · `t` toggle TOC · `o` link picker (`Enter` open, `O` external) · `w` reading width ·
`r` reload · `L` save · `Backspace` back.

**Player** — `Space` pause/resume · `,`/`.` seek 5s · `<`/`>` seek 30s · `+`/`-` volume · `m` mute ·
`v` audio/video · `x` stop · `n` next in queue.

## Configuration

Config lives at `$XDG_CONFIG_HOME/dontbrowse/config.toml` (usually
`~/.config/dontbrowse/config.toml`). Every field is optional:

```toml
theme = "auto"                  # auto | dark | light
yt_dlp_path = "yt-dlp"
mpv_path = "mpv"
cookies_from_browser = "firefox" # optional
cookies_txt = "/home/me/cookies.txt" # optional

[reader]
width = 92                      # maximum article measure in columns
images = true
send_cookies_to_images = "same-site"
user_agent = "Mozilla/5.0 ..."

[youtube]
limit = 40
home = "auto"                   # auto | trending | recommended
discover_query = "trending videos"

[ui]
animations = true
```

`DONTBROWSE_REDUCED_MOTION=1` disables shimmer/transition effects. `NO_COLOR=1` switches to a
monochrome hierarchy.

## Paywalled articles (FT and friends)

Export cookies from a logged-in browser as Netscape `cookies.txt`, then either launch with
`--cookies ~/cookies.txt` or use `Ctrl+P → Import cookies.txt`. The jar is stored at
`$XDG_DATA_HOME/dontbrowse/cookies.json` with mode `0600`; cookie values are never written to logs.
The same file is passed to yt-dlp for age-restricted or personalized YouTube content.

If a page is still blocked, the reader explains whether cookies are missing or expired, and `O`
opens the URL in your system browser. JavaScript-only pages are not rendered; the extractor falls
back to JSON-LD/plain text and says so.

## Subscriptions

Import a CSV (`channel_id_or_handle,title`) or a JSON array of
`{"id|channel_id|channelId": "...", "title": "...", "handle": "..."}`:

```bash
dontbrowse --import-subscriptions ~/subscriptions.csv
```

You can also press `s` on any video card. Subscriptions are merged from YouTube's public RSS feeds
(no login); RSS has no duration, so those cards omit the badge rather than invent one.

## How it works

```
HTTP / yt-dlp ─▶ adapter (YouTube web + RSS, Article) ─▶ typed model
                                                        │
                                          ratatui layout / widgets
                                                        │
                                             custom OSC 8 backend
```

- YouTube cards come from YouTube's page data (`ytInitialData`) with `yt-dlp --flat-playlist` as a
  fallback and for playback resolution. Publish dates and view counts come from the page data.
- Articles use `dom_smoothie` (Readability port) with heuristic, JSON-LD, and plain-text fallbacks,
  then a DOM-to-block mapper. Untrusted text is stripped of ANSI/control/bidi characters.
- A small custom crossterm backend injects OSC 8 hyperlinks cell-by-cell from a per-frame link
  registry.
- Thumbnails and article images are downloaded asynchronously, cached on disk, resized, and
  converted to half-block cells. No image protocol is required.

## Limitations

- JavaScript-rendered pages cannot be read; there is no headless browser.
- YouTube's Discover feed is an anonymous search feed ("trending videos"). Personalized
  recommendations need browser cookies and may still be unavailable.
- RSS subscriptions do not provide video durations.
- Video output uses a separate mpv window; the terminal only renders the controls.
- OSC 8 clickability depends on the terminal; `o` is the universal fallback.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Tests cover yt-dlp JSON, RSS/Atom entities, YouTube page data, relative dates and view counts,
article extraction/fallbacks, block mapping, wrapping, layout edge cases, and IPC parsing. No test
requires network access.
