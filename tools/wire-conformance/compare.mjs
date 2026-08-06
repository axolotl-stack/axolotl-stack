import fs from "node:fs/promises";
import path from "node:path";

// Inputs and generated reports live in WIRE_CONFORMANCE_DIR (the workflow
// points this at a scratch directory); the accepted-divergence baseline is
// always read from this tool's own directory, since it is reviewed code.
const toolDir = path.dirname(new URL(import.meta.url).pathname.replace(/^\/(.:)/, "$1"));
const here = process.env.WIRE_CONFORMANCE_DIR
  ? path.resolve(process.env.WIRE_CONFORMANCE_DIR)
  : toolDir;
const readJSON = async name => JSON.parse(await fs.readFile(path.join(here, name), "utf8"));

const FIXED_EQUIV = new Map([
  ["I8", "FIXED8"], ["U8", "FIXED8"],
  ["I16LE", "FIXED16LE"], ["U16LE", "FIXED16LE"],
  ["I16BE", "FIXED16BE"], ["U16BE", "FIXED16BE"],
  ["I32LE", "FIXED32LE"], ["U32LE", "FIXED32LE"],
  ["I32BE", "FIXED32BE"], ["U32BE", "FIXED32BE"],
  ["I64LE", "FIXED64LE"], ["U64LE", "FIXED64LE"],
  ["I64BE", "FIXED64BE"], ["U64BE", "FIXED64BE"],
]);

function canonical(op) { return FIXED_EQUIV.get(op) ?? op; }
function canonicalPrimitive(op, field) {
  // gophertunnel intentionally stores these already-encoded NetworkLittleEndian NBT compounds as []byte and writes them verbatim.
  if (op === "RawBytes" && /Serialised(?:Offers|InventoryData|EntityIdentifiers)$/.test(field ?? "")) return "Nbt";
  return canonical(op);
}
function atom(token, field, display = token) { return { token, field: field ?? "", display }; }

function unresolvedIn(operations, side) {
  const reasons = [];
  const visit = (op, trail) => {
    if (!op || typeof op !== "object") {
      reasons.push(`${side}: malformed operation at ${trail}`);
      return;
    }
    if (op.kind === "unresolved") reasons.push(`${side}: ${op.reason ?? "unresolved"}${op.site ? ` (${op.site})` : ""}`);
    if (op.kind === "recursive_reference") reasons.push(`${side}: recursive reference to ${op.type_name ?? "unknown type"} at ${op.field ?? trail}`);
    if (op.kind === "union") {
      if (!Array.isArray(op.variants) || !op.variants.length) reasons.push(`${side}: empty/unresolved union at ${op.field ?? trail}`);
      for (const v of op.variants ?? []) for (const child of v.operations ?? []) visit(child, `${trail}/variant:${v.value}`);
    }
    for (const key of ["element", "value"]) for (const child of op[key] ?? []) visit(child, `${trail}/${key}`);
  };
  for (let i = 0; i < (operations ?? []).length; i++) visit(operations[i], `${i}`);
  return [...new Set(reasons)];
}

