# spmp3

spmp3 is a server to keep track of my spotify library, and make it available for
offline syncing to an mp3 player.

## Crates

1. spsync - tracks spotify library and maintains a local copy
2. mp3sync - manages mp3 syncing and tracks diff between lib and most recent sync
3. daemon - primary binary

## Configuration

Set via environment. See `.env.example`.

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `CACHE_DIR` | yes | | Spotify credentials |
| `LIBRARY_DIR` | yes | | Local mp3 library and manifest |
| `DEVICE_STATE` | yes | | Record of what is on the player |
| `MOUNT_DIR` | yes | | Mount point of the player |
| `PRESERVE` | yes | | Keep local files after a track is unliked |
| `REALTIME` | no | `true` | Pace downloads to track duration |
| `LIBRARY_INTERVAL_SECS` | no | `1800` | How often to check Spotify |
| `DEVICE_POLL_SECS` | no | `5` | How often to check for the player |
| `OAUTH_PORT` | no | `5588` | Port the login flow listens on |
| `OAUTH_REDIRECT_HOST` | no | `127.0.0.1` | Redirect host given to Spotify |

`MOUNT_DIR` must be the player's own mount point, reached through mount
propagation.

## Running

```sh
docker build --build-arg SERVICE=daemon -t spmp3:daemon .
docker run --env-file .env -v spmp3:/data -v /media:/media:rslave spmp3:daemon
```

## Authorizing

Run the login binary, open the url it prints on any machine, approve, then paste
the address you were redirected to back into the terminal. Credentials are
written to `CACHE_DIR`.

```sh
docker build --build-arg SERVICE=login -t spmp3:login .
docker run --rm -it --env-file .env -v spmp3:/data spmp3:login
```
