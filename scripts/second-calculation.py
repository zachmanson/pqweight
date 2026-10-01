"""The Second calculation: rebuilds every mapped input's migrated bytes and checks pqweight's numbers.

For each transaction, `pqweight migrate --json-lines` gives each input's Baseline spend
type (and Multisig threshold). This script parses the raw transaction itself, builds the
migrated scriptSig and witness for that spend type, serializes the migrated transaction
and measures it. It compares:

- every mapped input's `template_weight`;
- for fully mapped transactions, `migrated.weight`, `vsize`, `stripped_size` and
  `total_size`.

It is written from `docs/migration-templates.md` only, not from `migration.rs`, so a
misreading in the Rust can't be copied across. When the two disagree, that doc decides
which side is wrong. Agreement shows the arithmetic matches the stated templates, not
that the templates are right, so this is never an Oracle (ticket 21).

Modes:

- `--fixtures <dir>`: every `*.hex` in the directory. Also fails if a template this
  script implements was compared zero times, except those in ALLOWED_UNCOMPARED. CI runs
  this on the Fixtures.
- `<sample file>`: one transaction hex per line, as `pqweight aggregate` reads (from
  `scripts/fetch-blocks.py`). Counts are reported but zero counts don't fail.

In `--fixtures` mode it also checks Move cost (ticket 22, "Move layouts" in the same doc)
on every snapshot Fixture in `<dir>/snapshot/`: it classifies each coin from the Oracle's
full scriptPubKey (not from the snapshot's compressed form pqweight reads), builds the
1-input, 1-output move transaction, measures it, and compares every row's coin count,
value, floor and ceiling against `pqweight move-cost --json`, all coins and at least 546
sats. Fails if any Exposed coin kind has no coin in the Fixtures.

Exits non-zero on any mismatch.

Usage:
    python scripts/second-calculation.py <pqweight binary> --fixtures <dir>
    python scripts/second-calculation.py <pqweight binary> <sample file>
"""

import hashlib
import json
import subprocess
import sys
from pathlib import Path

# Parameter set -> (PQ signature bytes, PQ public key bytes). docs/migration-templates.md, "Sizes".
PARAMETER_SETS = {
    "ml-dsa-44": (2420, 1312),
    "falcon-512": (666, 897),
    "slh-dsa-128s": (7856, 32),
}

OP_0, OP_PUSHDATA1, OP_PUSHDATA2, OP_PUSHDATA4 = 0x00, 0x4C, 0x4D, 0x4E
OP_CHECKSIG, OP_CHECKSIGVERIFY, OP_CHECKMULTISIG = 0xAC, 0xAD, 0xAE

# No-op template with no arithmetic to catch; no Fixture records one (ticket 21, decision 9).
ALLOWED_UNCOMPARED = {"pay-to-anchor"}


# --- Parsing -------------------------------------------------------------------------


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


def parse_transaction(raw: bytes) -> dict:
    r = Reader(raw)
    version = r.take(4)
    count = r.compact_size()
    segwit = count == 0
    if segwit:
        if r.take(1) != b"\x01":
            raise ValueError("segwit flag is not 01")
        count = r.compact_size()
    inputs = []
    for _ in range(count):
        outpoint = r.take(36)
        script_sig = r.take(r.compact_size())
        sequence = r.take(4)
        inputs.append({"outpoint": outpoint, "script_sig": script_sig, "sequence": sequence, "witness": []})
    outputs = []
    for _ in range(r.compact_size()):
        value = r.take(8)
        script_pubkey = r.take(r.compact_size())
        outputs.append(value + compact_size(len(script_pubkey)) + script_pubkey)
    if segwit:
        for item in inputs:
            item["witness"] = [r.take(r.compact_size()) for _ in range(r.compact_size())]
    locktime = r.take(4)
    if r.offset != len(raw):
        raise ValueError(f"{len(raw) - r.offset} bytes left after the locktime")
    return {"version": version, "inputs": inputs, "outputs": outputs, "locktime": locktime}


