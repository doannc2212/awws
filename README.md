# awws

a personal wallpaper daemon for Hyprland / Wayland, built for my own use. the main idea is that every piece — where wallpapers come from, how they get applied, and when they change — lives as its own independent layer. swap out any one part without touching the others.

the goal that drives most of the design: wallpaper changes that feel alive, the way Windows Spotlight or macOS's dynamic wallpapers do — without needing to think about it.

## how it works

```
source  →  cache  →  setter
   ↑
trigger
```

- **source** — where images come from: your local folder, Bing daily, NASA APOD, Unsplash, or Wallhaven
- **trigger** — when to change: on a timer, on login, or on screen unlock (via D-Bus)
- **setter** — how to apply: `awww`, `hyprpaper`, `swaybg`, or `swww`
- **cache** — downloaded images are stored locally so things stay fast and offline-friendly

each layer is independent. if you only want local wallpapers with `awww`, that's all you configure.

## install

```bash
cargo install --path .
```

or build manually:

```bash
cargo build --release
cp target/release/awws ~/.local/bin/
```

## usage

```bash
awws start          # start the daemon
awws next           # change wallpaper now
awws history        # show recent wallpapers
```

## config

my config lives at `~/.config/awws/config.toml`. if the file doesn't exist yet, awws will just use its built-in defaults — so you only need to write the parts you actually want to change. you can also copy the [`config.toml`](config.toml) in this repo as a starting point.

to apply changes without restarting, run `awws reload` and the daemon will pick them up immediately.

---

### daemon

controls when wallpapers change automatically.

```toml
[daemon]
interval_secs = 1800    # how often to rotate, in seconds (1800 = 30 minutes)
change_on_login = true  # change when you log in
change_on_unlock = true # change when you unlock your screen
# on_change = "wallust run %w"  # shell command to run after every change
```

#### hooks

hooks are how awws reaches *out* to the rest of your rice — when something happens inside the daemon, it runs a command you've chosen. (for the other direction, driving awws *from* a script, see [scripting](#scripting) below.)

there are three of them. all run as `sh -c <command>`, so pipes, `&&`, and environment variables all work. if a hook exits with an error, awws just logs a warning and keeps going — a broken hook never interrupts the daemon. they also run fire-and-forget, so a slow hook won't hold up the next wallpaper change.

here's the whole surface at a glance:

| hook | runs when | placeholder |
|------|-----------|-------------|
| `on_change` | after every successful wallpaper change | `%w` = new wallpaper path |
| `on_start` | once when the daemon starts | `%w` = last wallpaper from history |
| `on_error` | an automatic change fails | `%e` = the error message |

the placeholders are plain text substitution — every `%w` (or `%e`) in your command is swapped for the value before the shell sees it, so feel free to use it more than once.

**`on_change`** — runs after every successful wallpaper change. `%w` = absolute path of the new wallpaper. this is the main integration point for colorscheme pipelines.

```toml
[daemon]
on_change = "wallust run %w"
# on_change = "matugen image %w"
# on_change = "wallust run %w && pkill -SIGUSR2 waybar"
```

**`on_start`** — runs once when the daemon starts, using the last wallpaper from history. `%w` = that path. the practical use: after a reboot, your colorscheme is restored immediately — before the first interval fires or any new image is fetched — so waybar, kitty, and rofi all come up already themed.

```toml
[daemon]
on_start = "wallust run %w"
```

if you set both `on_start` and `on_change`, and `change_on_login` is also true, you'll see both fire on login: `on_start` with the previous wallpaper, then `on_change` with the new one. that's intentional — `on_start` restores the known state, `on_change` updates it.

