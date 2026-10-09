import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { expect, test } from "@playwright/test";

test("repository registration defaults on, preserves opt-out, detects imported Wiki and rejects invalid adoption without a model run", async ({
  request,
}) => {
  test.skip(
    !process.env.LVK_E2E_SOURCE_ROOT,
    "Requires a development server sharing the source mount",
  );
  const root = mkdtempSync(
    join(dirname(process.env.LVK_E2E_SOURCE_ROOT!), ".lvk-adoption-api-"),
  );
  const git = (...args: string[]) =>
    execFileSync("git", ["-C", root, ...args], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  let repoId: string | undefined;
  try {
    git("init", "-b", "main");
    git("config", "user.name", "LVK test");
    git("config", "user.email", "test@example.invalid");
    git("commit", "--allow-empty", "-m", "source");
    await expect
      .poll(async () => (await request.get("/api/health")).status(), {
        timeout: 180_000,
      })
      .toBe(200);
    const registration = await request.post("/api/repos", {
      data: { path: root, display_name: "Adoption regression" },
    });
    expect(registration.ok()).toBe(true);
    repoId = (await registration.json()).data.id;
    const memoryUrl = `/api/repos/${repoId}/memory`;
    const state = async () =>
      (await (await request.get(memoryUrl)).json()).data;
    expect(await state()).toMatchObject({
      enabled: true,
      wiki_exists: false,
      last_success: null,
      active_run_id: null,
      target_branch: "main",
    });
    const configure = async (enabled: boolean) => {
      const response = await request.put(memoryUrl, {
        data: { enabled, target_branch: "main", output_language: "ja" },
      });
      expect(response.ok()).toBe(true);
      return (await response.json()).data;
    };
    await configure(false);
    expect(
      (await request.post("/api/repos", { data: { path: root } })).ok(),
    ).toBe(true);
    expect((await state()).enabled).toBe(false);
    // Compatibility: old clients send an empty JSON POST. It reaches the
    // enablement guard rather than failing to parse the empty body.
    const disabled = await request.post(`${memoryUrl}/sync`, {
      headers: { "Content-Type": "application/json" },
    });
    expect(disabled.status()).toBe(400);
    expect(await disabled.text()).toContain("Enable OpenWiki");
    await configure(true);
    mkdirSync(join(root, "openwiki"));
    writeFileSync(join(root, "openwiki", "guide.md"), "# Existing knowledge\n");
    expect((await state()).wiki_exists).toBe(false); // Uncommitted files are not canonical.
    git("add", "openwiki");
    git("commit", "-m", "import existing Wiki without local sync history");
    expect(await state()).toMatchObject({
      wiki_exists: true,
      last_success: null,
      status: "stale",
    });
    expect(await configure(true)).toMatchObject({
      wiki_exists: true,
      status: "stale",
    });
    const source = git("rev-parse", "HEAD");
    for (const from_commit of ["HEAD~1", "f".repeat(40)]) {
      const invalid = await request.post(`${memoryUrl}/sync`, {
        data: { from_commit },
      });
      expect(invalid.status()).toBe(400);
      expect(await state()).toMatchObject({
        active_run_id: null,
        last_success: null,
        error: null,
      });
    }
    expect(git("rev-parse", "HEAD")).toBe(source);
  } finally {
    if (repoId)
      expect((await request.delete(`/api/repos/${repoId}`)).ok()).toBe(true);
    rmSync(root, { recursive: true, force: true });
  }
});