def script_ops(script: bytes) -> list[tuple[int, bytes, bytes | None]]:
    """Splits a script into (opcode, raw bytes of the op, pushed data or None)."""
    ops = []
    i = 0
    while i < len(script):
        start = i
        op = script[i]
        i += 1
        data = None
        if OP_0 < op <= OP_PUSHDATA4:
            if op < OP_PUSHDATA1:
                n = op
            else:
                width = {OP_PUSHDATA1: 1, OP_PUSHDATA2: 2, OP_PUSHDATA4: 4}[op]
                n = int.from_bytes(script[i : i + width], "little")
                i += width
            if i + n > len(script):
                raise ValueError("truncated push")
            data = script[i : i + n]
            i += n
        ops.append((op, script[start:i], data))
    return ops


# --- Encoding (docs/migration-templates.md, "Encoding rules") ------------------------


def compact_size(n: int) -> bytes:
    if n < 0xFD:
        return bytes([n])
    if n <= 0xFFFF:
        return b"\xfd" + n.to_bytes(2, "little")
    return b"\xfe" + n.to_bytes(4, "little")


def push(data: bytes) -> bytes:
    """The shortest push of `data` inside a script."""
    n = len(data)
    if 1 <= n <= 75:
        return bytes([n]) + data
    if n <= 0xFF:
        return bytes([OP_PUSHDATA1, n]) + data
    if n <= 0xFFFF:
        return bytes([OP_PUSHDATA2]) + n.to_bytes(2, "little") + data
    raise ValueError(f"push of {n} bytes is too long")


def serialize_witness(witness: list[bytes]) -> bytes:
    return compact_size(len(witness)) + b"".join(compact_size(len(w)) + w for w in witness)


def serialize(tx: dict, with_witness: bool) -> bytes:
    body = b"".join(
        i["outpoint"] + compact_size(len(i["script_sig"])) + i["script_sig"] + i["sequence"] for i in tx["inputs"]
    )
    outputs = compact_size(len(tx["outputs"])) + b"".join(tx["outputs"])
    if not with_witness:
        return tx["version"] + compact_size(len(tx["inputs"])) + body + outputs + tx["locktime"]
    witnesses = b"".join(serialize_witness(i["witness"]) for i in tx["inputs"])
    return tx["version"] + b"\x00\x01" + compact_size(len(tx["inputs"])) + body + outputs + witnesses + tx["locktime"]


def has_witness(tx: dict) -> bool:
    """docs/migration-templates.md, "Marker and flag": segwit iff some input has a witness item."""
    return any(i["witness"] for i in tx["inputs"])


def input_weight(item: dict, segwit: bool) -> int:
    script_sig = item["script_sig"]
    non_witness = 36 + len(compact_size(len(script_sig))) + len(script_sig) + 4
    return 4 * non_witness + (len(serialize_witness(item["witness"])) if segwit else 0)


# --- Templates (docs/migration-templates.md, "Templates") ----------------------------


def is_key_shaped(item: bytes) -> bool:
    return (len(item) == 33 and item[0] in (0x02, 0x03)) or (len(item) == 65 and item[0] == 0x04)


def is_strict_der(item: bytes) -> bool:
    """`30 L 02 Lr <r> 02 Ls <s> <sighash>`, 9 to 73 bytes, with every length consistent."""
    if not 9 <= len(item) <= 73 or item[0] != 0x30 or item[1] != len(item) - 3:
        return False
    if item[2] != 0x02:
        return False
    r_len = item[3]
    if r_len < 1 or 6 + r_len > len(item):  # room for r, the 02 tag and Ls
        return False
    if item[4 + r_len] != 0x02:
        return False
    s_len = item[5 + r_len]
    return s_len >= 1 and item[1] == r_len + s_len + 4


