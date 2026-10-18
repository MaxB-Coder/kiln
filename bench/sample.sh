#!/bin/sh
# The same screenful for every terminal: used for side-by-side rendering screenshots.
e=$(printf '\033')
printf '%s[1mKiln rendering sample%s[0m  (JetBrains Mono 13pt, Gruvbox)\n\n' "$e" "$e"
printf '%s[38;5;208mfn%s[0m %s[33mmain%s[0m() -> %s[36mResult%s[0m<()> {  %s[90m// ligatures: -> => != === <= >= :: |> www%s[0m\n' "$e" "$e" "$e" "$e" "$e" "$e" "$e" "$e"
printf '    let x = vec![1, 2, 3].iter().map(|v| v * 2).collect::<Vec<_>>();\n}\n\n'
for i in 0 1 2 3 4 5 6 7; do printf '%s[4%sm  %s[0m' "$e" "$i" "$e"; done; printf '  '
for i in 0 1 2 3 4 5 6 7; do printf '%s[10%sm  %s[0m' "$e" "$i" "$e"; done; printf '\n'
for i in 1 2 3 4 5 6; do printf '%s[3%sm normal %s[1;9%sm bold %s[0m' "$e" "$i" "$e" "$i" "$e"; done; printf '\n'
printf '%s[3mitalic%s[0m %s[4munderline%s[0m %s[4:3mcurly%s[0m %s[9mstrike%s[0m %s[2mdim%s[0m %s[7minverse%s[0m\n' "$e" "$e" "$e" "$e" "$e" "$e" "$e" "$e" "$e" "$e" "$e" "$e"
i=0; while [ $i -lt 72 ]; do printf '%s[48;2;%d;%d;%dm %s[0m' "$e" $((i*3)) $((100+i)) $((220-i*3)) "$e"; i=$((i+1)); done; printf '\n\n'
printf '┌─────────┬──────────┐  ╭──────╮  █▓▒░ ▁▂▃▄▅▆▇█  ⣿⡇⠿ \n'
printf '│ box     │ drawing  │  │ round│  ←↑→↓ ✓ ✗ ● ○ ◆  \n'
printf '└─────────┴──────────┘  ╰──────╯  \n\n'
printf 'Icons: \356\202\266%s[30;43m pill %s[0m\356\202\264  \357\204\225 folder  \357\200\226 file  \356\234\202 git  \363\260\210\231 md\n' "$e" "$e"
printf 'Unicode: café naïve Ελληνικά Кириллица  宽字符 日本語  emoji 🙂 🚀\n'
printf 'Hairlines: |||||||| iiiillll 0O 1lI {}[]() ,.;: ~`'"'"'"\n'
