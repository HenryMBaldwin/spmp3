# spmp3

spmp3 is a server to keep track of my spotify library, and make it available for
offline syncing to an mp3 player (in my case my beloved
[SandDisk Clip Sport Plus](https://documents.sandisk.com/content/dam/asset-library/en_us/assets/public/sandisk/product/mp3-players/clip-sport-plus/data-sheet-clip-sport-plus.pdf)).

## Crates

1. spsync - tracks spotify library and maintains a local copy
2. mp3sync - manages mp3 syncing and tracks diff between lib and most recent
   sync
3. daemon - primary binary
4. web - browser client for syncing a player on another machine

## Configuration

Set via environment. See `.env.example`.

| Variable                | Required | Default        | Purpose                                   |
| ----------------------- | -------- | -------------- | ----------------------------------------- |
| `CACHE_DIR`             | yes      |                | Spotify credentials                       |
| `LIBRARY_DIR`           | yes      |                | Local mp3 library and manifest            |
| `DEVICE_STATE`          | yes      |                | Record of what is on the player           |
| `MOUNT_DIR`             | yes      |                | Mount point of the player                 |
| `PRESERVE`              | yes      |                | Keep local files after a track is unliked |
| `REALTIME`              | no       | `true`         | Pace downloads to track duration          |
| `LIBRARY_INTERVAL_SECS` | no       | `1800`         | How often to check Spotify                |
| `DEVICE_POLL_SECS`      | no       | `5`            | How often to check for the player         |
| `OAUTH_PORT`            | no       | `5588`         | Port the login flow listens on            |
| `OAUTH_REDIRECT_HOST`   | no       | `127.0.0.1`    | Redirect host given to Spotify            |
| `WEB_BIND`              | no       | `0.0.0.0:8080` | Address the web client listens on         |

`MOUNT_DIR` must be the player's own mount point, reached through mount
propagation.

## Running

Images are published to `ghcr.io/henrymbaldwin/spmp3/{daemon,login}`, versioned
from conventional commits. Copy `docker-compose.example.yaml` and
`.env.example`, then:

```sh
docker compose up -d
```

To build locally instead, pass the binary as a build argument:

```sh
docker build --build-arg SERVICE=daemon -t spmp3:daemon .
```

## Authorizing

Run the login binary, open the url it prints on any machine, approve, then paste
the address you were redirected to back into the terminal. Credentials are
written to `CACHE_DIR`.

```sh
docker compose run --rm login
```

## Remote sync

`web` serves a page that syncs a player plugged into another machine. It needs a
Chromium-based desktop browser and should only be reachable over Tailscale.

```sh
docker compose up -d web
```

Open it, choose the player, and press sync. The browser reads what is on the
player, asks the server what should change, and copies, moves and deletes to
match.
