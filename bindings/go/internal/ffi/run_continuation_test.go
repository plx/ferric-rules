package ffi

import "testing"

func TestEngineContinueRunEx(t *testing.T) {
	lockThread(t)

	handle := EngineNewWithSource(`
		(defrule first
			(initial-fact)
			=>
			(assert (next)))
		(defrule second
			(next)
			=>
			(assert (done)))
	`)
	if handle == nil {
		t.Fatal("EngineNewWithSource returned nil")
	}
	defer EngineFree(handle)

	fired, reason, rc := EngineRunEx(handle, 1)
	if rc != ErrOK || fired != 1 || reason != HaltReasonLimitReached {
		t.Fatalf("first chunk = (%d, %d, %d), want (1, LimitReached, OK)", fired, reason, rc)
	}

	fired, reason, rc = EngineContinueRunEx(handle, -1)
	if rc != ErrOK || fired != 1 || reason != HaltReasonAgendaEmpty {
		t.Fatalf("continuation = (%d, %d, %d), want (1, AgendaEmpty, OK)", fired, reason, rc)
	}
}
