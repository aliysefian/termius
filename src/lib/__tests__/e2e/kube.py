"""Kubernetes in a real (headless) browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/kube.py [chromium-executable]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

HOSTS = """
const mk = (id, label, env) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group: '', tags: [], notes: '', environment: env, identity_id: 'i1' } });
window.__invoke = async (cmd) => {
  if (cmd === 'list_hosts') return [mk('h1', 'k8s-prod', 'production')];
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  return undefined;
};
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 1000})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(HOSTS)
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)

        await pg.click('button[aria-label="Containers"]')
        await pg.wait_for_timeout(300)
        await pg.click('[role=tab]:has-text("Kubernetes")')
        await pg.wait_for_timeout(300)
        check("Kubernetes is a tab of the Containers entry, which stays highlighted", await pg.locator('h1:has-text("Kubernetes")').count() >= 1 and await pg.locator('button[aria-label="Containers"][aria-current=page]').count() == 1)

        await pg.click('button:has-text("Open this computer")')
        await pg.wait_for_timeout(900)
        check("opening it reads the contexts and starts on the current one", await pg.input_value('select[aria-label=Context]') == "prod" and await pg.locator("text=kubectl v1.30.2").count() == 1)
        rows = pg.locator("tbody tr")
        check("pods from every namespace are listed", await rows.count() == 3)
        check("a crash-looping pod is marked and its restarts shown", "CrashLoopBackOff" in await pg.inner_text('tbody tr:has-text("api-0")') and await pg.locator("text=1 unhealthy").count() == 1)
        check("age and owner are shown", "3h" in await pg.inner_text('tbody tr:has-text("web-7d9f")') and "ReplicaSet/web-7d9f" in await pg.inner_text('tbody tr:has-text("web-7d9f")'))

        await pg.select_option('select[aria-label=Namespace]', "jobs")
        await pg.wait_for_timeout(600)
        calls = await pg.evaluate("window.__kubePodCalls")
        check("choosing a namespace asks for just that namespace", calls[-1]["scope"] == {"scope": "namespace", "name": "jobs"} and await rows.count() == 1, calls[-1])
        await pg.select_option('select[aria-label=Namespace]', "")
        await pg.wait_for_timeout(500)
        await pg.fill('input[aria-label="Search pods"]', "api")
        check("search narrows the list", await rows.count() == 1)
        await pg.fill('input[aria-label="Search pods"]', "")

        await pg.click('tbody tr:has-text("web-7d9f") button[aria-label^="Logs of"]')
        await pg.wait_for_timeout(700)
        logs = await pg.evaluate("window.__kubeLogs")
        check("logs are followed for the pod, starting with the first container", logs[-1]["pod"] == "web-7d9f-abc" and logs[-1]["container"] == "app" and logs[-1]["options"]["follow"] is True, logs[-1])
        check("the log lines arrive", "listening on :8080" in await pg.inner_text('[role=log][aria-label="Pod log"]'))
        await pg.select_option('select[aria-label=Container]', "sidecar")
        await pg.wait_for_timeout(500)
        check("changing the container restarts the log for it", (await pg.evaluate("window.__kubeLogs"))[-1]["container"] == "sidecar" and (await pg.evaluate("window.__kubeStopped")) >= 1)
        await pg.click('button[aria-label="Close the log"]')

        await pg.click('tbody tr:has-text("api-0") button[aria-label^="Describe"]')
        await pg.wait_for_timeout(400)
        check("describe shows the cluster's text", "Name: api-0" in await pg.inner_text("pre"))

        await pg.click('tbody tr:has-text("api-0") button[aria-label^="Shell in"]')
        await pg.wait_for_timeout(1500)
        sent = bytes([x for c in await pg.evaluate("window.__sent") for x in c]).decode("utf-8", "replace")
        check("a shell opens a terminal tab that runs kubectl exec for that pod", "kubectl --context 'prod' -n shop exec -it api-0 -c api --" in sent or "kubectl --context 'prod' -n shop exec -it api-0 --" in sent, sent[-300:])
        await pg.click('button[aria-label="Containers"]')
        await pg.wait_for_timeout(300)
        await pg.click('[role=tab]:has-text("Kubernetes")')
        await pg.wait_for_timeout(500)

        # deleting asks first
        await pg.click('tbody tr:has-text("batch-1") button[aria-label^="Delete"]')
        await pg.wait_for_timeout(400)
        check("deleting asks first", await pg.locator('text=Delete the pod jobs/batch-1').count() == 1 and not await pg.evaluate("window.__kubeDeleted"))
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(200)
        check("cancelling deletes nothing", not await pg.evaluate("window.__kubeDeleted"))
        await pg.click('tbody tr:has-text("batch-1") button[aria-label^="Delete"]')
        await pg.wait_for_timeout(300)
        await pg.click('button:has-text("Delete"):not([aria-label])')
        await pg.wait_for_timeout(500)
        check("confirming deletes that pod", await pg.evaluate("window.__kubeDeleted") == ["batch-1"])

        # port forward
        await pg.click('tbody tr:has-text("web-7d9f") button[aria-label^="Port-forward"]')
        await pg.fill("#kf-remote", "80")
        await pg.fill("#kf-local", "99999")
        check("a bad port is refused", await pg.locator('button:has-text("Forward")').is_disabled())
        await pg.fill("#kf-local", "8080")
        await pg.click('button:has-text("Forward")[type=submit]')
        await pg.wait_for_timeout(500)
        fw = await pg.evaluate("window.__kubeForwards")
        check("the forward is started for the pod and shown as listening", fw == [{"to": {"kind": "pod", "name": "web-7d9f-abc"}, "local": 8080, "remote": 80}] and await pg.locator("text=listening on 127.0.0.1").count() == 1, fw)
        await pg.click('button:has-text("Stop")')
        await pg.wait_for_timeout(300)
        check("stopping it ends the forward", await pg.locator("text=Port-forwards").count() == 0)

        # a host: production asks for the name; no port-forward button there
        await pg.click('button[aria-label="Close This computer"]')
        await pg.wait_for_timeout(300)
        await pg.click('input[aria-label="Open a source"]')
        await pg.click('text=k8s-prod')
        await pg.wait_for_timeout(900)
        check("a host is opened through its own connection", (await pg.evaluate("window.__kubeOpened"))[-1] == "h1")
        check("a host gets no port-forward button (the port would open on the host)", await pg.locator('button[aria-label^="Port-forward"]').count() == 0)
        await pg.click('tbody tr:has-text("api-0") button[aria-label^="Delete"]')
        await pg.wait_for_timeout(400)
        check("on a production host deleting asks for the pod's name", await pg.locator('#dlg-typed').count() == 1 and await pg.locator("text=api-0").count() >= 2)
        await pg.fill("#dlg-typed", "wrong")
        check("and the button stays off until it is typed exactly", await pg.locator('button[type=submit]:has-text("Delete")').is_disabled())
        await pg.keyboard.press("Escape")

        check("no page errors", not errors, errors[:3])
        await b.close()
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
