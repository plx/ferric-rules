import Foundation

/// Owned CLIPS values. Integers remain exact signed 64-bit values.
public indirect enum Value: Sendable, Equatable {
  /// An absent native result. Fact assertion rejects this value, including nested instances.
  case void
  case integer(Int64)
  case float(Double)
  case symbol(String)
  case string(String)
  /// Arbitrary STRING bytes, including embedded NUL and invalid UTF-8.
  case stringBytes(Data)
  /// Arbitrary SYMBOL bytes, including embedded NUL and invalid UTF-8.
  case symbolBytes(Data)
  /// An INSTANCE-NAME is distinct from a symbol with the same bytes.
  case instanceName(Data)
  case multifield([Value])
}

/// A transient identity scoped to one Engine. Reset and restore return fresh identifiers.
public struct FactID: Sendable, Hashable {
  /// The native identifier, meaningful only within its owning engine.
  public let rawValue: UInt64
  let owner: UUID
}

/// Facts and their values remain usable after the engine closes.
public enum Fact: Sendable, Equatable, Identifiable {
  case ordered(id: FactID, relation: String, fields: [Value])
  case template(id: FactID, name: String, slots: [String: Value])

  /// This fact's engine-scoped identity.
  public var id: FactID {
    switch self {
    case .ordered(let id, _, _), .template(let id, _, _): id
    }
  }
}

/// The native reason that a rule run stopped.
public enum HaltReason: Sendable, Equatable {
  case agendaEmpty, limitReached, haltRequested, actionError
}

/// Rule execution progress, including bounded-run completion.
public struct RunResult: Sendable, Equatable {
  /// Number of rules fired during this call.
  public let rulesFired: UInt64
  /// Why execution stopped.
  public let haltReason: HaltReason
  /// Owned action failures from this run, copied before another operation can clear them.
  public let diagnostics: [String]
}

/// Owned wrapper or native diagnostics, safe to retain after closing the engine.
public enum EngineError: Error, Sendable, Equatable, CustomStringConvertible {
  case closed
  case invalidArgument(String)
  case unsupportedValue(String)
  case native(code: Int32, message: String)

  /// A human-readable diagnostic, preserving native error details.
  public var description: String {
    switch self {
    case .closed: "engine has been closed"
    case .invalidArgument(let message), .unsupportedValue(let message): message
    case .native(let code, let message): "Ferric error \(code): \(message)"
    }
  }
}
