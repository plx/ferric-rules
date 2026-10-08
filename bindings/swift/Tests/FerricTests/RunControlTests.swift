import Dispatch
import Foundation
import Synchronization
import Testing

@testable import Ferric

/// Block only dispatch threads; the test's cooperative executor awaits a signal.
private final class RunGate: Sendable {
  private let arrived = DispatchSemaphore(value: 0)
  private let released = DispatchSemaphore(value: 0)
  private let timedOut = Atomic<Bool>(false)

  func pause() {
    arrived.signal()
    if released.wait(timeout: .now() + 10) == .timedOut {
      timedOut.store(true, ordering: .releasing)
    }
  }

  func signal() { arrived.signal() }
  func open() {
    released.signal()
    if timedOut.exchange(false, ordering: .acquiringAndReleasing) {
      Issue.record("run gate timed out while waiting for the test to release it")
    }
  }

  func wait() async throws {
    let reached = await withCheckedContinuation { continuation in
      DispatchQueue.global().async { [self] in
        continuation.resume(returning: arrived.wait(timeout: .now() + 10) == .success)
      }
    }
    try #require(reached, "run did not reach the expected admission/chunk boundary")
  }
}

@Suite
struct RunControlTests {
  @Test
  func defaultFactoryRetainsItsZeroArgumentFunctionType() async throws {
    let factory: @Sendable () async throws -> Engine = Engine.create
    let engine = try await factory()
    engine.halt()  // Idle halt must not affect the next run.
    #expect(try await engine.run().haltReason == .agendaEmpty)
    try await engine.close()
  }

  // Calls still use unlimited run(). This emergency source ceiling ensures an
  // ignored cancellation fails the exact-64 assertion instead of hanging CI.
  private func loopingEngine(stopAt: Int? = 4096, haltAt: Int? = nil) async throws -> Engine {
    let engine = try await Engine.create()
    let constraint = stopAt.map { "&:(< ?x \($0))" } ?? ""
    let halt = haltAt.map { "(if (= (+ ?x 1) \($0)) then (halt))" } ?? ""
    try await engine.load(
      """
      (deffacts seed (n 0))
      (defrule step ?f <- (n ?x\(constraint)) =>
        (retract ?f) (assert (n (+ ?x 1))) \(halt))
      """
    )
    try await engine.reset()
    return engine
  }

  @Test(.timeLimit(.minutes(1)), arguments: [false, true])
  func cancellationAndHaltStopAnActiveUnboundedRun(useHalt: Bool) async throws {
    let engine = try await loopingEngine()
    let gate = RunGate()
    engine.storage.observeRuns { event in
      if case .chunk(_, let count, _) = event, count == UInt64(nativeRunChunkSize) { gate.pause() }
    }
    let task = Task { try await engine.run() }
    defer { task.cancel(); engine.halt(); gate.open() }
    try await gate.wait()
    if useHalt {
      await Task.detached { engine.halt() }.value
    } else {
      task.cancel()
    }
    gate.open()
    let result = try await task.value
    #expect(result.rulesFired == UInt64(nativeRunChunkSize))
    #expect(result.haltReason == .haltRequested)
    #expect(result.diagnostics.isEmpty)
    #expect(try await engine.isHalted == false)
    engine.storage.observeRuns(nil)
    #expect(try await engine.run(limit: 1).rulesFired == 1)
    try await engine.close()
  }

  @Test(.timeLimit(.minutes(1)))
  func cancellingAQueuedRunDoesNotStopTheActiveRun() async throws {
    let engine = try await loopingEngine()
    let activeGate = RunGate()
    let queued = RunGate()
    let activeToken = Mutex<RunControl?>(nil)
    let waitingToken = Mutex<RunControl?>(nil)
    engine.storage.observeRuns { event in
      switch event {
      case .admitted(let control):
        activeToken.withLock { if $0 == nil { $0 = control } }
      case .queued(let control):
        if let active = activeToken.withLock({ $0 }), active !== control {
          waitingToken.withLock { $0 = control }
          queued.signal()
        }
      case .chunk(_, let count, _) where count == UInt64(nativeRunChunkSize): activeGate.pause()
      default: break
      }
    }
    let active = Task { try await engine.run() }
    defer { active.cancel(); engine.halt(); activeGate.open() }
    try await activeGate.wait()
    let waiting = Task { try await engine.run() }
    defer { waiting.cancel() }
    try await queued.wait()
    waiting.cancel()
    let activeControl = try #require(activeToken.withLock { $0 })
    let waitingControl = try #require(waitingToken.withLock { $0 })
    #expect(activeControl !== waitingControl)
    #expect(!activeControl.isStopped)
    #expect(waitingControl.isStopped)
    engine.halt()
    activeGate.open()
    #expect(try await active.value.rulesFired == UInt64(nativeRunChunkSize))
    let waitingResult = try await waiting.value
    #expect(waitingResult.rulesFired == 0)
    #expect(waitingResult.haltReason == .haltRequested)
    try await engine.close()
  }

