package ffi

import "testing"

func TestEngineAssertTemplateRejectsMismatchedSlotSlicesBeforeNativeCall(t *testing.T) {
	tests := []struct {
		name       string
		slotNames  []string
		slotValues []Value
	}{
		{name: "name_without_value", slotNames: []string{"payload"}},
		{name: "value_without_name", slotValues: []Value{ValueInteger(1)}},
		{
			name:       "more_names_than_values",
			slotNames:  []string{"first", "second"},
			slotValues: []Value{ValueInteger(1)},
		},
		{
			name:       "more_values_than_names",
			slotNames:  []string{"first"},
			slotValues: []Value{ValueInteger(1), ValueInteger(2)},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			defer func() {
				if recovered := recover(); recovered != nil {
					t.Fatalf("EngineAssertTemplate panicked: %v", recovered)
				}
			}()

			factID, rc := EngineAssertTemplate(nil, "item", tt.slotNames, tt.slotValues)
			if factID != 0 || rc != ErrInvalidArgument {
				t.Fatalf(
					"EngineAssertTemplate = (%d, %d), want (0, ErrInvalidArgument=%d)",
					factID,
					rc,
					ErrInvalidArgument,
				)
			}
		})
	}
}

func TestEngineAssertTemplateAcceptsEmptySlotSlices(t *testing.T) {
	lockThread(t)

	handle := EngineNewWithSource(`
		(deftemplate item
			(slot payload (type INTEGER) (default 42)))
	`)
	if handle == nil {
		t.Fatal("EngineNewWithSource returned nil")
	}
	t.Cleanup(func() {
		if rc := EngineFree(handle); rc != ErrOK {
			t.Errorf("EngineFree returned %d", rc)
		}
	})

	tests := []struct {
		name       string
		slotNames  []string
		slotValues []Value
	}{
		{name: "nil_slices"},
		{name: "non_nil_empty_slices", slotNames: []string{}, slotValues: []Value{}},
	}

	for _, tt := range tests {
		factID, rc := EngineAssertTemplate(handle, "item", tt.slotNames, tt.slotValues)
		if rc != ErrOK || factID == 0 {
			t.Fatalf(
				"%s: EngineAssertTemplate = (%d, %d), want nonzero fact ID and ErrOK",
				tt.name,
				factID,
				rc,
			)
		}

		value, rc := EngineGetFactSlotByName(handle, factID, "payload")
		if rc != ErrOK {
			t.Fatalf("%s: EngineGetFactSlotByName returned %d", tt.name, rc)
		}
		if got := ValueGetType(&value); got != ValueTypeInteger {
			ValueFree(&value)
			t.Fatalf("%s: default slot type = %d, want ValueTypeInteger=%d", tt.name, got, ValueTypeInteger)
		}
		if got := ValueGetInteger(&value); got != 42 {
			ValueFree(&value)
			t.Fatalf("%s: default slot value = %d, want 42", tt.name, got)
		}
		ValueFree(&value)
	}
}
