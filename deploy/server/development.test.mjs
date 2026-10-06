import assert from "node:assert/strict";
import { test } from "node:test";
import { developmentEnvironment } from "./development.mjs";

test("source development overrides inherited production listeners and origin", () => {
  const env = developmentEnvironment(
    {
      HOST: "0.0.0.0",
      BACKEND_PORT: "3000",
      VK_ALLOWED_ORIGINS: "https://wrong",
      LVK_DEV_HTTPS_PORT: "9443",
    },
    { app: "2001:db8::1" },
  );
  assert.equal(env.HOST, "127.0.0.1");
  assert.equal(env.BACKEND_PORT, "4021");
  assert.equal(env.PREVIEW_PROXY_PORT, "4022");
  assert.equal(env.VK_ALLOWED_ORIGINS, "https://[2001:db8::1]:9443");
  assert.equal(env.CARGO_INCREMENTAL, "1");
  assert.throws(() =>
    developmentEnvironment(
      { LVK_DEV_HTTPS_PORT: "443" },
      { app: "test.example" },
    ),
  );
});
