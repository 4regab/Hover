package office

import (
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"testing"
)

// crates/hover-office/tests/office.rs and helpers.rs: the office's model against the
// page: the fixture's sessions become bots at their desks, clicks on them open them, a new
// session walks in through the door and a gone one walks out, the camera keeps to its
// limits, the pacing slows when idle; and a session's subagents as helpers at its desk.
// Expected values are main.js's own constants (DESKS, DOOR, the zoom limits).

func fixture(t testing.TB) (state map[string]any, now float64) {
	_, f, _, _ := runtime.Caller(0)
	b, err := os.ReadFile(filepath.Join(filepath.Dir(f), "..", "..", "tests", "golden", "fixtures", "office-state.json"))
	if err != nil {
		t.Fatal(err)
	}
	var fx map[string]any
	if err := json.Unmarshal(b, &fx); err != nil {
		t.Fatal(err)
	}
	return fx["state"].(map[string]any), fx["now"].(float64)
}

func clone(v any) any {
	b, _ := json.Marshal(v)
	var o any
	_ = json.Unmarshal(b, &o)
	return o
}

type clk float64

func (c *clk) run(o *Office, frames int) {
	for i := 0; i < frames; i++ {
		o.Frame(float64(*c), 16)
		*c += 16
	}
}

// settled is the office settled with the fixture's sessions at their desks (6.4 s, past
// the capture's 6 s settle: the done bubble has gone by then).
func settled(t testing.TB) (*Office, *clk, map[string]any) {
	state, now := fixture(t)
	o := NewOffice(1104, 424, false)
	o.WallClock = func() (float64, int) { return now, 0 }
	o.State(state)
	c := new(clk)
	c.run(o, 400)
	return o, c, state
}

func (o *Office) project(x, y, z float64) (float64, float64) {
	view, proj := o.Camera()
	p := proj.Mul(view).Point(v3(x, y, z))
	return (p.X + 1) / 2 * 1104, (1 - p.Y) / 2 * 424
}

func TestTheFixturesSessionsSitAtTheirDesks(t *testing.T) {
	o, _, _ := settled(t)
	if len(o.Sessions) != 5 {
		t.Fatal(len(o.Sessions))
	}
	var names, bubbles []string
	for _, s := range o.Sessions {
		if !s.B.Seated {
			t.Errorf("%s is not seated", s.B.Name)
		}
		if math.Abs(s.B.X-(Seat(s.Desk)+0.05)) >= 1e-9 || math.Abs(s.B.Z-Desks[s.Desk][1]) >= 1e-9 {
			t.Errorf("%s is not at its desk", s.B.Name)
		}
		names = append(names, s.B.Name)
		bubbles = append(bubbles, Bubble(s))
	}
	if !reflect.DeepEqual(names, []string{"Pip", "Juno", "Moss", "Nova", "Ada"}) {
		t.Fatal(names)
	}
	// bubbleFor, for each stage in the fixture.
	if want := []string{"Reading refresh.ts", "", "Waking up…", "Couldn’t finish", "z z z"}; !reflect.DeepEqual(bubbles, want) {
		t.Fatalf("%q", bubbles)
	}
}

func TestAClickOnABotOpensItsSessionAndPropsDoTheirThing(t *testing.T) {
	o, _, _ := settled(t)
	for _, s := range o.Tags() {
		// Just under the tag: the bot's head.
		o.Pointer, o.HasPointer = [2]float64{s.X, s.Y + 12}, true
		o.Pick()
		// A sleeping bot's head is down on its desk: under its tag is only air over the
		// desk, where neither surface is under the pointer, and main.js gives that to the desk.
		want := Click{Kind: ClickOpen, ID: s.ID}
		if s.Stage == StageStopped {
			want = Click{Kind: ClickDesk, ID: s.ID, X: s.X, Y: s.Y + 12}
		}
		if got := o.Click(); got != want {
			t.Errorf("%s: %+v, want %+v", s.Name, got, want)
		}
	}
	// The TV and the window, through their hit boxes.
	x, y := o.project(5.4, 2.38, -5.3)
	o.Pointer = [2]float64{x, y}
	o.Pick()
	if got := o.Click(); got != (Click{Kind: ClickPanel, Panel: "tv"}) {
		t.Errorf("the TV: %+v", got)
	}
	x, y = o.project(1.5, 2.3, -5.3)
	o.Pointer = [2]float64{x, y}
	o.Pick()
	if o.Hint(PropWindow) != "Make it day" {
		t.Error(o.Hint(PropWindow))
	}
	if o.Hovered.Kind != HoverProp || o.Hovered.Prop != PropWindow || o.Click().Kind != ClickTime {
		t.Errorf("the window: %+v", o.Hovered)
	}
	o.Pointer = [2]float64{5, 5}
	o.Pick()
	if got := o.Click(); got.Kind != ClickNothing {
		t.Errorf("nothing: %+v", got)
	}
}

