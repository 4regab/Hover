package chat

import (
	"fmt"
	"strings"
	"testing"
)

// geometry is the laid-out thread: positions, sizes, clips and copy tokens. A streamed
// answer must match a thread that saw only the final text.
func geometry(th *Thread) string {
	var b strings.Builder
	fmt.Fprintf(&b, "height %.4f sections %d\n", th.Height, len(th.Sections))
	for si, s := range th.Sections {
		fmt.Fprintf(&b, "sec %d y %.4f h %.4f texts %d shapes %d scrolls %d hits %d\n",
			si, s.Y, s.H, len(s.Frag.Texts), len(s.Frag.Shapes), len(s.Frag.Scrollers), len(s.Frag.Hits))
		for i, t := range s.Frag.Texts {
			fmt.Fprintf(&b, "t %d x %.4f y %.4f w %.4f h %.4f %q\n", i, t.X, t.Y, t.Layout.Width(), t.Layout.Height(), t.Text)
			if t.Clip != nil {
				fmt.Fprintf(&b, "clip %.4f %.4f %.4f %.4f\n", t.Clip[0], t.Clip[1], t.Clip[2], t.Clip[3])
			}
		}
		for i, sh := range s.Frag.Shapes {
			fmt.Fprintf(&b, "sh %d k %d x %.4f y %.4f w %.4f h %.4f src %q\n", i, sh.Kind, sh.X, sh.Y, sh.W, sh.H, sh.Src)
		}
		for i, sc := range s.Frag.Scrollers {
			fmt.Fprintf(&b, "sc %d %.4f %.4f %.4f %.4f c %.4f\n", i, sc.Clip[0], sc.Clip[1], sc.Clip[2], sc.Clip[3], sc.Content)
		}
		for i, h := range s.Frag.Hits {
			fmt.Fprintf(&b, "hit %d %.4f %.4f %.4f %.4f k %d %q\n", i, h.R[0], h.R[1], h.R[2], h.R[3], h.Act.Kind, h.Act.Text)
		}
		for i, tok := range s.Frag.Copy {
			fmt.Fprintf(&b, "tok %d k %d n %d %q\n", i, tok.Kind, tok.N, tok.S)
		}
	}
	return b.String()
}

func mustMatch(t *testing.T, streamed, cold *Thread) {
	t.Helper()
	a, b := geometry(streamed), geometry(cold)
	if a == b {
		return
	}
	as, bs := strings.Split(a, "\n"), strings.Split(b, "\n")
	n := min(len(as), len(bs))
	for i := 0; i < n; i++ {
		if as[i] != bs[i] {
			t.Fatalf("line %d\nstream: %s\ncold:   %s", i, as[i], bs[i])
		}
	}
	t.Fatalf("geometry length %d vs %d", len(as), len(bs))
}

func TestStreamedAnswerMatchesAColdLayout(t *testing.T) {
	src := rich(t)
	turns := []Turn{
		turnOf("Earlier", src),
		turnOf("Now?", src),
	}
	th := juno(t)
	th.Set(turns, 360)
	cold0 := juno(t)
	cold0.Set(append([]Turn(nil), turns...), 360)
	mustMatch(t, th, cold0)

	acc := src
	for _, chunk := range []string{" more", " text", "\n\nAnother paragraph with **bold**.", "\n\n- one\n- two"} {
		acc += chunk
		turns[1].Answer = acc
		th.Set(turns, 360)
		cold := juno(t)
		cold.Set(append([]Turn(nil), turns...), 360)
		mustMatch(t, th, cold)
	}

	th.Set(turns, 280)
	cold := juno(t)
	cold.Set(append([]Turn(nil), turns...), 280)
	mustMatch(t, th, cold)

	turns[0].Answer = "Edited in the middle, with `code`."
	th.Set(turns, 280)
	cold = juno(t)
	cold.Set(append([]Turn(nil), turns...), 280)
	mustMatch(t, th, cold)

	var copied string
	for _, s := range cold.Sections {
		for _, h := range s.Frag.Hits {
			if h.Act.Kind == ActCopy && strings.Contains(h.Act.Text, "expired") {
				copied = h.Act.Text
			}
		}
	}
	if copied == "" {
		t.Fatal("no code block to copy")
	}
	th.Copied, cold.Copied = &copied, &copied
	th.Set(turns, 280)
	cold.Set(append([]Turn(nil), turns...), 280)
	mustMatch(t, th, cold)

	// An image's size arrives after the text. ImageChanged is what asks the section
	// to be laid out again; the block that holds the image is shaped from the new size.
	const img = "https://example.com/flow.png"
	state := ImageState{Kind: ImageReady, W: 400, H: 200}
	see := func(string) ImageState { return state }
	th.ImageState = see
	if !th.ImageChanged(img) {
		t.Fatal("the answer does not show the image")
	}
	th.Set(turns, 280)
	fresh := juno(t)
	fresh.ImageState = see
	fresh.Copied = &copied
	fresh.Set(append([]Turn(nil), turns...), 280)
	mustMatch(t, th, fresh)
}
