#!/usr/bin/env bash
# Installs a udev rule that mounts the player when it is plugged in.
set -euo pipefail

LABEL="${LABEL:-SPORT*}"
MOUNT_DIR="${MOUNT_DIR:-/media/spmp3-player}"
OWNER_UID="${OWNER_UID:-$(id -u)}"
OWNER_GID="${OWNER_GID:-$(id -g)}"
RULE=/etc/udev/rules.d/99-spmp3-player.rules

if [ "$(id -u)" -ne 0 ]; then
  echo "run with sudo -E" >&2
  exit 1
fi

# iocharset=utf8 matters: the default mangles every non-ascii filename.
OPTIONS="rw,uid=${OWNER_UID},gid=${OWNER_GID},fmask=0022,dmask=0022,iocharset=utf8,utf8=1"

install -d -m 0755 "$MOUNT_DIR"

cat > "$RULE" <<RULE_EOF
ACTION=="add", SUBSYSTEM=="block", ENV{ID_FS_TYPE}=="vfat", ENV{ID_FS_LABEL}=="${LABEL}", \\
  RUN+="/usr/bin/systemd-mount --no-block --collect --type=vfat --options=${OPTIONS} --fsck=no \$devnode ${MOUNT_DIR}"
RULE_EOF

udevadm verify "$RULE"
udevadm control --reload
udevadm trigger --subsystem-match=block --action=add

echo "installed $RULE"
echo "  label      ${LABEL}"
echo "  mount dir  ${MOUNT_DIR}"
echo "  options    ${OPTIONS}"
