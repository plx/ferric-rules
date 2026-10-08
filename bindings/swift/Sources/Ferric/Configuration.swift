import CFerric

/// Encoding accepted by native symbols and strings.
public enum StringEncoding: UInt32, Sendable, Equatable {
  case ascii = 0
  case utf8 = 1
  case asciiSymbolsUTF8Strings = 2
}

/// Ordering used to select equally salient rule activations.
public enum ConflictResolutionStrategy: UInt32, Sendable, Equatable {
  case depth = 0
  case breadth = 1
  case lex = 2
  case mea = 3
}

/// Configuration used when creating an engine.
public struct EngineConfig: Sendable, Equatable {
  public var stringEncoding: StringEncoding
  public var strategy: ConflictResolutionStrategy
  /// Requested callable depth. Zero disables user calls; the native ceiling is 32.
  public var maxCallDepth: Int

  public init(
    stringEncoding: StringEncoding = .utf8,
    strategy: ConflictResolutionStrategy = .depth,
    maxCallDepth: Int = 64
  ) {
    self.stringEncoding = stringEncoding
    self.strategy = strategy
    self.maxCallDepth = maxCallDepth
  }

  func nativeConfiguration() throws -> FerricConfig {
    guard maxCallDepth >= 0 else {
      throw EngineError.invalidArgument("maximum call depth must be nonnegative")
    }
    return FerricConfig(
      string_encoding: stringEncoding.rawValue,
      strategy: strategy.rawValue,
      max_call_depth: UInt(maxCallDepth)
    )
  }
}

/// One native stepping result. A fired rule may have produced action diagnostics.
public enum StepResult: Sendable, Equatable {
  /// Owned diagnostics are copied before another operation can clear them.
  case fired(diagnostics: [String])
  case agendaEmpty
  case halted
}

/// Registered rule metadata returned by the native introspection API.
public struct RuleInfo: Sendable, Equatable {
  public let name: String
  public let salience: Int32
}

/// Registered template metadata; slot names retain declaration order.
public struct TemplateInfo: Sendable, Equatable {
  public let name: String
  public let slotNames: [String]
}
