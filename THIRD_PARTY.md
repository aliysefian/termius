# Third-party material

## Command specs for smart completion

`src/lib/completion/specs/*.json` is converted from
[`@withfig/autocomplete`](https://github.com/withfig/autocomplete) version
2.692.3: the names, descriptions, subcommands and options of about 60 common
commands. Only data is taken. Code in the specs (generators, `loadSpec`) is
dropped, and descriptions are shortened to one line. The conversion is
`scripts/build-completion-specs.mjs`; see "Command specs" in
`docs/DEVELOPMENT.md` to redo it.

The package's `LICENSE` file is the MIT licence, reproduced below. (Its
`package.json` says `ISC`, which is also permissive; the licence file is the
text kept here.)

```
MIT License

Copyright (c) 2021 Hercules Labs Inc. (Fig)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
