"""The "host key trusted" banner next to the floating terminal toolbar, at 1920 / 1440 / 1204 / 1024 / 768 px with a
short and a very long host and fingerprint: the dismiss (X) button must be the topmost element at its centre (so a
click reaches it, not the toolbar's Find button), the banner must not overlap the toolbar or overflow, and the Find
bar must stack under the banner. Backend mocked (mock.js).

    pnpm dev &
    python3 src/lib/__tests__/e2e/hostkeynotice.py [chromium-executable] [screenshot-prefix]
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

E2E = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
PREFIX = sys.argv[2] if len(sys.argv) > 2 else None
WIDTHS = [1920, 1440, 1204, 1024, 768]
SHORT = ("203.0.113.7", "SHA256:Zq8Jw0n1u3yQ7mH4kT2bXcVd9LrPaE5sFgYiOoUeNtA")
LONG = ("very-long-hostname-for-the-production-database-cluster-primary.internal.corp.example.com:2222",
        "SHA256:Zq8Jw0n1u3yQ7mH4kT2bXcVd9LrPaE5sFgYiOoUeNtA+Zq8Jw0n1u3yQ7mH4kT2bXcVd9LrPaE5sFgYiOoUeNtAZq8Jw0n1u3yQ7mH4kT2bXcVd9LrPaE5sFgYiOoUeNtA")
failures = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


MEASURE = """() => {
  const r = (e) => { if (!e) return null; const b = e.getBoundingClientRect(); return { l: b.left, t: b.top, r: b.right, b: b.bottom, w: b.width, h: b.height }; };
  const x = document.querySelector('[data-testid=hostkey-dismiss]') || [...document.querySelectorAll('button[aria-label=Dismiss]')].find((b) => b.closest('[data-testid=hostkey-notice]') || b.parentElement.textContent.includes('Host key trusted'));
  const banner = x && (x.closest('[data-testid=hostkey-notice]') || x.parentElement);
  const tb = document.querySelector('[role=toolbar][aria-label="Terminal actions"]');
  const find = document.querySelector('input[placeholder=Find]')?.parentElement;
  const out = { x: r(x), banner: r(banner), toolbar: r(tb), find: r(find), vw: innerWidth };
  if (x) {
    const b = x.getBoundingClientRect();
    const hit = document.elementFromPoint((b.left + b.right) / 2, (b.top + b.bottom) / 2);
    out.xOnTop = hit === x || x.contains(hit);
    out.hit = hit ? hit.tagName + '.' + (hit.getAttribute('aria-label') || hit.className).toString().slice(0, 50) : null;
    out.bannerOverflowX = banner.scrollWidth > banner.clientWidth + 1;
  }
  if (find && tb) { const f = find.getBoundingClientRect(), t = tb.getBoundingClientRect(); out.findOverlapsToolbar = f.right > t.left && f.left < t.right && f.bottom > t.top && f.top < t.bottom; }
  if (banner && tb) { const f = banner.getBoundingClientRect(), t = tb.getBoundingClientRect(); out.bannerOverlapsToolbar = f.right > t.left && f.left < t.right && f.bottom > t.top && f.top < t.bottom; }
  out.pageScrollX = document.documentElement.scrollWidth > innerWidth;
  return out;
}"""


async def fresh(b, w):
    pg = await b.new_page(viewport={"width": w, "height": 800})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    await pg.add_init_script("window.__invoke = async (c, a) => { if (a && a.paneId) window.__pane = a.paneId; return undefined; };")
    await pg.add_init_script(path=os.path.join(E2E, "mock.js"))
    await pg.goto(URL)
    await pg.evaluate("() => { localStorage.clear(); }")
    await pg.goto(URL)
    await pg.wait_for_timeout(3500)
    await pg.keyboard.press("Control+Shift+Backquote")
    await pg.wait_for_selector('[role=toolbar][aria-label="Terminal actions"]', timeout=20000)
    await pg.wait_for_function("() => !!window.__pane")
    await pg.wait_for_timeout(800)
    return pg


async def notice(pg, host, fp):
    await pg.evaluate("([h, f]) => window.__emit('ssh:status', { pane_id: window.__pane, status: { kind: 'new_host_key', host: h, fingerprint: f } })", [host, fp])
    await pg.wait_for_timeout(300)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        for w in WIDTHS:
            for label, (host, fp) in (("short", SHORT), ("long", LONG)):
                pg = await fresh(b, w)
                await notice(pg, host, fp)
                m = await pg.evaluate(MEASURE)
                tag = f"{w}px/{label}"
                check(f"{tag}: banner is shown", m.get("x") is not None, m)
                if m.get("x"):
                    check(f"{tag}: X is the topmost element at its centre", m["xOnTop"], m["hit"])
                    check(f"{tag}: X fully inside the viewport", m["x"]["r"] <= m["vw"] and m["x"]["l"] >= 0)
                    check(f"{tag}: banner does not overlap the toolbar", not m["bannerOverlapsToolbar"])
                    check(f"{tag}: banner content does not overflow", not m["bannerOverflowX"])
                    check(f"{tag}: X is a usable target (>=24px)", m["x"]["w"] >= 24 and m["x"]["h"] >= 24, m["x"])
                check(f"{tag}: no horizontal page scroll", not m["pageScrollX"])
                if PREFIX:
                    await pg.screenshot(path=f"{PREFIX}-{w}-{label}.png")
                if w == 1204 and label == "short":
                    # Dismiss by really clicking the X (a click on whatever is on top of it fails this).
                    if m.get("x"):
                        bx = m["x"]
                        await pg.mouse.click((bx["l"] + bx["r"]) / 2, (bx["t"] + bx["b"]) / 2)
                        await pg.wait_for_timeout(200)
                        gone = await pg.evaluate("() => !document.body.textContent.includes('Host key trusted')")
                        check("1204px: clicking the X dismisses the banner", gone)
                    # The toolbar still works: Find opens and does not collide with the toolbar.
                    await pg.click('[aria-label="Find in terminal"]')
                    await pg.wait_for_timeout(300)
                    m2 = await pg.evaluate(MEASURE)
                    check("1204px: Find bar opens from the toolbar", m2["find"] is not None)
                    check("1204px: Find bar does not overlap the toolbar", m2.get("findOverlapsToolbar") is False, m2.get("find"))
                    await notice(pg, *LONG)
                    m3 = await pg.evaluate(MEASURE)
                    check("1204px: banner and Find bar are both usable together", m3.get("xOnTop") is True, m3.get("hit"))
                    if PREFIX:
                        await pg.screenshot(path=f"{PREFIX}-{w}-find.png")
                check(f"{tag}: no page errors", not pg.errors, pg.errors)
                if label == "short" or w == 1204:
                    print("   ", json.dumps({k: m[k] for k in ("x", "toolbar", "banner")}, default=str)[:300])
                await pg.close()
        await b.close()
    print("\nall passed" if not failures else f"\nFAILED: {failures}")
    sys.exit(1 if failures else 0)


asyncio.run(main())
