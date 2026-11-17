// End-to-end smoke test of the darm-wasm binding in plain Node (no npm deps).
//
//   node crates/darm-wasm/tests/smoke.mjs
//
// It builds the crate for wasm32-unknown-unknown (profile wasm-release), runs
// `wasm-bindgen --target web` into a temporary directory — the same bundle
// shape the reader app uses — loads it with `initSync`, and then:
//   * opens the golden (clear) fixture and the encrypted fixtures of
//     crates/darm-package/tests/fixtures,
//   * decodes every chunk of every entry, reading the stored bytes from the
//     file at chunkOffset/chunkLen like a browser would slice a File,
//   * compares each entry's SHA-256 with the manifest,
//   * checks the error contract (code property, "<CODE>: message").
//
// Pass --no-build to reuse an existing target/…/darm_wasm.wasm.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const fixtures = path.join(root, "crates/darm-package/tests/fixtures");
const targetDir = process.env.CARGO_TARGET_DIR ?? path.join(root, "target");
const wasmPath = path.join(targetDir, "wasm32-unknown-unknown/wasm-release/darm_wasm.wasm");

// Credentials of the committed fixtures (see crates/darm-package/tests/golden.rs).
const PASSWORD = "golden-password";
const KEY = Uint8Array.from([
  0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
  0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
]);

let checks = 0;
function check(condition, message) {
  checks += 1;
  if (!condition) {
    throw new Error(`check failed: ${message}`);
  }
}

function expectThrow(code, fn, label) {
  let thrown = null;
  try {
    fn();
  } catch (error) {
    thrown = error;
  }
  check(thrown instanceof Error, `${label}: expected a thrown Error`);
  check(thrown.code === code, `${label}: expected code ${code}, got ${thrown.code} (${thrown.message})`);
  check(thrown.message.startsWith(`${code}: `), `${label}: message must start with the code: ${thrown.message}`);
  check(thrown.message.length > code.length + 10, `${label}: message must be human readable`);
  return thrown;
}

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

// ---- build and load ---------------------------------------------------------

if (!process.argv.includes("--no-build")) {
  execFileSync(
    "cargo",
    ["build", "-p", "darm-wasm", "--target", "wasm32-unknown-unknown", "--profile", "wasm-release"],
    { cwd: root, stdio: "inherit" },
  );
}
const outDir = fs.mkdtempSync(path.join(os.tmpdir(), "darm-wasm-smoke-"));
let wasm;
try {
  execFileSync(
    "wasm-bindgen",
    ["--target", "web", "--out-dir", outDir, "--out-name", "darm_wasm", wasmPath],
    { cwd: root, stdio: ["ignore", "inherit", "pipe"] },
  );
  // Make Node treat the generated .js as an ES module regardless of its defaults.
  fs.writeFileSync(path.join(outDir, "package.json"), '{"type":"module"}\n');
  wasm = await import(pathToFileURL(path.join(outDir, "darm_wasm.js")).href);
  const module = fs.readFileSync(path.join(outDir, "darm_wasm_bg.wasm"));
  wasm.initSync({ module });
  console.log(`loaded darm_wasm_bg.wasm (${module.length} bytes) from ${outDir}`);
} finally {
  fs.rmSync(outDir, { recursive: true, force: true });
}
const { DarmPackage, probe, headerLen, parseKeyfile } = wasm;

// ---- helpers ----------------------------------------------------------------

/** Minimal stand-in for a browser File: positional reads only. */
class FileSource {
  constructor(file) {
    this.fd = fs.openSync(file, "r");
    this.size = fs.fstatSync(this.fd).size;
  }
  slice(offset, length) {
    const buffer = new Uint8Array(length);
    const read = fs.readSync(this.fd, buffer, 0, length, offset);
    return buffer.subarray(0, read);
  }
  close() {
    fs.closeSync(this.fd);
  }
}

