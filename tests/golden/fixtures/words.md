### Word boundaries, for double-click

Plain words, then foo.bar and foo_bar and foo-bar, don't, it's, rock'n'roll, 3.14 and 1,000,000 and 2026-09-28.
Paths: src/auth/refresh.ts, C:\hover\notes.txt, ~/.config/hover and ../up/there.
Mail and web: someone@example.com, https://example.com/a/b?c=d&e=f#g and www.example.org.
Punctuation: (parens) [brackets] {braces} "quotes" 'single' — dash – en … ellipsis!? ;: ,.
Mixed: **bold**part, *em*phasis, `inline_code()` and `a.b.c`, x+y=z, a*b, 50%, $12.99, #hash, @user.
Unicode: café naïve Zürich, 日本語のテキスト, 中文分词测试, 한국어 단어, emoji 🙂🎉 here, ×÷.

- A list item with some words
- Second item: `code` then more
  - nested item, deeper words

1. First ordered thing
2. Second ordered thing

| Name | Value |
|---|---|
| alpha_beta | 42.5 units |
| gamma-delta | x.y.z |

> A quoted line with words,
> and a second quoted line.

```ts
export function refresh(token: Token): boolean {
  return token.expiresAt > Date.now(); // still_valid
}
```

Line one
line two after a break
line three.

Last paragraph, short.
