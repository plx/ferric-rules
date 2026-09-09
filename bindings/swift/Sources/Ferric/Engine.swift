import CFerric
import Dispatch
import Foundation
import Synchronization

struct NativeState {
  var handle: OpaquePointer?

  func requireHandle() throws -> OpaquePointer {
    guard let handle else { throw EngineError.closed }
    return handle
  }

  mutating func close() throws {
    guard let handle else { return }
    // A failed ownership-consuming C free can leave lifetime indeterminate.
    // Remove the pointer before calling, so cleanup never retries a stale one.
    self.handle = nil
    try check(ferric_engine_free(handle))
  }
}

/// Mutex owns the non-Sendable opaque pointer. Only owned Sendable results leave
/// its synchronous protected block; no custom unchecked conformance is needed.
final class Storage: Sendable {
  let identity = UUID()
  private let state = Mutex(NativeState())
  private let queue = DispatchQueue(label: "org.ferric-rules.engine", qos: .userInitiated)

  func perform<Result: Sendable>(
    _ operation: @escaping @Sendable (inout NativeState) throws -> Result
  ) async throws -> Result {
    try await withCheckedThrowingContinuation { continuation in
      queue.async { [self] in
        do {
          let result = try state.withLock { try operation(&$0) }
          continuation.resume(returning: result)
        } catch {
          continuation.resume(throwing: error)
        }
      }
    }
  }

  func scheduleCleanup() {
    // The queued block retains storage until all earlier work and cleanup
    // finish. Deinitializing an Engine never blocks a UI/cooperative executor.
    queue.async { [self] in
      state.withLock { try? $0.close() }
    }
  }
}

/// A transferable rules engine with serialized, asynchronous native operations.
/// Each operation runs on a dispatch queue, away from cooperative/UI executors.
/// Independent tasks may share this object; individual calls are serialized.
public final class Engine: Sendable {
  let storage: Storage

  private init(storage: Storage) { self.storage = storage }

  deinit { storage.scheduleCleanup() }

  /// Create an empty engine with the default native configuration.
  public static func create() async throws -> Engine {
    let storage = Storage()
    try await storage.perform { state in
      guard let handle = ferric_engine_new() else {
        try check(FERRIC_ERROR_INTERNAL_ERROR)
        return
      }
      state.handle = handle
    }
    return Engine(storage: storage)
  }

  /// Restore the recommended CBOR snapshot, including its versioned envelope.
  public static func restore(_ data: Data) async throws -> Engine {
    let storage = Storage()
    try await storage.perform { state in
      var handle: OpaquePointer?
      let code = data.withUnsafeBytes { bytes in
        ferric_engine_deserialize_as(
          bytes.baseAddress?.assumingMemoryBound(to: UInt8.self),
          UInt(bytes.count),
          FERRIC_SERIALIZATION_FORMAT_CBOR.rawValue,
          &handle
        )
      }
      try check(code)
      guard let handle else {
        throw EngineError.unsupportedValue("native restore returned no engine")
      }
      state.handle = handle
    }
    return Engine(storage: storage)
  }

  /// Wait for earlier work and destroy the native engine. Repeated calls succeed.
  public func close() async throws {
    try await storage.perform { try $0.close() }
  }

