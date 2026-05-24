//nolint:gochecknoglobals // Package-level FFI hooks let tests simulate native edge cases deterministically.
package ferric

import "github.com/prb/ferric-rules/bindings/go/internal/ffi"

var (
	ffiEngineNew                    = ffi.EngineNew
	ffiEngineNewWithConfig          = ffi.EngineNewWithConfig
	ffiEngineNewWithSource          = ffi.EngineNewWithSource
	ffiEngineNewWithSourceConfig    = ffi.EngineNewWithSourceConfig
	ffiEngineDeserializeAs          = ffi.EngineDeserializeAs
	ffiLastErrorGlobal              = ffi.LastErrorGlobal
	ffiEngineFree                   = ffi.EngineFree
	ffiEngineFreeUnchecked          = ffi.EngineFreeUnchecked
	ffiEngineLoadString             = ffi.EngineLoadString
	ffiEngineAssertString           = ffi.EngineAssertString
	ffiEngineAssertOrdered          = ffi.EngineAssertOrdered
	ffiEngineAssertTemplate         = ffi.EngineAssertTemplate
	ffiEngineRetract                = ffi.EngineRetract
	ffiEngineFactIDs                = ffi.EngineFactIDs
	ffiEngineFindFactIDs            = ffi.EngineFindFactIDs
	ffiEngineFactCount              = ffi.EngineFactCount
	ffiEngineRunEx                  = ffi.EngineRunEx
	ffiEngineStep                   = ffi.EngineStep
	ffiEngineHalt                   = ffi.EngineHalt
	ffiEngineReset                  = ffi.EngineReset
	ffiEngineClear                  = ffi.EngineClear
	ffiEngineSerializeAs            = ffi.EngineSerializeAs
	ffiEngineRuleCount              = ffi.EngineRuleCount
	ffiEngineRuleInfo               = ffi.EngineRuleInfo
	ffiEngineTemplateCount          = ffi.EngineTemplateCount
	ffiEngineTemplateName           = ffi.EngineTemplateName
	ffiEngineGetGlobal              = ffi.EngineGetGlobal
	ffiEngineCurrentModule          = ffi.EngineCurrentModule
	ffiEngineGetFocus               = ffi.EngineGetFocus
	ffiEngineFocusStackDepth        = ffi.EngineFocusStackDepth
	ffiEngineFocusStackEntry        = ffi.EngineFocusStackEntry
	ffiEngineAgendaCount            = ffi.EngineAgendaCount
	ffiEngineIsHalted               = ffi.EngineIsHalted
	ffiEngineGetOutput              = ffi.EngineGetOutput
	ffiEngineClearOutput            = ffi.EngineClearOutput
	ffiEnginePushInput              = ffi.EnginePushInput
	ffiEngineActionDiagnosticCount  = ffi.EngineActionDiagnosticCount
	ffiEngineActionDiagnosticCopy   = ffi.EngineActionDiagnosticCopy
	ffiEngineClearActionDiagnostics = ffi.EngineClearActionDiagnostics
	ffiEngineGetFactType            = ffi.EngineGetFactType
	ffiEngineGetFactFieldCount      = ffi.EngineGetFactFieldCount
	ffiEngineGetFactField           = ffi.EngineGetFactField
	ffiEngineGetFactTemplateName    = ffi.EngineGetFactTemplateName
	ffiEngineTemplateSlotCount      = ffi.EngineTemplateSlotCount
	ffiEngineTemplateSlotName       = ffi.EngineTemplateSlotName
	ffiEngineGetFactRelation        = ffi.EngineGetFactRelation
	ffiValueFree                    = ffi.ValueFree
)

var factsToWire = FactsToWire