  @Test(.timeLimit(.minutes(1)))
  func oldRunTokensCannotCancelTheNextRun() async throws {
    let engine = try await loopingEngine()
    let tokens = Mutex<[RunControl]>([])
    let gate = RunGate()
    engine.storage.observeRuns { event in
      switch event {
      case .admitted(let token): tokens.withLock { $0.append(token) }
      case .chunk(_, let count, _) where count == UInt64(nativeRunChunkSize): gate.pause()
      default: break
      }
    }
    #expect(try await engine.run(limit: 1).haltReason == .limitReached)
    let first = try #require(tokens.withLock { $0.first })
    let task = Task { try await engine.run() }
    defer { task.cancel(); engine.halt(); gate.open() }
    try await gate.wait()
    first.requestStop()
    let current = try #require(tokens.withLock { $0.last })
    #expect(first !== current)
    #expect(!current.isStopped)
    task.cancel()
    gate.open()
    #expect(try await task.value.haltReason == .haltRequested)
    engine.storage.observeRuns(nil)
    #expect(try await engine.run(limit: 1).rulesFired == 1)
    try await engine.close()
  }

  @Test(.timeLimit(.minutes(1)))
  func closeStopsActiveRunAndRejectsQueuedAndLaterRuns() async throws {
    let engine = try await loopingEngine()
    let gate = RunGate()
    let queued = RunGate()
    let closing = RunGate()
    let activeToken = Mutex<RunControl?>(nil)
    engine.storage.observeRuns { event in
      switch event {
      case .admitted(let control):
        activeToken.withLock { if $0 == nil { $0 = control } }
      case .queued(let control):
        if let active = activeToken.withLock({ $0 }), active !== control { queued.signal() }
      case .chunk(_, let count, _) where count == UInt64(nativeRunChunkSize): gate.pause()
      case .closing: closing.signal()
      default: break
      }
    }
    let active = Task { try await engine.run() }
    defer { active.cancel(); engine.halt(); gate.open() }
    try await gate.wait()
    let waiting = Task { try await engine.run() }
    defer { waiting.cancel() }
    try await queued.wait()
    let closer = Task { try await engine.close() }
    try await closing.wait()
    closer.cancel()  // Cancelling the waiter must not cancel destruction.
    let later = Task { try await engine.run() }
    defer { later.cancel() }
    gate.open()
    #expect(try await active.value.haltReason == .haltRequested)
    await #expect(throws: EngineError.closed) { try await waiting.value }
    await #expect(throws: EngineError.closed) { try await later.value }
    try await closer.value
    try await engine.close()
    engine.halt()  // Closed engines accept an idle no-op request.
    await #expect(throws: EngineError.closed) { try await engine.facts() }
  }

  @Test(.timeLimit(.minutes(1)), arguments: [63, 64, 65], [false, true])
  func sourceHaltAtInternalBoundariesIsNotCleared(haltAt: Int, bounded: Bool) async throws {
    let engine = try await loopingEngine(stopAt: haltAt + 2, haltAt: haltAt)
    let limit: Int? = bounded ? haltAt + 10 : nil
    let result = try await engine.run(limit: limit)
    #expect(result.rulesFired == UInt64(haltAt))
    #expect(result.haltReason == .haltRequested)
    #expect(try await engine.isHalted)
    let resumed = try await engine.run()
    #expect(resumed.rulesFired == 2)
    #expect(resumed.haltReason == .agendaEmpty)
    try await engine.close()
  }

