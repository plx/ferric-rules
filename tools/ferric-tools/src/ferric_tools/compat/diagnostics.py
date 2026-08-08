"""Shared diagnostic and process-termination evidence for compatibility runs."""

from __future__ import annotations

DIAGNOSTIC_TAXONOMY_VERSION = 1

DIAGNOSTIC_SCHEMA = "ferric.compat-diagnostic"
ENGINE_DIAGNOSTIC_PHASES = frozenset({"parse", "load", "reset", "run"})
ENGINE_DIAGNOSTIC_CATEGORIES = frozenset({"syntax-error", "construct-error", "evaluation-error"})
SEMANTIC_DIAGNOSTIC_PAIRS = frozenset(
    {
        ("parse", "syntax-error"),
        ("load", "construct-error"),
        ("reset", "evaluation-error"),
        ("run", "evaluation-error"),
    }
)
UNKNOWN_DIAGNOSTIC = "unknown"
PROCESS_DIAGNOSTIC_PHASES = frozenset({"process", "harness"})
PROCESS_DIAGNOSTIC_CATEGORIES = frozenset({"timeout", "signal", "nonzero-exit", "harness-error"})

_VALID_DIAGNOSTIC_PAIRS = frozenset(
    {
        ("none", "none"),
        *SEMANTIC_DIAGNOSTIC_PAIRS,
        ("process", "timeout"),
        ("process", "signal"),
        ("process", "nonzero-exit"),
        ("harness", "harness-error"),
        (UNKNOWN_DIAGNOSTIC, UNKNOWN_DIAGNOSTIC),
    }
)


def diagnostic(
    phase: str,
    category: str,
    *,
    continued: bool,
) -> dict[str, object]:
    """Create one versioned, engine-neutral diagnostic summary."""
    return {
        "schema": DIAGNOSTIC_SCHEMA,
        "version": DIAGNOSTIC_TAXONOMY_VERSION,
        "phase": phase,
        "category": category,
        "continued": continued,
    }


def termination(
    *,
    exit_code: int | None,
    timed_out: bool,
    spawn_error: bool = False,
) -> dict[str, object]:
    """Describe process termination without conflating it with engine diagnostics."""
    if timed_out:
        return {"kind": "timeout", "exit_code": None, "signal": None}
    if spawn_error:
        return {"kind": "spawn-error", "exit_code": None, "signal": None}
    if type(exit_code) is not int:
        return {"kind": "unknown", "exit_code": None, "signal": None}
    if exit_code < 0:
        return {"kind": "signal", "exit_code": None, "signal": -exit_code}
    return {"kind": "exit", "exit_code": exit_code, "signal": None}


def process_diagnostic(result: dict) -> dict[str, object] | None:
    """Return a transport/harness diagnostic for an untrusted observer result."""
    if result.get("not_run") is True:
        return None
    process = result.get("termination")
    if not isinstance(process, dict):
        exit_code = result.get("exit_code")
        if (
            type(exit_code) is int
            and exit_code < 0
            and result.get("timed_out") is not True
            and result.get("spawn_error") is not True
        ):
            # Historical and synthetic manifests used -1 as a generic
            # "not run" sentinel. Only an explicit termination envelope can
            # prove that a negative code represents a process signal.
            process = {"kind": "unknown", "exit_code": None, "signal": None}
        else:
            process = termination(
                exit_code=exit_code,
                timed_out=result.get("timed_out") is True,
                spawn_error=result.get("spawn_error") is True,
            )
    kind = process.get("kind")
    if kind == "timeout":
        return diagnostic("process", "timeout", continued=False)
    if kind == "signal":
        return diagnostic("process", "signal", continued=False)
    if kind == "spawn-error" or result.get("harness_error") is True:
        return diagnostic("harness", "harness-error", continued=False)
    if kind == "exit" and process.get("exit_code") != 0:
        return diagnostic("process", "nonzero-exit", continued=False)
    if result.get("observation_error"):
        return diagnostic("harness", "harness-error", continued=False)
    return None


def validated_result_diagnostic(raw: object) -> dict[str, object] | None:
    """Validate an already-canonical result diagnostic without coercion."""
    if not isinstance(raw, dict):
        return None
    version = raw.get("version")
    phase = raw.get("phase")
    category = raw.get("category")
    continued = raw.get("continued")
    if (
        raw.get("schema") != DIAGNOSTIC_SCHEMA
        or type(version) is not int
        or version != DIAGNOSTIC_TAXONOMY_VERSION
        or type(phase) is not str
        or type(category) is not str
        or type(continued) is not bool
        or (phase, category) not in _VALID_DIAGNOSTIC_PAIRS
    ):
        return None
    if (phase, category) == ("none", "none") and not continued:
        return None
    if phase in {"process", "harness", UNKNOWN_DIAGNOSTIC} and continued:
        return None
    return {
        "schema": DIAGNOSTIC_SCHEMA,
        "version": version,
        "phase": phase,
        "category": category,
        "continued": continued,
    }


def diagnostic_evidence_state(
    result: object,
) -> tuple[str, tuple[str, str, bool] | None]:
    """Distinguish trusted, missing, malformed, and harness diagnostic evidence."""
    if isinstance(result, dict) and result.get("harness_error") is True:
        return "harness", ("harness", "harness-error", False)
    if not isinstance(result, dict) or "diagnostic" not in result:
        return "missing", None
    canonical = validated_result_diagnostic(result.get("diagnostic"))
    if canonical is None:
        return "invalid", None
    state = (
        str(canonical["phase"]),
        str(canonical["category"]),
        bool(canonical["continued"]),
    )
    if state[0] == "harness":
        return "harness", state
    if state[0] == UNKNOWN_DIAGNOSTIC:
        return "invalid", state
    return "valid", state


def result_diagnostic_view(result: object) -> dict[str, object]:
    """Return a defensive report view of one persisted engine result."""
    if not isinstance(result, dict):
        return diagnostic(UNKNOWN_DIAGNOSTIC, UNKNOWN_DIAGNOSTIC, continued=False)
    raw = result.get("diagnostic")
    if isinstance(raw, dict):
        canonical = validated_result_diagnostic(raw)
        if canonical is not None:
            return canonical
        return diagnostic(UNKNOWN_DIAGNOSTIC, UNKNOWN_DIAGNOSTIC, continued=False)
    fallback = process_diagnostic(result)
    if fallback is not None:
        return fallback
    return diagnostic("none", "none", continued=True)
