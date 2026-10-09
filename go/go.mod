module github.com/4regab/Hover/go

go 1.27.0

require (
	gioui.org v0.10.3
	github.com/dlclark/regexp2 v1.12.0
	github.com/ebitengine/purego v0.11.1
	github.com/go-text/typesetting v0.3.5
	github.com/godbus/dbus/v5 v5.2.2
	github.com/jfreymuth/oggvorbis v1.0.5
	golang.org/x/image v0.26.0
	golang.org/x/sys v0.48.0
)

require (
	gioui.org/shader v1.0.9 // indirect
	github.com/jfreymuth/vorbis v1.0.2 // indirect
	golang.org/x/exp/shiny v0.0.0-20250408133849-7e4ce0ab07d0 // indirect
	golang.org/x/text v0.32.0 // indirect
)

replace gioui.org => ./third_party/gioui.org
