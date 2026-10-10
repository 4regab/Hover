package ui

import (
	"image"
	"image/color"

	"gioui.org/op/paint"
)

// The types office.slint's Office global passes between the app and the view.

// QOpt is a question's choice (label, description), and whether it is picked now.
type QOpt struct {
	Label, Desc string
	On          bool
}

// QData is one of the questions an agent asks the user (OpenCode's question tool): its
// header, the question, its choices, whether several may be picked, whether one's own
// answer may be typed, and what was typed so far.
type QData struct {
	Header, Question string
	Options          []QOpt
	Multiple, Custom bool
	Text             string
}

// AskData is what an agent asks before it acts, as its card shows it.
type AskData struct {
	ID, Title, Command, Path, Preview, Reason string
	Danger                                    bool
	Allow                                     string
	More                                      int
	Question                                  bool
	Qs                                        []QData
}

// MOpt is a model or an effort in the model menu (and an editor, an agent in the ⋯ menu).
type MOpt struct {
	ID, Label string
	On        bool
}

// MRate is a model's credit rate against Auto, as Kiro's picker tags it ("2.2x"); tone 1 is
// cheap (green), 2 dear (amber).
type MRate struct {
	Text string
	Tone int
}

// PopRow is a row of the reply box's @ / / list. Kind: 0 a group's label, 1 a file (name,
// dir), 2 a command (name, note), 3 a word that nothing matches. Idx counts what can be
// picked (-1 for the rest); Sel is the lit one.
type PopRow struct {
	Kind            int
	Name, Dir, Note string
	Sel             bool
	Idx             int
}

// AccessOpt is a choice in a menu: an access, a repo, a folder.
type AccessOpt struct {
	ID, Label, Note string
	On              bool
}

// ToolData is a tool in the new-task circle.
type ToolData struct {
	ID, Name string
	Ready    bool
	Hint     string
}

// PanelRow is a panel row. Kind: 0 heading (text, color, count), 1 card, 2 stats, 3 meter,
// 4 note, 5 search, 6 a history row (tool in sub, stage word in meta, its date in s1, the
// rest in count).
type PanelRow struct {
	Kind            int
	Text, Sub, Meta string
	Color           color.NRGBA
	Stage           int
	Count           string
	Open            int
	Key             string
	Pct             float32
	Desk            bool
	S1, S2, S3, S4  string
}

// ListRow is a row of the expanded chat's session list: a project folder (folded or not),
// or a session (live, or saved) with the tool's id, its stage and how long ago it ended
// ("" while it is at work).
type ListRow struct {
	Head  bool
	Text  string
	Tool  string
	Stage int
	On    bool
	When  string
	Shut  bool
}

// Thumb is a picture attached to a reply or a task, shown small. The app makes them once;
// the view makes their image ops once.
type Thumb struct {
	Img  *image.RGBA
	op   paint.ImageOp
	have bool
}

func (t *Thumb) imageOp() paint.ImageOp {
	if !t.have {
		t.op, t.have = paint.NewImageOp(t.Img), true
	}
	return t.op
}
