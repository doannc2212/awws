# awws

a personal wallpaper daemon for Hyprland / Wayland, built for my own use. the main idea is that every piece — where wallpapers come from, how they get applied, and when they change — lives as its own independent layer. swap out any one part without touching the others.

the goal that drives most of the design: wallpaper changes that feel alive, the way Windows Spotlight or macOS's dynamic wallpapers do — without needing to think about it.

## how it works

```
source  →  cache  →  setter
   ↑
trigger
```

- **source** — where images come from: your local folder, Bing daily, NASA APOD, or Unsplash
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

#### mixing sources

here's an example of how i like to combine them — local wallpapers most of the time, with bing filling in when i want something fresh:

```toml
[sources]
rotation = "weighted_random"

[[sources.list]]
type = "local"
path = "~/Pictures/Wallpapers"
order = "random"
weight = 3

[[sources.list]]
type = "bing"
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

## autostart

add to your `hyprland.conf`:

```
exec-once = awws start
```

---

built on [Hyprland](https://hyprland.org/), inspired by Windows Spotlight and macOS dynamic wallpapers. hope it's useful to someone else too.
