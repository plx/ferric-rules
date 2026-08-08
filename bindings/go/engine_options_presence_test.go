package ferric

import (
	"bytes"
	"errors"
	"os"
	"reflect"
	"testing"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

func TestEngineOptionDefaultsMatchRuntimeDefaults(t *testing.T) {
	got := defaultEngineConfig()
	want := engineConfig{
		strategy:     StrategyDepth,
		encoding:     EncodingUTF8,
		maxCallDepth: 64,
	}

	if !reflect.DeepEqual(got, want) {
		t.Fatalf("default engine config = %+v, want %+v", got, want)
	}
	if got.hasEngineConfig() || got.hasSource() || got.hasSnapshot() {
		t.Fatalf("default config reports an explicitly supplied option: %+v", got)
	}
	if constructor := observeEngineConstructor(t); constructor != "default" {
		t.Fatalf("NewEngine() constructor = %q, want default", constructor)
	}
}

func TestEngineOptionsTrackPresenceIndependentlyFromValues(t *testing.T) {
	strategies := []Strategy{StrategyDepth, StrategyBreadth, StrategyLEX, StrategyMEA}
	for _, strategy := range strategies {
		t.Run("strategy-"+strategyName(strategy), func(t *testing.T) {
			got := applyEngineOptions(WithStrategy(strategy))
			want := defaultEngineConfig()
			want.strategy = strategy
			want.strategySet = true
			assertEngineConfigEqual(t, got, want)
			if !got.hasEngineConfig() {
				t.Fatal("explicit strategy was not recorded as present")
			}
		})
	}

	encodings := []Encoding{EncodingASCII, EncodingUTF8, EncodingASCIISymbolsUTF8Strings}
	for _, encoding := range encodings {
		t.Run("encoding-"+encodingName(encoding), func(t *testing.T) {
			got := applyEngineOptions(WithEncoding(encoding))
			want := defaultEngineConfig()
			want.encoding = encoding
			want.encodingSet = true
			assertEngineConfigEqual(t, got, want)
			if !got.hasEngineConfig() {
				t.Fatal("explicit encoding was not recorded as present")
			}
		})
	}

	for _, depth := range []int{0, 64, 255, 256, 257} {
		t.Run("max-call-depth-"+depthName(depth), func(t *testing.T) {
			got := applyEngineOptions(WithMaxCallDepth(depth))
			want := defaultEngineConfig()
			want.maxCallDepth = depth
			want.maxCallDepthSet = true
			assertEngineConfigEqual(t, got, want)
			if !got.hasEngineConfig() {
				t.Fatal("explicit max call depth was not recorded as present")
			}
		})
	}
}

func TestSourceAndSnapshotOptionsTrackPresenceIndependentlyFromValues(t *testing.T) {
	t.Run("empty-source", func(t *testing.T) {
		got := applyEngineOptions(WithSource(""))
		want := defaultEngineConfig()
		want.sourceSet = true
		assertEngineConfigEqual(t, got, want)
		if !got.sourceSet {
			t.Fatal("explicit empty source lost its presence bit")
		}
	})

	t.Run("nil-snapshot", func(t *testing.T) {
		got := applyEngineOptions(WithSnapshot(nil, FormatBincode))
		want := defaultEngineConfig()
		want.snapshotSet = true
		assertEngineConfigEqual(t, got, want)
		if !got.snapshotSet {
			t.Fatal("explicit nil snapshot lost its presence bit")
		}
	})

	t.Run("non-empty-source", func(t *testing.T) {
		const source = "(deffacts startup (ready))"
		got := applyEngineOptions(WithSource(source))
		want := defaultEngineConfig()
		want.source = source
		want.sourceSet = true
		assertEngineConfigEqual(t, got, want)
		if !got.hasSource() {
			t.Fatal("non-empty source was not recognized")
		}
	})

	t.Run("non-nil-snapshot", func(t *testing.T) {
		data := []byte("snapshot")
		got := applyEngineOptions(WithSnapshot(data, FormatCBOR))
		want := defaultEngineConfig()
		want.snapshot = data
		want.snapshotSet = true
		want.snapshotFormat = FormatCBOR
		assertEngineConfigEqual(t, got, want)
		if !got.hasSnapshot() {
			t.Fatal("non-nil snapshot was not recognized")
		}
	})
}

func TestEngineOptionConstructorSelection(t *testing.T) {
	tests := []struct {
		name string
		opt  EngineOption
	}{
		{"strategy-depth-zero", WithStrategy(StrategyDepth)},
		{"strategy-breadth", WithStrategy(StrategyBreadth)},
		{"strategy-lex", WithStrategy(StrategyLEX)},
		{"strategy-mea", WithStrategy(StrategyMEA)},
		{"encoding-ascii-zero", WithEncoding(EncodingASCII)},
		{"encoding-utf8-default", WithEncoding(EncodingUTF8)},
		{"encoding-mixed", WithEncoding(EncodingASCIISymbolsUTF8Strings)},
		{"depth-zero", WithMaxCallDepth(0)},
		{"depth-runtime-default", WithMaxCallDepth(64)},
		{"depth-255", WithMaxCallDepth(255)},
		{"depth-former-sentinel-256", WithMaxCallDepth(256)},
		{"depth-257", WithMaxCallDepth(257)},
	}

	for _, tc := range tests {
		t.Run(tc.name+"-fresh", func(t *testing.T) {
			if got := observeEngineConstructor(t, tc.opt); got != "configured" {
				t.Fatalf("constructor = %q, want configured", got)
			}
		})
		t.Run(tc.name+"-source", func(t *testing.T) {
			if got := observeEngineConstructor(t, WithSource("(deffacts startup (ready))"), tc.opt); got != "source-configured" {
				t.Fatalf("source constructor = %q, want source-configured", got)
			}
		})
	}

	t.Run("source-alone", func(t *testing.T) {
		if got := observeEngineConstructor(t, WithSource("(deffacts startup (ready))")); got != "source" {
			t.Fatalf("constructor = %q, want source", got)
		}
	})

	t.Run("snapshot-alone", func(t *testing.T) {
		if got := observeEngineConstructor(t, WithSnapshot([]byte("snapshot"), FormatBincode)); got != "snapshot" {
			t.Fatalf("constructor = %q, want snapshot", got)
		}
	})
}

func TestEngineOptionPairsAreOrderIndependent(t *testing.T) {
	options := []struct {
		name string
		opt  EngineOption
	}{
		{"strategy", WithStrategy(StrategyMEA)},
		{"encoding", WithEncoding(EncodingASCIISymbolsUTF8Strings)},
		{"max-call-depth", WithMaxCallDepth(257)},
		{"source", WithSource("(deffacts startup (ready))")},
		{"snapshot", WithSnapshot([]byte("snapshot"), FormatMessagePack)},
	}

	for i := range options {
		for j := i + 1; j < len(options); j++ {
			left, right := options[i], options[j]
			t.Run(left.name+"-then-"+right.name, func(t *testing.T) {
				forward := applyEngineOptions(left.opt, right.opt)
				reverse := applyEngineOptions(right.opt, left.opt)
				assertEngineConfigEqual(t, forward, reverse)
			})
		}
	}
}

func TestInvalidEngineOptionsFailBeforeNativeConstruction(t *testing.T) {
	snapshotPath := t.TempDir() + "/snapshot.bin"
	if err := os.WriteFile(snapshotPath, []byte("file snapshot"), 0o600); err != nil {
		t.Fatal(err)
	}

	invalid := []struct {
		name string
		opt  EngineOption
	}{
		{"strategy", WithStrategy(Strategy(99))},
		{"encoding", WithEncoding(Encoding(99))},
		{"max-call-depth", WithMaxCallDepth(-1)},
	}
	contexts := []struct {
		name string
		new  func(EngineOption) error
	}{
		{"fresh", func(opt EngineOption) error {
			_, err := NewEngine(opt)
			return err
		}},
		{"source", func(opt EngineOption) error {
			_, err := NewEngine(WithSource("(deffacts startup (ready))"), opt)
			return err
		}},
		{"snapshot", func(opt EngineOption) error {
			_, err := NewEngine(WithSnapshot([]byte("snapshot"), FormatBincode), opt)
			return err
		}},
		{"snapshot-file", func(opt EngineOption) error {
			_, err := NewEngineFromFile(snapshotPath, FormatBincode, opt)
			return err
		}},
	}

	for _, bad := range invalid {
		for _, context := range contexts {
			t.Run(bad.name+"-"+context.name, func(t *testing.T) {
				calls := recordEngineConstructors(t)
				err := context.new(bad.opt)
				if !errors.Is(err, ErrInvalidArgument) {
					t.Fatalf("error = %v, want ErrInvalidArgument", err)
				}
				if len(*calls) != 0 {
					t.Fatalf("native constructors called before validation: %v", *calls)
				}
			})
		}
	}

	t.Run("snapshot-format", func(t *testing.T) {
		calls := recordEngineConstructors(t)
		_, err := NewEngine(WithSnapshot([]byte("snapshot"), Format(99)))
		if !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("error = %v, want ErrInvalidArgument", err)
		}
		if len(*calls) != 0 {
			t.Fatalf("native constructors called before format validation: %v", *calls)
		}
	})
}

