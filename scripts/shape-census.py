"""Counts the rare spend shapes that decide backlog tickets, in a coverage sample.

Reads the same input as `pqweight aggregate` (one transaction hex per line, from
`scripts/fetch-blocks.py`) and prints one table: for each shape, the number of
inputs and their Input weight, next to the sample's total Input weight.

Shapes counted (ticket 08, decision 5):

- multi-key leaves (ticket 09): Unmapped P2TR script-path inputs whose leaf has
  more than one signature check, split into `multi_a` (any OP_CHECKSIGADD) and
  other multi-key leaves. Every other Unmapped script-path leaf is listed by
  reason too, so the bucket adds up.
- bare P2SH non-multisig (ticket 16): inputs with Unmapped reason P2SH non-multisig.
- script-path stacks with more than one 64 or 65-byte item (ticket 18), counted only
  for leaves with one signature check, where size alone can't pick the signature.
  Leaves with two checks carry two signatures legitimately.
- taproot spends with an annex (ticket 05, decision 6), script-path and key-path.
- P2PK inputs whose spent output is really P2SH (ticket 20): every input pqweight
  files as P2PK is looked up on mempool.space.

Bucket membership (mapped spend type or Unmapped reason) comes from
`pqweight migrate --json`, not from a second classifier here. To keep it to a few
thousand calls, only transactions with a taproot-looking witness or a legacy input
that isn't plain P2PKH are passed to pqweight. Input weight is computed here, so
check the "Unmapped P2TR script-path (all)" row against `aggregate`'s row for the
same sample: equal counts and weights mean the pre-filter and the weight agree.

This script is not tested and not run in CI, like `fetch-blocks.py`.

Usage:
    python scripts/shape-census.py <pqweight binary> <sample file>
"""

import json
import subprocess
import sys
import urllib.request

API = "https://mempool.space/api"

OP_PUSHDATA1, OP_PUSHDATA2, OP_PUSHDATA4 = 0x4C, 0x4D, 0x4E
OP_CHECKSIG, OP_CHECKSIGVERIFY, OP_CHECKSIGADD = 0xAC, 0xAD, 0xBA
SIGNATURE_CHECKS = {OP_CHECKSIG, OP_CHECKSIGVERIFY, OP_CHECKSIGADD}
ANNEX_TAG = 0x50
# Labels pqweight gives to taproot spends; the annex and ticket 18 checks only look at these.
TAPROOT_LABELS = {"P2TR key-path", "P2TR script-path single-key", "P2TR script-path", "P2TR key-path with annex"}


class Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.offset = 0

    def take(self, n: int) -> bytes:
        if self.offset + n > len(self.data):
            raise ValueError("transaction ended early")
        chunk = self.data[self.offset : self.offset + n]
        self.offset += n
        return chunk

    def compact_size(self) -> int:
        first = self.take(1)[0]
        if first < 0xFD:
            return first
        width = {0xFD: 2, 0xFE: 4, 0xFF: 8}[first]
        return int.from_bytes(self.take(width), "little")


def compact_size_len(n: int) -> int:
    return 1 if n < 0xFD else 3 if n <= 0xFFFF else 5 if n <= 0xFFFFFFFF else 9


def parse_transaction(raw: bytes) -> list[dict]:
    """Returns the inputs in order, each with its outpoint, scriptSig, witness and Input weight."""
    r = Reader(raw)
    r.take(4)  # version
    count = r.compact_size()
    segwit = count == 0
    if segwit:
        r.take(1)  # flag
        count = r.compact_size()
    inputs = []
    for _ in range(count):
        outpoint = r.take(36)
        script_sig = r.take(r.compact_size())
        r.take(4)  # sequence
        inputs.append({"outpoint": outpoint, "script_sig": script_sig, "witness": []})
    for _ in range(r.compact_size()):
        r.take(8)  # value
        r.take(r.compact_size())  # scriptPubKey
    if segwit:
        for item in inputs:
            item["witness"] = [r.take(r.compact_size()) for _ in range(r.compact_size())]
    r.take(4)  # locktime
    if r.offset != len(raw):
        raise ValueError(f"{len(raw) - r.offset} bytes left after the locktime")
    for item in inputs:
        non_witness = 36 + compact_size_len(len(item["script_sig"])) + len(item["script_sig"]) + 4
        witness = 0
        if segwit:
            witness = compact_size_len(len(item["witness"])) + sum(
                compact_size_len(len(w)) + len(w) for w in item["witness"]
            )
        item["weight"] = 4 * non_witness + witness
    return inputs


def pushes(script: bytes) -> list[tuple[int, bytes | None]] | None:
    """Splits a script into (opcode, pushed data) pairs; None if a push runs off the end."""
    ops = []
    i = 0
    while i < len(script):
        op = script[i]
        i += 1
        if op <= OP_PUSHDATA4:
            if op < OP_PUSHDATA1:
                n = op
            else:
                width = {OP_PUSHDATA1: 1, OP_PUSHDATA2: 2, OP_PUSHDATA4: 4}[op]
                if i + width > len(script):
                    return None
                n = int.from_bytes(script[i : i + width], "little")
                i += width
            if i + n > len(script):
                return None
            ops.append((op, script[i : i + n]))
            i += n
        else:
            ops.append((op, None))
    return ops


