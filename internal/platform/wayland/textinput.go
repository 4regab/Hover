//go:build linux

package wayland

import (
	"unicode/utf8"

	"gioui.org/f32"
	"gioui.org/io/input"
	"gioui.org/io/key"
)

// zwp_text_input_v3: how the desktop's input method (fcitx5, ibus, the compositor's own, an
// on-screen keyboard) writes into a text box. The window tells it when a box has the keyboard,
// what surrounds the caret and where the caret is; it answers with a preedit (text being
// composed, shown underlined in the box), text to commit, and text to delete. Gio's editor
// already speaks this model (key.EditEvent replaces a range, key.CompositionEvent marks the
// composing range, key.SelectionEvent places the caret), so the answers are queued to it as
// those events and the editor does the rest.
//
// The preedit is real text in the box while it is composed, and replaced by every update.
// If the box loses the keyboard mid-composition it stays, as typed text: the window cannot
// tell which box would take an edit once the focus has moved.

// snippetReach is how many runes each side of the selection the input method is told.
const snippetReach = 100

// Gio's content hints, as the protocol's content_purpose, and the protocol's content_hint bits.
const (
	purposeNormal   = 0
	purposeNumber   = 3
	purposePhone    = 4
	purposeURL      = 5
	purposeEmail    = 6
	purposePassword = 8
	hintHiddenText  = 0x40
	hintSensitive   = 0x80

	causeInputMethod = 0
	causeOther       = 1
)

// imeSent is what the input method was last told, to send only what changed.
type imeSent struct {
	text           string
	hasText        bool
	cursor, anchor int32
	x, y, w, h     int32
	purpose, hint  uint32
}

type textInput struct {
	d    *Display
	obj  *Object
	surf *Win // the window the compositor gave the text input to (enter ... leave)

	on     bool   // enabled, as last committed
	serial uint32 // commits sent
	sent   imeSent
	// viaIM says the input method's own edit was just applied, so the change in the text it
	// sees next is its own; any other change is told as "other".
	viaIM bool
	// tries counts frames spent waiting for the editor's snippet.
	tries int
	asked key.Range

	// What the input method has said since the last done.
	preedit             string
	preBeg, preEnd      int32
	commit              string
	delBefore, delAfter uint32

	// The preedit as it stands in the editor, in runes; Start is -1 when there is none.
	comp key.Range
}

var noComp = key.Range{Start: -1, End: -1}

func newTextInput(d *Display, mgr, seat *Object) *textInput {
	ti := &textInput{d: d, comp: noComp}
	ti.obj = d.C.New("zwp_text_input_v3")
	d.C.Req(mgr.ID, textManagerGetInput).Obj(ti.obj).Obj(seat).Send()
	ti.obj.On = ti.event
	return ti
}

func (ti *textInput) event(op uint16, r *Reader) {
	switch op {
	case textInputEventEnter:
		surf := r.Obj()
		ti.reset()
		if surf != nil {
			ti.surf = ti.d.surfaces[surf.ID]
		}
		if ti.surf != nil {
			ti.update(ti.surf, true)
		}
	case textInputEventLeave:
		r.Obj()
		ti.reset()
		ti.surf = nil
	case textInputEventPreedit:
		ti.preedit, ti.preBeg, ti.preEnd = r.Str(), r.I32(), r.I32()
	case textInputEventCommit:
		ti.commit = r.Str()
	case textInputEventDelete:
		ti.delBefore, ti.delAfter = r.U32(), r.U32()
	case textInputEventDone:
		ti.done(r.U32())
	}
}

// reset forgets what the input method was told and what it has sent: after enter and leave
// all state is invalid.
func (ti *textInput) reset() {
	ti.on, ti.sent, ti.viaIM, ti.tries, ti.asked = false, imeSent{}, false, 0, noComp
	ti.preedit, ti.commit, ti.preBeg, ti.preEnd, ti.delBefore, ti.delAfter = "", "", 0, 0, 0, 0
	ti.forget()
}

// forget lets a preedit in the editor stand as plain text.
func (ti *textInput) forget() {
	if ti.comp.Start >= 0 && ti.surf != nil {
		ti.surf.router.Queue(key.CompositionEvent(noComp))
		ti.surf.dirty = true
	}
	ti.comp = noComp
}

