package agents

import "testing"

func TestShortNamesAFileOrACommand(t *testing.T) {
	for in, want := range map[string]string{
		"src/app/refresh.ts":                       "refresh.ts",
		"npm test --watch":                         "npm test",
		"/usr/bin/cargo build":                     "cargo build",
		"src/deep/":                                "deep",
		"a-really-long-file-name-for-the-notch.rs": "a-really-long-file-name-for…",
	} {
		if got := Short(&in); got == nil || *got != want {
			t.Errorf("%q: %v", in, deref(got))
		}
	}
	if Short(sp("  ")) != nil {
		t.Error("blank")
	}
}