func TestAClickOnADeskWithASessionOpensItsCardAndTheBotStillWinsOverItsDesk(t *testing.T) {
	o, _, _ := settled(t)
	type pair struct {
		id   int64
		desk int
	}
	var seated []pair
	for _, s := range o.Sessions {
		seated = append(seated, pair{s.ID, s.Desk})
	}
	for _, p := range seated {
		dx, dz := Desks[p.desk][0], Desks[p.desk][1]
		// The monitor is in front of the seated bot seen from the camera: both hit boxes are
		// under the pointer, and the room's surface is what it is on.
		x, y := o.project(dx+0.11, 1.1, dz)
		o.Pointer, o.HasPointer = [2]float64{x, y}, true
		o.Pick()
		if o.Hovered != (Hover{Kind: HoverDesk, ID: p.id}) {
			t.Errorf("desk %d: the monitor: %+v", p.desk, o.Hovered)
		}
		if got := o.Click(); got != (Click{Kind: ClickDesk, ID: p.id, X: x, Y: y}) {
			t.Errorf("desk %d: %+v", p.desk, got)
		}
		// The bot's own head, over its desk's box.
		for _, tag := range o.Tags() {
			if tag.ID == p.id && tag.Stage != StageStopped {
				o.Pointer, o.HasPointer = [2]float64{tag.X, tag.Y + 12}, true
				o.Pick()
				if got := o.Click(); got != (Click{Kind: ClickOpen, ID: p.id}) {
					t.Errorf("desk %d: the head: %+v", p.desk, got)
				}
			}
		}
	}
	// A desk nobody sits at is just the room.
	free := 6
	for d := 0; d < 6 && free == 6; d++ {
		taken := false
		for _, s := range o.Sessions {
			taken = taken || s.Desk == d
		}
		if !taken {
			free = d
		}
	}
	if free < 6 {
		x, y := o.project(Desks[free][0]+0.11, 1.1, Desks[free][1])
		o.Pointer, o.HasPointer = [2]float64{x, y}, true
		o.Pick()
		if got := o.Click(); got.Kind != ClickNothing {
			t.Errorf("a free desk: %+v", got)
		}
	}
}

// withNewSession adds a copy of session 2 as session id, at desk 5 with bot 5.
func withNewSession(state map[string]any, id int) map[string]any {
	s := clone(state).(map[string]any)
	list := s["sessions"].([]any)
	n := clone(list[2]).(map[string]any)
	n["id"], n["seat"], n["bot"] = float64(id), 5.0, 5.0
	s["sessions"] = append(list, n)
	return s
}

func TestANewSessionWalksInAndAGoneOneWalksOut(t *testing.T) {
	o, _, state := settled(t)
	// A sixth session, waking: in through the door, then seated.
	o.State(withNewSession(state, 9))
	find := func(id int64) *Session {
		for _, s := range o.Sessions {
			if s.ID == id {
				return s
			}
		}
		return nil
	}
	n := find(9)
	if n == nil || n.B.Seated || !n.B.Walking() {
		t.Fatal("the new session should be walking in")
	}
	if Bubble(n) != "On my way…" {
		t.Error(Bubble(n))
	}
	for i := 400; i < 1100; i++ {
		o.Frame(float64(i)*16, 16)
	}
	n = find(9)
	if !n.B.Seated || n.B.Walking() {
		t.Error("it should have sat down")
	}
	if n.Last().Stage != StageWaking {
		t.Error(n.Last().Stage)
	}
	// Gone from the state: retired.
	o.State(state)
	if find(9) != nil {
		t.Error("it should be retired")
	}
}

