"""Fetches raw mainnet blocks from mempool.space and writes one transaction hex per line.

The output is the input format `pqweight aggregate` reads. Blocks are immutable, so
the block hashes alone reproduce a sample exactly: commit the hashes, not the hex.

Splitting a block means walking each transaction's length (compact sizes, the segwit
marker and flag, witnesses). This script is not tested and not run in CI. A wrong
split fails loudly: the misaligned line no longer parses, `pqweight aggregate`
reports it as a parse error, and this script also checks that the transactions
consume the whole block.

Usage:
    python scripts/fetch-blocks.py <block hash>... > sample.txt
"""

import sys
import urllib.request

API = "https://mempool.space/api"
HEADER_SIZE = 80


class Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.offset = 0

    def take(self, n: int) -> bytes:
        if self.offset + n > len(self.data):
            raise ValueError(f"block ended at byte {len(self.data)}, needed {n} more from {self.offset}")
        chunk = self.data[self.offset : self.offset + n]
        self.offset += n
        return chunk

    def compact_size(self) -> int:
        first = self.take(1)[0]
        if first < 0xFD:
            return first
        width = {0xFD: 2, 0xFE: 4, 0xFF: 8}[first]
        return int.from_bytes(self.take(width), "little")


def read_transaction(r: Reader) -> bytes:
    """Reads one transaction and returns its raw bytes."""
    start = r.offset
    r.take(4)  # version
    inputs = r.compact_size()
    segwit = inputs == 0
    if segwit:
        flag = r.take(1)[0]
        if flag != 1:
            raise ValueError(f"segwit flag {flag} at byte {r.offset - 1}")
        inputs = r.compact_size()
    for _ in range(inputs):
        r.take(36)  # previous output
        r.take(r.compact_size())  # scriptSig
        r.take(4)  # sequence
    for _ in range(r.compact_size()):
        r.take(8)  # value
        r.take(r.compact_size())  # scriptPubKey
    if segwit:
        for _ in range(inputs):
            for _ in range(r.compact_size()):
                r.take(r.compact_size())
    r.take(4)  # locktime
    return r.data[start : r.offset]


def split_block(block: bytes) -> list[bytes]:
    r = Reader(block)
    r.take(HEADER_SIZE)
    transactions = [read_transaction(r) for _ in range(r.compact_size())]
    if r.offset != len(block):
        raise ValueError(f"{len(block) - r.offset} bytes left after the last transaction")
    return transactions


def fetch_block(block_hash: str) -> bytes:
    with urllib.request.urlopen(f"{API}/block/{block_hash}/raw", timeout=60) as response:
        return response.read()


def main(block_hashes: list[str]) -> int:
    if not block_hashes:
        print(__doc__, file=sys.stderr)
        return 2
    for block_hash in block_hashes:
        transactions = split_block(fetch_block(block_hash))
        for tx in transactions:
            sys.stdout.write(tx.hex() + "\n")
        print(f"{block_hash}: {len(transactions)} transactions", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
