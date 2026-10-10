package core

// time.rs: DateTime, as much of it as the history keeps: ticks (100 ns since 0001-01-01)
// and a Kind, written and read as System.Text.Json does (ISO 8601, the fraction trimmed).

import (
	"errors"
	"fmt"
	"math"
	"strconv"
	"strings"
	"time"
)

const (
	ticksPerSec = int64(10_000_000)
	ticksPerDay = 86_400 * ticksPerSec
	// unixTicks is DateTime.UnixEpoch.Ticks.
	unixTicks = int64(621_355_968_000_000_000)
)

type StampKind uint8

const (
	Unspecified StampKind = iota
	UTC
	Local
)

// Stamp is a DateTime. For UTC and Local the ticks are UTC; for Unspecified they are the
// wall clock as written. Local is shown in the machine's zone, as .NET shows it. The zero
// Stamp is default(DateTime): 0001-01-01T00:00:00, Unspecified.
type Stamp struct {
	Ticks int64
	Kind  StampKind
}

// Day is a calendar day (chrono's NaiveDate).
type Day struct{ Y, M, D int }

func (d Day) Before(o Day) bool {
	if d.Y != o.Y {
		return d.Y < o.Y
	}
	if d.M != o.M {
		return d.M < o.M
	}
	return d.D < o.D
}

func floorDiv(a, b int64) int64 {
	q := a / b
	if (a%b != 0) && ((a < 0) != (b < 0)) {
		q--
	}
	return q
}

func floorMod(a, b int64) int64 { return a - floorDiv(a, b)*b }

// Now is DateTime.Now.
func Now() Stamp {
	return Stamp{unixTicks + time.Now().UnixNano()/100, Local}
}

func StampFromUnixMS(ms int64, kind StampKind) Stamp { return Stamp{unixTicks + ms*10_000, kind} }

// UTCTicks is the instant in UTC ticks (an Unspecified time is taken as local, as new
// DateTimeOffset(DateTime) takes it).
func (s Stamp) UTCTicks() int64 {
	if s.Kind == Unspecified {
		return s.Ticks - localOffsetMinWall(s.Ticks)*60*ticksPerSec
	}
	return s.Ticks
}

// LocalDate is the calendar day on this machine's clock: an Unspecified stamp is already
// a wall time; the others are an instant, shown in the machine's zone.
func (s Stamp) LocalDate() Day {
	wall := s.Ticks
	if s.Kind != Unspecified {
		wall += LocalOffsetMin(s.Ticks) * 60 * ticksPerSec
	}
	y, m, d := civil(floorDiv(wall, ticksPerDay))
	return Day{int(y), int(m), int(d)}
}

// UnixMS is new DateTimeOffset(t).ToUnixTimeMilliseconds(), floored as .NET floors it.
func (s Stamp) UnixMS() int64 { return floorDiv(s.UTCTicks()-unixTicks, 10_000) }

func (s Stamp) AddSecs(secs float64) Stamp {
	return Stamp{s.Ticks + int64(math.Round(secs*float64(ticksPerSec))), s.Kind}
}

// SecsSince is (s - other).TotalSeconds.
func (s Stamp) SecsSince(other Stamp) float64 {
	return float64(s.Ticks-other.Ticks) / float64(ticksPerSec)
}

// Compare is DateTime's: ticks, ignoring Kind.
func (s Stamp) Compare(o Stamp) int {
	switch {
	case s.Ticks < o.Ticks:
		return -1
	case s.Ticks > o.Ticks:
		return 1
	}
	return 0
}

// ISO is the "O" round-trip form with trailing fraction zeros dropped (JsonWriterHelper.
// WriteDateTimeTrimmed); a Local time carries the zone's offset at that instant.
func (s Stamp) ISO() string { return s.ISOWith(LocalOffsetMin) }

