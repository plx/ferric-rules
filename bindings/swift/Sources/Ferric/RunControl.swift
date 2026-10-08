import CFerric
import Synchronization

/// A sticky request belonging to exactly one logical run, including queue wait.
final class RunControl: Sendable {
  private let stopped = Atomic<Bool>(false)

  var isStopped: Bool { stopped.load(ordering: .acquiring) }

  func requestStop() { stopped.store(true, ordering: .releasing) }
}

/// Internal, per-engine observation points used to synchronize lifecycle tests.
/// Observers receive owned state only, never a native handle.
enum RunEvent: Sendable {
  case queued(RunControl)
  case admitted(RunControl)
  case chunk(RunControl, rulesFired: UInt64, reason: HaltReason)
  case finished(RunControl)
  case closing
}

typealias RunObserver = @Sendable (RunEvent) -> Void

struct RunAdmissionState {
  var closing = false
  var active: RunControl?
  var observer: RunObserver?
}

// An implementation boundary, not a public execution limit. The entire logical
// run remains one serialized operation; no other native mutation can interleave.
let nativeRunChunkSize = 64

func executeNativeRun(
  handle: OpaquePointer,
  limit: Int?,
  control: RunControl,
  observeChunk: (@Sendable (UInt64, HaltReason) -> Void)? = nil
) throws -> RunResult {
  var count: UInt64 = 0
  var nativeReason = FERRIC_HALT_REASON_AGENDA_EMPTY
  try check(ferric_engine_run_ex(handle, 0, &count, &nativeReason), handle: handle)
  var reason = try decodeHaltReason(nativeReason)
  var total: UInt64 = count
  var remaining = limit

  // Even a pre-cancelled zero-limit call starts a fresh native logical run.
  while remaining != 0 && reason == .limitReached {
    if control.isStopped {
      reason = .haltRequested
      break
    }
    let chunk = min(remaining ?? nativeRunChunkSize, nativeRunChunkSize)
    try check(
      ferric_engine_continue_run_ex(handle, Int64(chunk), &count, &nativeReason), handle: handle
    )
    reason = try decodeHaltReason(nativeReason)
    let (nextTotal, overflow) = total.addingReportingOverflow(count)
    guard !overflow, count <= UInt64(chunk) else {
      throw EngineError.unsupportedValue("native rule firing count exceeds its execution limit")
    }
    total = nextTotal
    if let previous = remaining { remaining = previous - Int(count) }
    observeChunk?(total, reason)

    // A completed native run, or the caller's explicit finite limit, wins over
    // cancellation arriving during that chunk. Internal limits do not end a run.
    if reason != .limitReached || remaining == 0 { break }
    var halted: Int32 = 0
    try check(ferric_engine_is_halted(handle, &halted), handle: handle)
    if halted != 0 {
      reason = .haltRequested
      break
    }
  }

  return RunResult(
    rulesFired: total, haltReason: reason, diagnostics: try nativeActionDiagnostics(handle: handle)
  )
}

private func decodeHaltReason(_ reason: FerricHaltReason) throws -> HaltReason {
  switch reason {
  case FERRIC_HALT_REASON_AGENDA_EMPTY: .agendaEmpty
  case FERRIC_HALT_REASON_LIMIT_REACHED: .limitReached
  case FERRIC_HALT_REASON_HALT_REQUESTED: .haltRequested
  case FERRIC_HALT_REASON_ACTION_ERROR: .actionError
  default: throw EngineError.unsupportedValue("unknown native halt reason")
  }
}
