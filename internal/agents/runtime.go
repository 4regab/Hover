package agents

// IAgentRuntime (KiroRunner.cs): one agent tool as Hover runs it, an ACP server (Kiro,
// Codex, Cursor), OpenCode's own server, or Claude Code in its SDK mode. Shared by all of
// that tool's sessions; the sessions and the views only ever see this.

import (
	"errors"

	"github.com/4regab/Hover/internal/core"
)

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

// Runtime is one of the three hosts; exactly one field is set (Rust's enum).
type Runtime struct {
	Acp      *AcpHost
	OpenCode *OpenCodeHost
	Claude   *ClaudeHost
}

// NewRuntime is the tool as Agents finds and starts it: OpenCode's own server, Claude
// Code, or an ACP one.
func NewRuntime(t core.AgentTool, options func() core.AgentOptions) Runtime {
	switch t {
	case core.OpenCode:
		return Runtime{OpenCode: NewOpenCodeHost(options)}
	case core.Claude:
		return Runtime{Claude: NewClaudeHost(options)}
	}
	return Runtime{Acp: NewAcpHost(t, options)}
}

// runtimeHost is what every host has.
type runtimeHost interface {
	Tool() core.AgentTool
	Alive() bool
	OnOptionsSeen(func(core.AgentTool, []core.AcpOption))
	SetAsking(Asking)
	Shutdown(why string)
	Runner() RunTask
}

func (r Runtime) host() runtimeHost {
	switch {
	case r.OpenCode != nil:
		return r.OpenCode
	case r.Claude != nil:
		return r.Claude
	}
	return r.Acp
}

func (r Runtime) Tool() core.AgentTool { return r.host().Tool() }
func (r Runtime) Caps() AgentCaps      { return Caps(r.Tool()) }

// Alive: the tool's process is up.
func (r Runtime) Alive() bool { return r.host().Alive() }

// OnOptionsSeen: the models, efforts and modes it offers, whenever they are read. Off the
// UI goroutine.
func (r Runtime) OnOptionsSeen(f func(core.AgentTool, []core.AcpOption)) { r.host().OnOptionsSeen(f) }
func (r Runtime) SetAsking(f Asking)                                     { r.host().SetAsking(f) }

// SetQuestioning: ACP agents don't ask questions; OpenCode's and Claude Code's are passed on.
func (r Runtime) SetQuestioning(f Questioning) {
	switch {
	case r.OpenCode != nil:
		r.OpenCode.SetQuestioning(f)
	case r.Claude != nil:
		r.Claude.SetQuestioning(f)
	}
}

// Shutdown ends the tool's process now. Runs still going fail; the next one starts it again.
func (r Runtime) Shutdown(why string) { r.host().Shutdown(why) }
func (r Runtime) Runner() RunTask     { return r.host().Runner() }

var errNotKiro = errors.New("Only Kiro runs Kiro Web sessions.")

// Repos are the GitHub repos a Kiro Web session can be given (AcpHost.Repos). Blocks.
func (r Runtime) Repos() ([]string, error) {
	if r.Acp == nil {
		return nil, errNotKiro
	}
	return r.Acp.Repos()
}

// CloudSessions are the user's Kiro Web sessions (AcpHost.CloudSessions). Blocks.
func (r Runtime) CloudSessions() (CloudList, error) {
	if r.Acp == nil {
		return CloudList{}, errNotKiro
	}
	return r.Acp.CloudSessions()
}

// CloudTranscript is a Kiro Web session's conversation (AcpHost.CloudTranscript). Blocks.
func (r Runtime) CloudTranscript(id, folder string) ([]CloudTurn, error) {
	if r.Acp == nil {
		return nil, errNotKiro
	}
	return r.Acp.CloudTranscript(id, folder)
}