func (s Stamp) ISOWith(offsetOf func(int64) int64) string {
	wall, suffix := s.Ticks, ""
	switch s.Kind {
	case UTC:
		suffix = "Z"
	case Local:
		off := offsetOf(s.Ticks)
		a, sign := off, '+'
		if off < 0 {
			a, sign = -off, '-'
		}
		wall = s.Ticks + off*60*ticksPerSec
		suffix = fmt.Sprintf("%c%02d:%02d", sign, a/60, a%60)
	}
	y, mo, d := civil(floorDiv(wall, ticksPerDay))
	t := floorMod(wall, ticksPerDay)
	h, mi, sec, f := t/(3600*ticksPerSec), t/(60*ticksPerSec)%60, t/ticksPerSec%60, t%ticksPerSec
	out := fmt.Sprintf("%04d-%02d-%02dT%02d:%02d:%02d", y, mo, d, h, mi, sec)
	if f != 0 {
		out += "." + strings.TrimRight(fmt.Sprintf("%07d", f), "0")
	}
	return out + suffix
}

func (s Stamp) ToJSON() JSON { return JStr(s.ISO()) }

// ParseStamp is JsonHelpers.TryParseAsISO for DateTime: a date, optionally
// THH:mm[:ss[.f…]], and optionally Z or ±HH[:mm]. An offset makes it Local
// (DateTimeOffset.LocalDateTime), Z keeps it UTC, none leaves it Unspecified.
func ParseStamp(s string) (Stamp, bool) {
	b := s
	if len(b) > 42 {
		return Stamp{}, false
	}
	num := func(from, to int) (int64, bool) {
		if to > len(b) {
			return 0, false
		}
		for i := from; i < to; i++ {
			if b[i] < '0' || b[i] > '9' {
				return 0, false
			}
		}
		n, err := strconv.ParseInt(b[from:to], 10, 64)
		return n, err == nil
	}
	at := func(i int) byte {
		if i < len(b) {
			return b[i]
		}
		return 0
	}
	y, ok1 := num(0, 4)
	mo, ok2 := num(5, 7)
	d, ok3 := num(8, 10)
	if !ok1 || !ok2 || !ok3 || at(4) != '-' || at(7) != '-' || mo < 1 || mo > 12 || d < 1 || d > daysIn(y, mo) || y < 1 {
		return Stamp{}, false
	}
	ticks := daysFromCivil(y, mo, d) * ticksPerDay
	i := 10
	kind := Unspecified
	if i < len(b) {
		if b[i] != 'T' {
			return Stamp{}, false
		}
		h, ok1 := num(11, 13)
		mi, ok2 := num(14, 16)
		if !ok1 || !ok2 || at(13) != ':' || h > 23 || mi > 59 {
			return Stamp{}, false
		}
		ticks += (h*3600 + mi*60) * ticksPerSec
		i = 16
		if at(i) == ':' {
			sec, ok := num(17, 19)
			if !ok || sec > 59 {
				return Stamp{}, false
			}
			ticks += sec * ticksPerSec
			i = 19
			if at(i) == '.' {
				i++
				st := i
				for i < len(b) && b[i] >= '0' && b[i] <= '9' {
					i++
				}
				if i == st || i-st > 16 {
					return Stamp{}, false
				}
				var f int64
				for k := st; k < i && k-st < 7; k++ {
					f = f*10 + int64(b[k]-'0')
				}
				for k := i - st; k < 7; k++ {
					f *= 10
				}
				ticks += f
			}
		}
		switch c := at(i); {
		case i >= len(b):
		case c == 'Z' && i+1 == len(b):
			kind = UTC
		case c == '+' || c == '-':
			oh, ok := num(i+1, i+3)
			if !ok {
				return Stamp{}, false
			}
			var om int64
			switch len(b) - i {
			case 3:
			case 6:
				if b[i+3] != ':' {
					return Stamp{}, false
				}
				if om, ok = num(i+4, i+6); !ok {
					return Stamp{}, false
				}
			default:
				return Stamp{}, false
			}
			if oh > 14 || om > 59 {
				return Stamp{}, false
			}
			off := oh*60 + om
			if c == '-' {
				off = -off
			}
			ticks -= off * 60 * ticksPerSec
			kind = Local
		default:
			return Stamp{}, false
		}
	}
	return Stamp{ticks, kind}, true
}

