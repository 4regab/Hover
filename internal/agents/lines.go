package agents

import (
	"bufio"
	"os"
	"strings"

	"github.com/4regab/Hover/internal/core"
)

// feedLines reads p and emits each progress piece. A carriage return starts a new
// piece. A line over limit is discarded rather than kept: installers and desktop
// tools redraw one line, and one runaway line must not hold the whole output.
func feedLines(p *os.File, limit int, emit func(string)) {
	defer p.Close()
	r := bufio.NewReader(p)
	for {
		raw, skipped, err := core.ReadLineLimit(r, limit)
		if !skipped {
			for _, piece := range strings.FieldsFunc(core.Lossy(raw), func(c rune) bool { return c == '\r' }) {
				if piece != "" {
					emit(piece)
				}
			}
		}
		if err != nil {
			return
		}
	}
}