def migrate_multisig(script: bytes, m: int, n: int, sig: bytes, pk: bytes) -> list[bytes]:
    """Witness `[<empty>, SIG × m, script']`; OP_m and OP_n keep their original encoding."""
    ops = script_ops(script)
    if len(ops) != n + 3 or ops[-1][0] != OP_CHECKMULTISIG:
        raise ValueError(f"script is not OP_m <{n} keys> OP_n OP_CHECKMULTISIG")
    new_script = ops[0][1] + push(pk) * n + ops[-2][1] + bytes([OP_CHECKMULTISIG])
    return [b""] + [sig] * m + [new_script]


def migrate_contract(witness: list[bytes], sig: bytes, pk: bytes) -> list[bytes]:
    *stack, script = witness
    new_script = b"".join(
        push(pk) if 0x01 <= op <= 0x4B and is_key_shaped(data) else raw for op, raw, data in script_ops(script)
    )
    new_stack = [pk if is_key_shaped(item) else sig if is_strict_der(item) else item for item in stack]
    return new_stack + [new_script]


def migrate_single_key_leaf(witness: list[bytes], sig: bytes, pk: bytes) -> list[bytes]:
    *stack, leaf, control_block = witness
    signature_items = [i for i, item in enumerate(stack) if len(item) in (64, 65)]
    if len(signature_items) != 1:
        raise ValueError(f"{len(signature_items)} stack items of 64 or 65 bytes, expected 1")
    new_stack = [sig if i == signature_items[0] else item for i, item in enumerate(stack)]

    ops = script_ops(leaf)
    checks = [i for i, (op, _, _) in enumerate(ops) if op in (OP_CHECKSIG, OP_CHECKSIGVERIFY)]
    if len(checks) != 1 or checks[0] == 0 or ops[checks[0] - 1][2] is None or len(ops[checks[0] - 1][2]) != 32:
        raise ValueError("leaf is not a single-key leaf")
    key_index = checks[0] - 1
    new_leaf = b"".join(push(pk) if i == key_index else raw for i, (_, raw, _) in enumerate(ops))

    if (len(control_block) - 33) % 32 != 0:
        raise ValueError(f"control block of {len(control_block)} bytes")
    new_control_block = control_block[:1] + control_block[33:]
    return new_stack + [new_leaf, new_control_block]


def script_sig_pushes(script_sig: bytes) -> list[bytes]:
    return [data if data is not None else b"" for _, _, data in script_ops(script_sig)]


def multisig_threshold(spend: dict) -> tuple[int, int]:
    return spend["threshold"]["m"], spend["threshold"]["n"]


# Baseline spend type label (as pqweight prints it) -> builder taking the input's
# (scriptSig, witness), its pqweight result and the PQ signature and key, and returning
# the migrated (scriptSig, witness).
TEMPLATES = {
    "coinbase": lambda script_sig, witness, spend, sig, pk: (script_sig, witness),
    "pay-to-anchor": lambda script_sig, witness, spend, sig, pk: (script_sig, witness),
    "P2WPKH": lambda script_sig, witness, spend, sig, pk: (script_sig, [sig, pk]),
    "P2SH-P2WPKH": lambda script_sig, witness, spend, sig, pk: (script_sig, [sig, pk]),
    "P2TR key-path": lambda script_sig, witness, spend, sig, pk: (script_sig, [sig, pk]),
    "P2PKH": lambda script_sig, witness, spend, sig, pk: (b"", [sig, pk]),
    "P2WSH multisig": lambda script_sig, witness, spend, sig, pk: (
        script_sig,
        migrate_multisig(witness[-1], *multisig_threshold(spend), sig, pk),
    ),
    "P2SH-P2WSH multisig": lambda script_sig, witness, spend, sig, pk: (
        script_sig,
        migrate_multisig(witness[-1], *multisig_threshold(spend), sig, pk),
    ),
    "P2SH multisig": lambda script_sig, witness, spend, sig, pk: (
        b"",
        migrate_multisig(script_sig_pushes(script_sig)[-1], *multisig_threshold(spend), sig, pk),
    ),
    "P2TR script-path single-key": lambda script_sig, witness, spend, sig, pk: (
        script_sig,
        migrate_single_key_leaf(witness, sig, pk),
    ),
    "P2WSH contract": lambda script_sig, witness, spend, sig, pk: (script_sig, migrate_contract(witness, sig, pk)),
    "P2SH-P2WSH contract": lambda script_sig, witness, spend, sig, pk: (
        script_sig,
        migrate_contract(witness, sig, pk),
    ),
}