/** Probes, opens, decodes every chunk of every entry and verifies the hashes. */
function verifyPackage(label, file, { password, key } = {}) {
  const source = new FileSource(path.join(fixtures, file));
  try {
    // Step 1: the fixed header is enough to learn how long the head is.
    const first = JSON.parse(probe(source.slice(0, headerLen())));
    check(first.format === "1.0", `${label}: format`);
    check(first.totalLen === source.size, `${label}: totalLen ${first.totalLen} != file size ${source.size}`);
    check(first.preludeLoaded === false, `${label}: header-only probe must not claim a prelude`);
    check(first.headLen > headerLen() && first.headLen <= source.size, `${label}: headLen`);

    // Step 2: the head (header + prelude + manifest).
    const head = source.slice(0, first.headLen);
    const info = JSON.parse(probe(head));
    check(info.preludeLoaded === true, `${label}: preludeLoaded`);
    check(info.encrypted === (password !== undefined || key !== undefined), `${label}: encrypted flag`);

    const started = performance.now();
    const pkg = DarmPackage.open(head, password, key);
    const openMs = performance.now() - started;
    try {
      check(pkg.totalLen() === source.size, `${label}: totalLen()`);
      check(pkg.dataOffset() === first.headLen, `${label}: dataOffset()`);
      check(pkg.isEncrypted() === info.encrypted, `${label}: isEncrypted()`);
      const manifest = JSON.parse(pkg.manifestJson());
      check(manifest.manifest_version === 1, `${label}: manifest_version`);
      check(pkg.entryCount() === manifest.entries.length, `${label}: entryCount`);
      check(pkg.entryIndex("no/such/entry") === -1, `${label}: entryIndex of a missing name`);

      let chunks = 0;
      let bytes = 0;
      for (const entry of manifest.entries) {
        check(pkg.entryIndex(entry.name) === entry.index, `${label}: entryIndex(${entry.name})`);
        const count = pkg.chunkCount(entry.index);
        check(count === entry.chunks.length, `${label}: chunkCount(${entry.name})`);
        const hash = createHash("sha256");
        let size = 0;
        for (let chunk = 0; chunk < count; chunk += 1) {
          const offset = pkg.chunkOffset(entry.index, chunk);
          const length = pkg.chunkLen(entry.index, chunk);
          check(length === entry.chunks[chunk].len, `${label}: chunkLen(${entry.name}, ${chunk})`);
          const plain = pkg.decodeChunk(entry.index, chunk, source.slice(offset, length));
          check(plain instanceof Uint8Array, `${label}: decodeChunk returns a Uint8Array`);
          hash.update(plain);
          size += plain.length;
          chunks += 1;
        }
        check(size === entry.size, `${label}: ${entry.name} size ${size} != ${entry.size}`);
        check(hash.digest("hex") === entry.sha256, `${label}: ${entry.name} SHA-256 mismatch`);
        bytes += size;

        // Uncompressed entries of clear packages are stored verbatim at dataOffset + offset.
        if (!info.encrypted && entry.compression === "none" && entry.size > 0) {
          const direct = source.slice(pkg.dataOffset() + entry.offset, entry.size);
          check(sha256(direct) === entry.sha256, `${label}: ${entry.name} is not stored verbatim`);
        }
      }

      // A tampered chunk must be refused.
      const victim = manifest.entries.find((entry) => entry.chunks.length > 0);
      const stored = source.slice(pkg.chunkOffset(victim.index, 0), pkg.chunkLen(victim.index, 0));
      stored[stored.length >> 1] ^= 0x01;
      expectThrow("E_PKG_INTEGRITY", () => pkg.decodeChunk(victim.index, 0, stored), `${label}: tampered chunk`);
      expectThrow("E_PKG_NOT_FOUND", () => pkg.chunkOffset(9999, 0), `${label}: bad entry index`);
      expectThrow("E_PKG_NOT_FOUND", () => pkg.chunkLen(0, 9999), `${label}: bad chunk index`);

      console.log(
        `ok  ${label}: ${manifest.entries.length} entries, ${chunks} chunks, ${bytes} bytes verified ` +
          `(open took ${openMs.toFixed(1)} ms)`,
      );
      return { info, manifest, openMs, head };
    } finally {
      pkg.free();
    }
  } finally {
    source.close();
  }
}

// ---- tests ------------------------------------------------------------------

check(headerLen() === 128, "headerLen() must be 128");

