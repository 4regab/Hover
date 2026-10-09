package office

import (
	"bytes"
	"math"
	"testing"
)

// The byte tables give what the float ones did, pixel for pixel.
func TestTheByteTablesComposeAsTheFloatOnesDid(t *testing.T) {
	w, h := 97, 41
	seed := uint32(7)
	frame := make([]byte, w*h*4)
	for i := range frame {
		seed = seed*1664525 + 1013904223
		frame[i] = byte(seed >> 24)
	}
	for _, day := range []bool{false, true} {
		// The float tables, as they were.
		white := bytes.Repeat([]byte{255}, w*h*4)
		black := bytes.Repeat([]byte{0, 0, 0, 255}, w*h)
		clear := Compose(make([]byte, w*h*4), w, h, day)
		wh, bl := Compose(white, w, h, day), Compose(black, w, h, day)
		var want []byte
		for i := 0; i < w*h; i++ {
			p := frame[i*4 : i*4+4]
			a := float32(p[3]) / 255
			ov := (float32(wh[i*3]) - float32(bl[i*3])) / 255
			over := [4]float32{ov, float32(bl[i*3]), float32(bl[i*3+1]), float32(bl[i*3+2])}
			for k := 0; k < 3; k++ {
				v := float32(p[k])*over[0] + over[k+1]*a + float32(clear[i*3+k])*(1-a)
				want = append(want, uint8(math.Min(math.Max(math.Round(float64(v)), 0), 255)))
			}
		}
		for i := range wh {
			if wh[i] < bl[i] {
				t.Fatal("white is darker than black")
			}
		}
		var c Composer
		if got := c.Compose(frame, w, h, day); !bytes.Equal(got, want) {
			n := 0
			for i := range got {
				if got[i] != want[i] {
					n++
				}
			}
			t.Fatalf("day=%v: %d of %d bytes differ", day, n, len(got))
		}
	}
}

func TestCSSColours(t *testing.T) {
	if CSS("#fff") != [4]float64{1, 1, 1, 1} || CSS("#ff0000") != [4]float64{1, 0, 0, 1} {
		t.Fatal("hex")
	}
	if got := CSS("rgba(255, 0, 51, 0.5)"); got != [4]float64{1, 0, 0.2, 0.5} {
		t.Fatal(got)
	}
}
