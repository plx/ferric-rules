package ffi

/*
#include "lib/ferric.h"
*/
import "C"

// EngineHandle wraps the opaque C engine pointer.
type EngineHandle = *C.struct_FerricEngine

// ErrorCode mirrors the C FerricError enum.
type ErrorCode = C.enum_FerricError

// ErrorCode values mirror C.FERRIC_ERROR_*.
const (
	ErrOK              ErrorCode = C.FERRIC_ERROR_OK
	ErrNullPointer     ErrorCode = C.FERRIC_ERROR_NULL_POINTER
	ErrThreadViolation ErrorCode = C.FERRIC_ERROR_THREAD_VIOLATION
	ErrNotFound        ErrorCode = C.FERRIC_ERROR_NOT_FOUND
	ErrParseError      ErrorCode = C.FERRIC_ERROR_PARSE_ERROR
	ErrCompileError    ErrorCode = C.FERRIC_ERROR_COMPILE_ERROR
	ErrRuntimeError    ErrorCode = C.FERRIC_ERROR_RUNTIME_ERROR
	ErrIOError         ErrorCode = C.FERRIC_ERROR_IO_ERROR
	ErrBufferTooSmall  ErrorCode = C.FERRIC_ERROR_BUFFER_TOO_SMALL
	ErrInvalidArgument ErrorCode = C.FERRIC_ERROR_INVALID_ARGUMENT
	ErrInternalError   ErrorCode = C.FERRIC_ERROR_INTERNAL_ERROR
)

// ValueType mirrors the C FerricValueType enum.
type ValueType = C.enum_FerricValueType

// ValueType values mirror C.FERRIC_VALUE_TYPE_*.
const (
	ValueTypeVoid            ValueType = C.FERRIC_VALUE_TYPE_VOID
	ValueTypeInteger         ValueType = C.FERRIC_VALUE_TYPE_INTEGER
	ValueTypeFloat           ValueType = C.FERRIC_VALUE_TYPE_FLOAT
	ValueTypeSymbol          ValueType = C.FERRIC_VALUE_TYPE_SYMBOL
	ValueTypeString          ValueType = C.FERRIC_VALUE_TYPE_STRING
	ValueTypeMultifield      ValueType = C.FERRIC_VALUE_TYPE_MULTIFIELD
	ValueTypeExternalAddress ValueType = C.FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS
)

// FactType mirrors the C FerricFactType enum.
type FactType = C.enum_FerricFactType

// FactType values mirror C.FERRIC_FACT_TYPE_*.
const (
	FactTypeOrdered  FactType = C.FERRIC_FACT_TYPE_ORDERED
	FactTypeTemplate FactType = C.FERRIC_FACT_TYPE_TEMPLATE
)

// HaltReason mirrors the C FerricHaltReason enum.
type HaltReason = C.enum_FerricHaltReason

// HaltReason values mirror C.FERRIC_HALT_REASON_*.
const (
	HaltReasonAgendaEmpty   HaltReason = C.FERRIC_HALT_REASON_AGENDA_EMPTY
	HaltReasonLimitReached  HaltReason = C.FERRIC_HALT_REASON_LIMIT_REACHED
	HaltReasonHaltRequested HaltReason = C.FERRIC_HALT_REASON_HALT_REQUESTED
)

// StringEncoding mirrors the C FerricStringEncoding enum.
type StringEncoding = C.enum_FerricStringEncoding

// StringEncoding values mirror C.FERRIC_STRING_ENCODING_*.
const (
	StringEncodingASCII                   StringEncoding = C.FERRIC_STRING_ENCODING_ASCII
	StringEncodingUTF8                    StringEncoding = C.FERRIC_STRING_ENCODING_UTF8
	StringEncodingASCIISymbolsUTF8Strings StringEncoding = C.FERRIC_STRING_ENCODING_ASCII_SYMBOLS_UTF8_STRINGS
)

// ConflictStrategy mirrors the C FerricConflictStrategy enum.
type ConflictStrategy = C.enum_FerricConflictStrategy

// ConflictStrategy values mirror C.FERRIC_CONFLICT_STRATEGY_*.
const (
	ConflictStrategyDepth   ConflictStrategy = C.FERRIC_CONFLICT_STRATEGY_DEPTH
	ConflictStrategyBreadth ConflictStrategy = C.FERRIC_CONFLICT_STRATEGY_BREADTH
	ConflictStrategyLEX     ConflictStrategy = C.FERRIC_CONFLICT_STRATEGY_LEX
	ConflictStrategyMEA     ConflictStrategy = C.FERRIC_CONFLICT_STRATEGY_MEA
)

// Config mirrors the C FerricConfig struct.
type Config = C.struct_FerricConfig

// Value mirrors the C FerricValue struct.
type Value = C.struct_FerricValue
