// Package ffi provides thin 1:1 Go wrappers around the ferric C FFI.
// This package is internal — external users should use the ferric package.
package ffi

/*
#cgo LDFLAGS: -L${SRCDIR}/lib -lferric_ffi -lm -ldl -lpthread
#cgo darwin LDFLAGS: -framework Security -framework CoreFoundation
#include "lib/ferric.h"
#include <stdlib.h>
*/
import "C"
import "unsafe"

// ---------------------------------------------------------------------------
// Engine lifecycle
// ---------------------------------------------------------------------------

// EngineNew creates a new engine with default configuration.
func EngineNew() EngineHandle {
	return C.ferric_engine_new()
}

// EngineNewWithConfig creates a new engine with explicit configuration.
func EngineNewWithConfig(config *Config) EngineHandle {
	return C.ferric_engine_new_with_config(config)
}

// EngineNewWithConfigHelper creates a new engine with the given config values.
// This avoids callers needing to deal with CGo-typed struct fields directly.
func EngineNewWithConfigHelper(encoding StringEncoding, strategy ConflictStrategy, maxCallDepth uintptr) EngineHandle {
	config := C.struct_FerricConfig{
		string_encoding: C.uint32_t(encoding),
		strategy:        C.uint32_t(strategy),
		max_call_depth:  C.uintptr_t(maxCallDepth),
	}
	return C.ferric_engine_new_with_config(&config)
}

// EngineNewWithSource creates an engine from CLIPS source (load + reset).
func EngineNewWithSource(source string) EngineHandle {
	cs := C.CString(source)
	defer C.free(unsafe.Pointer(cs))
	return C.ferric_engine_new_with_source(cs)
}

// EngineNewWithSourceConfig creates an engine from CLIPS source with config.
func EngineNewWithSourceConfig(source string, config *Config) EngineHandle {
	cs := C.CString(source)
	defer C.free(unsafe.Pointer(cs))
	return C.ferric_engine_new_with_source_config(cs, config)
}

// EngineFree frees an engine handle (thread-checked).
func EngineFree(h EngineHandle) ErrorCode {
	return ErrorCode(C.ferric_engine_free(h))
}