def split_annex(witness: list[bytes]) -> tuple[list[bytes], bool]:
    if len(witness) >= 2 and witness[-1][:1] == bytes([ANNEX_TAG]):
        return witness[:-1], True
    return witness, False


def is_control_block(item: bytes) -> bool:
    return len(item) >= 33 and (len(item) - 33) % 32 == 0 and item[0] & 0xFE == 0xC0


def looks_taproot(witness: list[bytes]) -> bool:
    stack, annex = split_annex(witness)
    return annex or (len(stack) >= 2 and is_control_block(stack[-1]))


def is_plain_p2pkh(script_sig: bytes) -> bool:
    ops = pushes(script_sig)
    return (
        ops is not None
        and len(ops) == 2
        and all(data is not None for _, data in ops)
        and len(ops[1][1]) in (33, 65)
    )


def is_candidate(inputs: list[dict]) -> bool:
    for item in inputs:
        if item["witness"] and looks_taproot(item["witness"]):
            return True
        if not item["witness"] and item["script_sig"] and not is_plain_p2pkh(item["script_sig"]):
            return True
    return False


SINGLE_CHECK_SHAPES = {"single-key leaf", "one signature check, not single-key template"}


def leaf_shape(leaf: bytes) -> str:
    ops = pushes(leaf)
    if ops is None:
        return "unparseable leaf"
    checks = [op for op, _ in ops if op in SIGNATURE_CHECKS]
    if OP_CHECKSIGADD in checks:
        return "multi_a (CHECKSIGADD)"
    if len(checks) >= 2:
        return "other multi-key leaf"
    if len(checks) == 1:
        key = next((data for (_, data), (op, _) in zip(ops, ops[1:]) if op in SIGNATURE_CHECKS), None)
        if key is not None and len(key) == 32:
            return "single-key leaf"
        return "one signature check, not single-key template"
    return "no signature check"


def spent_output_type(outpoint: bytes) -> str:
    txid = outpoint[:32][::-1].hex()
    vout = int.from_bytes(outpoint[32:], "little")
    with urllib.request.urlopen(f"{API}/tx/{txid}", timeout=60) as response:
        return json.load(response)["vout"][vout]["scriptpubkey_type"]


def migrate(binary: str, hex_tx: str) -> list[dict]:
    out = subprocess.run(
        [binary, "migrate", "--scheme", "ml-dsa-44", "--json"],
        input=hex_tx, capture_output=True, text=True, check=True,
    )
    return json.loads(out.stdout)["inputs"]


def main(binary: str, sample_path: str) -> int:
    # Every decision row is printed, so a zero is shown rather than left out.
    counts: dict[str, list[int]] = {
        shape: [0, 0]
        for shape in (
            "Unmapped P2TR script-path (all)",
            "  multi_a (CHECKSIGADD)",
            "  other multi-key leaf",
            "bare P2SH non-multisig",
            "one-check leaf, more than one 64/65-byte stack item",
            "taproot spend with an annex",
            "P2PK, spent output p2sh",
        )
    }

    def add(shape: str, weight: int) -> None:
        row = counts.setdefault(shape, [0, 0])
        row[0] += 1
        row[1] += weight

    total_weight = 0
    total_inputs = 0
    with open(sample_path) as f:
        lines = [line.strip() for line in f if line.strip()]
    for line in lines:
        inputs = parse_transaction(bytes.fromhex(line))
        total_inputs += len(inputs)
        total_weight += sum(item["weight"] for item in inputs)
        if not is_candidate(inputs):
            continue
        results = migrate(binary, line)
        for item, result in zip(inputs, results, strict=True):
            reason = result.get("reason")
            spend_type = result.get("spend_type")
            taproot = (spend_type or reason) in TAPROOT_LABELS
            stack, annex = split_annex(item["witness"]) if taproot else (item["witness"], False)
            script_path = taproot and len(stack) >= 2 and is_control_block(stack[-1])
            if annex:
                add("taproot spend with an annex", item["weight"])
            one_check = script_path and leaf_shape(stack[-2]) in SINGLE_CHECK_SHAPES
            if one_check and sum(1 for w in stack[:-2] if len(w) in (64, 65)) > 1:
                add("one-check leaf, more than one 64/65-byte stack item", item["weight"])
            if reason == "P2TR script-path":
                add("Unmapped P2TR script-path (all)", item["weight"])
                shape = leaf_shape(stack[-2]) if script_path else "annex or no control block"
                add(f"  {shape}", item["weight"])
            elif reason == "P2SH non-multisig":
                add("bare P2SH non-multisig", item["weight"])
            elif reason == "P2PK":
                kind = spent_output_type(item["outpoint"])
                add(f"P2PK, spent output {kind}", item["weight"])
            elif spend_type == "P2TR script-path single-key" and not script_path:
                raise ValueError("single-key template matched without a control block")

    if total_weight == 0:
        print("no transactions in the sample", file=sys.stderr)
        return 1
    print(f"inputs: {total_inputs}")
    print(f"Input weight: {total_weight}")
    for shape, (n, weight) in counts.items():
        print(f"{shape:<60} {n:>7} {weight:>10} {100 * weight / total_weight:>8.3f}%")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    sys.exit(main(sys.argv[1], sys.argv[2]))