  @Test(.timeLimit(.minutes(1)))
  func explicitLimitWinsAtACombinedHaltAndCancellationBoundary() async throws {
    let engine = try await loopingEngine(stopAt: 66, haltAt: 64)
    let gate = RunGate()
    engine.storage.observeRuns { event in
      if case .chunk(_, 64, _) = event { gate.pause() }
    }
    let task = Task { try await engine.run(limit: 64) }
    defer { task.cancel(); engine.halt(); gate.open() }
    try await gate.wait()
    task.cancel()
    gate.open()
    let result = try await task.value
    #expect(result.rulesFired == 64)
    #expect(result.haltReason == .limitReached)
    #expect(try await engine.isHalted)
    engine.storage.observeRuns(nil)
    #expect(try await engine.run().rulesFired == 2)
    try await engine.close()
  }

  @Test(.timeLimit(.minutes(1)), arguments: [HaltReason.agendaEmpty, .actionError, .haltRequested])
  func nativeTerminalReasonWinsOverCancellation(expected: HaltReason) async throws {
    let engine: Engine
    if expected == .actionError {
      engine = try await Engine.create()
      try await engine.load("(defrule fail => (bind ?x (/ 1 0)))")
      try await engine.reset()
    } else {
      engine = try await loopingEngine(stopAt: 1, haltAt: expected == .haltRequested ? 1 : nil)
    }
    let gate = RunGate()
    engine.storage.observeRuns { event in
      if case .chunk(_, _, let reason) = event, reason != .limitReached { gate.pause() }
    }
    let task = Task { try await engine.run() }
    defer { task.cancel(); engine.halt(); gate.open() }
    try await gate.wait()
    task.cancel()
    gate.open()
    let result = try await task.value
    #expect(result.rulesFired == 1)
    #expect(result.haltReason == expected)
    #expect(result.diagnostics.isEmpty == (expected != .actionError))
    engine.storage.observeRuns(nil)
    engine.halt()  // Finished-run cleanup leaves no token to poison the next run.
    #expect(try await engine.run(limit: 0).haltReason == .limitReached)
    try await engine.close()
  }

  @Test(.timeLimit(.minutes(1)), arguments: [0, 1])
  func preCancelledCallsStillStartFreshAndZeroLimitWins(limit: Int) async throws {
    let engine = try await loopingEngine(stopAt: 2, haltAt: 1)
    #expect(try await engine.run().haltReason == .haltRequested)
    #expect(try await engine.isHalted)
    let task = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      return try await engine.run(limit: limit)
    }
    let result = try await task.value
    #expect(result.rulesFired == 0)
    #expect(result.haltReason == (limit == 0 ? .limitReached : .haltRequested))
    #expect(try await engine.isHalted == false)
    #expect(try await engine.run().rulesFired == 1)
    try await engine.close()
  }

  @Test(.timeLimit(.minutes(1)))
  func diagnosticsSurviveInternalContinuationAndOrdinaryOperationsWait() async throws {
    let engine = try await loopingEngine(stopAt: 65)
    try await engine.load(
      "(defrule invalid (n 64) (test (> (/ 1 0) 0)) => (assert (unreachable)))"
    )
    let gate = RunGate()
    engine.storage.observeRuns { event in
      if case .chunk(_, 64, _) = event { gate.pause() }
    }
    let task = Task { try await engine.run() }
    defer { task.cancel(); engine.halt(); gate.open() }
    try await gate.wait()
    let completed = Mutex(false)
    let ordinary = Task {
      let facts = try await engine.facts()
      completed.withLock { $0 = true }
      return facts
    }
    #expect(!completed.withLock { $0 })
    gate.open()
    let result = try await task.value
    #expect(result.rulesFired == 65)
    #expect(result.haltReason == .agendaEmpty)
    #expect(result.diagnostics.count == 1)
    #expect(result.diagnostics.first?.contains("zero") == true)
    let facts = try await ordinary.value
    #expect(facts.contains { if case .ordered(_, "n", [.integer(65)]) = $0 { true } else { false } })
    try await engine.close()
    #expect(result.diagnostics.count == 1)
  }
}
