package quota

// num.rs: numbers as the C# interpolations print them.

import (
	"math"
	"strconv"
	"strings"

	"github.com/4regab/Hover/go/internal/core"
)

// Custom is a double through a custom format of "0" and up to hashes "#" decimals ("0",
// "0.##"). .NET first takes the value to 15 significant digits (the precision it uses for
// custom formats), then rounds that decimal half away from zero: so 36.5 is "37" and 2.675
// is "2.68", where Go's own formatting gives "36" and "2.67".
func Custom(v float64, hashes int) string {
	if math.IsInf(v, 0) || math.IsNaN(v) {
		return core.DotnetDouble(v)
	}
	neg := v < 0
	// d.dddddddddddddde±x: 15 significant digits.
	sci := strconv.FormatFloat(math.Abs(v), 'e', 14, 64)
	mant, expText, _ := strings.Cut(sci, "e")
	exp, _ := strconv.Atoi(expText)
	var digits []byte
	for i := 0; i < len(mant); i++ {
		if mant[i] >= '0' && mant[i] <= '9' {
			digits = append(digits, mant[i]-'0')
		}
	}
	// The value is 0.d1d2d3… × 10^(exp+1): split it into integer and fraction digits.
	point := exp + 1
	var whole, frac []byte
	if point > 0 {
		whole = append(whole, digits[:min(point, len(digits))]...)
		for len(whole) < point {
			whole = append(whole, 0)
		}
		if point < len(digits) {
			frac = append(frac, digits[point:]...)
		}
	} else {
		frac = make([]byte, -point)
		frac = append(frac, digits...)
	}
	var next byte
	if hashes < len(frac) {
		next = frac[hashes]
	}
	for len(frac) < hashes {
		frac = append(frac, 0)
	}
	kept := append(whole, frac[:hashes]...)
	if next >= 5 {
		// Away from zero: carry through the kept digits.
		for i := len(kept); ; {
			if i == 0 {
				kept = append([]byte{1}, kept...)
				break
			}
			i--
			if kept[i] == 9 {
				kept[i] = 0
			} else {
				kept[i]++
				break
			}
		}
	}
	intLen := max(len(kept)-hashes, 0)
	toText := func(ds []byte) string {
		b := make([]byte, len(ds))
		for i, d := range ds {
			b[i] = '0' + d
		}
		return string(b)
	}
	s := strings.TrimLeft(toText(kept[:intLen]), "0")
	if s == "" {
		s = "0"
	}
	if f := strings.TrimRight(toText(kept[intLen:]), "0"); f != "" {
		s += "." + f
	}
	// .NET Core 3.0+ keeps the sign of a negative value that rounds to zero ("-0").
	if neg {
		s = "-" + s
	}
	return s
}