func TestSnapshotInputsReachDeserializerUnchanged(t *testing.T) {
	directData := []byte("direct snapshot")
	assertSnapshotInput(t, directData, FormatMessagePack, func() error {
		_, err := NewEngine(WithSnapshot(directData, FormatMessagePack))
		return err
	})

	fileData := []byte("file snapshot")
	path := t.TempDir() + "/snapshot.cbor"
	if err := os.WriteFile(path, fileData, 0o600); err != nil {
		t.Fatal(err)
	}
	assertSnapshotInput(t, fileData, FormatCBOR, func() error {
		_, err := NewEngineFromFile(path, FormatCBOR)
		return err
	})
}

func applyEngineOptions(opts ...EngineOption) engineConfig {
	cfg := defaultEngineConfig()
	for _, opt := range opts {
		opt(&cfg)
	}
	return cfg
}

func assertEngineConfigEqual(t *testing.T, got, want engineConfig) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("engine config = %+v, want %+v", got, want)
	}
}

func observeEngineConstructor(t *testing.T, opts ...EngineOption) string {
	t.Helper()
	calls := recordEngineConstructors(t)
	_, _ = NewEngine(opts...)
	if len(*calls) != 1 {
		t.Fatalf("native constructor calls = %v, want exactly one", *calls)
	}
	return (*calls)[0]
}

