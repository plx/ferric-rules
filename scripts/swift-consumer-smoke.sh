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
cat > "$smoke_dir/Consumer/Sources/Consumer/main.swift" <<'SWIFT'
import Ferric
import Foundation

@main struct Consumer {
    static func main() async throws {
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
        print("Swift external consumer: sign-in selected once; pending/completed snapshot resume, high IDs and invalid-input diagnostics passed")
    }
}
SWIFT
swift run --package-path "$smoke_dir/Consumer" -Xswiftc -strict-concurrency=complete Consumer
