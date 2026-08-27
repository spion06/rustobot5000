# rustobot5000

This is discord bot for me and my friends. Currently supports basic operations against a kubernetes api and streaming from an emby instance. This is mostly just for me to have some fun with rust.

## Building

Local: `cargo build --release` (needs the GStreamer 1.x dev libraries and plugins installed).

Docker (BuildKit required for the cache mounts):

```
DOCKER_BUILDKIT=1 docker build -t rustobot5000 .
```

## Configuration

Set via environment variables:

- `DISCORD_TOKEN` (required), `EMBY_API_TOKEN` (required)
- `EMBY_API_URL` (default `http://localhost:8096`)
- `RTMP_URI` (default `rtmp://localhost:7788/live/livestream`)
- `DISCORD_SERVER_IDS` — comma-separated guild IDs
- `PATH_REMAP_FROM` / `PATH_REMAP_TO` — optional, rewrites Emby media paths to local paths
