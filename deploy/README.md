# deploy

Host setup for the machine the player plugs into.

## Auto-mounting the player

```sh
sudo -E ./install-mount.sh
```

Mounts the player at `/media/spmp3-player` when it is plugged in, and unmounts it when it is
removed. `systemd-mount` binds the mount to the device, so pulling the cable leaves no stale
entry behind.

`iocharset=utf8,utf8=1` is required. Without it the kernel re-encodes every non-ascii filename
and the player ends up with names like `BÃRNS` instead of `BØRNS`.

Override the defaults with environment variables:

| Variable    | Default              | Meaning                              |
| ----------- | -------------------- | ------------------------------------ |
| `LABEL`     | `SPORT*`             | Filesystem label to match            |
| `MOUNT_DIR` | `/media/spmp3-player`| Must match `MOUNT_DIR` in the daemon |
| `OWNER_UID` | invoking user        | Owner of files on the player         |
| `OWNER_GID` | invoking user        | Group of files on the player         |

Find the label of a plugged-in player with:

```sh
lsblk -o NAME,FSTYPE,LABEL
```

If plugging the player in does not mount it, check that the label matches the rule:

```sh
udevadm info -q property -n /dev/sda1 | grep ID_FS_
journalctl -u systemd-udevd -n 50
```