func StampFromJSON(v JSON) (Stamp, error) {
	if s, ok := v.AsStr(); ok {
		if t, ok := ParseStamp(s); ok {
			return t, nil
		}
	}
	return Stamp{}, errors.New("not an ISO 8601 date")
}

func OptStampFromJSON(v JSON) (*Stamp, error) {
	if v.IsNull() {
		return nil, nil
	}
	t, err := StampFromJSON(v)
	if err != nil {
		return nil, err
	}
	return &t, nil
}

// LocalCompact is local time as yyyyMMdd-HHmmss (DateTime.Now's wall clock).
func LocalCompact() string {
	now := Now()
	wall := now.Ticks + LocalOffsetMin(now.Ticks)*60*ticksPerSec
	y, m, d := civil(floorDiv(wall, ticksPerDay))
	t := floorMod(wall, ticksPerDay) / ticksPerSec
	return fmt.Sprintf("%04d%02d%02d-%02d%02d%02d", y, m, d, t/3600, t/60%60, t%60)
}

// LocalClock is local time as HH:mm:ss.fff, for the log.
func LocalClock() string {
	now := Now()
	t := floorMod(now.Ticks+LocalOffsetMin(now.Ticks)*60*ticksPerSec, ticksPerDay)
	return fmt.Sprintf("%02d:%02d:%02d.%03d", t/(3600*ticksPerSec), t/(60*ticksPerSec)%60, t/ticksPerSec%60, t%ticksPerSec/10_000)
}

// LocalOffsetMin is the machine zone's offset in minutes at a UTC instant (in ticks).
func LocalOffsetMin(utcTicks int64) int64 {
	secs := floorDiv(utcTicks-unixTicks, ticksPerSec)
	_, off := time.Unix(secs, 0).In(time.Local).Zone()
	return int64(off) / 60
}

// localOffsetMinWall is near enough for a wall time: the offset at the instant the wall
// time would be in UTC, corrected once.
func localOffsetMinWall(wallTicks int64) int64 {
	first := LocalOffsetMin(wallTicks)
	return LocalOffsetMin(wallTicks - first*60*ticksPerSec)
}

func daysIn(y, m int64) int64 {
	switch m {
	case 2:
		if (y%4 == 0 && y%100 != 0) || y%400 == 0 {
			return 29
		}
		return 28
	case 4, 6, 9, 11:
		return 30
	}
	return 31
}

// daysFromCivil is days since 0001-01-01 (Howard Hinnant's algorithm, shifted from 1970).
func daysFromCivil(y, m, d int64) int64 {
	if m <= 2 {
		y--
	}
	era := floorDiv(y, 400)
	yoe := y - era*400
	mp := (m + 9) % 12
	doy := (153*mp+2)/5 + d - 1
	doe := yoe*365 + yoe/4 - yoe/100 + doy
	return era*146097 + doe - 719468 + 719162
}

func civil(days int64) (y, m, d int64) {
	z := days - 719162 + 719468
	era := floorDiv(z, 146097)
	doe := z - era*146097
	yoe := (doe - doe/1460 + doe/36524 - doe/146096) / 365
	doy := doe - (365*yoe + yoe/4 - yoe/100)
	mp := (5*doy + 2) / 153
	d = doy - (153*mp+2)/5 + 1
	if mp < 10 {
		m = mp + 3
	} else {
		m = mp - 9
	}
	y = yoe + era*400
	if m <= 2 {
		y++
	}
	return y, m, d
}