// EngineFreeUnchecked frees an engine without thread-affinity check.
// Intended for GC finalizers that run on arbitrary threads.
func EngineFreeUnchecked(h EngineHandle) ErrorCode {
	return ErrorCode(C.ferric_engine_free_unchecked(h))
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

// EngineLoadString loads CLIPS source into the engine.
func EngineLoadString(h EngineHandle, source string) ErrorCode {
	cs := C.CString(source)
	defer C.free(unsafe.Pointer(cs))
	return ErrorCode(C.ferric_engine_load_string(h, cs))
}

// ---------------------------------------------------------------------------
// Engine state
// ---------------------------------------------------------------------------

// EngineReset resets the engine to its initial state (facts cleared, rules kept).
func EngineReset(h EngineHandle) ErrorCode {
	return ErrorCode(C.ferric_engine_reset(h))
}

// EngineClear clears the engine completely (removes all rules, facts, etc.).
func EngineClear(h EngineHandle) ErrorCode {
	return ErrorCode(C.ferric_engine_clear(h))
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

// EngineRun runs the engine with a limit (-1 for unlimited).
// Returns the error code and number of rules fired.
func EngineRun(h EngineHandle, limit int64) (uint64, ErrorCode) {
	var fired C.uint64_t
	rc := ErrorCode(C.ferric_engine_run(h, C.int64_t(limit), &fired))
	return uint64(fired), rc
}

// EngineRunEx runs the engine with a limit and returns the halt reason.
func EngineRunEx(h EngineHandle, limit int64) (uint64, HaltReason, ErrorCode) {
	var fired C.uint64_t
	var reason C.enum_FerricHaltReason
	rc := ErrorCode(C.ferric_engine_run_ex(h, C.int64_t(limit), &fired, &reason))
	return uint64(fired), HaltReason(reason), rc
}

// EngineStep executes a single rule firing.
// Returns a status: 1 = rule fired, 0 = agenda empty, -1 = halted.
func EngineStep(h EngineHandle) (int32, ErrorCode) {
	var status C.int32_t
	rc := ErrorCode(C.ferric_engine_step(h, &status))
	return int32(status), rc
}

// EngineHalt requests the engine to halt.
func EngineHalt(h EngineHandle) ErrorCode {
	return ErrorCode(C.ferric_engine_halt(h))
}

// EngineIsHalted checks whether the engine is halted.
func EngineIsHalted(h EngineHandle) (bool, ErrorCode) {
	var halted C.int32_t
	rc := ErrorCode(C.ferric_engine_is_halted(h, &halted))
	return halted != 0, rc
}

// ---------------------------------------------------------------------------
// Fact assertion
// ---------------------------------------------------------------------------

// EngineAssertString asserts a fact from a CLIPS source string.
func EngineAssertString(h EngineHandle, source string) (uint64, ErrorCode) {
	cs := C.CString(source)
	defer C.free(unsafe.Pointer(cs))
	var factID C.uint64_t
	rc := ErrorCode(C.ferric_engine_assert_string(h, cs, &factID))
	return uint64(factID), rc
}

// EngineAssertOrdered asserts an ordered fact from structured values.
func EngineAssertOrdered(h EngineHandle, relation string, fields []Value) (uint64, ErrorCode) {
	crel := C.CString(relation)
	defer C.free(unsafe.Pointer(crel))

	var fieldsPtr *C.struct_FerricValue
	if len(fields) > 0 {
		fieldsPtr = &fields[0]
	}

	var factID C.uint64_t
	rc := ErrorCode(C.ferric_engine_assert_ordered(
		h, crel, fieldsPtr, C.uintptr_t(len(fields)), &factID,
	))
	return uint64(factID), rc
}

// EngineAssertTemplate asserts a template fact with named slots.
func EngineAssertTemplate(h EngineHandle, templateName string, slotNames []string, slotValues []Value) (uint64, ErrorCode) {
	ctmpl := C.CString(templateName)
	defer C.free(unsafe.Pointer(ctmpl))

	count := len(slotNames)

	// Allocate C strings for slot names.
	cnames := make([]*C.char, count)
	for i, name := range slotNames {
		cnames[i] = C.CString(name)
	}
	defer func() {
		for _, cn := range cnames {
			C.free(unsafe.Pointer(cn))
		}
	}()

	var namesPtr **C.char
	if count > 0 {
		namesPtr = &cnames[0]
	}

	var valuesPtr *C.struct_FerricValue
	if count > 0 {
		valuesPtr = &slotValues[0]
	}

	var factID C.uint64_t
	rc := ErrorCode(C.ferric_engine_assert_template(
		h, ctmpl, namesPtr, valuesPtr, C.uintptr_t(count), &factID,
	))
	return uint64(factID), rc
}

// EngineRetract retracts a fact by ID.
func EngineRetract(h EngineHandle, factID uint64) ErrorCode {
	return ErrorCode(C.ferric_engine_retract(h, C.uint64_t(factID)))
}

// ---------------------------------------------------------------------------
// Fact queries
// ---------------------------------------------------------------------------

// EngineFactCount returns the number of user-visible facts.
func EngineFactCount(h EngineHandle) (uintptr, ErrorCode) {
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_fact_count(h, &count))
	return uintptr(count), rc
}

// EngineFactIDs retrieves all user-visible fact IDs.
func EngineFactIDs(h EngineHandle) ([]uint64, ErrorCode) {
	// Size query
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_fact_ids(h, nil, 0, &count))
	if rc != ErrOK {
		return nil, rc
	}
	if count == 0 {
		return nil, ErrOK
	}

	ids := make([]C.uint64_t, count)
	rc = ErrorCode(C.ferric_engine_fact_ids(h, &ids[0], count, &count))
	if rc != ErrOK {
		return nil, rc
	}

	result := make([]uint64, count)
	for i := range count {
		result[i] = uint64(ids[i])
	}
	return result, ErrOK
}

// EngineFindFactIDs finds fact IDs by relation name.
func EngineFindFactIDs(h EngineHandle, relation string) ([]uint64, ErrorCode) {
	crel := C.CString(relation)
	defer C.free(unsafe.Pointer(crel))

	// Size query
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_find_fact_ids(h, crel, nil, 0, &count))
	if rc != ErrOK {
		return nil, rc
	}
	if count == 0 {
		return nil, ErrOK
	}

	ids := make([]C.uint64_t, count)
	rc = ErrorCode(C.ferric_engine_find_fact_ids(h, crel, &ids[0], count, &count))
	if rc != ErrOK {
		return nil, rc
	}

	result := make([]uint64, count)
	for i := range count {
		result[i] = uint64(ids[i])
	}
	return result, ErrOK
}

