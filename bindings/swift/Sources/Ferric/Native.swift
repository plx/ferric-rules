import CFerric
import Foundation

// All helpers run synchronously inside the storage's protected dispatch block.
// In particular, TLS errors are copied on the OS thread of the failing call.
func check(_ code: CFerric.FerricError, handle: OpaquePointer? = nil) throws {
  guard code != FERRIC_ERROR_OK else { return }
  var length: UInt = 0
  let query:
    (UnsafeMutablePointer<CChar>?, UInt, UnsafeMutablePointer<UInt>) -> CFerric.FerricError = {
      bytes,
      count,
      length in
      if let handle {
        ferric_engine_last_error_copy(handle, bytes, count, length)
      } else {
        ferric_last_error_global_copy(bytes, count, length)
      }
    }
  var message = "native call failed without a diagnostic"
  if query(nil, 0, &length) == FERRIC_ERROR_OK, length > 0 {
    var bytes = [CChar](repeating: 0, count: try checkedCount(length))
    if query(&bytes, UInt(bytes.count), &length) == FERRIC_ERROR_OK {
      message = String(decoding: bytes.dropLast().map { UInt8(bitPattern: $0) }, as: UTF8.self)
    }
  }
  throw EngineError.native(code: Int32(code.rawValue), message: message)
}

func withCString<Result>(_ text: String, _ body: (UnsafePointer<CChar>) throws -> Result) throws
  -> Result
{
  guard !text.utf8.contains(0) else {
    throw EngineError.invalidArgument("C string input contains an embedded NUL")
  }
  return try text.withCString(body)
}

func copiedString(
  handle: OpaquePointer,
  optional: Bool = false,
  _ copy: (UnsafeMutablePointer<CChar>?, UInt, UnsafeMutablePointer<UInt>) -> CFerric.FerricError
) throws -> String? {
  var length: UInt = 0
  let code = copy(nil, 0, &length)
  if optional && code == FERRIC_ERROR_NOT_FOUND { return nil }
  try check(code, handle: handle)
  guard length > 0 else { return "" }
  var bytes = [CChar](repeating: 0, count: try checkedCount(length))
  try check(copy(&bytes, UInt(bytes.count), &length), handle: handle)
  // Length includes the C terminator. Embedded NUL output remains intact.
  return String(decoding: bytes.dropLast().map { UInt8(bitPattern: $0) }, as: UTF8.self)
}

func requiredString(
  handle: OpaquePointer,
  _ copy: (UnsafeMutablePointer<CChar>?, UInt, UnsafeMutablePointer<UInt>) -> CFerric.FerricError
) throws -> String {
  guard let result = try copiedString(handle: handle, copy) else {
    throw EngineError.unsupportedValue("native call returned no required string")
  }
  return result
}

func withNativeValues<Result>(
  _ values: [Value],
  depth: Int = 0,
  _ body: (UnsafeBufferPointer<FerricValue>) throws -> Result
) throws -> Result {
  var native: [FerricValue] = []
  defer {
    for index in native.indices { _ = ferric_value_free(&native[index]) }
  }
  for value in values { native.append(try encode(value, depth: depth)) }
  return try native.withUnsafeBufferPointer(body)
}

private func encode(_ value: Value, depth: Int) throws -> FerricValue {
  switch value {
  case .void: return ferric_value_void()
  case .integer(let number): return ferric_value_integer(number)
  case .float(let number): return ferric_value_float(number)
  case .symbol(let string), .string(let string):
    var result = ferric_value_void()
    let bytes = Array(string.utf8)
    let code = bytes.withUnsafeBufferPointer { buffer in
      if case .symbol = value {
        ferric_value_symbol_bytes(buffer.baseAddress, UInt(buffer.count), &result)
      } else {
        ferric_value_string_bytes(buffer.baseAddress, UInt(buffer.count), &result)
      }
    }
    try check(code)
    return result
  case .multifield(let values):
    guard depth < 128 else {
      throw EngineError.invalidArgument("multifield nesting exceeds 128 levels")
    }
    return try withNativeValues(values, depth: depth + 1) { buffer in
      var result = ferric_value_void()
      try check(ferric_value_multifield_copy(buffer.baseAddress, UInt(buffer.count), &result))
      return result
    }
  }
}

func decode(_ value: FerricValue, depth: Int = 0) throws -> Value {
  switch value.value_type {
  case FERRIC_VALUE_TYPE_VOID.rawValue: return .void
  case FERRIC_VALUE_TYPE_INTEGER.rawValue: return .integer(value.integer)
  case FERRIC_VALUE_TYPE_FLOAT.rawValue: return .float(value.float_)
  case FERRIC_VALUE_TYPE_SYMBOL.rawValue, FERRIC_VALUE_TYPE_STRING.rawValue:
    guard let pointer = value.string_ptr else {
      throw EngineError.unsupportedValue("native string has no data")
    }
    let string = String(cString: pointer)
    return value.value_type == FERRIC_VALUE_TYPE_SYMBOL.rawValue ? .symbol(string) : .string(string)
  case FERRIC_VALUE_TYPE_MULTIFIELD.rawValue:
    guard depth < 128 else {
      throw EngineError.unsupportedValue("native multifield nesting exceeds 128 levels")
    }
    let elements = UnsafeBufferPointer(
      start: value.multifield_ptr,
      count: try checkedCount(value.multifield_len)
    )
    return .multifield(try elements.map { try decode($0, depth: depth + 1) })
  default:
    throw EngineError.unsupportedValue(
      "unsupported native value type \(value.value_type); external values are not transferable Swift values"
    )
  }
}

func field(handle: OpaquePointer, fact: UInt64, index: Int) throws -> Value {
  var value = ferric_value_void()
  try check(ferric_engine_get_fact_field(handle, fact, UInt(index), &value), handle: handle)
  defer { _ = ferric_value_free(&value) }
  return try decode(value)
}

func fact(handle: OpaquePointer, id: UInt64, owner: UUID) throws -> Fact {
  let identity = FactID(rawValue: id, owner: owner)
  var kind = FERRIC_FACT_TYPE_ORDERED
  try check(ferric_engine_get_fact_type(handle, id, &kind), handle: handle)
  var nativeCount: UInt = 0
  try check(ferric_engine_get_fact_field_count(handle, id, &nativeCount), handle: handle)
  let count = try checkedCount(nativeCount)
  if kind == FERRIC_FACT_TYPE_ORDERED {
    let relation = try requiredString(handle: handle) {
      ferric_engine_get_fact_relation(handle, id, $0, $1, $2)
    }
    return .ordered(
      id: identity,
      relation: relation,
      fields: try (0..<count).map { try field(handle: handle, fact: id, index: $0) }
    )
  }
  guard kind == FERRIC_FACT_TYPE_TEMPLATE else {
    throw EngineError.unsupportedValue("unsupported native fact kind")
  }
  let name = try requiredString(handle: handle) {
    ferric_engine_get_fact_template_name(handle, id, $0, $1, $2)
  }
  var slots: [String: Value] = [:]
  for index in 0..<count {
    let slot = try withCString(name) { name in
      try requiredString(handle: handle) {
        ferric_engine_template_slot_name(handle, name, UInt(index), $0, $1, $2)
      }
    }
    slots[slot] = try field(handle: handle, fact: id, index: index)
  }
  return .template(id: identity, name: name, slots: slots)
}

func checkedCount(_ count: UInt) throws -> Int {
  guard let result = Int(exactly: count) else {
    throw EngineError.unsupportedValue("native length exceeds Swift collection capacity")
  }
  return result
}