func TestTheCameraKeepsToItsLimitsAndThePaceStaysFlatWhenIdle(t *testing.T) {
	o, _, state := settled(t)
	o.ZoomBy(10, 0, 0)
	if o.User[2] != 2.8 {
		t.Fatal(o.User)
	}
	o.ZoomBy(0.01, 0, 0)
	if o.User[2] != 0.85 {
		t.Fatal(o.User)
	}
	o.Drag(10000, 0)
	if math.Abs(o.User[0]) > 6 || math.Abs(o.User[1]) > 5 {
		t.Fatal(o.User)
	}
	o.ResetView()
	if o.User != [3]float64{0, 0, 1} {
		t.Fatal(o.User)
	}
	// The pace is flat (one fixed schedule, PR #44): the empty office draws as often as a
	// busy one, not at 10 fps.
	e := NewOffice(1104, 424, false)
	empty := clone(state).(map[string]any)
	empty["sessions"] = []any{}
	e.State(empty)
	for i := 0; i < 1000; i++ {
		e.Frame(float64(i)*16, 16)
	}
	late := 0
	for i := 1000; i < 1625; i++ {
		if e.Frame(float64(i)*16, 16) {
			late++
		}
	}
	// 10 s at a flat 60 fps schedule, on a 16 ms clock that draws every other tick: about 312.
	if late < 300 || late > 325 {
		t.Fatalf("%d frames in 10 s", late)
	}
}

// Sessions come and go all day in an office that stays open: a bot that walked out is used
// again for the next session with its name, so the scene stops growing once each name has
// had its bot, and the new one still walks in and sits down.
func TestBotsThatLeftAreUsedAgain(t *testing.T) {
	o, _, state := settled(t)
	n := 400
	run := func(k int) {
		for i := 0; i < k; i++ {
			o.Frame(float64(n)*16, 16)
			n++
		}
	}
	var sizes []int
	for id := 100; id < 106; id++ {
		o.State(withNewSession(state, id))
		run(700)
		var b *Session
		for _, s := range o.Sessions {
			if s.ID == int64(id) {
				b = s
			}
		}
		if b == nil || !b.B.Seated || b.B.Walking() {
			t.Fatalf("session %d should have sat down", id)
		}
		o.State(state)
		run(700)
		sizes = append(sizes, len(o.G.Nodes))
	}
	for i := 1; i < len(sizes); i++ {
		if sizes[i] != sizes[i-1] {
			t.Fatalf("the scene grew: %v", sizes)
		}
	}
}

// ---- helpers (subagents) ----------------------------------------------------------------

// withAgents is the fixture's state with the newest turn of session i holding one subagent
// row per status (as state.rs writes them: k "agent", a status).
func withAgents(state map[string]any, i int, statuses []string) map[string]any {
	s := clone(state).(map[string]any)
	sess := s["sessions"].([]any)[i].(map[string]any)
	turns := sess["turns"].([]any)
	last := turns[len(turns)-1].(map[string]any)
	var rows []any
	for _, st := range statuses {
		rows = append(rows, map[string]any{"k": "agent", "verb": "Subagent", "status": st})
	}
	last["steps"] = rows
	return s
}

func repeat(s string, n int) []string {
	out := make([]string, n)
	for i := range out {
		out[i] = s
	}
	return out
}

func out(o *Office, i int) int {
	n := 0
	for _, m := range o.Crew.Minis {
		if m.Sid == o.Sessions[i].ID && !m.Leaving {
			n++
		}
	}
	return n
}

