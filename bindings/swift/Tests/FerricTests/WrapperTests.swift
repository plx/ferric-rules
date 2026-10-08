import Testing

@testable import Ferric

@Suite
struct WrapperTests {
  @Test
  func configurationControlsStrategiesAndValidatesDepth() async throws {
    #expect(EngineConfig() == EngineConfig(stringEncoding: .utf8, strategy: .depth, maxCallDepth: 64))
    await #expect(throws: EngineError.invalidArgument("maximum call depth must be nonnegative")) {
      _ = try await Engine.create(config: EngineConfig(maxCallDepth: -1))
    }
    for strategy in [ConflictResolutionStrategy.depth, .breadth, .lex, .mea] {
      let engine = try await Engine.create(config: EngineConfig(strategy: strategy))
      try await engine.load(
        "(deffacts seed (item 1) (item 2) (item 3)) (defrule show (item ?n) => (printout t ?n crlf))"
      )
      try await engine.reset()
      #expect(try await engine.run().rulesFired == 3)
      #expect(try await engine.output() == (strategy == .breadth ? "1\n2\n3\n" : "3\n2\n1\n"))
      try await engine.close()
    }
    let noCalls = try await Engine.create(config: EngineConfig(maxCallDepth: 0))
    try await noCalls.load("(deffunction f () 7) (defrule invoke => (f))")
    try await noCalls.reset()
    #expect(try await noCalls.run().haltReason == .actionError)
    try await noCalls.close()
    let bounded = try await Engine.create(config: EngineConfig(maxCallDepth: 100))
    try await bounded.load(
      "(deffunction recurse () (recurse)) (defrule invoke => (recurse))"
    )
    try await bounded.reset()
    let result = try await bounded.run()
    #expect(result.haltReason == .actionError)
    #expect(result.diagnostics.contains { $0.contains("32") && $0.contains("depth") }, "\(result.diagnostics)")
    try await bounded.close()
  }

  @Test
  func configurationSelectsSymbolAndStringEncoding() async throws {
    let utf8 = try await Engine.create()
    _ = try await utf8.assertFact("résumé", fields: [.symbol("café"), .string("🦀")])
    try await utf8.close()
    let ascii = try await Engine.create(config: EngineConfig(stringEncoding: .ascii))
    await #expect(throws: EngineError.self) {
      _ = try await ascii.assertFact("text", fields: [.string("café")])
    }
    #expect(try await ascii.facts().isEmpty)
    try await ascii.close()
    let mixed = try await Engine.create(config: EngineConfig(stringEncoding: .asciiSymbolsUTF8Strings))
    let id = try await mixed.assertFact("text", fields: [.string("café 🦀")])
    await #expect(throws: EngineError.self) {
      _ = try await mixed.assertFact("text", fields: [.symbol("café")])
    }
    #expect(try await mixed.findFacts(relation: "text") == [
      .ordered(id: id, relation: "text", fields: [.string("café 🦀")])
    ])
    try await mixed.close()
  }

  @Test
  func stepReportsAllStatusesAndOwnsActionFailures() async throws {
    let engine = try await Engine.create()
    #expect(try await engine.agendaCount == 0)
    #expect(try await engine.step() == .agendaEmpty)
    try await engine.load("(defrule stop => (halt))")
    try await engine.reset()
    #expect(try await engine.agendaCount == 1)
    #expect(try await engine.step() == .fired(diagnostics: []))
    #expect(try await engine.isHalted)
    #expect(try await engine.step() == .halted)
    #expect(try await engine.run(limit: 0).rulesFired == 0)
    #expect(try await !engine.isHalted)
    #expect(try await engine.step() == .agendaEmpty)
    try await engine.clear()
    try await engine.load("(defrule bad => (bind ?x (/ 1 0)))")
    try await engine.reset()
    let failed = try await engine.step()
    guard case .fired(let diagnostics) = failed else {
      Issue.record("action failure was not reported as a fired activation")
      try await engine.close()
      return
    }
    #expect(diagnostics.count == 1)
    #expect(diagnostics[0].contains("zero"))
    #expect(try await engine.step() == .agendaEmpty)
    try await engine.close()
    #expect(failed == .fired(diagnostics: diagnostics))
    await #expect(throws: EngineError.closed) { _ = try await engine.step() }
  }

  @Test
  func stepFiresThroughAPendingHaltWithoutClearingIt() async throws {
    let engine = try await Engine.create()
    try await engine.load("(defrule a (declare (salience 10)) => (halt)) (defrule b =>)")
    try await engine.reset()
    #expect(try await engine.step() == .fired(diagnostics: []))
    #expect(try await engine.isHalted)
    #expect(try await engine.step() == .fired(diagnostics: []))
    #expect(try await engine.isHalted)
    #expect(try await engine.step() == .halted)
    #expect(try await engine.run(limit: 0).rulesFired == 0)
    #expect(try await !engine.isHalted)
    #expect(try await engine.step() == .agendaEmpty)
    try await engine.close()
  }

  @Test
  func globalsAndSlotValuesRemainOwnedAfterClose() async throws {
    let engine = try await Engine.create()
    try await engine.load(
      "(defglobal ?*answer* = 42 ?*many* = (create$ red 7 \"text\")) (deftemplate item (slot name) (multislot tags))"
    )
    let scalar = try await engine.global("answer")
    let many = try await engine.global("many")
    #expect(scalar == .integer(42))
    #expect(many == .multifield([.symbol("red"), .integer(7), .string("text")]))
    #expect(try await engine.global("missing") == nil)
    let id = try await engine.assertTemplate("item", slots: [
      "name": .string("owned"), "tags": .multifield([.symbol("a"), .integer(2)]),
    ])
    let name = try await engine.slotValue("name", of: id)
    let tags = try await engine.slotValue("tags", of: id)
    await #expect(throws: EngineError.self) { _ = try await engine.slotValue("absent", of: id) }
    let ordered = try await engine.assertFact("ordered", fields: [.integer(3)])
    await #expect(throws: EngineError.self) { _ = try await engine.slotValue("name", of: ordered) }
    let other = try await Engine.create()
    await #expect(throws: EngineError.invalidArgument("fact identity belongs to a different engine")) {
      _ = try await other.slotValue("name", of: id)
    }
    try await other.close()
    try await engine.retract(id)
    await #expect(throws: EngineError.self) { _ = try await engine.slotValue("name", of: id) }
    try await engine.close()
    #expect(name == .string("owned"))
    #expect(tags == .multifield([.symbol("a"), .integer(2)]))
    #expect(scalar == .integer(42))
    #expect(many == .multifield([.symbol("red"), .integer(7), .string("text")]))
    await #expect(throws: EngineError.closed) { _ = try await engine.global("answer") }
  }

  @Test
  func orderedLookupAndClearRespectFactIdentity() async throws {
    let engine = try await Engine.create()
    try await engine.load("(deftemplate item (slot n)) (defrule old =>)")
    let first = try await engine.assertFact("selected", fields: [.integer(1)])
    let second = try await engine.assertFact("selected", fields: [.integer(2)])
    _ = try await engine.assertFact("other")
    let template = try await engine.assertTemplate("item", slots: ["n": .integer(7)])
    let matches = try await engine.findFacts(relation: "selected")
    #expect(Set(matches.map(\.id)) == [first, second])
    #expect(try await engine.findFacts(relation: "missing").isEmpty)
    #expect(try await engine.findFacts(relation: "item").isEmpty)
    try await engine.retract(first)
    #expect(try await engine.findFacts(relation: "selected").map(\.id) == [second])
    try await engine.clear()
    #expect(try await engine.facts().isEmpty)
    #expect(try await engine.rules().isEmpty)
    #expect(try await engine.templates().isEmpty)
    #expect(try await engine.agendaCount == 0)
    await #expect(throws: EngineError.self) { try await engine.retract(second) }
    await #expect(throws: EngineError.self) { _ = try await engine.slotValue("n", of: template) }
    let fresh = try await engine.assertFact("selected", fields: [.integer(2)])
    #expect(fresh != second)
    try await engine.close()
    #expect(matches.count == 2)
  }

  @Test
  func inputAndPerChannelOutputClearingUseOwnedStrings() async throws {
    let engine = try await Engine.create()
    try await engine.load(
      "(defrule read-input => (printout t (read) crlf (readline)) (printout audit kept))"
    )
    try await engine.pushInput("42 ignored")
    try await engine.pushInput("résumé text")
    try await engine.reset()
    #expect(try await engine.run().rulesFired == 1)
    let output = try await engine.output()
    #expect(output == "42\nrésumé text")
    try await engine.clearOutput()
    #expect(try await engine.output() == nil)
    #expect(try await engine.output(channel: "audit") == "kept")
    try await engine.clearOutput(channel: "audit")
    #expect(try await engine.output(channel: "audit") == nil)
    await #expect(throws: EngineError.invalidArgument("C string input contains an embedded NUL")) {
      try await engine.pushInput("a\0b")
    }
    await #expect(throws: EngineError.invalidArgument("C string input contains an embedded NUL")) {
      try await engine.clearOutput(channel: "a\0b")
    }
    try await engine.load("(defrule noisy => (printout t pending crlf) (printout audit pending))")
    try await engine.reset()
    #expect(try await engine.run().rulesFired == 2)
    #expect(try await engine.output() != nil)
    #expect(try await engine.output(channel: "audit") != nil)
    try await engine.pushInput("discarded")
    try await engine.clear()
    #expect(try await engine.output() == nil)
    #expect(try await engine.output(channel: "audit") == nil)
    try await engine.load("(defrule echo => (printout t (read) crlf))")
    try await engine.reset()
    #expect(try await engine.run().rulesFired == 1)
    #expect(try await engine.output() == "EOF\n")
    try await engine.close()
    #expect(output == "42\nrésumé text")
  }

  @Test
  func metadataAndFocusAreOwnedAndDoNotMutateTheAgenda() async throws {
    let engine = try await Engine.create()
    try await engine.load(
      "(deftemplate person (slot name) (multislot tags)) (defrule enter (declare (salience 10)) => (focus WORK)) (defmodule WORK) (defrule worker =>)"
    )
    #expect(try await engine.currentModule == "WORK")
    #expect(Set(try await engine.modules()) == ["MAIN", "WORK"])
    let templates = try await engine.templates()
    #expect(templates.count == 1)
    #expect(templates[0].slotNames == ["name", "tags"])
    let rules = try await engine.rules()
    #expect(rules.count == 2)
    #expect(rules.contains { $0.name == "enter" && $0.salience == 10 })
    try await engine.reset()
    #expect(try await engine.focus == "MAIN")
    #expect(try await engine.focusStack() == ["MAIN"])
    #expect(try await engine.agendaCount == 2)
    #expect(try await engine.step() == .fired(diagnostics: []))
    #expect(try await engine.focus == "WORK")
    #expect(try await engine.focusStack() == ["MAIN", "WORK"])
    #expect(try await engine.step() == .fired(diagnostics: []))
    #expect(try await engine.step() == .agendaEmpty)
    #expect(try await engine.focus == nil)
    #expect(try await engine.focusStack().isEmpty)
    try await engine.close()
    #expect(templates[0].slotNames == ["name", "tags"])
    #expect(rules.count == 2)
  }
}
