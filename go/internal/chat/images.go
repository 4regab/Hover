package chat

import (
	"bytes"
	"image"
	_ "image/gif"  // decoders for what a prompt or an answer may carry
	_ "image/jpeg" // (webp below)
	_ "image/png"
	"sync"

	xdraw "golang.org/x/image/draw"
	_ "golang.org/x/image/webp"
)

// Images for the thread, shared by the layout (which needs their sizes) and the painter
// (which needs their pixels). Bytes come from a loader that may answer "not yet": a web
// image is fetched in the background, and the thread lays its section out again when it
// arrives (Thread.ImageChanged).

type FetchKind uint8

const (
	FetchBytes FetchKind = iota
	// FetchPending: asked for, not here yet.
	FetchPending
	FetchFailed
)

type Fetch struct {
	Kind  FetchKind
	Bytes []byte
}

type Loader func(src string) Fetch

type ImageKind uint8

const (
	ImageReady ImageKind = iota
	ImagePending
	ImageBroken
)

// ImageState is an image as the layout sees it. A pending and a failed image look the
// same in the page (Chromium shows the broken-image icon and the alt text for both).
type ImageState struct {
	Kind ImageKind
	W, H float32
}

type slot struct {
	img    image.Image
	failed bool
	// pending: asked for again until the loader has it.
	pending bool
}

type Images struct {
	mu     sync.Mutex
	loader Loader
	m      map[string]*slot
}

// maxSide is a decoded image's longest side at most (a larger one is scaled down once,
// on load): the chat never draws one bigger than the drawer, even at 400 %.
const maxSide = 4096

func NewImages(l Loader) *Images { return &Images{loader: l, m: map[string]*slot{}} }

// NoImages loads nothing (tests, and the headless screenshots unless told otherwise).
func NoImages() *Images { return NewImages(func(string) Fetch { return Fetch{Kind: FetchFailed} }) }

func (im *Images) slot(src string) *slot {
	if s := im.m[src]; s != nil && !s.pending {
		return s
	}
	s := &slot{}
	switch f := im.loader(src); f.Kind {
	case FetchBytes:
		if img := decode(f.Bytes); img != nil {
			s.img = img
		} else {
			s.failed = true
		}
	case FetchPending:
		s.pending = true
	default:
		s.failed = true
	}
	im.m[src] = s
	return s
}

func (im *Images) State(src string) ImageState {
	im.mu.Lock()
	defer im.mu.Unlock()
	s := im.slot(src)
	switch {
	case s.img != nil:
		b := s.img.Bounds()
		return ImageState{ImageReady, float32(b.Dx()), float32(b.Dy())}
	case s.pending:
		return ImageState{Kind: ImagePending}
	}
	return ImageState{Kind: ImageBroken}
}

// Image is the decoded picture, or nil when there is none (yet).
func (im *Images) Image(src string) image.Image {
	im.mu.Lock()
	defer im.mu.Unlock()
	return im.slot(src).img
}

// Forget drops an image, so it is asked for again (a retry, or a file that changed).
func (im *Images) Forget(src string) {
	im.mu.Lock()
	delete(im.m, src)
	im.mu.Unlock()
}

func decode(b []byte) image.Image {
	img, _, err := image.Decode(bytes.NewReader(b))
	if err != nil {
		return nil
	}
	bb := img.Bounds()
	if max(bb.Dx(), bb.Dy()) > maxSide {
		k := float64(maxSide) / float64(max(bb.Dx(), bb.Dy()))
		dst := image.NewNRGBA(image.Rect(0, 0, max(int(float64(bb.Dx())*k), 1), max(int(float64(bb.Dy())*k), 1)))
		xdraw.ApproxBiLinear.Scale(dst, dst.Bounds(), img, bb, xdraw.Src, nil)
		return dst
	}
	return img
}
