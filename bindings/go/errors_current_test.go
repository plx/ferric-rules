package ferric

import (
	"errors"
	"strings"
	"testing"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

//nolint:funlen // The ordered failure sequence is the regression contract.
func TestEngineErrorsDescribeCurrentOperation(t *testing.T) {
	lockThread(t)

	first, err := NewEngine(WithEncoding(EncodingASCII))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, first)

	second, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, second)

	err = first.Load(`(defrule retained-first`)
	var firstParse *ParseError
	if !errors.As(err, &firstParse) {
		t.Fatalf("first Load error = %T %v, want *ParseError", err, err)
	}
	firstParseMessage := assertCurrentErrorDetails(
		t, err, ErrParse, ffi.ErrParseError, firstParse.FerricError, "", nil, nil,
	)

	err = second.Load(`)`)
	var secondParse *ParseError
	if !errors.As(err, &secondParse) {
		t.Fatalf("second Load error = %T %v, want *ParseError", err, err)
	}
	secondParseMessage := assertCurrentErrorDetails(
		t,
		err,
		ErrParse,
		ffi.ErrParseError,
		secondParse.FerricError,
		"",
		nil,
		[]string{firstParseMessage},
	)
	staleMessages := make([]string, 0, 6)
	staleMessages = append(staleMessages, firstParseMessage, secondParseMessage)

	const missingFactID = ^uint64(0)
	_, err = first.GetFact(missingFactID)
	var missingFact *NotFoundError
	if !errors.As(err, &missingFact) {
		t.Fatalf("GetFact error = %T %v, want *NotFoundError", err, err)
	}
	missingFactMessage := assertCurrentErrorDetails(
		t,
		err,
		ErrNotFound,
		ffi.ErrNotFound,
		missingFact.FerricError,
		"fact not found: 18446744073709551615",
		nil,
		staleMessages,
	)
	staleMessages = append(staleMessages, missingFactMessage)

	// A relation name is an ordered-fact lookup key, not a registered
	// construct. A missing relation therefore succeeds with an empty result.
	facts, err := second.FindFacts("missing-relation")
	if err != nil {
		t.Fatalf("FindFacts for a missing relation returned an error: %v", err)
	}
	if len(facts) != 0 {
		t.Fatalf("FindFacts for a missing relation returned %d facts, want 0", len(facts))
	}

	_, err = second.AssertTemplate("missing-template-current", nil)
	var missingTemplate *NotFoundError
	if !errors.As(err, &missingTemplate) {
		t.Fatalf("AssertTemplate error = %T %v, want *NotFoundError", err, err)
	}
	missingTemplateMessage := assertCurrentErrorDetails(
		t,
		err,
		ErrNotFound,
		ffi.ErrNotFound,
		missingTemplate.FerricError,
		"",
		[]string{"template not found", "missing-template-current"},
		staleMessages,
	)
	staleMessages = append(staleMessages, missingTemplateMessage)

	_, err = first.AssertFact("non-ascii-☃")
	var invalidArgument *InvalidArgumentError
	if !errors.As(err, &invalidArgument) {
		t.Fatalf("AssertFact error = %T %v, want *InvalidArgumentError", err, err)
	}
	invalidArgumentMessage := assertCurrentErrorDetails(
		t,
		err,
		ErrInvalidArgument,
		ffi.ErrInvalidArgument,
		invalidArgument.FerricError,
		"",
		[]string{"encoding error", "non-ASCII symbol", "non-ascii-☃"},
		staleMessages,
	)
	staleMessages = append(staleMessages, invalidArgumentMessage)

	_, err = NewEngine(WithSnapshot([]byte("not-a-ferric-snapshot"), FormatBincode))
	var serialization *SerializationError
	if !errors.As(err, &serialization) {
		t.Fatalf("corrupt snapshot error = %T %v, want *SerializationError", err, err)
	}
	serializationMessage := assertCurrentErrorDetails(
		t,
		err,
		ErrSerialization,
		ffi.ErrSerializationError,
		serialization.FerricError,
		"",
		[]string{"deserialization failed"},
		staleMessages,
	)
	staleMessages = append(staleMessages, serializationMessage)

	// A positive capacity makes this distinguishable from a nil slice while
	// retaining the empty payload that fails before the native deserializer.
	emptySnapshot := make([]byte, 0, 1)
	_, err = NewEngine(WithSnapshot(emptySnapshot, FormatBincode))
	var emptySnapshotError *InvalidArgumentError
	if !errors.As(err, &emptySnapshotError) {
		t.Fatalf("empty snapshot error = %T %v, want *InvalidArgumentError", err, err)
	}
	assertCurrentErrorDetails(
		t,
		err,
		ErrInvalidArgument,
		ffi.ErrInvalidArgument,
		emptySnapshotError.FerricError,
		"snapshot data is empty",
		nil,
		staleMessages,
	)
}

func assertCurrentErrorDetails(
	t *testing.T,
	err error,
	sentinel error,
	code ffi.ErrorCode,
	details FerricError,
	exactMessage string,
	messageFragments []string,
	staleMessages []string,
) string {
	t.Helper()

	if !errors.Is(err, sentinel) {
		t.Fatalf("error = %v, want errors.Is(_, %v)", err, sentinel)
	}
	if details.Code != int(code) {
		t.Fatalf("error code = %d, want %d", details.Code, code)
	}
	if details.Message == "" {
		t.Fatal("error message is empty")
	}
	if err.Error() != "ferric: "+details.Message {
		t.Fatalf("error text = %q, want %q", err.Error(), "ferric: "+details.Message)
	}
	if exactMessage != "" && details.Message != exactMessage {
		t.Fatalf("error message = %q, want %q", details.Message, exactMessage)
	}
	for _, fragment := range messageFragments {
		if !strings.Contains(details.Message, fragment) {
			t.Fatalf("error message = %q, want fragment %q", details.Message, fragment)
		}
	}
	for _, stale := range staleMessages {
		if stale != "" && strings.Contains(details.Message, stale) {
			t.Fatalf("current error message %q retained prior text %q", details.Message, stale)
		}
	}

	return details.Message
}
