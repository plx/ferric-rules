# Queued input reference fixtures

These 30 byte-exact CLIPS 6.30 programs accompany [issue #346](https://github.com/plx/ferric-rules/issues/346). Each has a `.clp` source, `.in` input and `.out` reference output (90 payloads). The seven original corpus triples are unchanged. `provenance.json` records every payload hash, the pinned reference identity, command-program hashes and the classification of each case.

| Cases | Coverage |
| --- | --- |
| 23 queued-stdin cases | Exact normal and late-rule output; all five snapshot codecs preserve unread, partially consumed and completed state. |
| 4 router-error cases | Semantic return value, operand effects, diagnostics, halt and input preservation; enclosing CLIPS diagnostic prose is retained as evidence, not asserted as Ferric prose. |
| 1 arity case | Source-load rejection. |
| 2 named-file cases | Exploratory CLIPS controls requiring real file `open`/`close` and a stream cursor; unsupported by Ferric's queued-input API and excluded from its passing fixture matrix. |

The reference receipts record 30 exact successful replays, each with exit status 0 and empty process stderr. Expected diagnostics are often in stdout, so exit status alone does not establish success. This metadata preparation did not rerun the reference or establish a Ferric test result.

The facade test is `../../read_fields.rs`. Its local input adapter frames each CR or LF separately, preserving the measured CRLF distinction; `Engine::push_input` receives already-framed text. The API does not split physical input automatically. `read` discards the rest of its selected line, while `readline` shares the queue. Exact output includes nonfatal scanner notices; tests compare the `t`, `wwarning` and `werror` buffers separately, without asserting a cross-channel ordering API.

Keep the ordinary `.clp`/`.in`/`.out` LF attributes and these later exact exceptions:

```gitattributes
crates/ferric-rules/tests/fixtures/read/crlf-readline-state.in -text
crates/ferric-rules/tests/fixtures/read/unterminated-terminal-backslash.out -text
```

The former contains physical CRLF; the latter contains raw byte FF. Do not normalize or decode/re-encode payloads.

## Reproduce the reference

The pinned image is the recorded local Docker image ID, not a registry pull address. `provenance.json` also identifies the CLIPS version, Debian package, platform, binary/library hashes and official source archive/hash. Make that exact image available first. From this fixture directory, the following Python recipe reproduces the recorded source-command and stdin protocol for any case (the default is the issue original):

```python
from pathlib import Path
import hashlib
import json
import subprocess
import tempfile

fixtures = Path.cwd()
meta = json.loads((fixtures / "provenance.json").read_text())
case = next(row for row in meta["cases"] if row["name"] == "read-string")
for payload in case["payloads"]:
    raw = (fixtures / payload["path"]).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == payload["sha256"]
source = (fixtures / (case["name"] + ".clp")).read_bytes()
stdin = (fixtures / (case["name"] + ".in")).read_bytes()
program = source + b"(reset)\n(run)\n" + case["post_run_commands"].encode() + b"(exit)\n"
assert hashlib.sha256(program).hexdigest() == case["reference"]["program_sha256"]
with tempfile.TemporaryDirectory(prefix="clips-read-reference-") as directory:
    work = Path(directory).resolve()
    program_name = case["name"] + ".input.clp"
    (work / program_name).write_bytes(program)
    (work / (case["name"] + ".in")).write_bytes(stdin)
    result = subprocess.run(
        ["docker", "run", "--rm", "-i", "-v", str(work) + ":/work:ro",
         meta["reference"]["image_id"], "-f2", "/work/" + program_name],
        input=stdin, capture_output=True, timeout=20,
    )
assert result.returncode == 0
assert result.stderr == b""
assert result.stdout == (fixtures / (case["name"] + ".out")).read_bytes()
```

The `.clp` is loaded with `-f2`, never sent on stdin. Four error programs additionally run the exact post-run inspection commands recorded in JSON. For the two exploratory named-file cases, the mounted companion `.in` is essential: their source opens `/work/<case>.in` as logical name `source`. Passing those bytes only on process stdin cannot reproduce the named stream's token retention or multiline quoted string. The recipe stages both inputs to reproduce the reference, without claiming Ferric supports those programs.
