package chat

// Stage is how far a turn has got.
type Stage uint8

const (
	StageWaking Stage = iota
	StageWorking
	StageDone
	StageFailed
	StageStopped
)

// StepIcon is a step's icon (main.js ICON): the host sends one of these per step.
// Thought is the reasoning the tool exposed; Agent a subagent it started (OpenCode's task tool).
type StepIcon uint8

const (
	IconThink StepIcon = iota
	IconRead
	IconEdit
	IconRun
	IconSearch
	IconThought
	IconAgent
)

func ParseIcon(s string) StepIcon {
	switch s {
	case "read":
		return IconRead
	case "edit":
		return IconEdit
	case "run":
		return IconRun
	case "search":
		return IconSearch
	case "thought":
		return IconThought
	case "agent":
		return IconAgent
	}
	return IconThink
}

// Step is one step as the host sends it (KiroPage.Row): its kind's icon, a verb, and the
// file (name and folder) or the command it was about, with the change it made or what
// the command printed, and how it went. A string field is empty when the host sent none.
type Step struct {
	Kind      StepIcon
	Verb      string
	Name, Dir string
	Cmd       string
	Status    string
	Add, Del  int
	Diff, Out string
	Exit      int
	HasExit   bool
	Ms        float64
	HasMs     bool
	// Tag is the demo's own tag ("81 passed"): a check and the words.
	Tag string
}

func (s *Step) ended() bool { return s.Status == "completed" || s.Status == "failed" }

// hasBlock: a change or a command's output to open under the row (a thought's text and a
// subagent's result are drawn their own way).
func (s *Step) hasBlock() bool {
	return s.Kind != IconThought && s.Kind != IconAgent && (s.Diff != "" || s.Out != "")
}

// Turn is one turn as the office shows it.
type Turn struct {
	Prompt string
	// Images are the prompt's pasted images (URLs the painter's loader can fetch).
	Images []string
	Queued bool
	Steps  []Step
	// Took is how long it worked ("3m 07s"); TookMs the same in ms.
	Took      string
	TookMs    float64
	HasTookMs bool
	// Credits is what the turn cost ("0.09 credits"), when the tool says.
	Credits string
	Stage   Stage
	// Live: the turn running now (working, or waiting on the user).
	Live bool
	// Clock is how long the live turn has gone ("0:12").
	Clock string
	// When the prompt was sent (.me .when).
	When   string
	Status string
	Answer string
	// Waiting: the live turn waits on the user (a question, a permission).
	Waiting bool
	// Stopping: asked to stop or pause, and the tool hasn't said it has yet.
	Stopping bool
	// Restore and Again: its acts row offers Restore (the chat and the folder back to just
	// after this answer) and Try again (the folder back to before this message, which goes
	// again): a checkpoint was kept there and nothing runs.
	Restore, Again bool
}

func NewTurn(prompt string) Turn { return Turn{Prompt: prompt, Stage: StageDone} }

// Equal says whether two turns would be laid out alike.
func (t *Turn) Equal(o *Turn) bool {
	if len(t.Steps) != len(o.Steps) || len(t.Images) != len(o.Images) {
		return false
	}
	for i := range t.Steps {
		if t.Steps[i] != o.Steps[i] {
			return false
		}
	}
	for i := range t.Images {
		if t.Images[i] != o.Images[i] {
			return false
		}
	}
	return t.Prompt == o.Prompt && t.Queued == o.Queued && t.Took == o.Took && t.TookMs == o.TookMs && t.HasTookMs == o.HasTookMs &&
		t.Credits == o.Credits && t.Stage == o.Stage && t.Live == o.Live && t.Clock == o.Clock && t.When == o.When &&
		t.Status == o.Status && t.Answer == o.Answer && t.Waiting == o.Waiting && t.Stopping == o.Stopping &&
		t.Restore == o.Restore && t.Again == o.Again
}

func (t Turn) clone() Turn {
	t.Steps = append([]Step(nil), t.Steps...)
	t.Images = append([]string(nil), t.Images...)
	return t
}

// ActKind is what a click on a drawn control does.
type ActKind uint8

const (
	// ActCopy is a Copy button: Text is what it copies (a code block, a diff, an answer).
	ActCopy ActKind = iota + 1
	// ActStep: a step with a change or output opens or folds: I is the step, Now the "now" row.
	ActStep
	// ActFlag is one of a step's own switches: I is the step, K which. 0 shows all of a long
	// change, output or thought; 1 the subagents past the fourth; 2 + k subagent k's result.
	ActFlag
	// ActOpenDiff: a file under the answer: its change opens in the timeline (I is its step).
	ActOpenDiff
	// ActRetry: the newest turn's Retry: its prompt goes again.
	ActRetry
	// ActRestore: back to just after this turn's answer.
	ActRestore
	// ActTryAgain: back to just before this turn's message, which goes again.
	ActTryAgain
	// ActEdit: a queued reply taken back into the composer to be changed.
	ActEdit
	// ActEditPrompt: a sent prompt's Edit: its words go into the reply box to be changed and sent again.
	ActEditPrompt
	// ActSendNow: a queued reply sent now, ahead of the others.
	ActSendNow
)

type Act struct {
	Kind ActKind
	Text string
	I    int
	Now  bool
	K    uint32
}
