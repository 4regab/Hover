package agents

import "github.com/4regab/Hover/go/internal/core"

// field is the first of names that the call's input (a JSON object) has as a string
// that isn't empty.
func field(input *string, names []string) *string {
	if input == nil || len(*input) == 0 || (*input)[0] != '{' {
		return nil
	}
	v, err := core.ParseJSON(*input)
	if err != nil {
		return nil
	}
	for _, n := range names {
		if s, ok := str(v, n); ok && s != "" {
			return &s
		}
	}
	return nil
}

var agentKeys = []string{"subagent_type", "subagent", "agent_type", "agent_name", "agentName"}