func TestABusySeatedSessionGetsAHelperPerRunningSubagent(t *testing.T) {
	o, c, state := settled(t)
	if len(o.Crew.Minis) != 0 {
		t.Fatal("the fixture has no subagents")
	}
	if o.Sessions[0].Last().Stage != StageWorking {
		t.Fatal(o.Sessions[0].Last().Stage)
	}
	// Two out, one back already: two helpers.
	o.State(withAgents(state, 0, []string{"in_progress", "completed", "in_progress"}))
	if o.Sessions[0].Last().Agents != 2 {
		t.Fatal(o.Sessions[0].Last().Agents)
	}
	c.run(o, 60)
	if out(o, 0) != 2 || len(o.Crew.Minis) != 2 {
		t.Fatalf("%d out, %d helpers", out(o, 0), len(o.Crew.Minis))
	}
	desk := o.Sessions[0].Desk
	shown := o.G.Shown()
	for _, m := range o.Crew.Minis {
		sx, sz := Spot(desk, m.Slot)
		p := m.At(o.G)
		if math.Abs(p.X-sx) >= 1e-6 || math.Abs(p.Z-sz) >= 1e-6 {
			t.Errorf("slot %d stands at its spot: %v vs %v,%v", m.Slot, p, sx, sz)
		}
		if p.Y >= 0.02 {
			t.Error("not on the floor")
		}
		// Around the desk, clear of the chair side: within reach of the desk's centre.
		if math.Abs(p.X-Desks[desk][0]) > 0.75 || math.Abs(p.Z-Desks[desk][1]) > 1.1 {
			t.Error("too far from the desk")
		}
		if m.Out != 1 {
			t.Error(m.Out)
		}
		// Small: about MINI the bot's size.
		if math.Abs(o.G.Nodes[m.Root].S.X-MiniScale) >= 1e-9 {
			t.Error(o.G.Nodes[m.Root].S.X)
		}
		// Facing the room: the camera looks from +x and +z, so it faces that way, as the
		// page's three-quarter turn does.
		if !(m.Yaw > 0.2 && m.Yaw < 1.4 && math.Abs(m.Yaw-math.Pi/4) > 0.5) {
			t.Errorf("yaw %v", m.Yaw)
		}
		if !shown[m.Root] {
			t.Error("hidden")
		}
	}
	// Two places, both different; the tags carry each helper's colour, which is not the bot's.
	if o.Crew.Minis[0].Slot == o.Crew.Minis[1].Slot {
		t.Error("same place")
	}
	var tag Tag
	for _, x := range o.Tags() {
		if x.ID == o.Sessions[0].ID {
			tag = x
		}
	}
	if len(tag.Helpers) != 2 || tag.Helpers[0] == tag.Helpers[1] || tag.Helpers[0] == tag.Color || tag.Helpers[1] == tag.Color {
		t.Errorf("helpers' colours: %v vs %v", tag.Helpers, tag.Color)
	}
	// The other desks have none.
	for _, x := range o.Tags() {
		if x.ID != tag.ID && len(x.Helpers) != 0 {
			t.Error("another desk has helpers")
		}
	}
}

func TestHelpersComeAndGoWithTheSubagentsAndAreAtMostFour(t *testing.T) {
	o, c, state := settled(t)
	o.State(withAgents(state, 0, repeat("in_progress", 6)))
	c.run(o, 60)
	if out(o, 0) != len(MiniSpots) {
		t.Fatalf("four places to stand: %d", out(o, 0))
	}
	var slots []int
	for _, m := range o.Crew.Minis {
		slots = append(slots, m.Slot)
	}
	sort.Ints(slots)
	if !reflect.DeepEqual(slots, []int{0, 1, 2, 3}) {
		t.Fatal(slots)
	}
	// Down to one: the newest hop back into their bot and are gone within the hop's 0.6 s.
	o.State(withAgents(state, 0, []string{"in_progress", "completed", "failed"}))
	c.run(o, 3)
	if out(o, 0) != 1 || len(o.Crew.Minis) != 4 {
		t.Fatalf("the three still on their way back: %d out, %d helpers", out(o, 0), len(o.Crew.Minis))
	}
	if !o.Crew.Moving() {
		t.Error("hopping: the shadows follow every frame")
	}
	c.run(o, 40)
	if len(o.Crew.Minis) != 1 || o.Crew.Minis[0].Slot != 0 {
		t.Fatal("the first one stays")
	}
	// None: the last goes too.
	o.State(withAgents(state, 0, []string{"completed"}))
	c.run(o, 60)
	if len(o.Crew.Minis) != 0 || o.Crew.Any() {
		t.Error("the last one stays")
	}
}