func recordEngineConstructors(t *testing.T) *[]string {
	t.Helper()
	withFFIHooks(t)
	calls := make([]string, 0, 1)
	ffiEngineNew = func() ffi.EngineHandle {
		calls = append(calls, "default")
		return nil
	}
	ffiEngineNewWithConfig = func(*ffi.Config) ffi.EngineHandle {
		calls = append(calls, "configured")
		return nil
	}
	ffiEngineNewWithSource = func(string) ffi.EngineHandle {
		calls = append(calls, "source")
		return nil
	}
	ffiEngineNewWithSourceConfig = func(string, *ffi.Config) ffi.EngineHandle {
		calls = append(calls, "source-configured")
		return nil
	}
	ffiEngineDeserializeAs = func([]byte, ffi.SerializationFormat) (ffi.EngineHandle, ffi.ErrorCode) {
		calls = append(calls, "snapshot")
		return nil, ffi.ErrOK
	}
	ffiLastErrorGlobal = func() string { return "" }
	return &calls
}

func assertSnapshotInput(t *testing.T, wantData []byte, wantFormat Format, construct func() error) {
	t.Helper()
	withFFIHooks(t)
	var gotData []byte
	var gotFormat ffi.SerializationFormat
	ffiEngineDeserializeAs = func(data []byte, format ffi.SerializationFormat) (ffi.EngineHandle, ffi.ErrorCode) {
		gotData = append([]byte(nil), data...)
		gotFormat = format
		return nil, ffi.ErrOK
	}

	if err := construct(); err == nil {
		t.Fatal("nil deserializer handle unexpectedly produced a valid engine")
	}
	if !bytes.Equal(gotData, wantData) {
		t.Fatalf("snapshot data = %q, want %q", gotData, wantData)
	}
	wantFFIFormat, err := formatToFFI(wantFormat)
	if err != nil {
		t.Fatal(err)
	}
	if gotFormat != wantFFIFormat {
		t.Fatalf("snapshot format = %d, want %d", gotFormat, wantFFIFormat)
	}
}

func strategyName(strategy Strategy) string {
	return []string{"depth", "breadth", "lex", "mea"}[strategy]
}

func encodingName(encoding Encoding) string {
	return []string{"ascii", "utf8", "mixed"}[encoding]
}

func depthName(depth int) string {
	switch depth {
	case 0:
		return "zero"
	case 64:
		return "runtime-default"
	case 255:
		return "255"
	case 256:
		return "former-sentinel-256"
	case 257:
		return "257"
	default:
		return "other"
	}
}