// EngineGetFactType returns the type (ordered vs template) of a fact.
func EngineGetFactType(h EngineHandle, factID uint64) (FactType, ErrorCode) {
	var ft C.enum_FerricFactType
	rc := ErrorCode(C.ferric_engine_get_fact_type(h, C.uint64_t(factID), &ft))
	return FactType(ft), rc
}

// EngineGetFactFieldCount returns the number of fields/slots in a fact.
func EngineGetFactFieldCount(h EngineHandle, factID uint64) (uintptr, ErrorCode) {
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_get_fact_field_count(h, C.uint64_t(factID), &count))
	return uintptr(count), rc
}

// EngineGetFactField retrieves a single field from a fact.
// The caller owns the returned Value and must free it with ValueFree.
func EngineGetFactField(h EngineHandle, factID uint64, index uintptr) (Value, ErrorCode) {
	var val C.struct_FerricValue
	//nolint:gocritic // false positive from cgo pointer argument pattern.
	rawRC := C.ferric_engine_get_fact_field(h, C.uint64_t(factID), C.uintptr_t(index), &val)
	rc := ErrorCode(rawRC)
	return Value(val), rc
}

// EngineGetFactRelation retrieves the relation name for an ordered fact.
func EngineGetFactRelation(h EngineHandle, factID uint64) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_get_fact_relation(h, C.uint64_t(factID), buf, bufLen, outLen))
	})
}

// EngineGetFactTemplateName retrieves the template name for a template fact.
func EngineGetFactTemplateName(h EngineHandle, factID uint64) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_get_fact_template_name(h, C.uint64_t(factID), buf, bufLen, outLen))
	})
}

// EngineGetFactSlotByName retrieves a slot value from a template fact by name.
// The caller owns the returned Value and must free it with ValueFree.
func EngineGetFactSlotByName(h EngineHandle, factID uint64, slotName string) (Value, ErrorCode) {
	cs := C.CString(slotName)
	defer C.free(unsafe.Pointer(cs))
	var val C.struct_FerricValue
	//nolint:gocritic // false positive from cgo pointer argument pattern.
	rawRC := C.ferric_engine_get_fact_slot_by_name(h, C.uint64_t(factID), cs, &val)
	rc := ErrorCode(rawRC)
	return Value(val), rc
}

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

// EngineGetGlobal retrieves a global variable's value by name.
// The caller owns the returned Value and must free it with ValueFree.
func EngineGetGlobal(h EngineHandle, name string) (Value, ErrorCode) {
	cn := C.CString(name)
	defer C.free(unsafe.Pointer(cn))
	var val C.struct_FerricValue
	//nolint:gocritic // false positive from cgo pointer argument pattern.
	rawRC := C.ferric_engine_get_global(h, cn, &val)
	rc := ErrorCode(rawRC)
	return Value(val), rc
}

// ---------------------------------------------------------------------------
// Output / Input
// ---------------------------------------------------------------------------

// EngineGetOutput retrieves captured output for a named channel.
// Returns empty string and false if no output.
func EngineGetOutput(h EngineHandle, channel string) (string, bool) {
	cc := C.CString(channel)
	defer C.free(unsafe.Pointer(cc))
	ptr := C.ferric_engine_get_output(h, cc)
	if ptr == nil {
		return "", false
	}
	return C.GoString(ptr), true
}

// EngineClearOutput clears a specific output channel.
func EngineClearOutput(h EngineHandle, channel string) ErrorCode {
	cc := C.CString(channel)
	defer C.free(unsafe.Pointer(cc))
	return ErrorCode(C.ferric_engine_clear_output(h, cc))
}

// EnginePushInput pushes an input line for read/readline.
func EnginePushInput(h EngineHandle, line string) ErrorCode {
	cl := C.CString(line)
	defer C.free(unsafe.Pointer(cl))
	return ErrorCode(C.ferric_engine_push_input(h, cl))
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

// EngineActionDiagnosticCount returns the number of action diagnostics.
func EngineActionDiagnosticCount(h EngineHandle) (uintptr, ErrorCode) {
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_action_diagnostic_count(h, &count))
	return uintptr(count), rc
}

