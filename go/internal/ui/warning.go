package ui

// app.slint's WarningWindow: MessageBox with the warning icon and OK. 440 wide, 20 of
// padding, the icon and the words 14 apart, then OK at the right.

// WarningView is the dialog's state between frames.
type WarningView struct{ ok PillButton }

func (v *WarningView) words(c *Ctx, msg string) (float32, float32) {
	return c.MeasureBox(msg, TextBox{Font: Font{Size: 13}, W: 440 - 40 - 28 - 14, Wrap: true})
}

// Height is the dialog's height: the taller of the icon and the words, then the button.
func (v *WarningView) Height(c *Ctx, msg string) float32 {
	_, th := v.words(c, msg)
	p := NewPill(c.Pal, "OK")
	p.PadX = 22
	_, bh := v.ok.Size(c, p)
	return 20 + max(28, th) + 16 + bh + 20
}

// Layout draws the dialog w wide and returns whether OK was pressed.
func (v *WarningView) Layout(c *Ctx, w float32, msg string) (ok bool) {
	c.Box(0, 0, w, 10000, R(0), c.Pal.Sheet)
	_, th := v.words(c, msg)
	c.Icon(IconWarning, 20, 20, 28, c.Pal.Orange)
	c.Text(msg, 20+28+14, 20, TextBox{Font: Font{Size: 13}, Color: c.Pal.Ink, W: w - 40 - 28 - 14, Wrap: true})
	p := NewPill(c.Pal, "OK")
	p.PadX, p.Bg, p.Fg = 22, c.Pal.Blue, White
	bw, _ := v.ok.Size(c, p)
	return v.ok.Layout(c, w-20-bw, 20+max(28, th)+16, 0, p)
}
