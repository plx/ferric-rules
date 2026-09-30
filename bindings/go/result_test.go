package ferric

import "testing"

func TestHaltReasonString(t *testing.T) {
	tests := []struct {
		hr   HaltReason
		want string
	}{
		{HaltAgendaEmpty, "agenda_empty"},
		{HaltLimitReached, "limit_reached"},
		{HaltRequested, "requested"},
		{HaltActionError, "action_error"},
		{HaltReason(99), "unknown"},
	}
	for _, tt := range tests {
		if got := tt.hr.String(); got != tt.want {
			t.Errorf("HaltReason(%d).String() = %q, want %q", int(tt.hr), got, tt.want)
		}
	}
}
