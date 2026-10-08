package agents

import "github.com/4regab/Hover/go/internal/core"

// CompactPrompt: a run of exactly this is Kiro's own compaction (_kiro/session/compact).
const CompactPrompt = "/compact"

// AttachPrompt is not a prompt: a run of exactly this attaches to a Kiro Web session that
// is still working in the cloud (its connection was lost, or Hover was closed) and follows
// it on, in the turn that was cut off. It ends with the answer, or with AttachNothing when
// the session sent nothing new.
const (
	AttachPrompt  = "/hover-attach-cloud"
	AttachNothing = "The cloud session sent nothing new."
	// AttachFailed is how a failed attempt to open the session begins (the reply the
	// cloud gave, if any, follows).
	AttachFailed = "Couldn’t open this Kiro Web session again"
)

// Attached is how a pasted picture's line in a prompt begins (KiroTurn.Text), then its file.
const Attached = "Attached image (read it from this file): "

// CloudTurn is a turn of a Kiro Web conversation as its replay gives it.
type CloudTurn struct {
	Prompt, Text string
	Steps        []core.KiroStep
	Completed    bool
}