// EngineActionDiagnosticCopy copies a diagnostic message by index.
func EngineActionDiagnosticCopy(h EngineHandle, index uintptr) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_action_diagnostic_copy(h, C.uintptr_t(index), buf, bufLen, outLen))
	})
}

// EngineClearActionDiagnostics clears all stored action diagnostics.
func EngineClearActionDiagnostics(h EngineHandle) ErrorCode {
	return ErrorCode(C.ferric_engine_clear_action_diagnostics(h))
}

// ---------------------------------------------------------------------------
// Introspection: templates
// ---------------------------------------------------------------------------

// EngineTemplateCount returns the number of registered templates.
func EngineTemplateCount(h EngineHandle) (uintptr, ErrorCode) {
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_template_count(h, &count))
	return uintptr(count), rc
}

// EngineTemplateName retrieves a template name by index.
func EngineTemplateName(h EngineHandle, index uintptr) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_template_name(h, C.uintptr_t(index), buf, bufLen, outLen))
	})
}

// EngineTemplateSlotCount returns the number of slots in a named template.
func EngineTemplateSlotCount(h EngineHandle, templateName string) (uintptr, ErrorCode) {
	ct := C.CString(templateName)
	defer C.free(unsafe.Pointer(ct))
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_template_slot_count(h, ct, &count))
	return uintptr(count), rc
}

// EngineTemplateSlotName retrieves a slot name by template name and slot index.
func EngineTemplateSlotName(h EngineHandle, templateName string, slotIndex uintptr) (string, ErrorCode) {
	ct := C.CString(templateName)
	defer C.free(unsafe.Pointer(ct))
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_template_slot_name(h, ct, C.uintptr_t(slotIndex), buf, bufLen, outLen))
	})
}

// ---------------------------------------------------------------------------
// Introspection: rules
// ---------------------------------------------------------------------------

// EngineRuleCount returns the number of registered rules.
func EngineRuleCount(h EngineHandle) (uintptr, ErrorCode) {
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_rule_count(h, &count))
	return uintptr(count), rc
}

// EngineRuleInfo retrieves the name and salience of a rule by index.
func EngineRuleInfo(h EngineHandle, index uintptr) (string, int32, ErrorCode) {
	var salience C.int32_t
	name, rc := getStringWith(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_rule_info(h, C.uintptr_t(index), buf, bufLen, outLen, &salience))
	})
	return name, int32(salience), rc
}

// ---------------------------------------------------------------------------
// Introspection: modules
// ---------------------------------------------------------------------------

// EngineModuleCount returns the number of registered modules.
func EngineModuleCount(h EngineHandle) (uintptr, ErrorCode) {
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_module_count(h, &count))
	return uintptr(count), rc
}

// EngineModuleName retrieves a module name by index.
func EngineModuleName(h EngineHandle, index uintptr) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_module_name(h, C.uintptr_t(index), buf, bufLen, outLen))
	})
}

// EngineCurrentModule retrieves the current module name.
func EngineCurrentModule(h EngineHandle) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_current_module(h, buf, bufLen, outLen))
	})
}

// ---------------------------------------------------------------------------
// Introspection: focus stack
// ---------------------------------------------------------------------------

// EngineGetFocus retrieves the top of the focus stack.
func EngineGetFocus(h EngineHandle) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_get_focus(h, buf, bufLen, outLen))
	})
}

// EngineFocusStackDepth returns the depth of the focus stack.
func EngineFocusStackDepth(h EngineHandle) (uintptr, ErrorCode) {
	var depth C.uintptr_t
	rc := ErrorCode(C.ferric_engine_focus_stack_depth(h, &depth))
	return uintptr(depth), rc
}

// EngineFocusStackEntry retrieves a focus stack entry by index.
func EngineFocusStackEntry(h EngineHandle, index uintptr) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_focus_stack_entry(h, C.uintptr_t(index), buf, bufLen, outLen))
	})
}

// EngineAgendaCount returns the number of activations on the agenda.
func EngineAgendaCount(h EngineHandle) (uintptr, ErrorCode) {
	var count C.uintptr_t
	rc := ErrorCode(C.ferric_engine_agenda_count(h, &count))
	return uintptr(count), rc
}

