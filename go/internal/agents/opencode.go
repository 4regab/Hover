package agents

// opencode.rs.

// Questioning (OpenCodeHost.Questioning) asks the user a question the agent has
// (AgentAsk.Questions), for the agent's session id named first. The answer is each
// question's picked labels, in order; nil when the user skipped it.
type Questioning func(sid string, ask AgentAsk, ct *Cancel, reply func(Answers))
