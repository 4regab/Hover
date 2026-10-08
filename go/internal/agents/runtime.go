package agents

// IAgentRuntime (KiroRunner.cs): one agent tool as Hover runs it, an ACP server (Kiro,
// Codex, Cursor), OpenCode's own server, or Claude Code in its SDK mode. Shared by all of
// that tool's sessions; the sessions and the views only ever see this. The Runtime itself
// comes with the hosts (acp, opencode, claude).

import "github.com/4regab/Hover/go/internal/core"

// AgentCaps is what a tool can really do, so the office only shows what works.
type AgentCaps struct {
	Questions, ReadOnly, Resume bool
	EffortLabel                 string
}

// Caps are the caps a tool has, without its runtime (AcpHost.Caps, OpenCodeHost.Caps).
func Caps(t core.AgentTool) AgentCaps {
	switch t {
	case core.OpenCode:
		// OpenCode's variants are the model's own, so they aren't called Effort.
		return AgentCaps{Questions: true, ReadOnly: true, Resume: true, EffortLabel: "Variant"}
	case core.Claude:
		// AskUserQuestion; its efforts are each model's, but they are efforts.
		return AgentCaps{Questions: true, ReadOnly: true, Resume: true, EffortLabel: "Effort"}
	}
	// ACP has no questions of its own; effort is a session option where offered.
	return AgentCaps{ReadOnly: ReadOnlyWorks(t), Resume: true, EffortLabel: "Effort"}
}

// PerModelEffort: whether a tool's efforts belong to each model (its offered models carry
// their levels) rather than being one list for all of them.
func PerModelEffort(t core.AgentTool) bool { return t == core.OpenCode || t == core.Claude }

// tagOf is the Hover session (its key) a run is for: the tag Hover's browser server is made
// for, so the agent's browser calls reach that session's page. A run that is no session's
// (voice's routing turn) has none, and the browser is not offered to it; OpenCode's one
// server passes its own tag.
func tagOf(a *RunArgs) *string { return a.Tag }
