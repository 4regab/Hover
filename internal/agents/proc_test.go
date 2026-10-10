package agents

import "testing"

func TestEscapeCodesGoAsTheRegexTakesThem(t *testing.T) {
	if got := StripANSI("\x1b[31merror:\x1b[0m x\x1b[?25l ▰\x1b]0;title\x07!"); got != "error: x ▰!" {
		t.Errorf("%q", got)
	}
	if got := StripANSI("\x1b[31"); got != "\x1b[31" {
		t.Errorf("%q", got)
	}
}