// ---------------------------------------------------------------------------
// Per-engine error
// ---------------------------------------------------------------------------

// EngineLastError retrieves the last per-engine error message.
func EngineLastError(h EngineHandle) string {
	ptr := C.ferric_engine_last_error(h)
	if ptr == nil {
		return ""
	}
	return C.GoString(ptr)
}

// EngineLastErrorCopy copies the per-engine error into a Go string.
func EngineLastErrorCopy(h EngineHandle) (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_engine_last_error_copy(h, buf, bufLen, outLen))
	})
}

// EngineClearError clears the per-engine error state.
func EngineClearError(h EngineHandle) ErrorCode {
	return ErrorCode(C.ferric_engine_clear_error(h))
}

// ---------------------------------------------------------------------------
// Global error
// ---------------------------------------------------------------------------

// LastErrorGlobal retrieves the last global error message.
func LastErrorGlobal() string {
	ptr := C.ferric_last_error_global()
	if ptr == nil {
		return ""
	}
	return C.GoString(ptr)
}

// LastErrorGlobalCopy copies the global error into a Go string.
func LastErrorGlobalCopy() (string, ErrorCode) {
	return getString(func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode {
		return ErrorCode(C.ferric_last_error_global_copy(buf, bufLen, outLen))
	})
}

// ClearErrorGlobal clears the global error channel.
func ClearErrorGlobal() {
	C.ferric_clear_error_global()
}

// ---------------------------------------------------------------------------
// Value construction
// ---------------------------------------------------------------------------

// ValueInteger creates an integer FerricValue.
func ValueInteger(v int64) Value {
	return Value(C.ferric_value_integer(C.int64_t(v)))
}

// ValueFloat creates a float FerricValue.
func ValueFloat(v float64) Value {
	return Value(C.ferric_value_float(C.double(v)))
}

// ValueSymbol creates a symbol FerricValue (heap-copies the string).
func ValueSymbol(name string) Value {
	cs := C.CString(name)
	defer C.free(unsafe.Pointer(cs))
	return Value(C.ferric_value_symbol(cs))
}

// ValueString creates a string FerricValue (heap-copies the string).
func ValueString(s string) Value {
	cs := C.CString(s)
	defer C.free(unsafe.Pointer(cs))
	return Value(C.ferric_value_string(cs))
}

// ValueVoid creates a void FerricValue.
func ValueVoid() Value {
	return Value(C.ferric_value_void())
}

// ---------------------------------------------------------------------------
// Value deallocation
// ---------------------------------------------------------------------------

// ValueFree frees a FerricValue and its owned resources.
func ValueFree(v *Value) {
	C.ferric_value_free((*C.struct_FerricValue)(v))
}

// StringFree frees a heap-allocated C string.
func StringFree(ptr *C.char) {
	C.ferric_string_free(ptr)
}

// ValueArrayFree frees an array of FerricValues.
func ValueArrayFree(arr *Value, length uintptr) {
	C.ferric_value_array_free((*C.struct_FerricValue)(arr), C.uintptr_t(length))
}

// ---------------------------------------------------------------------------
// String buffer helper
// ---------------------------------------------------------------------------

// getString implements the two-call buffer-copy pattern used by most
// string-returning FFI functions. First call with nil to get size,
// then allocate and copy.
func getString(fn func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode) (string, ErrorCode) {
	return getStringWith(fn)
}

// getStringWith implements the two-call buffer-copy pattern.
// Separated from getString to allow callers that need to capture extra
// out-params from the same FFI call (e.g., EngineRuleInfo captures salience).
func getStringWith(fn func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode) (string, ErrorCode) {
	// Size query
	var needed C.uintptr_t
	rc := fn(nil, 0, &needed)
	if rc != ErrOK {
		return "", rc
	}
	if needed == 0 {
		return "", ErrOK
	}

	// Allocate and copy
	buf := make([]byte, needed)
	rc = fn((*C.char)(unsafe.Pointer(&buf[0])), C.uintptr_t(needed), &needed)
	if rc != ErrOK {
		return "", rc
	}

	// Convert to Go string (exclude NUL terminator)
	if needed > 0 {
		return string(buf[:needed-1]), ErrOK
	}
	return "", ErrOK
}
