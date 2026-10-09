package shell

import (
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/ui"
)

// officePlace is the office's hooks until the office view is ported: what the title bar's
// File menu and the island's Review ask of it.
type officePlace struct{}

func (officePlace) openSession(int32) {}
func (officePlace) newChat()          {}
func (officePlace) openFolder()       {}

// drawNotchView is the open notch's view: the office, with Settings over it.
func (s *Shell) drawNotchView(c *ui.Ctx, w, h float32) { s.drawOffice(c, w, h, false) }

// drawOffice is OfficeView in one of the two windows (dashboard says which).
func (s *Shell) drawOffice(c *ui.Ctx, w, h float32, dashboard bool) {
	in, ov, ph := s.notchS, &s.ovN, &s.phN
	if dashboard {
		in, ov, ph = s.dashS, &s.ovD, &s.phD
	}
	if in {
		ev, acts := ov.Layout(c, w, h, ui.OverlayState{
			Dashboard: dashboard, Sections: ui.SideOf(c.Pal), Current: int(s.pane.Section),
			Blocks: s.lastBlocks, Recording: s.pane.Recording, Menu: s.pane.Menu,
		})
		if len(ev) > 0 || len(acts) > 0 {
			ev = append([]ui.Event(nil), ev...)
			s.env.UIDo(func() { s.handlePage(ev, acts, dashboard) })
		}
		return
	}
	if open, esc := ph.Layout(c, w, h, dashboard); open || esc {
		s.env.UIDo(func() {
			if open {
				which := 0
				if dashboard {
					which = 1
				}
				s.ShowSettingsIn(which, app.SecGeneral)
			} else {
				s.escapeView(dashboard)
			}
		})
	}
}
