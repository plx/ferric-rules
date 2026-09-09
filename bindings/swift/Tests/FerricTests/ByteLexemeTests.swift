import Foundation
import Testing

@testable import Ferric

@Suite
struct ByteLexemeTests {
  @Test
  func typedBytesAndSnapshotsAreLossless() async throws {
    let engine = try await Engine.create()
    let bytes = Data([0x61, 0x00, 0xff])
    let fields: [Value] = [.stringBytes(bytes), .symbolBytes(bytes), .instanceName(bytes)]
    let id = try await engine.assertFact("bytes", fields: fields)
    #expect(try await engine.facts() == [.ordered(id: id, relation: "bytes", fields: fields)])
    let snapshot = try await engine.snapshot()
    let restored = try await Engine.restore(snapshot)
    let facts = try await restored.facts()
    guard let first = facts.first, case .ordered(_, "bytes", let actual) = first else {
      Issue.record("missing restored byte fact")
      return
    }
    #expect(actual == fields)
    try await engine.close()
    try await restored.close()
  }

  @Test
  func rawOutputSurvivesCheckedTextFailureAndClose() async throws {
    let engine = try await Engine.create()
    try await engine.load("(defrule emit (bytes ?x) => (printout t ?x))")
    try await engine.reset()
    let bytes = Data([0x61, 0x00, 0xff])
    _ = try await engine.assertFact("bytes", fields: [.stringBytes(bytes)])
    #expect(try await engine.run().rulesFired == 1)
    let output = try await engine.outputBytes()
    await #expect(throws: EngineError.self) { try await engine.output() }
    try await engine.close()
    #expect(output == bytes)
  }

  @Test(arguments: ["type", "named"])
  func missingInstanceLookupPreservesTypedName(operation: String) async throws {
    let engine = try await Engine.create()
    try await engine.load(
      """
      (defgeneric named)
      (defmethod named ((?x INSTANCE-NAME)) unreachable)
      (defrule check (name ?x) =>
       (printout t (instance-namep ?x) crlf) (assert (before ?x))
       (\(operation) ?x) (assert (after)))
      """)
    try await engine.reset()
    let name = Value.instanceName(Data("missing".utf8))
    _ = try await engine.assertFact("name", fields: [name])
    let result = try await engine.run()
    #expect(result.haltReason == .actionError)
    #expect(result.rulesFired == 1)
    #expect(!result.diagnostics.isEmpty)
    #expect(try await engine.output() == "TRUE\n")
    let facts = try await engine.facts()
    let before = facts.compactMap { fact -> [Value]? in
      if case .ordered(_, "before", let fields) = fact { return fields }
      return nil
    }
    #expect(before == [[name]])
    #expect(
      !facts.contains { fact in
        if case .ordered(_, "after", _) = fact { return true }
        return false
      })
    try await engine.close()
  }

}