// update tells the input method about the window's text box, enabling it when the window has
// one with the keyboard and disabling it when not. reopen: the box is a new one, so the input
// method starts over. Called after each frame and on enter.
func (ti *textInput) update(w *Win, reopen bool) {
	if ti.surf != w {
		return
	}
	c, id := ti.d.C, ti.obj.ID
	if !w.imeOn {
		if ti.on {
			c.Req(id, textInputDisable).Send()
			c.Req(id, textInputCommit).Send()
			ti.serial++
			ti.on, ti.sent, ti.tries = false, imeSent{}, 0
			ti.forget()
		}
		return
	}
	changed := false
	if !ti.on || reopen {
		ti.forget()
		c.Req(id, textInputEnable).Send()
		// A new text input has no earlier text to have changed, so its first text is not "other".
		ti.on, ti.sent, ti.tries, ti.asked, ti.viaIM = true, imeSent{}, 0, noComp, true
		changed = true
	}

	hint, _ := w.router.TextInputHint()
	n := imeSent{purpose: purposeNormal}
	switch hint {
	case key.HintPassword:
		n.purpose, n.hint = purposePassword, hintHiddenText|hintSensitive
	case key.HintNumeric:
		n.purpose = purposeNumber
	case key.HintTelephone:
		n.purpose = purposePhone
	case key.HintURL:
		n.purpose = purposeURL
	case key.HintEmail:
		n.purpose = purposeEmail
	}

	st := w.ime
	rs := st.Selection.Range
	lo, hi := min(rs.Start, rs.End), max(rs.Start, rs.End)
	// What surrounds the caret comes from the editor in a snippet; it is asked for, and
	// arrives with the next frame. A secret is never told: the input method would learn it.
	if n.purpose != purposePassword {
		if want := (key.Range{Start: max(0, lo-snippetReach), End: hi + snippetReach}); want != ti.asked {
			w.router.Queue(key.SnippetEvent(want))
			ti.asked = want
		}
		if text, cursor, anchor, ok := ti.surrounding(st.Snippet, rs); ok {
			n.text, n.hasText, n.cursor, n.anchor = text, true, cursor, anchor
			ti.tries = 0
		} else {
			// The editor has not answered yet: what the input method knows stands until it does.
			n.text, n.hasText, n.cursor, n.anchor = ti.sent.text, ti.sent.hasText, ti.sent.cursor, ti.sent.anchor
			if ti.tries++; ti.tries < 4 {
				w.dirty = true // look again once the editor has answered
			}
		}
	}

	kx, ky := w.ratio()
	car := st.Selection.Caret
	top := st.Selection.Transform.Transform(f32.Pt(car.Pos.X, car.Pos.Y-car.Ascent))
	bot := st.Selection.Transform.Transform(f32.Pt(car.Pos.X, car.Pos.Y+car.Descent))
	n.x, n.y = int32(float64(top.X)/kx), int32(float64(top.Y)/ky)
	n.w, n.h = 1, max(1, int32(float64(bot.Y-top.Y)/ky))

	if n != ti.sent {
		if n.hasText && (!ti.sent.hasText || n.text != ti.sent.text || n.cursor != ti.sent.cursor || n.anchor != ti.sent.anchor) {
			c.Req(id, textInputSetSurrounding).Str(n.text).I32(n.cursor).I32(n.anchor).Send()
			if !ti.viaIM {
				c.Req(id, textInputSetChangeCause).U32(causeOther).Send()
			}
			ti.viaIM = false
		}
		c.Req(id, textInputSetContentType).U32(n.hint).U32(n.purpose).Send()
		c.Req(id, textInputSetCursorRect).I32(n.x).I32(n.y).I32(n.w).I32(n.h).Send()
		ti.sent = n
		changed = true
	}
	if changed {
		c.Req(id, textInputCommit).Send()
		ti.serial++
	}
}

// surrounding is the text around the caret to tell the input method, with the preedit taken
// out (the protocol wants the text without it, the caret where it is). cursor and anchor are
// byte offsets in the text. ok is false while the snippet does not cover the selection yet.
func (ti *textInput) surrounding(sn key.Snippet, sel key.Range) (text string, cursor, anchor int32, ok bool) {
	rs := []rune(sn.Text)
	at := func(r int) int { return r - sn.Start }
	lo, hi := min(sel.Start, sel.End), max(sel.Start, sel.End)
	if at(lo) < 0 || at(hi) > len(rs) {
		return "", 0, 0, false
	}
	cur, anc := at(sel.Start), at(sel.End)
	if ti.comp.Start >= 0 {
		cs, ce := at(ti.comp.Start), at(ti.comp.End)
		if cs < 0 || ce > len(rs) || cs > ce {
			return "", 0, 0, false
		}
		rs = append(rs[:cs:cs], rs[ce:]...)
		cur, anc = cs, cs
	}
	bytesTo := func(n int) int32 { return int32(len(string(rs[:min(max(n, 0), len(rs))]))) }
	text = string(rs)
	if len(text) > 4000 {
		return "", 0, 0, false // 200 runes of at most 4 bytes fit; a longer snippet is not told
	}
	return text, bytesTo(cur), bytesTo(anc), true
}

