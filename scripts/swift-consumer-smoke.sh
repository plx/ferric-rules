#!/usr/bin/env bash
# Exercise a self-contained copy of the local Swift package outside the checkout.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
package="$root/bindings/swift"
if [[ ! -d "$package/Artifacts/CFerric.xcframework" ]]; then
    echo "swift-consumer-smoke: run scripts/build-swift.sh first" >&2
    exit 1
fi
if ! cmp -s "$root/examples/embedding/launch-selection.clp" "$package/Tests/FerricTests/Fixtures/launch.clp"; then
    echo "swift-consumer-smoke: Swift fixture differs from the canonical launch source" >&2
    exit 1
fi
smoke_dir="$(mktemp -d)"
trap 'rm -rf "$smoke_dir"' EXIT
mkdir -p "$smoke_dir/Ferric" "$smoke_dir/Consumer/Sources/Consumer/Resources"
tar -C "$package" --exclude=.build --exclude=.swiftpm -cf - . | tar -C "$smoke_dir/Ferric" -xf -
cp "$root/examples/embedding/launch-selection.clp" "$smoke_dir/Consumer/Sources/Consumer/Resources/launch.clp"
cat > "$smoke_dir/Consumer/Package.swift" <<'PACKAGE'
// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "Consumer", platforms: [.macOS(.v15), .iOS(.v18)],
    dependencies: [.package(path: "../Ferric")],
    targets: [.executableTarget(name: "Consumer", dependencies: [.product(name: "Ferric", package: "ferric")], resources: [.copy("Resources")])],
    swiftLanguageModes: [.v6]
)
PACKAGE
cat > "$smoke_dir/Consumer/Sources/Consumer/Consumer.swift" <<'SWIFT'
import Ferric
import Foundation

@main struct Consumer {
    static func configuredControl() async throws {
        let engine = try await Engine.create(config: EngineConfig(
            stringEncoding: .asciiSymbolsUTF8Strings, strategy: .lex, maxCallDepth: 32
        ))
        try await engine.load("""
            (defglobal ?*answer* = 0 ?*payload* = (create$ "owned" [unit] 9))
            (deftemplate detail (slot value))
            (deffacts startup (ready))
            (defrule consume ?f <- (ready) =>
              (retract ?f)
              (bind ?*answer* (read))
              (assert (read-result ?*answer*) (detail (value ?*answer*)))
              (printout t "read:" ?*answer* crlf)
              (printout audit "kept"))
            """)
        try await engine.pushInput("42")
        try await engine.reset()
        let pending = try await engine.agendaCount
        precondition(pending == 1)
        let first = try await engine.step()
        precondition(first == .fired(diagnostics: []))
        let exhausted = try await engine.step()
        precondition(exhausted == .agendaEmpty)
        let halted = try await engine.isHalted
        precondition(!halted)
        let answer = try await engine.global("answer")
        let payload = try await engine.global("payload")
        let absent = try await engine.global("absent")
        precondition(answer == .integer(42) && absent == nil)
        let results = try await engine.findFacts(relation: "read-result")
        guard results.count == 1,
              case .ordered(_, "read-result", let fields) = results[0]
        else { fatalError("missing input result") }
        precondition(fields == [.integer(42)])
        let details = try await engine.facts().filter {
            if case .template(_, "detail", _) = $0 { true } else { false }
        }
        precondition(details.count == 1)
        let slot = try await engine.slotValue("value", of: details[0].id)
        precondition(slot == .integer(42))
        let output = try await engine.output()
        precondition(output == "read:42\n")
        try await engine.clearOutput()
        let cleared = try await engine.output()
        let audit = try await engine.output(channel: "audit")
        precondition(cleared == nil && audit == "kept")
        _ = try await engine.assertFact("utf8", fields: [.string("caf\u{00e9}")])
        do {
            _ = try await engine.assertFact("invalid-symbol", fields: [.symbol("caf\u{00e9}")])
            fatalError("ASCII symbol policy ignored")
        } catch EngineError.native(_, let message) {
            precondition(!message.isEmpty)
        }
        try await engine.close()
        precondition(payload == .multifield([.string("owned"), .instanceName("unit"), .integer(9)]))
        precondition(fields == [.integer(42)] && output == "read:42\n")
    }

    static func main() async throws {
        try await configuredControl()
        let source = try String(contentsOf: Bundle.module.url(forResource: "launch", withExtension: "clp", subdirectory: "Resources")!, encoding: .utf8)
        let engine = try await Engine.create()
        try await engine.load(source)
        try await engine.reset()
        let checkpoint = try await engine.snapshot()
        let resumed = try await Engine.restore(checkpoint)
        for current in [engine, resumed] {
            let result = try await current.run(limit: 100)
            precondition(result.rulesFired == 1 && result.haltReason == .agendaEmpty)
            let output = try await current.output()
            precondition(output == "action session-42 sign-in\n")
            let actions = try await current.facts().filter {
                if case .ordered(_, "action", _) = $0 { true } else { false }
            }
            precondition(actions.count == 1)
            guard case .ordered(let actionID, _, let fields) = actions[0] else { fatalError("missing action") }
            precondition(actionID.rawValue >= 1 << 63)
            precondition(fields == [.symbol("session-42"), .symbol("sign-in")])
            let complete = try await Engine.restore(current.snapshot())
            let completedRun = try await complete.run()
            precondition(completedRun.rulesFired == 0)
            let completedActions = try await complete.facts().filter {
                if case .ordered(_, "action", _) = $0 { true } else { false }
            }
            precondition(completedActions.count == 1)
            guard case .ordered(let resumedID, _, let resumedFields) = completedActions[0] else { fatalError("missing resumed action") }
            precondition(resumedID.rawValue != actionID.rawValue && resumedFields == fields)
            try await complete.close()
            try await current.close()
        }
        let invalid = try await Engine.create()
        do {
            try await invalid.load("(defrule incomplete")
            fatalError("invalid source accepted")
        } catch EngineError.native(let code, let message) {
            precondition(code == 4 && !message.isEmpty)
        }
        do {
            _ = try await invalid.assertFact("invalid", fields: [.multifield([.void])])
            fatalError("void accepted as persisted fact data")
        } catch EngineError.invalidArgument(let message) {
            precondition(message.contains("void"))
        }
        let invalidFacts = try await invalid.facts()
        precondition(invalidFacts.isEmpty)
        try await invalid.close()
        print("Swift external consumer: configured input/step, owned globals/facts, isolated output clearing, snapshot resume, high IDs and invalid-input diagnostics passed")
    }
}
SWIFT
swift run --package-path "$smoke_dir/Consumer" -Xswiftc -strict-concurrency=complete Consumer
