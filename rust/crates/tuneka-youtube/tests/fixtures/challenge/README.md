# Indexed player regression fixture

`player-22cb88bc-indexed.js` is an altered, minimized set of definitions from the public YouTube player:

https://www.youtube.com/s/player/22cb88bc/player_es6.vflset/en_US/base.js

Retrieved on 2026-10-14. Original copyright and license headers are retained. Definitions are rewritten as standalone variables; two small Tuneka/fixture call wrappers are added. This is test material, not an embedded copy of the browser player. No response bodies, transport URLs, visitor data, cookies, or live challenge inputs are included.

The fixture exercises numeric dispatch selectors, an indexed string table, helper functions referenced as array values, a required `typeof` guard constant, and the current obfuscated failure marker. Golden test inputs are synthetic. A successful fixture test does not prove CDN acceptance.

`player-22cb88bc-plasma-signature.js` is a second minimized fragment from the public mobile player at `https://www.youtube.com/s/player/22cb88bc/player-plasma-es6-tcc-en_US.vflset/base.js`, retrieved on 2026-10-14. It retains the public player copyright/license notices and only the string table, signature helper, selector function and call-site shape needed to exercise Tuneka's plasma signature extractor. Its golden input is synthetic. A separate genuine anonymous MWEB response supplied one `signatureCipher`. The original signature-only probe left its nested `n` unchanged and returned HTTP 403. The corrected probe transforms both and returns HTTP 206; the full Rust pipeline subsequently completed and decoded the 192-second MP4. See `docs/m2-cdn-followup.md`.

`player-21090930-direct-signature.js` is a minimized fragment from the public player at `https://www.youtube.com/s/player/21090930/player_es6.vflset/en_US/base.js`, retrieved on 2026-10-14. It retains the public player Apache notices and the table, helper, decoder and signature call-site shape needed for the direct indexed branch. Its golden input is synthetic. The matching live response's `n` uses a distinct dispatcher and is not represented by this signature fixture.

`player-21090930-direct-n.js` is a synthetic miniature of that revision's separate `n` dispatcher and URL setter. It verifies the XOR-derived selector, branch and table guards, and rejection of a duplicate setter or altered binding. The complete public player was also exercised locally with a synthetic input under the production QuickJS limits; the miniature is only a regression test.

`player-22cb88bc-plasma-n.js` is a synthetic miniature of the matching mobile player's `n` dispatcher. Its indexed table has the empty separator at index 13 instead of zero. The test verifies the selector through the unique URL `set("n", ...)` call, the XOR-indexed separator, guard constant and rejection of ambiguous definitions. The genuine response carried both `signatureCipher` and `n` in the cipher URL; transforming both yielded a bounded HTTP 206 MP4 response.

The opt-in `challenge_probe` example accepts a caller-provided local player script and JSON on stdin containing optional `signature` / `n` strings. It writes transformed values only to a caller-selected transient path and prints lengths/status/timing. Use a private transient directory for real responses and challenge material. The example applies the same source/function/dependency bounds and QuickJS time, memory and stack quotas as the application.

`player-21090930-direct-n.js` is a tiny synthetic fixture for the independently verified direct-split/AND-mask dispatcher and indexed URL setter shape. It checks selector derivation, unique wrapper definitions and XOR parameter bindings. It does not contain the full live n algorithm or a real challenge input; full captured-player execution and CDN proof are recorded separately.
