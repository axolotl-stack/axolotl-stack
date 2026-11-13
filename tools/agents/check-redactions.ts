// Fails if a redacted BDS-internal name appears in any tracked file: text, binary captures,
// and hex- or base64-encoded fixtures (e.g. `"hex"` fields in capture `.jsonl` files).
//
//   bun tools/agents/check-redactions.ts            # working tree (CI)
//   bun tools/agents/check-redactions.ts <rev>...   # e.g. origin/some-branch, before merging it
//
// The provenance rule (AGENTS.md) bans BDS internal names everywhere. BDS sends some of them
// to clients inside packets, so raw captures can carry them; redact those captures by
// re-encoding the packet without the name (see `crates/unastar/src/network/violation.rs`).
//
// Names are assembled from pieces so this file does not contain them itself.
import { resolve } from "node:path";

const root = resolve(import.meta.dir, "../..");
const redacted = [
  // The packet reader named in BDS's `PacketViolationWarning` messages (owner decision 2026-11-12).
  ["read", "No", "Header"].join(""),
];

/** Every substring a base64 encoding of `bytes` must contain, one per alignment. */
function base64Forms(bytes: Uint8Array): string[] {
  const forms: string[] = [];
  for (let pad = 0; pad < 3; pad++) {
    const padded = new Uint8Array(pad + bytes.length);
    padded.set(bytes, pad);
    const encoded = Buffer.from(padded).toString("base64");
    // Drop characters touched by the padding bytes or the unknown bytes after the name.
    const start = Math.ceil((pad * 4) / 3);
    const end = Math.floor(((pad + bytes.length) * 4) / 3);
    forms.push(encoded.slice(start, end));
  }
  return forms;
}

const patterns = new Set<string>();
for (const name of redacted) {
  const bytes = new TextEncoder().encode(name);
  patterns.add(name);
  patterns.add(Buffer.from(bytes).toString("hex"));
  for (const form of base64Forms(bytes)) patterns.add(form);
}

// `-a` searches binary files as text; `-i` also catches upper-case hex. Case folding can only
// add false positives for the base64 forms, never hide a match.
const revs = process.argv.slice(2);
const args = ["grep", "-n", "-a", "-i", "-F", "-o"];
for (const pattern of patterns) args.push("-e", pattern);
args.push(...revs);
const result = Bun.spawnSync(["git", ...args], { cwd: root, stdout: "pipe", stderr: "pipe" });
if (result.exitCode === 1) {
  console.log(`no redacted names in tracked files (${patterns.size} encodings checked)`);
  process.exit(0);
}
if (result.exitCode !== 0) {
  console.error(result.stderr.toString());
  process.exit(2);
}
const located = new Set<string>();
for (const line of result.stdout.toString().trim().split("\n")) {
  // `[rev:]path:line:match`; print only the location so binary content stays off the console.
  located.add(line.split(":").slice(0, revs.length > 0 ? 3 : 2).join(":"));
}
for (const location of located) {
  console.error(`${location}: contains a redacted BDS-internal name (provenance rule, AGENTS.md)`);
}
process.exit(1);
