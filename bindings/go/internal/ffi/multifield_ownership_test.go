package ffi

import "testing"

func TestValueMultifieldCopyEmpty(t *testing.T) {
	result, rc := ValueMultifieldCopy(nil)
	if rc != ErrOK {
		t.Fatalf("ValueMultifieldCopy(nil) returned %d", rc)
	}
	defer ValueFree(&result)
	if got := ValueGetType(&result); got != ValueTypeMultifield {
		t.Fatalf("empty result type = %d, want multifield", got)
	}
	if got := ValueGetMultifieldLen(&result); got != 0 {
		t.Fatalf("empty result length = %d, want 0", got)
	}
}

func TestValueMultifieldCopyOwnsIndependentNestedTree(t *testing.T) {
	sourceString := ValueString("borrowed")
	nestedSource, rc := ValueMultifieldCopy([]Value{
		ValueInteger(7),
		sourceString,
	})
	if rc != ErrOK {
		ValueFree(&sourceString)
		t.Fatalf("nested ValueMultifieldCopy returned %d", rc)
	}

	sourceSymbol := ValueSymbol("outer")
	result, rc := ValueMultifieldCopy([]Value{
		sourceSymbol,
		nestedSource,
	})
	if rc != ErrOK {
		ValueFree(&sourceString)
		ValueFree(&nestedSource)
		ValueFree(&sourceSymbol)
		t.Fatalf("outer ValueMultifieldCopy returned %d", rc)
	}
	defer ValueFree(&result)

	// The copy API borrows its inputs. Releasing every source value must leave
	// the recursively Ferric-owned result intact.
	ValueFree(&sourceString)
	ValueFree(&nestedSource)
	ValueFree(&sourceSymbol)

	if got := ValueGetMultifieldLen(&result); got != 2 {
		t.Fatalf("outer multifield length = %d, want 2", got)
	}
	outerSymbol := ValueGetMultifieldElement(&result, 0)
	if got := ValueGetStringPtr(&outerSymbol); got != "outer" {
		t.Fatalf("outer symbol = %q, want outer", got)
	}
	nested := ValueGetMultifieldElement(&result, 1)
	if got := ValueGetMultifieldLen(&nested); got != 2 {
		t.Fatalf("nested multifield length = %d, want 2", got)
	}
	nestedInteger := ValueGetMultifieldElement(&nested, 0)
	if got := ValueGetInteger(&nestedInteger); got != 7 {
		t.Fatalf("nested integer = %d, want 7", got)
	}
	nestedString := ValueGetMultifieldElement(&nested, 1)
	if got := ValueGetStringPtr(&nestedString); got != "borrowed" {
		t.Fatalf("nested string = %q, want borrowed", got)
	}
}

func TestValueMultifieldCopyRejectsInvalidNestedTag(t *testing.T) {
	invalid := ValueVoid()
	invalid.value_type = ValueType(9999)

	result, rc := ValueMultifieldCopy([]Value{invalid})
	if rc != ErrInvalidArgument {
		t.Fatalf("invalid nested tag returned %d, want %d", rc, ErrInvalidArgument)
	}
	if got := ValueGetType(&result); got != ValueTypeVoid {
		t.Fatalf("failed copy result type = %d, want void", got)
	}
	ValueFree(&result)
}