func TestOnlyABusyBotAtItsDeskHasHelpers(t *testing.T) {
	o, c, state := settled(t)
	// Pip's done (or failed): nothing to help with, whatever rows say.
	failed := -1
	for i, s := range o.Sessions {
		if s.Last().Stage == StageFailed {
			failed = i
		}
	}
	o.State(withAgents(state, failed, []string{"in_progress", "in_progress"}))
	c.run(o, 60)
	if len(o.Crew.Minis) != 0 {
		t.Fatal("a failed session has helpers")
	}
	// A session that walks in is not at its desk until it has sat down: no helpers before.
	s2 := withAgents(state, 2, []string{"in_progress"})
	list := s2["sessions"].([]any)
	n := clone(list[2]).(map[string]any)
	n["id"], n["seat"], n["bot"] = 77.0, 5.0, 5.0
	s2["sessions"] = append(list, n)
	o.State(s2)
	idx := -1
	for i, s := range o.Sessions {
		if s.ID == 77 {
			idx = i
		}
	}
	if idx < 0 || !o.Sessions[idx].B.Walking() {
		t.Fatal("the new session should be walking")
	}
	c.run(o, 5)
	if out(o, idx) != 0 {
		t.Error("walking in")
	}
	c.run(o, 700)
	if !o.Sessions[idx].B.Seated {
		t.Fatal("not seated")
	}
	if out(o, idx) != 1 {
		t.Error("seated and waking up, with a subagent out")
	}
	// The session goes: its helpers hop back in and are gone, with its bot on the way out.
	o.State(state)
	c.run(o, 60)
	for _, m := range o.Crew.Minis {
		if m.Sid == 77 {
			t.Error("a helper of the gone session")
		}
	}
}

func TestHelpersAreUsedAgainSoTheSceneStopsGrowing(t *testing.T) {
	o, c, state := settled(t)
	var sizes []int
	for round := 0; round < 6; round++ {
		o.State(withAgents(state, 0, repeat("in_progress", 4)))
		c.run(o, 60+round)
		if len(o.Crew.Minis) != 4 {
			t.Fatal(len(o.Crew.Minis))
		}
		o.State(withAgents(state, 0, repeat("completed", 4)))
		c.run(o, 60)
		if len(o.Crew.Minis) != 0 {
			t.Fatal("helpers stay")
		}
		sizes = append(sizes, len(o.G.Nodes))
	}
	for i := 1; i < len(sizes); i++ {
		if sizes[i] != sizes[i-1] {
			t.Fatalf("the scene grew: %v", sizes)
		}
	}
	// Nodes hidden with the helpers: nothing of them is drawn once they are gone.
	drawn := func() int {
		n := 0
		shown := o.G.Shown()
		for i := range o.G.Nodes {
			if shown[i] && o.G.Nodes[i].Draw != nil {
				n++
			}
		}
		return n
	}
	empty := drawn()
	o.State(withAgents(state, 0, repeat("in_progress", 2)))
	c.run(o, 60)
	if drawn() <= empty+2*20 {
		t.Errorf("two helpers draw tens of boxes: %d vs %d", drawn(), empty)
	}
	// Even all desks' helpers stay within the renderer's draw buffer (2048) with room to spare.
	if drawn()+22*50 >= 2048 {
		t.Errorf("%d", drawn())
	}
}

