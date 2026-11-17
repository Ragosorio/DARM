/**
 * Compiles every book the tests use with the real compiler, from source:
 * the pipeline under test starts at `.darml`, not at a pre-built file.
 */
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const ROOT = join(here, "..", "..", "..");
export const TMP = join(here, ".tmp");
export const DARM = join(ROOT, "target", "debug", process.platform === "win32" ? "darm.exe" : "darm");
export const PASSWORD = "kitchen sink password";
export const LIGHTHOUSE_PASSWORD = "keeper of the light";

export default function globalSetup(): void {
  rmSync(TMP, { recursive: true, force: true });
  mkdirSync(TMP, { recursive: true });
  execFileSync("cargo", ["build", "-p", "darm-cli"], { cwd: ROOT, stdio: "inherit" });
  const build = (project: string, out: string, extra: string[] = [], env: Record<string, string> = {}) =>
    execFileSync(DARM, ["build", join(ROOT, project), "-o", join(TMP, out), ...extra], { cwd: ROOT, stdio: "pipe", env: { ...process.env, ...env } });
  const fast = ["--kdf-memory", "8", "--kdf-passes", "1"];
  build("apps/reader/tests/fixtures/kitchen-sink", "kitchen-sink.darm");
  build("apps/reader/tests/fixtures/kitchen-sink", "kitchen-sink.locked.darm", ["--encrypt", "--password-env", "PW", ...fast], { PW: PASSWORD });
  build("apps/reader/tests/fixtures/kitchen-sink", "kitchen-sink.keyed.darm", ["--new-keyfile", join(TMP, "kitchen-sink.darmkey"), "--hide-card"]);
  build("examples/atlas-de-mareas/book.darml", "atlas-de-mareas.darm");
  // Real Argon2id parameters (64 MiB, 3 passes) for the showcase book.
  build("examples/the-lighthouse-protocol/book.darml", "the-lighthouse-protocol.darm", ["--encrypt", "--password-env", "PW"], { PW: LIGHTHOUSE_PASSWORD });
  // The same book without protection, for tests that are about reading it rather than unlocking it.
  build("examples/the-lighthouse-protocol/book.darml", "lighthouse-clear.darm");

  // Hostile variants of a valid book.
  const good = readFileSync(join(TMP, "kitchen-sink.darm"));
  const flipped = Buffer.from(good);
  flipped[flipped.length - 2000]! ^= 0x01; // inside a stored chunk
  writeFileSync(join(TMP, "tampered.darm"), flipped);
  writeFileSync(join(TMP, "truncated.darm"), good.subarray(0, good.length - 500));
  writeFileSync(join(TMP, "garbage.darm"), Buffer.from("PK\u0003\u0004 this is not a DARM book, it only has the extension ".repeat(40)));
  const head = Buffer.from(good);
  head[40]! ^= 0xff; // inside the fixed header
  writeFileSync(join(TMP, "bad-header.darm"), head);
}