// runesAround converts the byte lengths of delete_surrounding_text, which count from the
// caret in the text the input method was told, to runes.
func (ti *textInput) runesAround(before, after uint32) (nb, na int) {
	t := ti.sent.text
	lo, hi := int(min(ti.sent.cursor, ti.sent.anchor)), int(max(ti.sent.cursor, ti.sent.anchor))
	if lo > len(t) || hi > len(t) {
		return 0, 0
	}
	if b := int(before); b <= lo {
		nb = utf8.RuneCountInString(t[lo-b : lo])
	}
	if a := int(after); hi+a <= len(t) {
		na = utf8.RuneCountInString(t[hi : hi+a])
	}
	return nb, na
}

// done applies what the input method sent, in the order the protocol gives: the old preedit
// goes, the requested text is deleted, the commit goes in with the caret after it, and the
// new preedit goes in with the caret inside it.
func (ti *textInput) done(serial uint32) {
	pre, commit, before, after := ti.preedit, ti.commit, ti.delBefore, ti.delAfter
	cb, ce := ti.preBeg, ti.preEnd
	ti.preedit, ti.commit, ti.preBeg, ti.preEnd, ti.delBefore, ti.delAfter = "", "", 0, 0, 0, 0
	w := ti.surf
	if w == nil || !ti.on {
		return
	}
	q := &w.router
	edit := func(s, e int, text string) {
		q.Queue(key.EditEvent{Range: key.Range{Start: s, End: e}, Text: text})
	}
	sel := w.ime.Selection.Range
	s, e := min(sel.Start, sel.End), max(sel.Start, sel.End)

	if ti.comp.Start >= 0 {
		edit(ti.comp.Start, ti.comp.End, "")
		s, e = ti.comp.Start, ti.comp.Start
		ti.comp = noComp
	}
	if before > 0 || after > 0 {
		nb, na := ti.runesAround(before, after)
		if na > 0 {
			edit(e, e+na, "")
		}
		if nb > 0 {
			edit(s-nb, s, "")
			s, e = s-nb, e-nb
		}
	}
	switch {
	case commit != "":
		edit(s, e, commit)
		s += utf8.RuneCountInString(commit)
		e = s
	case pre != "" && s != e:
		// The selection gives way to what is being composed.
		edit(s, e, "")
		e = s
	}

	selStart, selEnd := s, s
	if pre != "" {
		edit(s, s, pre)
		n := utf8.RuneCountInString(pre)
		ti.comp = key.Range{Start: s, End: s + n}
		inPre := func(b int32) int {
			if b < 0 {
				return n
			}
			i := min(int(b), len(pre))
			for i > 0 && i < len(pre) && !utf8.RuneStart(pre[i]) {
				i--
			}
			return utf8.RuneCountInString(pre[:i])
		}
		// The cursor range is shown as a caret at its end: Gio can't mark a part of the
		// composition, and a selection left behind by an abandoned one would be replaced by
		// the next text typed.
		_ = cb
		selStart = s + inPre(ce)
		selEnd = selStart
	}
	q.Queue(key.CompositionEvent(ti.comp))
	q.Queue(key.SelectionEvent{Start: selStart, End: selEnd})
	w.ime.Selection.Range = key.Range{Start: selStart, End: selEnd}
	ti.viaIM = true
	w.dirty = true
	_ = serial
}

// imeFrame is called after each frame: the window's text input state, and then the input
// method told what it needs.
func (w *Win) imeFrame() {
	ti := (*textInput)(nil)
	if w.d.seat != nil {
		ti = w.d.seat.ti
	}
	reopen := false
	switch w.router.TextInputState() {
	case input.TextInputOpen:
		w.imeOn, reopen = true, true
	case input.TextInputClose:
		w.imeOn = false
	}
	if ti != nil {
		ti.update(w, reopen)
	}
}