func TestAHelperIsRecolouredFromItsBotAndDoesItsPaperwork(t *testing.T) {
	o, c, state := settled(t)
	o.State(withAgents(state, 0, []string{"in_progress", "in_progress"}))
	// Each duty comes round, the file one hands a sheet to the desk's tray.
	seen := map[Duty]bool{}
	air, movingFrames, frames := 0, 0, 0
	for i := 0; i < 1200; i++ {
		c.run(o, 1)
		for _, m := range o.Crew.Minis {
			seen[m.DutyNow()] = true
		}
		air = max(air, o.Crew.SheetsInAir())
		all := true
		for _, m := range o.Crew.Minis {
			all = all && m.Out == 1
		}
		if all {
			frames++
			if o.Crew.Moving() {
				movingFrames++
			}
		}
	}
	for _, d := range []Duty{DutyWrite, DutyStamp, DutyFlip, DutyFile} {
		if !seen[d] {
			t.Errorf("duty %v never came round", d)
		}
	}
	if air < 1 {
		t.Error("a sheet was handed in")
	}
	// The redraw rule: only while a sheet is in the air do the shadows need every frame.
	if movingFrames*2 >= frames {
		t.Errorf("%d of %d frames", movingFrames, frames)
	}
	// The palette: the bot's hue turned per slot, lighter, within three.js's limits.
	bot := Hex(0x9b6bff)
	h0, _, l0 := bot.HSL()
	hue := [4]float64{0.5, 0.17, -0.17, 0.33}
	var mains []Rgb
	for k := 0; k < 4; k++ {
		pal := Palette(bot, k)
		mains = append(mains, pal[0])
		h, _, l := pal[0].HSL()
		want := math.Mod(h0+hue[k]+1, 1)
		if !(math.Abs(h-want) < 1e-6 || math.Abs(h-want) > 1-1e-6) {
			t.Errorf("slot %d: hue %v vs %v", k, h, want)
		}
		if l > 0.7+1e-9 || math.Abs(l-math.Min(l0+0.06, 0.7)) >= 1e-6 {
			t.Errorf("slot %d: lightness %v", k, l)
		}
		if pal[1] != pal[0].Mul(0.5) {
			t.Errorf("slot %d: dark", k)
		}
		if pal[2].R < pal[0].R || pal[2].G < pal[0].G || pal[2].B < pal[0].B {
			t.Errorf("slot %d: pale", k)
		}
	}
	for a := 0; a < 4; a++ {
		for b := a + 1; b < 4; b++ {
			if mains[a] == mains[b] {
				t.Errorf("slots %d and %d share a colour", a, b)
			}
		}
	}
}

func TestTheColourRoundTripMatchesThree(t *testing.T) {
	for _, h := range []uint32{0x9b6bff, 0x2fc9b0, 0xff9a4a, 0xff6fae, 0x5aa8ff, 0xb4e04a, 0x808080, 0x000000, 0xffffff} {
		c := Hex(h)
		a, b, l := c.HSL()
		d := FromHSL(a, b, l)
		if math.Abs(c.R-d.R) >= 1e-9 || math.Abs(c.G-d.G) >= 1e-9 || math.Abs(c.B-d.B) >= 1e-9 {
			t.Errorf("%x", h)
		}
	}
	// A pure red: hue 0, full saturation, lightness ½ (in the linear space).
	if h, s, l := (Rgb{1, 0, 0}).HSL(); h != 0 || s != 1 || l != 0.5 {
		t.Errorf("red: %v %v %v", h, s, l)
	}
	// Hue wraps.
	if FromHSL(1.25, 1, 0.5) != FromHSL(0.25, 1, 0.5) {
		t.Error("the hue does not wrap")
	}
}

func TestThePaceIsTheSameOnceTheHelpersAreGone(t *testing.T) {
	o, c, state := settled(t)
	// Everyone done or sent away: an empty office, so nothing is lively but the helpers.
	e := clone(state).(map[string]any)
	e["sessions"] = []any{}
	o.State(withAgents(state, 0, []string{"in_progress"}))
	c.run(o, 60)
	if !o.Crew.Any() || !o.Lively {
		t.Fatal("helpers are at work")
	}
	o.State(e)
	c.run(o, 1500)
	if len(o.Crew.Minis) != 0 || len(o.Sessions) != 0 {
		t.Fatal("the office should be empty")
	}
	late := 0
	for i := 0; i < 625; i++ {
		if o.Frame(float64(*c), 16) {
			late++
		}
		*c += 16
	}
	// The pace is flat (PR #44): nothing slows it once the helpers are gone, so about 312 frames in 10 s.
	if late < 300 || late > 325 {
		t.Fatalf("%d frames in 10 s: the same flat pace", late)
	}
}