def migrate_input(tx_input: dict, spend: dict, sig: bytes, pk: bytes) -> dict:
    """The input's migrated scriptSig and witness for its Baseline spend type."""
    template = TEMPLATES.get(spend["spend_type"])
    if template is None:
        raise ValueError(f"no template for spend type {spend['spend_type']!r}")
    script_sig, witness = template(tx_input["script_sig"], tx_input["witness"], spend, sig, pk)
    return {**tx_input, "script_sig": script_sig, "witness": witness}


# --- Comparison ----------------------------------------------------------------------


def measure(tx: dict) -> dict:
    segwit = has_witness(tx)
    stripped = len(serialize(tx, with_witness=False))
    total = len(serialize(tx, with_witness=segwit))
    weight = 3 * stripped + total
    return {"weight": weight, "vsize": (weight + 3) // 4, "stripped_size": stripped, "total_size": total}


def txid(raw_tx: dict) -> str:
    return hashlib.sha256(hashlib.sha256(serialize(raw_tx, with_witness=False)).digest()).digest()[::-1].hex()


def check_transaction(label: str, raw: bytes, result: dict, scheme: str, counts: dict, mismatches: list) -> None:
    sig_size, pk_size = PARAMETER_SETS[scheme]
    sig, pk = bytes(sig_size), bytes(pk_size)
    tx = parse_transaction(raw)
    name = f"{label} ({txid(tx)})"
    if len(result["inputs"]) != len(tx["inputs"]):
        mismatches.append(f"{name} {scheme}: pqweight reports {len(result['inputs'])} inputs, the transaction has {len(tx['inputs'])}")
        return

    migrated_inputs = []
    for index, (item, spend) in enumerate(zip(tx["inputs"], result["inputs"])):
        if spend["status"] != "mapped":
            counts[("unmapped", scheme)] = counts.get(("unmapped", scheme), 0) + 1
            migrated_inputs.append(item)
            continue
        try:
            migrated_inputs.append(migrate_input(item, spend, sig, pk))
        except ValueError as err:
            mismatches.append(f"{name} input {index} {spend['spend_type']} {scheme}: template does not apply: {err}")
            return

    migrated = {**tx, "inputs": migrated_inputs}
    segwit = has_witness(migrated)
    for index, (item, spend) in enumerate(zip(migrated_inputs, result["inputs"])):
        if spend["status"] != "mapped":
            continue
        spend_type = spend["spend_type"]
        counts[(spend_type, scheme)] = counts.get((spend_type, scheme), 0) + 1
        ours = input_weight(item, segwit)
        if ours != spend["template_weight"]:
            mismatches.append(
                f"{name} input {index} {spend_type} {scheme}: template_weight pqweight {spend['template_weight']}, second calculation {ours}"
            )

    if all(spend["status"] == "mapped" for spend in result["inputs"]):
        ours = measure(migrated)
        theirs = result["migrated"] or {}
        for field, value in ours.items():
            if theirs.get(field) != value:
                mismatches.append(f"{name} {scheme}: migrated.{field} pqweight {theirs.get(field)}, second calculation {value}")


# --- Move layouts (docs/migration-templates.md, "Move layouts") ----------------------

SECP256K1_P = 2**256 - 2**32 - 977
DUST_LIMIT = 546
MOVE_KINDS = {"P2PK compressed", "P2PK uncompressed", "bare multisig", "P2TR"}


def on_curve(x: int, y: int) -> bool:
    return x < SECP256K1_P and y < SECP256K1_P and (y * y - x**3 - 7) % SECP256K1_P == 0


def script_number(op: tuple) -> int | None:
    opcode, _, data = op
    if 0x51 <= opcode <= 0x60:
        return opcode - 0x50
    if data is not None and len(data) == 1 and 17 <= data[0] <= 0x7F:
        return data[0]
    return None


def exposed_kind(script: bytes) -> tuple[str, tuple[int, int] | None] | None:
    """The Exposed coin kind of a scriptPubKey ("Which coins"), with (m, n) for bare multisig."""
    if len(script) == 35 and script[0] == 33 and script[1] in (2, 3) and script[34] == OP_CHECKSIG:
        return ("P2PK compressed", None)
    if len(script) == 67 and script[0] == 65 and script[1] == 4 and script[66] == OP_CHECKSIG:
        x, y = int.from_bytes(script[2:34], "big"), int.from_bytes(script[34:66], "big")
        return ("P2PK uncompressed", None) if on_curve(x, y) else None
    if len(script) == 34 and script[0] == 0x51 and script[1] == 0x20:
        return ("P2TR", None)
    try:
        ops = script_ops(script)
    except ValueError:
        return None
    if len(ops) < 4 or ops[-1][0] != OP_CHECKMULTISIG:
        return None
    m, n, keys = script_number(ops[0]), script_number(ops[-2]), ops[1:-2]
    direct_keys = all(op[0] <= 0x4B and op[2] is not None and len(op[2]) in (33, 65) for op in keys)
    if m is None or n is None or not (1 <= m <= n <= 20) or len(keys) != n or not direct_keys:
        return None
    return ("bare multisig", (m, n))


DER_72 = bytes([0x30, 0x45, 0x02, 0x20]) + b"\x11" * 32 + bytes([0x02, 0x21, 0x00]) + b"\x22" * 32 + b"\x01"
SCHNORR_64 = b"\x33" * 64
PQ_OUTPUT = (1000).to_bytes(8, "little") + compact_size(34) + bytes([0x53, 0x20]) + b"\x44" * 32


def move_transaction(kind: str, threshold: tuple[int, int] | None) -> dict:
    """The 1-input, 1-output transaction that moves one coin of `kind` into a PQ output."""
    assert len(DER_72) == 72
    if kind in ("P2PK compressed", "P2PK uncompressed"):
        script_sig, witness = push(DER_72), []
    elif kind == "bare multisig":
        script_sig, witness = bytes([OP_0]) + push(DER_72) * threshold[0], []
    else:
        script_sig, witness = b"", [SCHNORR_64]
    tx_input = {"outpoint": b"\x00" * 36, "script_sig": script_sig, "sequence": b"\xff" * 4, "witness": witness}
    return {"version": b"\x02\x00\x00\x00", "inputs": [tx_input], "outputs": [PQ_OUTPUT], "locktime": b"\x00" * 4}


def check_move_cost(binary: str, snapshot: Path, oracle_path: Path, kinds_seen: set, mismatches: list) -> None:
    oracle = json.loads(oracle_path.read_text())["oracle"]
    ours: dict = {}
    for coin in oracle["coins"]:
        kind = exposed_kind(bytes.fromhex(coin["script"]))
        if kind is None:
            continue
        kinds_seen.add(kind[0])
        tx = move_transaction(*kind)
        segwit = has_witness(tx)
        floor = input_weight(tx["inputs"][0], segwit)
        ceiling = measure(tx)["weight"]
        for cut in ("all", "above_dust") if coin["value"] >= DUST_LIMIT else ("all",):
            totals = ours.setdefault((kind, cut), {"coins": 0, "value": 0, "floor_weight": 0, "ceiling_weight": 0})
            totals["coins"] += 1
            totals["value"] += coin["value"]
            totals["floor_weight"] += floor
            totals["ceiling_weight"] += ceiling

    completed = subprocess.run([binary, "move-cost", "--json", str(snapshot)], capture_output=True, text=True, check=True)
    result = json.loads(completed.stdout)
    name = snapshot.name
    if result["scanned"]["coins"] != len(oracle["coins"]):
        mismatches.append(f"{name}: pqweight scanned {result['scanned']['coins']} coins, the Oracle lists {len(oracle['coins'])}")
    theirs: dict = {}
    for row in result["rows"]:
        threshold = (row["threshold"]["m"], row["threshold"]["n"]) if "threshold" in row else None
        for cut in ("all", "above_dust"):
            if row[cut]["coins"]:
                theirs[((row["exposed_type"], threshold), cut)] = {
                    field: row[cut][field] for field in ("coins", "value", "floor_weight", "ceiling_weight")
                }
    for key in sorted(ours.keys() | theirs.keys(), key=str):
        if ours.get(key) != theirs.get(key):
            mismatches.append(f"{name} {key}: pqweight {theirs.get(key)}, second calculation {ours.get(key)}")
    print(f"move cost ({name}): {sum(1 for _, cut in ours if cut == 'all')} rows compared")


def run_pqweight(binary: str, scheme: str, hex_lines: list[str]) -> list[dict]:
    completed = subprocess.run(
        [binary, "migrate", "--scheme", scheme, "--json-lines"],
        input="\n".join(hex_lines) + "\n",
        capture_output=True,
        text=True,
        check=True,
    )
    results = [json.loads(line) for line in completed.stdout.splitlines() if line.strip()]
    if len(results) != len(hex_lines):
        raise RuntimeError(f"pqweight returned {len(results)} lines for {len(hex_lines)} transactions")
    return results


def main(argv: list[str]) -> int:
    if len(argv) == 3 and argv[1] == "--fixtures":
        binary, fixtures = str(Path(argv[0]).resolve()), True
        paths = sorted(Path(argv[2]).glob("*.hex"))
        entries = [(p.stem, p.read_text().strip()) for p in paths]
    elif len(argv) == 2:
        binary, fixtures = str(Path(argv[0]).resolve()), False
        lines = Path(argv[1]).read_text().splitlines()
        entries = [(f"line {n}", line.strip()) for n, line in enumerate(lines, start=1) if line.strip()]
    else:
        print(__doc__, file=sys.stderr)
        return 2

    counts: dict = {}
    mismatches: list[str] = []
    for scheme in PARAMETER_SETS:
        results = run_pqweight(binary, scheme, [hex_ for _, hex_ in entries])
        for (label, hex_), result in zip(entries, results):
            if "error" in result:
                mismatches.append(f"{label} {scheme}: pqweight could not read it: {result['error']}")
                continue
            check_transaction(label, bytes.fromhex(hex_), result, scheme, counts, mismatches)

    spend_types = sorted({key for key, _ in counts} | (set(TEMPLATES) if fixtures else set()))
    width = max(len(s) for s in spend_types)
    print(f"transactions: {len(entries)}")
    print(f"inputs compared (unmapped inputs are skipped):")
    print(f"  {'':<{width}}  " + "  ".join(f"{s:>12}" for s in PARAMETER_SETS))
    for spend_type in spend_types:
        row = "  ".join(f"{counts.get((spend_type, s), 0):>12}" for s in PARAMETER_SETS)
        print(f"  {spend_type:<{width}}  {row}")

    if fixtures:
        kinds_seen: set = set()
        for snapshot in sorted((Path(argv[2]) / "snapshot").glob("*.dat")):
            check_move_cost(binary, snapshot, snapshot.with_suffix(".json"), kinds_seen, mismatches)
        for kind in sorted(MOVE_KINDS - kinds_seen):
            mismatches.append(f"move cost {kind}: no coin in the snapshot Fixtures")
        for spend_type in sorted(TEMPLATES.keys() - ALLOWED_UNCOMPARED):
            for scheme in PARAMETER_SETS:
                if counts.get((spend_type, scheme), 0) == 0:
                    mismatches.append(f"{spend_type} {scheme}: compared zero times across the Fixtures")

    print(f"mismatches: {len(mismatches)}")
    for mismatch in mismatches:
        print(f"- {mismatch}")
    return 1 if mismatches else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