function flatten(operations) {
  const out = [];
  const pushOps = ops => { for (const op of ops ?? []) push(op); };
  const push = op => {
    const field = op.field ?? "";
    switch (op.kind) {
      case "primitive": {
        const c = canonicalPrimitive(op.op, field);
        out.push(atom(`P:${c}`, field, op.op));
        break;
      }
      case "string": {
        // UTF-8 versus byte-slice is an interpretation constraint, not a different wire write:
        // both emit the same prefix followed by the exact supplied bytes.
        out.push(atom(`STRING:${canonical(op.prefix)}`, field, `string(prefix=${op.prefix},encoding=${op.encoding ?? "unspecified"})`));
        break;
      }
      case "array":
        out.push(atom(`ARRAY<${canonical(op.prefix)}>`, field, `array(prefix=${op.prefix})`));
        pushOps(op.element);
        out.push(atom("/ARRAY", field, "/array"));
        break;
      case "fixed_array":
        out.push(atom(`FIXED_ARRAY<${op.length}>`, field, `fixed_array(length=${op.length})`));
        pushOps(op.element);
        out.push(atom("/FIXED_ARRAY", field, "/fixed_array"));
        break;
      case "option":
        out.push(atom(`OPTION<${canonical(op.presence)}>`, field, `option(presence=${op.presence})`));
        pushOps(op.value);
        out.push(atom("/OPTION", field, "/option"));
        break;
      case "union": {
        out.push(atom(`UNION<${canonical(op.control)}>`, field, `union(control=${op.control})`));
        for (const v of [...(op.variants ?? [])].sort((a,b) => String(a.value).localeCompare(String(b.value), undefined, {numeric:true}))) {
          out.push(atom(`VARIANT<${v.value}>`, field, `variant(${v.value}:${v.name ?? ""})`));
          pushOps(v.operations);
          out.push(atom("/VARIANT", field, "/variant"));
        }
        out.push(atom("/UNION", field, "/union"));
        break;
      }
      case "unresolved":
      case "recursive_reference":
        break;
      default:
        out.push(atom(`UNKNOWN_KIND:${op.kind}`, field, `unknown kind ${op.kind}`));
    }
  };
  pushOps(operations);
  return out;
}

function diffSequence(a, b) {
  const n = a.length, m = b.length;
  const dp = Array.from({length:n+1}, () => new Uint16Array(m+1));
  for (let i=n; i>=0; i--) for (let j=m; j>=0; j--) {
    if (i === n) dp[i][j] = m-j;
    else if (j === m) dp[i][j] = n-i;
    else if (a[i].token === b[j].token) dp[i][j] = dp[i+1][j+1];
    else dp[i][j] = Math.min(1+dp[i+1][j+1], 1+dp[i+1][j], 1+dp[i][j+1]);
  }
  const rows = []; let i=0,j=0,wirePos=0;
  while (i<n || j<m) {
    if (i<n && j<m && a[i].token === b[j].token) { i++; j++; wirePos++; continue; }
    if (i<n && j<m && dp[i][j] === 1+dp[i+1][j+1]) {
      rows.push({position:wirePos, ours:a[i], gophertunnel:b[j]}); i++;j++;wirePos++;continue;
    }
    if (i<n && dp[i][j] === 1+dp[i+1][j]) { rows.push({position:wirePos, ours:a[i], gophertunnel:null});i++;wirePos++; }
    else { rows.push({position:wirePos, ours:null, gophertunnel:b[j]});j++;wirePos++; }
  }
  return rows;
}

function cloudTokens(op) {
  const p = x => `P:${canonical(x)}`;
  if (!op) return [];
  if (op === "String") return ["STRING:VarInt"];
  if (op === "Uuid") return ["FIXED_ARRAY<16>", p("U8"), "/FIXED_ARRAY"];
  if (op === "Vec3") return [p("F32LE"), p("F32LE"), p("F32LE")];
  if (op === "Option(?)") return ["OPTION<Bool>", "/OPTION"];
  let m = op.match(/^Array\(prefix=([^),]+)\)$/);
  if (m) return [`ARRAY<${canonical(m[1])}>`, "/ARRAY"];
  m = op.match(/^Option\((.+)\)$/);
  if (m) {
    const inner = m[1] === "String" ? ["STRING:VarInt"] : m[1] === "Vec3" ? [p("F32LE"),p("F32LE"),p("F32LE")] : [];
    return ["OPTION<Bool>", ...inner, "/OPTION"];
  }
  const direct = {
    "Unmapped(buffer.writeByte)":"U8", "Unmapped(buffer.writeLongLE)":"I64LE",
    "Unmapped(buffer.writeInt)":"I32BE", "Unmapped(buffer.writeFloat)":"F32BE",
    "Unmapped(buffer.writeBytes)":"RawBytes", "Unmapped(helper.writeByteAngle)":"U8",
  }[op];
  if (direct) return [p(direct)];
  if (op.startsWith("Unmapped(")) return [];
  return [p(op)];
}

