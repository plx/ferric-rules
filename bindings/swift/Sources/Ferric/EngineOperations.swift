import CFerric

extension Engine {
  /// Fire one eligible activation, or report an empty agenda or native halt.
  /// Action failures accompany `.fired`; the C ABI does not provide a rule name.
  public func step() async throws -> StepResult {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      var status: Int32 = 0
      try check(ferric_engine_step(handle, &status), handle: handle)
      switch status {
      case 1: return .fired(diagnostics: try nativeActionDiagnostics(handle: handle))
      case 0: return .agendaEmpty
      case -1: return .halted
      default: throw EngineError.unsupportedValue("unknown native step status")
      }
    }
  }

  /// Remove constructs and working memory, invalidating this engine's fact IDs.
  public func clear() async throws {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      try check(ferric_engine_clear(handle), handle: handle)
    }
  }

  /// The native halt flag. Cooperative host cancellation need not set this flag.
  public var isHalted: Bool {
    get async throws {
      try await storage.perform { state in
        let handle = try state.requireHandle()
        var halted: Int32 = 0
        try check(ferric_engine_is_halted(handle, &halted), handle: handle)
        return halted != 0
      }
    }
  }

  /// Number of pending activations across all modules.
  public var agendaCount: Int {
    get async throws {
      try await storage.perform { state in
        let handle = try state.requireHandle()
        var count: UInt = 0
        try check(ferric_engine_agenda_count(handle, &count), handle: handle)
        return try checkedCount(count)
      }
    }
  }

  /// Read an owned global value, using its base name without `?*` and `*`.
  /// Returns nil for the C API's NotFound result, including ambiguous lookup.
  public func global(_ name: String) async throws -> Value? {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      var value = ferric_value_void()
      defer { _ = ferric_value_free(&value) }
      let code = try withCString(name) { ferric_engine_get_global(handle, $0, &value) }
      if code == FERRIC_ERROR_NOT_FOUND { return nil }
      try check(code, handle: handle)
      return try decode(value)
    }
  }

  /// Clear one captured output channel without changing other channels.
  public func clearOutput(channel: String = "t") async throws {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      try withCString(channel) {
        try check(ferric_engine_clear_output(handle, $0), handle: handle)
      }
    }
  }

  /// Queue one complete input line for native `read` or `readline`.
  public func pushInput(_ line: String) async throws {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      try withCString(line) { try check(ferric_engine_push_input(handle, $0), handle: handle) }
    }
  }

  /// Return owned ordered facts with this relation name. This does not select templates.
  public func findFacts(relation: String) async throws -> [Fact] {
    let owner = storage.identity
    return try await storage.perform { state in
      let handle = try state.requireHandle()
      return try withCString(relation) { relation in
        var count: UInt = 0
        try check(ferric_engine_find_fact_ids(handle, relation, nil, 0, &count), handle: handle)
        var ids = [UInt64](repeating: 0, count: try checkedCount(count))
        try check(
          ferric_engine_find_fact_ids(handle, relation, &ids, UInt(ids.count), &count),
          handle: handle
        )
        return try ids.map { try fact(handle: handle, id: $0, owner: owner) }
      }
    }
  }

  /// Copy a template fact's named slot. Ordered, stale, and missing slots throw.
  public func slotValue(_ name: String, of id: FactID) async throws -> Value {
    guard id.owner == storage.identity else {
      throw EngineError.invalidArgument("fact identity belongs to a different engine")
    }
    return try await storage.perform { state in
      let handle = try state.requireHandle()
      var value = ferric_value_void()
      defer { _ = ferric_value_free(&value) }
      try withCString(name) {
        try check(ferric_engine_get_fact_slot_by_name(handle, id.rawValue, $0, &value), handle: handle)
      }
      return try decode(value)
    }
  }

  public var currentModule: String {
    get async throws {
      try await storage.perform { state in
        let handle = try state.requireHandle()
        return try requiredString(handle: handle) {
          ferric_engine_current_module(handle, $0, $1, $2)
        }
      }
    }
  }

  /// The current focus, or nil when the focus stack is empty.
  public var focus: String? {
    get async throws {
      try await storage.perform { state in
        let handle = try state.requireHandle()
        return try copiedString(handle: handle, optional: true) {
          ferric_engine_get_focus(handle, $0, $1, $2)
        }
      }
    }
  }

  /// Focus stack in native order: bottom first, current focus last.
  public func focusStack() async throws -> [String] {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      var count: UInt = 0
      try check(ferric_engine_focus_stack_depth(handle, &count), handle: handle)
      return try (0..<checkedCount(count)).map { index in
        try requiredString(handle: handle) {
          ferric_engine_focus_stack_entry(handle, UInt(index), $0, $1, $2)
        }
      }
    }
  }

  public func modules() async throws -> [String] {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      var count: UInt = 0
      try check(ferric_engine_module_count(handle, &count), handle: handle)
      return try (0..<checkedCount(count)).map { index in
        try requiredString(handle: handle) {
          ferric_engine_module_name(handle, UInt(index), $0, $1, $2)
        }
      }
    }
  }

  /// Registered rules, including each compiled branch of a disjunctive rule.
  public func rules() async throws -> [RuleInfo] {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      var count: UInt = 0
      try check(ferric_engine_rule_count(handle, &count), handle: handle)
      return try (0..<checkedCount(count)).map { index in
        var salience: Int32 = 0
        let name = try requiredString(handle: handle) {
          ferric_engine_rule_info(handle, UInt(index), $0, $1, $2, &salience)
        }
        return RuleInfo(name: name, salience: salience)
      }
    }
  }

  /// Explicit registered templates and their slots. Ordering follows the native registry.
  public func templates() async throws -> [TemplateInfo] {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      var count: UInt = 0
      try check(ferric_engine_template_count(handle, &count), handle: handle)
      return try (0..<checkedCount(count)).map { index in
        let name = try requiredString(handle: handle) {
          ferric_engine_template_name(handle, UInt(index), $0, $1, $2)
        }
        return try withCString(name) { nativeName in
          var slotCount: UInt = 0
          try check(ferric_engine_template_slot_count(handle, nativeName, &slotCount), handle: handle)
          let names = try (0..<checkedCount(slotCount)).map { slotIndex in
            try requiredString(handle: handle) {
              ferric_engine_template_slot_name(handle, nativeName, UInt(slotIndex), $0, $1, $2)
            }
          }
          return TemplateInfo(name: name, slotNames: names)
        }
      }
    }
  }
}