  /// Load constructs incrementally. Call reset to install named deffacts.
  public func load(_ source: String) async throws {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      try withCString(source) { try check(ferric_engine_load_string(handle, $0), handle: handle) }
    }
  }

  /// Reset working memory and activate the loaded named deffacts.
  public func reset() async throws {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      try check(ferric_engine_reset(handle), handle: handle)
    }
  }

  /// Assert an ordered fact with fully owned typed values.
  @discardableResult
  public func assertFact(_ relation: String, fields: [Value] = []) async throws -> FactID {
    let owner = storage.identity
    return try await storage.perform { state in
      let handle = try state.requireHandle()
      var id: UInt64 = 0
      try withCString(relation) { relation in
        try withNativeValues(fields) { fields in
          try check(
            ferric_engine_assert_ordered(
              handle,
              relation,
              fields.baseAddress,
              UInt(fields.count),
              &id
            ),
            handle: handle
          )
        }
      }
      return FactID(rawValue: id, owner: owner)
    }
  }

  /// Assert a declared template fact. Omitted slots use their native defaults.
  @discardableResult
  public func assertTemplate(_ name: String, slots: [String: Value] = [:]) async throws -> FactID {
    let owner = storage.identity
    return try await storage.perform { state in
      let handle = try state.requireHandle()
      var id: UInt64 = 0
      let entries = slots.sorted { $0.key < $1.key }
      let names = entries.map(\.key)
      let values = entries.map(\.value)
      // Keep each name's allocation alive for the one native assertion.
      var namesStorage: [UnsafeMutablePointer<CChar>] = []
      defer { for pointer in namesStorage { pointer.deallocate() } }
      for name in names {
        guard !name.utf8.contains(0) else {
          throw EngineError.invalidArgument("slot name contains an embedded NUL")
        }
        let bytes = Array(name.utf8CString)
        let pointer = UnsafeMutablePointer<CChar>.allocate(capacity: bytes.count)
        pointer.initialize(from: bytes, count: bytes.count)
        namesStorage.append(pointer)
      }
      let pointers = namesStorage.map { Optional(UnsafePointer($0)) }
      try withCString(name) { name in
        try withNativeValues(values) { values in
          try pointers.withUnsafeBufferPointer { names in
            try check(
              ferric_engine_assert_template(
                handle,
                name,
                names.baseAddress,
                values.baseAddress,
                UInt(values.count),
                &id
              ),
              handle: handle
            )
          }
        }
      }
      return FactID(rawValue: id, owner: owner)
    }
  }

  /// Retract a fact identified by this engine's assertion or query result.
  public func retract(_ id: FactID) async throws {
    guard id.owner == storage.identity else {
      throw EngineError.invalidArgument("fact identity belongs to a different engine")
    }
    try await storage.perform { state in
      let handle = try state.requireHandle()
      try check(ferric_engine_retract(handle, id.rawValue), handle: handle)
    }
  }

  /// A nil limit runs to completion. Use finite limits for potentially unbounded
  /// rules; canceling a Swift Task does not interrupt already-admitted native work.
  public func run(limit: Int? = nil) async throws -> RunResult {
    if let limit, limit < 0 { throw EngineError.invalidArgument("run limit must be nonnegative") }
    return try await storage.perform { state in
      let handle = try state.requireHandle()
      var fired: UInt64 = 0
      var reason = FERRIC_HALT_REASON_AGENDA_EMPTY
      try check(ferric_engine_run_ex(handle, Int64(limit ?? -1), &fired, &reason), handle: handle)
      let result: HaltReason
      switch reason {
      case FERRIC_HALT_REASON_AGENDA_EMPTY: result = .agendaEmpty
      case FERRIC_HALT_REASON_LIMIT_REACHED: result = .limitReached
      case FERRIC_HALT_REASON_HALT_REQUESTED: result = .haltRequested
      case FERRIC_HALT_REASON_ACTION_ERROR: result = .actionError
      default: throw EngineError.unsupportedValue("unknown native halt reason")
      }
      var diagnosticCount: UInt = 0
      try check(ferric_engine_action_diagnostic_count(handle, &diagnosticCount), handle: handle)
      let diagnostics = try (0..<checkedCount(diagnosticCount)).map { index in
        try requiredString(handle: handle) {
          ferric_engine_action_diagnostic_copy(handle, UInt(index), $0, $1, $2)
        }
      }
      return RunResult(rulesFired: fired, haltReason: result, diagnostics: diagnostics)
    }
  }

  /// Return owned copies of all current facts and their typed values.
  public func facts() async throws -> [Fact] {
    let owner = storage.identity
    return try await storage.perform { state in
      let handle = try state.requireHandle()
      var count: UInt = 0
      try check(ferric_engine_fact_ids(handle, nil, 0, &count), handle: handle)
      var ids = [UInt64](repeating: 0, count: try checkedCount(count))
      try check(ferric_engine_fact_ids(handle, &ids, UInt(ids.count), &count), handle: handle)
      return try ids.map { try fact(handle: handle, id: $0, owner: owner) }
    }
  }

  /// Return accumulated output, or nil when the channel has no output.
  public func output(channel: String = "t") async throws -> String? {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      return try withCString(channel) { channel in
        try copiedString(handle: handle, optional: true) {
          ferric_engine_get_output_copy(handle, channel, $0, $1, $2)
        }
      }
    }
  }

  /// Retrieve exact output bytes without UTF-8 replacement or NUL truncation.
  public func outputBytes(channel: String = "t") async throws -> Data? {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      return try withCString(channel) { channel in
        try copiedBytes(handle: handle, optional: true) {
          ferric_engine_get_output_copy(handle, channel, $0, $1, $2)
        }
      }
    }
  }

  /// Copy a recommended CBOR snapshot into independent Swift-owned bytes.
  public func snapshot() async throws -> Data {
    try await storage.perform { state in
      let handle = try state.requireHandle()
      var bytes: UnsafeMutablePointer<UInt8>?
      var count: UInt = 0
      try check(
        ferric_engine_serialize_as(
          handle,
          FERRIC_SERIALIZATION_FORMAT_CBOR.rawValue,
          nil,
          nil,
          &bytes,
          &count
        ),
        handle: handle
      )
      guard let bytes else {
        throw EngineError.unsupportedValue("native snapshot returned no bytes")
      }
      defer { ferric_bytes_free(bytes, count) }
      return Data(bytes: bytes, count: try checkedCount(count))
    }
  }
}
