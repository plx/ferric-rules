import Dispatch
import Foundation
import Testing

@testable import Ferric

@Suite
struct EngineTests {
  @Test
  func transfersAcrossIndependentTasksAndOwnsTypedValues() async throws {
    let engine = try await Task.detached { try await Engine.create() }.value
    let values: [Value] = [
      .integer(.max), .integer(.min), .float(1.25), .symbol("symbol"), .string("résumé 🦀"),
      .string(""),
      .multifield([.integer(7), .multifield([.string("nested")])]), .void,
    ]
    let id = try await Task.detached { try await engine.assertFact("typed", fields: values) }.value
    let facts = try await Task.detached { try await engine.facts() }.value
    #expect(facts == [.ordered(id: id, relation: "typed", fields: values)])
    try await Task.detached { try await engine.close() }.value
    #expect(facts == [.ordered(id: id, relation: "typed", fields: values)])
    await #expect(throws: EngineError.closed) { try await engine.reset() }
  }

  @Test
  func actionFailureDiagnosticsRemainOwnedAfterClose() async throws {
    let engine = try await Engine.create()
    try await engine.load(
      "(deffacts one (item 1)) (defrule fail (item 1) => (bind ?x (/ 1 0)) (assert (unreachable)))"
    )
    try await engine.reset()
    let result = try await engine.run()
    #expect(result.haltReason == .actionError)
    #expect(result.rulesFired == 1)
    #expect(try await engine.facts().count == 1)
    // A later run clears native diagnostics, but cannot alter this result.
    #expect(try await engine.run().diagnostics.isEmpty)
    try await engine.close()
    #expect(result.diagnostics.count == 1)
    #expect(result.diagnostics[0].contains("zero"))
  }

  @Test
  func copiedOutputAndErrorsSurviveResetAndClose() async throws {
    let engine = try await Engine.create()
    try await engine.load(
      "(deffacts one (item 1)) (defrule emit (item 1) => (printout t \"hello\" crlf))"
    )
    try await engine.reset()
    #expect(try await engine.run().rulesFired == 1)
    let output = try await engine.output()
    let diagnostic: EngineError
    do {
      try await engine.load("(defrule incomplete")
      Issue.record("invalid source accepted")
      try await engine.close()
      return
    } catch let error as EngineError {
      diagnostic = error
    }
    try await engine.reset()
    try await engine.close()
    #expect(output == "hello\n")
    guard case .native(4, let message) = diagnostic else {
      Issue.record("parse error lost its category")
      return
    }
    #expect(message.contains("unclosed"))
  }