// Clear golden fixture.
const golden = verifyPackage("golden-v1.darm (clear)", "golden-v1.darm");
check(golden.info.uuid === "7dcd6c88-abac-85ae-81d8-778e6b427bdc", "golden uuid");
check(golden.info.card.title === "The Golden Fixture", "golden card title");
check(golden.info.card.cover_color === "#c9a227", "golden card colour");
check(golden.info.slots.length === 0 && golden.info.kdf === null, "golden has no slots");
check(golden.manifest.entries[0].role === "ir", "entry 0 is the IR");
check(golden.manifest.book.title === "The Golden Fixture", "book summary");

// Encrypted fixture: password, then key file.
const withPassword = verifyPackage("encrypted-v1.darm (password)", "encrypted-v1.darm", { password: PASSWORD });
check(withPassword.info.slots.map((slot) => slot.kind).join() === "password,keyfile", "slot kinds");
check(withPassword.info.kdf.m_kib === 8192, "fixture KDF memory");
check(withPassword.info.card.title === "The Golden Fixture", "exposed card on a locked book");

const keyfileText = JSON.stringify({ darmkey: 1, key: Buffer.from(KEY).toString("base64"), created: "2026-01-01T00:00:00Z" });
const parsedKey = parseKeyfile(keyfileText);
check(parsedKey instanceof Uint8Array && parsedKey.length === 32, "parseKeyfile returns 32 bytes");
check(Buffer.from(parsedKey).equals(Buffer.from(KEY)), "parseKeyfile returns the key");
check(withPassword.info.slots[1].key_id.length === 16, "keyfile slot key_id");
verifyPackage("encrypted-v1.darm (key file)", "encrypted-v1.darm", { key: parsedKey });

// Error contract.
const head = withPassword.head;
const wrong = expectThrow("E_PKG_WRONG_CREDENTIALS", () => DarmPackage.open(head, "not the password"), "wrong password");
console.log(`ok  wrong password -> ${wrong.name}: ${wrong.message} (code=${wrong.code})`);
expectThrow("E_PKG_WRONG_CREDENTIALS", () => DarmPackage.open(head, undefined, new Uint8Array(32)), "wrong key");
const locked = expectThrow("E_PKG_CREDENTIALS_REQUIRED", () => DarmPackage.open(head), "no credentials");
console.log(`ok  no credentials -> ${locked.message}`);
expectThrow("E_PKG_BAD_OPTION", () => DarmPackage.open(head, undefined, new Uint8Array(5)), "short key");
expectThrow("E_PKG_BAD_MAGIC", () => probe(new TextEncoder().encode("this is not a darm package")), "bad magic");
expectThrow("E_PKG_TRUNCATED", () => probe(head.subarray(0, 64)), "short probe");
expectThrow("E_PKG_TRUNCATED", () => DarmPackage.open(head.subarray(0, head.length - 1), PASSWORD), "short head");
expectThrow("E_PKG_BAD_KEYFILE", () => parseKeyfile("{}"), "bad key file");
{
  const flipped = head.slice();
  flipped[200] ^= 0x40;
  expectThrow("E_PKG_INTEGRITY", () => DarmPackage.open(flipped, PASSWORD), "tampered prelude");
}
// The key is tried first, the password second; either suffices.
DarmPackage.open(head, "not the password", parsedKey).free();
DarmPackage.open(head, PASSWORD, new Uint8Array(32)).free();

// Default KDF parameters (Argon2id, 64 MiB, t=3, p=1) inside WebAssembly memory.
const real = verifyPackage("encrypted-default-kdf-v1.darm (Argon2id 64 MiB, t=3)", "encrypted-default-kdf-v1.darm", {
  password: PASSWORD,
});
check(real.info.kdf.m_kib === 65536 && real.info.kdf.t === 3 && real.info.kdf.p === 1, "default KDF parameters");
const again = performance.now();
expectThrow("E_PKG_WRONG_CREDENTIALS", () => DarmPackage.open(real.head, "wrong"), "wrong password, default KDF");
console.log(
  `ok  Argon2id m=64 MiB t=3 p=1 in WASM: ${real.openMs.toFixed(0)} ms to unlock, ` +
    `${(performance.now() - again).toFixed(0)} ms to reject a wrong password (node ${process.version})`,
);

console.log(`\nsmoke test passed: ${checks} checks`);
