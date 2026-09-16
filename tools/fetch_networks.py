#!/usr/bin/env python3
"""Restore the archived NNUE networks from the dedicated GitHub network releases.

The repository does not track ``networks/``. The archived Ember V1 and V2
networks ship with the dedicated GitHub network releases ``v1.1`` (Ember V1)
and ``v2.2`` (Ember V2); this script downloads the release assets into the
exact ``networks/`` layout that the tests and benchmarks expect and verifies
each file against its pinned SHA-256.

Network-dependent tests skip automatically while ``networks/`` is absent, so
running this script is optional; it only enables the archived-network
coverage. Engine releases stay separate and remain the ``Latest`` releases.

Usage:
    python tools/fetch_networks.py            # download missing files
    python tools/fetch_networks.py --force    # re-download and verify everything
"""

from __future__ import annotations

import argparse
import hashlib
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path

REPO = "ExxDreamerCode/Ember"
DOWNLOAD_URL = "https://github.com/{repo}/releases/download/{tag}/{asset}"

REPO_ROOT = Path(__file__).resolve().parents[1]
CHUNK_SIZE = 1 << 20


@dataclass(frozen=True)
class NetworkAsset:
    release: str
    asset_name: str
    relative_path: str
    sha256: str


NETWORK_ASSETS = [
    NetworkAsset(
        release="v1.1",
        asset_name="net.nnue",
        relative_path="networks/V1/1.1.0-1.3.0/net.nnue",
        sha256="e1db11a227e7286f31da6e3b1638cd07d3a6776157c5eaf6b73fe62db0ba19a8",
    ),
    NetworkAsset(
        release="v1.1",
        asset_name="net.compact.nnue",
        relative_path="networks/V1/1.1.1-1.3.0/net.compact.nnue",
        sha256="3de91e08cf7eb41f5623c72f820ead967c79d069e3f5eff6a5acba4714cbc317",
    ),
    NetworkAsset(
        release="v2.2",
        asset_name="ember-epoch29.nnue",
        relative_path="networks/V2/1.3.1-now/ember-epoch29.nnue",
        sha256="019da4232187e5474c28f4e304733fe3e20a615ea2ef73da738b91a85e27a135",
    ),
    NetworkAsset(
        release="v2.2",
        asset_name="ember-epoch38.nnue",
        relative_path="networks/V2/1.3.1-now/ember-epoch38.nnue",
        sha256="8793de8d2557bba3586265628a4c29b6c443137a601f147120419512b8c95411",
    ),
]


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(CHUNK_SIZE), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(asset: NetworkAsset, destination: Path) -> None:
    url = DOWNLOAD_URL.format(repo=REPO, tag=asset.release, asset=asset.asset_name)
    print(f"downloading {url}")
    print(f"          -> {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_name(destination.name + ".part")
    try:
        with urllib.request.urlopen(url) as response, partial.open("wb") as handle:
            while True:
                chunk = response.read(CHUNK_SIZE)
                if not chunk:
                    break
                handle.write(chunk)
    except BaseException:
        partial.unlink(missing_ok=True)
        raise
    actual = sha256_of(partial)
    if actual != asset.sha256:
        partial.unlink(missing_ok=True)
        raise RuntimeError(
            f"{asset.asset_name} from release {asset.release} has SHA-256 {actual}, "
            f"expected {asset.sha256}"
        )
    partial.replace(destination)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--force",
        action="store_true",
        help="re-download even when a verified copy already exists",
    )
    args = parser.parse_args()

    failures = 0
    for asset in NETWORK_ASSETS:
        destination = REPO_ROOT / asset.relative_path
        if destination.exists() and not args.force:
            if sha256_of(destination) == asset.sha256:
                print(f"already verified: {asset.relative_path}")
                continue
            print(f"hash mismatch, re-downloading: {asset.relative_path}")
        try:
            download(asset, destination)
        except (urllib.error.URLError, RuntimeError, OSError) as error:
            print(f"error: {asset.asset_name}: {error}", file=sys.stderr)
            failures += 1
        else:
            print(f"verified: {asset.relative_path}")

    if failures:
        print(f"{failures} asset(s) failed; network tests stay skipped", file=sys.stderr)
        return 1
    print("networks/ is ready; archived-network tests are enabled")
    return 0


if __name__ == "__main__":
    sys.exit(main())
