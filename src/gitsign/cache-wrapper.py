#!/opt/homebrew/bin/python3
"""Use the signing cache when its socket accepts connections."""

import os
import socket
import sys

cache_socket = os.environ.get("GITSIGN_CREDENTIAL_CACHE") or os.path.expanduser(
    "~/Library/Caches/sigstore/gitsign/cache.sock"
)
# A stopped helper leaves a socket behind. Send no RPC and leave Git's stdin
# untouched. This tests connectivity, not whether the RPC server is healthy.
try:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as probe:
        probe.settimeout(0.25)
        probe.connect(cache_socket)
except OSError:
    os.environ.pop("GITSIGN_CREDENTIAL_CACHE", None)
else:
    os.environ["GITSIGN_CREDENTIAL_CACHE"] = cache_socket

os.execv("/opt/homebrew/bin/gitsign", ["gitsign", *sys.argv[1:]])
