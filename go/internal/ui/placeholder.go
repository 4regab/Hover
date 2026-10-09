package ui

import "gioui.org/io/key"

// OfficePlaceholder stands where the office view goes until it is ported (phase 3's last
// step): the panel, a line saying so, and the way to Settings.
//
// ponytail: this is not in the Rust app. It is deleted when OfficeView lands.
type OfficePlaceholder struct {
	settings PillButton
	keys     Focus
}

// Layout draws it in w x h and returns whether Settings was asked for and whether Esc was
// pressed.
func (o *OfficePlaceholder) Layout(c *Ctx, w, h float32, dashboard bool) (settings, esc bool) {
	if dashboard {
		c.Box(0, 0, w, h, R(0), c.Pal.Panel)
	} else {
		c.Box(0, 0, w, h, R(32), c.Pal.Panel)
	}
	o.keys.Add(c, 0, 0, w, 1)
	for _, e := range o.keys.Keys(c, key.NameEscape) {
		if e.State == key.Press {
			esc = true
		}
	}
	c.Text("Agent office", 0, h/2-40, TextBox{Font: Font{Size: 18, Weight: 600}, Color: c.Pal.Ink, W: w, HAlign: Center})
	c.Text("The office view is not ported yet.", 0, h/2-14, TextBox{Font: Font{Size: 13}, Color: c.Pal.InkDim, W: w, HAlign: Center})
	p := NewPill(c.Pal, "Settings")
	p.Icon = IconSettings
	bw, _ := o.settings.Size(c, p)
	return o.settings.Layout(c, (w-bw)/2, h/2+14, 0, p), esc
}