function normField(x) {
  let s=String(x??"").split(".").at(-1).replaceAll("[]","").toLowerCase().replace(/[^a-z0-9]/g,"");
  if (s.endsWith("s") && s.length>3) s=s.slice(0,-1);
  return s;
}

function cloudEvidence(packet) {
  return (packet?.fields ?? []).map(f=>{
    const tokens=cloudTokens(f.op), p=x=>`P:${canonical(x)}`, notes=f.notes??"";
    // Use only explicit primitive writer calls visible in Cloudburst's extracted source note.
    if (f.op.startsWith("Array(") || f.op === "Option(?)") {
      const close=tokens.pop();
      if (/VarInts\.writeInt\(/.test(notes)) tokens.push(p("ZigZag32"));
      if (/VarInts\.writeUnsignedInt\(/.test(notes)) tokens.push(p("VarInt"));
      if (/writeIntLE/.test(notes)) tokens.push(p("I32LE"));
      if (/writeFloatLE/.test(notes)) tokens.push(p("F32LE"));
      if (/writeBoolean/.test(notes)) tokens.push(p("Bool"));
      if (close) tokens.push(close);
    }
    return {field:f.name,op:f.op,tokens,notes};
  });
}

function cloudVote(row, cloud) {
  const names=[normField(row.ours?.field),normField(row.gophertunnel?.field)].filter(Boolean);
  let candidates=cloud.filter(c=>names.some(n=>{const q=normField(c.field);return q===n||q.includes(n)||n.includes(q)}));
  if (!candidates.length) return {cloudburst:null,vote:cloud.length?"UNRESOLVED":"NO_CLOUDBURST_FIELD"};
  const tokens=new Set(candidates.flatMap(c=>c.tokens)), ours=row.ours?.token, gt=row.gophertunnel?.token;
  const ao=tokens.has(ours), ag=tokens.has(gt), evidence=candidates.map(c=>`${c.field}: ${c.op}`).join("; ");
  if (ao&&ag) return {cloudburst:evidence,vote:"BOTH"};
  if (ao) return {cloudburst:evidence,vote:"OURS"};
  if (ag) return {cloudburst:evidence,vote:"GOPHERTUNNEL"};
  return {cloudburst:evidence,vote:candidates.some(c=>c.tokens.length)?"NEITHER":"UNRESOLVED"};
}

function summarizeVotes(diffs) {
  const counts={}; for(const d of diffs) counts[d.cloudburst_vote]=(counts[d.cloudburst_vote]??0)+1;
  const decisive=["OURS","GOPHERTUNNEL"].filter(v=>counts[v]);
  if (decisive.length===1 && !counts.BOTH && !counts.NEITHER) return {vote:decisive[0],complete:!counts.UNRESOLVED&&!counts.NO_CLOUDBURST_FIELD,counts};
  if (!decisive.length && counts.UNRESOLVED===diffs.length) return {vote:"UNRESOLVED",complete:false,counts};
  return {vote:"INCONCLUSIVE",complete:false,counts};
}

function countReasons(packets) {
  const counts = new Map();
  for (const p of packets) if (p.classification === "UNRESOLVED") for (const r of p.reasons) {
    const clean = r.replace(/^\w+: /, "").replace(/ \([A-Z]:.*:\d+\)$/, "");
    counts.set(clean, (counts.get(clean) ?? 0) + 1);
  }
  return [...counts].map(([reason,count])=>({reason,count})).sort((a,b)=>b.count-a.count || a.reason.localeCompare(b.reason));
}

function esc(x) { return String(x ?? "—").replaceAll("|", "\\|").replaceAll("\n", " "); }

async function main() {
  const [ours, gt, diagnostics, cloudburst] = await Promise.all([
    readJSON("manifest.json"), readJSON("gophertunnel-flat.json"), readJSON("gtx2-diagnostics.json"),
    // Cloudburst is a secondary cross-check used to adjudicate divergences,
    // not a gate input. CI runs without it; its absence must never be able to
    // turn a divergence into an agreement.
    readJSON("cloudburst.json").catch(() => ({}))
  ]);
  const gtByID = new Map(gt.packets.map(p => [p.id,p]));
  const packets = [], divergenceRows = [];
  for (const p of ours.packets) {
    const oracle = gtByID.get(p.id);
    if (!oracle) { packets.push({id:p.id,name:p.name,classification:"NO_ORACLE_PACKET",reasons:["gophertunnel has no packet with this ID"]}); continue; }
    const reasons = [...unresolvedIn(p.operations,"ours"), ...unresolvedIn(oracle.operations,"gophertunnel")];
    if (reasons.length) { packets.push({id:p.id,name:p.name,gophertunnel_name:oracle.name,classification:"UNRESOLVED",reasons}); continue; }
    const a=flatten(p.operations), b=flatten(oracle.operations);
    if (a.length === b.length && a.every((x,i)=>x.token===b[i].token)) {
      packets.push({id:p.id,name:p.name,gophertunnel_name:oracle.name,classification:"AGREEMENT",operation_count:a.length});
      continue;
    }
    const cloud=cloudEvidence(cloudburst[String(p.id)]), diffs=diffSequence(a,b).map(d=>({...d,...cloudVote(d,cloud)}));
    for (const d of diffs) divergenceRows.push({packet_id:p.id,packet:p.name,gophertunnel_packet:oracle.name,field_position:d.position,ours:d.ours?.display??"missing",ours_field:d.ours?.field??null,gophertunnel:d.gophertunnel?.display??"missing",gophertunnel_field:d.gophertunnel?.field??null,cloudburst:d.cloudburst??"unresolved",cloudburst_vote:d.vote});
    const differences=diffs.map(d=>({field_position:d.position,ours:d.ours?.display??"missing",ours_field:d.ours?.field??null,gophertunnel:d.gophertunnel?.display??"missing",gophertunnel_field:d.gophertunnel?.field??null,cloudburst:d.cloudburst??"unresolved",cloudburst_vote:d.vote}));
    packets.push({id:p.id,name:p.name,gophertunnel_name:oracle.name,classification:"DIVERGENCE",cloudburst_vote_summary:summarizeVotes(differences),ours_sequence:a.map(x=>x.display),gophertunnel_sequence:b.map(x=>x.display),differences});
  }
  const counts={AGREEMENT:0,DIVERGENCE:0,UNRESOLVED:0,NO_ORACLE_PACKET:0};for(const p of packets)counts[p.classification]++;
  const topReasons=countReasons(packets);
  const report={schema_version:1,protocol_version:ours.protocol_version,minecraft_version:ours.minecraft_version,sources:{ours:"manifest.json",gophertunnel:"gophertunnel-flat.json",cloudburst:"cloudburst.json"},normalization:{fixed_width:"Signed and unsigned fixed-width integers of identical width and endianness are equivalent.",preserved_distinctions:["width","endianness","fixed-vs-varint","varint-vs-zigzag","floating-point-vs-integer","option presence","array prefix","fixed array length","union control and variants"],strings:"UTF-8 strings and byte slices with the same prefix are wire-shape equivalent because encoding does not transform the supplied bytes.",uuid:"gophertunnel Writer.UUID is represented as a fixed array of 16 U8 operations. This comparison checks byte length and wire position only; it does not validate gophertunnel's byte-swapped UUID ordering against a naive big-endian UUID layout.",preencoded_nbt:"gophertunnel SerialisedOffers, SerialisedInventoryData, and SerialisedEntityIdentifiers RawBytes are NetworkLittleEndian NBT blobs and normalize to Nbt; their Marshal methods write those supplied bytes verbatim."},counts,top_unresolved_reasons:topReasons,writer_expansion_differences:diagnostics.writer_expansion_differences,divergences:divergenceRows,packets};
  await fs.writeFile(path.join(here,"conformance-final.json"),JSON.stringify(report,null,2)+"\n");
  const md=["# Protocol 2168 conformance", "", `Compared ${ours.packets.length} manifest packets with the gophertunnel oracle.`, "", "| Classification | Packets |", "|---|---:|", ...Object.entries(counts).map(([k,v])=>`| ${k} | ${v} |`), "", "## Divergences", "", "| ID | Packet | Field position | Ours | gophertunnel | Cloudburst | Vote |", "|---:|---|---:|---|---|---|---|", ...divergenceRows.map(d=>`| ${d.packet_id} | ${esc(d.packet)} | ${d.field_position} | ${esc(d.ours)} | ${esc(d.gophertunnel)} | ${esc(d.cloudburst)} | ${d.cloudburst_vote} |`)];
  if (!divergenceRows.length) md.push("| — | None | — | — | — | — | — |");
  md.push("", "## Top unresolved reasons", "", "| Reason | Packets/sites |", "|---|---:|", ...topReasons.slice(0,20).map(x=>`| ${esc(x.reason)} | ${x.count} |`), "", "## Writer expansion verification", "");
  if (diagnostics.writer_expansion_differences.length) for(const d of diagnostics.writer_expansion_differences) md.push(`- ${d.helper}: requested ${d.requested}; actual writer is ${d.actual} (${d.file}:${d.line}).`); else md.push("All requested expansions matched the writer implementation.");
  md.push("", "## UUID normalization limitation", "", "gophertunnel `Writer.UUID` is represented as a fixed array of 16 U8 operations. This comparison checks byte length and wire position only; it does not validate gophertunnel's byte-swapped UUID ordering against a naive big-endian UUID layout.", "", "## Conservative comparison policy", "", "Any unresolved or recursive marker on either side makes the packet UNRESOLVED. Runtime branches are never linearized into a guessed sequence. Width, endianness, variable-integer family, array prefix, fixed length, and option presence remain distinct.", "");
  await fs.writeFile(path.join(here,"conformance-final.md"),md.join("\n"));
  console.log(JSON.stringify({counts,divergence_packets:packets.filter(p=>p.classification==="DIVERGENCE").length,divergence_rows:divergenceRows.length,top_reasons:topReasons.slice(0,8),writer_differences:diagnostics.writer_expansion_differences},null,2));
  return {counts, packets};
}

// Gate: a divergence the baseline does not accept fails the build. Each
// accepted entry carries a reason, so "known divergence" can never quietly
// become "any divergence".
async function gate({counts, packets}) {
  let baseline;
  try {
    baseline = JSON.parse(await fs.readFile(path.join(toolDir, "expected-divergences.json"), "utf8"));
  } catch {
    console.error("no expected-divergences.json next to compare.mjs; refusing to pass silently");
    process.exitCode = 1;
    return;
  }
  const accepted = new Map((baseline.packets ?? []).map(p => [p.id, p]));
  const actual = packets.filter(p => p.classification === "DIVERGENCE").map(p => p.id);

  const unexpected = actual.filter(id => !accepted.has(id));
  const resolved = [...accepted.keys()].filter(id => !actual.includes(id));

  for (const id of resolved) {
    console.warn(`note: packet ${id} no longer diverges; drop it from expected-divergences.json`);
  }
  if (counts.AGREEMENT < (baseline.min_agreement ?? 0)) {
    console.error(`FAIL: agreement ${counts.AGREEMENT} fell below the floor of ${baseline.min_agreement}`);
    process.exitCode = 1;
  }
  if (unexpected.length) {
    console.error(`FAIL: ${unexpected.length} unaccepted divergence(s): ${unexpected.join(", ")}`);
    console.error("Each is a candidate wire bug. Investigate against gophertunnel/Cloudburst,");
    console.error("fix via an override with cited evidence, or add it to expected-divergences.json with a reason.");
    process.exitCode = 1;
    return;
  }
  if (!process.exitCode) console.log("wire conformance gate: OK");
}

await gate(await main());