  @Test
  func rejectedValueConversionLeavesNoPartialFact() async throws {
    let engine = try await Engine.create()
    var nested = Value.integer(1)
    for _ in 0..<129 { nested = .multifield([nested]) }
    await #expect(throws: EngineError.invalidArgument("multifield nesting exceeds 128 levels")) {
      try await engine.assertFact("rejected", fields: [.string("already allocated"), nested])
    }
    #expect(try await engine.facts().isEmpty)
    _ = try await engine.assertFact("still-usable")
    #expect(try await engine.facts().count == 1)
    try await engine.close()
  }

  @Test
  func templateValuesAndRetraction() async throws {
    let engine = try await Engine.create()
    try await engine.load(
      "(deftemplate item (slot name) (slot count (default 3)) (multislot labels))"
    )
    let id = try await engine.assertTemplate(
      "item",
      slots: ["name": .string("widget"), "labels": .multifield([.symbol("ready"), .integer(7)])]
    )
    let facts = try await engine.facts()
    #expect(
      facts == [
        .template(
          id: id,
          name: "item",
          slots: [
            "name": .string("widget"), "count": .integer(3),
            "labels": .multifield([.symbol("ready"), .integer(7)]),
          ]
        )
      ]
    )
    try await engine.retract(id)
    #expect(try await engine.facts().isEmpty)
    try await engine.close()
  }

  @Test
  func serializesConcurrentCalls() async throws {
    let engine = try await Engine.create()
    let ids = try await withThrowingTaskGroup(of: FactID.self) { group in
      for index in 0..<32 {
        group.addTask { try await engine.assertFact("point", fields: [.integer(Int64(index))]) }
      }
      var ids: [FactID] = []
      for try await id in group { ids.append(id) }
      return ids
    }
    #expect(Set(ids).count == 32)
    #expect(try await engine.facts().count == 32)
    try await engine.close()
  }

  @Test
  func concurrentCloseAndUseHaveOwnedResults() async throws {
    let engine = try await Engine.create()
    await withTaskGroup(of: Void.self) { group in
      for index in 0..<24 {
        group.addTask {
          do { _ = try await engine.assertFact("point", fields: [.integer(Int64(index))]) } catch {
            #expect(error as? EngineError == .closed)
          }
        }
        if index % 4 == 0 {
          group.addTask { do { try await engine.close() } catch { Issue.record(error) } }
        }
      }
    }
    try await engine.close()
    await #expect(throws: EngineError.closed) { try await engine.facts() }
  }

  @Test
  func closeDrainsAnAdmittedOperationWithoutBlockingTasks() async throws {
    let engine = try await Engine.create()
    let entered = DispatchSemaphore(value: 0)
    let release = DispatchSemaphore(value: 0)
    let operation = Task {
      try await engine.storage.perform { state in
        _ = try state.requireHandle()
        entered.signal()
        release.wait()
        return 7
      }
    }
    // Wait on a dispatch thread, never block this cooperative test task.
    await withCheckedContinuation { continuation in
      DispatchQueue.global().async {
        entered.wait()
        continuation.resume()
      }
    }
    let closer = Task { try await engine.close() }
    // This task stays runnable while both admitted work and close are pending.
    await Task.yield()
    release.signal()
    #expect(try await operation.value == 7)
    try await closer.value
    await #expect(throws: EngineError.closed) { try await engine.reset() }
  }

  @Test
  func lastReferenceSchedulesExactlyOnceCleanup() async throws {
    var engine: Engine? = try await Engine.create()
    let storage = engine!.storage
    weak var weakEngine = engine
    _ = try await engine!.assertFact("owned", fields: [.string("until cleanup")])
    engine = nil
    #expect(weakEngine == nil)
    // FIFO observation runs after deinit's cleanup, while retaining only
    // storage to inspect completion. No global counter or polling is needed.
    #expect(try await storage.perform { $0.handle == nil })
    try await storage.perform { try $0.close() }
  }

  @Test
  func snapshotPreservesSubsequentRuleBehaviorAndFactOwnership() async throws {
    let engine = try await Engine.create()
    try await engine.load(
      "(deffacts work (work 1) (work 2) (work 3)) (defrule consume ?f <- (work ?n) => (retract ?f) (assert (done ?n)))"
    )
    try await engine.reset()
    let first = try await engine.run(limit: 1)
    #expect(first.rulesFired == 1)
    #expect(first.haltReason == .limitReached)
    let originalID = try await engine.facts()[0].id
    let bytes = try await engine.snapshot()
    try await engine.close()
    let restored = try await Engine.restore(bytes)
    do {
      try await restored.retract(originalID)
      Issue.record("cross-engine identity accepted")
    } catch {
      #expect(
        error as? EngineError == .invalidArgument("fact identity belongs to a different engine")
      )
    }
    let resumed = try await restored.run()
    #expect(resumed.rulesFired == 2)
    #expect(resumed.haltReason == .agendaEmpty)
    let facts = try await restored.facts()
    #expect(facts.count == 3)
    #expect(facts.allSatisfy { if case .ordered(_, "done", _) = $0 { true } else { false } })
    try await restored.close()
  }

  @Test
  func launchSelectionAndSnapshotResumeUseSameRuleSource() async throws {
    let source = try String(
      contentsOf: Bundle.module.url(
        forResource: "launch",
        withExtension: "clp",
        subdirectory: "Fixtures"
      )!,
      encoding: .utf8
    )
    let engine = try await Engine.create()
    try await engine.load(source)
    try await engine.reset()
    let bytes = try await engine.snapshot()
    let restored = try await Engine.restore(bytes)
    for current in [engine, restored] {
      let result = try await current.run()
      #expect(result.rulesFired == 1)
      #expect(try await current.output() == "action session-42 sign-in\n")
      let actions = try await current.facts().filter {
        if case .ordered(_, "action", _) = $0 { true } else { false }
      }
      #expect(actions.count == 1)
      if case .ordered(_, _, let fields) = actions[0] {
        #expect(fields == [.symbol("session-42"), .symbol("sign-in")])
      }
      #expect(try await current.run().rulesFired == 0)
      try await current.close()
    }
  }

  @Test
  func usefulErrorsAndEmbeddedNULPolicy() async throws {
    let engine = try await Engine.create()
    do {
      try await engine.load("(defrule incomplete")
      Issue.record("invalid source accepted")
    } catch {
      guard case .native(let code, let message) = error as? EngineError else { throw error }
      #expect(code == 4)
      #expect(message.contains("unclosed"))
    }
    await #expect(throws: EngineError.invalidArgument("C string input contains an embedded NUL")) {
      try await engine.load("(assert (x))\0ignored")
    }
    do {
      _ = try await engine.assertFact("value", fields: [.string("a\0b")])
      Issue.record("NUL value accepted")
    } catch {
      guard case .native(let code, let message) = error as? EngineError else { throw error }
      #expect(code == 9)
      #expect(message.contains("NUL"))
    }
    do {
      _ = try await Engine.restore(Data([0xff]))
      Issue.record("corrupt snapshot accepted")
    } catch {
      guard case .native(let code, let message) = error as? EngineError else { throw error }
      #expect(code == 10)
      #expect(!message.isEmpty)
    }
    try await engine.close()
  }
}