**`on_error`** — runs when an automatic wallpaper advance fails (network down, api rate limit, empty folder, etc.). `%e` = the error message. note that it fires on *automatic* advances only — when you run `awws next` / `awws prev` yourself, any error comes back to you directly (in the terminal, or in the json response if you're talking to the socket) rather than through this hook.

```toml
[daemon]
on_error = "notify-send -u critical 'awws' '%e'"
```

---

### setter

controls how the wallpaper gets applied. i mostly use `awww` but the others work too.

```toml
[setter]
backend = "awww"           # awww | hyprpaper | swww | swaybg
transition = "fade"        # transition effect (for awww|swww)
transition_duration = 1.5  # in seconds
```

awws passes `transition` directly to the backend binary, so the valid values depend on what you have installed. for `swww`, the supported types are:

`none` `fade` `left` `right` `top` `bottom` `wipe` `wave` `grow` `center` `any` `outer` `random`

for `awww`, run `awww img --help` to see what your installed version supports. `hyprpaper` and `swaybg` don't support transitions — the field is ignored for both.

---

### sources

this is the part i find most fun to configure. you can mix and match as many sources as you like — just stack as many `[[sources.list]]` blocks as you want and awws will handle the rest.

```toml
[sources]
rotation = "weighted_random"  # weighted_random | round_robin
```

the `rotation` setting controls how awws picks between your sources each time a wallpaper changes:

- **`weighted_random`** — picks a source at random, favouring ones with a higher `weight`. for example, if you have a local source at weight 2 and bing at weight 1, local wins roughly 67% of the time. order in the file doesn't matter.
- **`round_robin`** — cycles through sources in the order they appear in the file, top to bottom. `weight` is ignored in this mode.

if a source fails for any reason (network down, empty folder, api error), awws quietly warns and tries the next one — so having multiple sources also gives you a natural fallback chain.

#### local folder

pull images from a folder on your machine:

```toml
[[sources.list]]
type = "local"
path = "~/Pictures/Wallpapers"
order = "random"     # random | sequential
weight = 2
```

#### bing daily

bing's daily photo — downloaded and cached so it works offline too:

```toml
[[sources.list]]
type = "bing"
weight = 1
```

#### unsplash

random photos from unsplash. you'll need a free api key from [unsplash.com/developers](https://unsplash.com/developers).

```toml
[[sources.list]]
type = "unsplash"
api_key = "your_api_key_here"
query = "nature"          # optional — narrows the search
orientation = "landscape" # optional — landscape | portrait | squarish
weight = 1
```

#### nasa apod

nasa's astronomy picture of the day. api key available for free at [api.nasa.gov](https://api.nasa.gov).

```toml
[[sources.list]]
type = "nasa_apod"
api_key = "your_api_key_here"
weight = 1
```

#### wallhaven

random wallpapers from [wallhaven.cc](https://wallhaven.cc). no api key needed unless you want nsfw results.

```toml
[[sources.list]]
type = "wallhaven"
api_key = "your_api_key_here"  # optional — only needed for nsfw purity
query = "landscape"            # optional — search terms
categories = "111"             # optional — general/anime/people as on/off bits
purity = "100"                 # optional — sfw/sketchy/nsfw as on/off bits
weight = 1
```

#### source type and `next` behaviour

awws distinguishes between local and remote sources, and this affects how `awws next` works:

- **local-only** — `awws next` cycles through your history. when it reaches the latest wallpaper it wraps back to the oldest, so you get a smooth loop through everything that's been shown.
- **remote-only** (bing, unsplash, nasa apod, wallhaven) — `awws next` always fetches a fresh image from the source.
- **mixed** — combining local and remote sources in the same config is not supported. awws will refuse to start and print an error. keep all your sources either local or remote.

#### multiple remote sources

stack multiple remote sources and awws picks between them according to `rotation` and `weight`:

```toml
[sources]
rotation = "weighted_random"

[[sources.list]]
type = "bing"
weight = 1

[[sources.list]]
type = "unsplash"
api_key = "your_api_key_here"
query = "nature"
weight = 2

[[sources.list]]
type = "nasa_apod"
api_key = "your_api_key_here"
weight = 1
```

#### multiple local sources

point at multiple folders — useful if you keep wallpapers organised by category:

```toml
[sources]
rotation = "round_robin"

[[sources.list]]
type = "local"
path = "~/Pictures/Wallpapers/landscapes"
order = "random"
weight = 1

[[sources.list]]
type = "local"
path = "~/Pictures/Wallpapers/minimal"
order = "sequential"
weight = 1
```

---

### cache

downloaded images are stored locally. i find the defaults reasonable but here's how to adjust them:

```toml
[cache]
max_size_mb = 500              # max disk space for cached images
history_size = 100             # how many past wallpapers to remember
dir = "~/.cache/awws/images"  # where to store them
```

## scripting

if you want another program to *drive* awws — a keybind, a status bar, a little script — you have two options, and honestly the first one is enough for almost everything.

**just run the cli.** `awws next`, `awws status`, and the rest already do the right thing. under the hood the cli isn't doing anything special: it connects to a small unix socket, sends one line of json, and prints the reply. so when you shell out to `awws next`, you're already going through the same path the socket exposes.

```bash
# a hyprland keybind, for example
bind = $mod, W, exec, awws next
```

**talk to the socket directly.** if you'd rather skip spawning the binary — say you're polling `status` often from a bar widget — you can speak to the socket yourself. it lives at `$XDG_RUNTIME_DIR/awws.sock` and the protocol is deliberately tiny: write one json object followed by a newline, read one json line back.

a request is just a command name:

```json
{"cmd": "next"}
```

the valid commands are the same ones the cli has: `next`, `prev`, `pause`, `resume`, `status`, `reload`, `history`.

the reply is a single json line, tagged by a `result` field. there are three shapes:

```json
{"result": "ok", "message": "advanced", "status": { ... }}
{"result": "history", "entries": [ ... ], "cursor": 3}
{"result": "error", "message": "all sources failed"}
```

most commands answer with `ok` and a short `message` (`status` carries the same `status` block you'd see from `awws status`); `history` answers with its own shape; and anything that went wrong comes back as `error` with a human-readable `message`.

a quick one-liner to try it, no awws binary involved:

```bash
echo '{"cmd":"status"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/awws.sock
```

or from python, which is roughly what i'd reach for in a real script:

```python
import json, os, socket

def awws(cmd):
    path = os.path.join(os.environ["XDG_RUNTIME_DIR"], "awws.sock")
    with socket.socket(socket.AF_UNIX) as s:
        s.connect(path)
        s.sendall((json.dumps({"cmd": cmd}) + "\n").encode())
        return json.loads(s.makefile().readline())

print(awws("status"))
```

one honest caveat: the cli is a stable interface and i'll try to keep it that way, but the raw json schema is closer to an internal detail. if you build something against the socket directly, a future version might shift a field under you. shelling out to the cli insulates you from that, so reach for the socket only when you actually need to.

## autostart

add to your `hyprland.conf`:

```
exec-once = awws daemon
```

---

built on [Hyprland](https://hyprland.org/), inspired by Windows Spotlight and macOS dynamic wallpapers. hope it's useful to someone else too.
