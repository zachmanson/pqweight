"""Fetches raw mainnet blocks from mempool.space and writes one block hex per line.

That is the format `bitcoin-cli getblock <hash> 0` prints, and what `pqweight aggregate
--blocks` and `pqweight split-blocks` read. Blocks are immutable, so the block hashes
alone reproduce a sample exactly: commit the hashes, not the hex.

This script only downloads. pqweight verifies each block (merkle root, witness
commitment) and prints the hash it computes from the block's own bytes, so a wrong or
incomplete download is an error there, not a silent difference. Not tested and not run
in CI.

Usage:
    python scripts/fetch-blocks.py <block hash>... > blocks.txt
    pqweight aggregate --scheme ml-dsa-44 --blocks blocks.txt
    pqweight split-blocks blocks.txt > sample.txt   # one transaction hex per line
"""

import sys
import urllib.request

API = "https://mempool.space/api"


def fetch_block(block_hash: str) -> bytes:
    with urllib.request.urlopen(f"{API}/block/{block_hash}/raw", timeout=60) as response:
        return response.read()


def main(block_hashes: list[str]) -> int:
    if not block_hashes:
        print(__doc__, file=sys.stderr)
        return 2
    for block_hash in block_hashes:
        sys.stdout.write(fetch_block(block_hash).hex() + "\n")
        print(f"{block_hash}: fetched", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
