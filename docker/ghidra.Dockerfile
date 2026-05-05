# Wrapper around blacktop/ghidra that runs as a UID/GID matching the host
# user, so files written into the bind-mounted /projects and /scripts dirs
# (Ghidra project DB, function dumps, inventory CSVs) come back as the
# host user instead of root.
#
# The base blacktop/ghidra:latest container runs everything as root by
# default. That makes any output land root-owned on the host, which then
# needs `sudo chown -R $(id -u):$(id -g) ghidra/scripts/funcs/` after every
# script run before the host (or any non-root tool) can read the files.
# This wrapper resolves that by:
#   1. Creating (or reusing) a user inside the image whose UID/GID match the
#      host user, parametrised via the USER_ID / GROUP_ID build args.
#   2. Pre-chowning the writable mount points so volume mounts are writable
#      from inside the container.
#   3. Switching to that user as the default.
#
# The base image already ships with `ubuntu:1000:1000`, which is the common
# host-user UID on Linux. When USER_ID=1000 and GROUP_ID=1000 (the default),
# this Dockerfile reuses the existing ubuntu account.
#
# Build (defaults pick up host UID/GID via .env or shell env):
#   docker compose build ghidra
#
# Pass UID/GID explicitly:
#   USER_ID=$(id -u) GROUP_ID=$(id -g) docker compose build ghidra
#
# After building, run as before:
#   docker compose up -d ghidra
#   docker compose exec ghidra /ghidra/support/analyzeHeadless ...

FROM blacktop/ghidra:latest

ARG USER_ID=1000
ARG GROUP_ID=1000

# Reuse the matching group/user when present (e.g. base image's
# ubuntu:1000:1000); otherwise create a fresh `legaia` account.
RUN set -eux; \
    if ! getent group "${GROUP_ID}" >/dev/null; then \
        groupadd -g "${GROUP_ID}" legaia; \
    fi; \
    if ! getent passwd "${USER_ID}" >/dev/null; then \
        useradd -u "${USER_ID}" -g "${GROUP_ID}" -m -s /bin/bash legaia; \
    fi

# Pre-create the mount-point directories with matching ownership so the
# bind-mounted volumes are writable from inside the container. /data is
# mounted read-only, so chown isn't needed there. /tmp/ghidra-cache covers
# any analyzer scratch space the headless mode wants.
RUN mkdir -p /projects /scripts /tmp/ghidra-cache && \
    chown -R "${USER_ID}:${GROUP_ID}" /projects /scripts /tmp/ghidra-cache

USER ${USER_ID}:${GROUP_ID}
