package agents

import (
	"strings"
	"testing"
	"unicode/utf8"
)

// Expected text worked out from KiroText's patterns, applied in their order.
func TestMarkdownBecomesPlainWords(t *testing.T) {
	md := "# Done\r\n\r\nI **fixed** the _bug_ in `main.rs`, see [the docs](https://x.y).\n\n\n\n- [x] tests\n* ~~old~~ new\n> quoted\n---\n| a | b |\n|---|---|\n| 1 | 2 |\n```rust\n**kept** as is\n```\n![pic](a.png) snake_case_name"
	want := "Done\n\nI fixed the bug in main.rs, see the docs.\n\n• tests\n• old new\nquoted\na · b\n1 · 2\n**kept** as is\npic snake_case_name"
	if got := Plain(md); got != want {
		t.Errorf("%q", got)
	}
}

func TestTheFirstLineIsShortAndBare(t *testing.T) {
	if got := TextFirstLine("\n  ## Heading here\nmore"); got != "Heading here" {
		t.Error(got)
	}
	long := TextFirstLine(strings.Repeat("x", 130))
	if utf8.RuneCountInString(long) != 120 || !strings.HasSuffix(long, "…") || TextFirstLine("") != "" {
		t.Error(long)
	}
}
