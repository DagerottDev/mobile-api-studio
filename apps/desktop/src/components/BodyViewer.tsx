import { useState } from "react";
import { invoke } from "../api/invoke";
import type { BodyPayload, BodyRef } from "../types";

const MAX_PREVIEW = 100_000;
const MAX_DESCRIPTOR = 2 * 1024 * 1024;
const RASTER_TYPES = new Set(["image/png", "image/jpeg", "image/gif", "image/webp", "image/bmp", "image/avif"]);

interface DecodedMessage {
  index: number;
  offset: number;
  length: number;
  compressed: boolean;
  rawBase64: string;
  decoded: unknown | null;
  decodeError: string | null;
}

interface DecodeResult {
  kind: "grpc" | "protobuf" | "raw";
  contentType: string;
  byteSize: number;
  rawBase64: string;
  messages: DecodedMessage[];
  error: string | null;
}

export function BodyViewer({ title, bodyRef, payload }: { title: string; bodyRef: BodyRef | null; payload: BodyPayload | null }) {
  const [view, setView] = useState<"formatted" | "raw">("formatted");
  const [descriptorBase64, setDescriptorBase64] = useState<string | null>(null);
  const [descriptorName, setDescriptorName] = useState("");
  const [messageType, setMessageType] = useState("");
  const [decoded, setDecoded] = useState<{ sha256: string; result: DecodeResult } | null>(null);
  const [decodeError, setDecodeError] = useState<string | null>(null);
  const [decoding, setDecoding] = useState(false);

  if (!bodyRef) return <section className="inspector-section"><h3>{title}</h3><p className="muted-copy">No body captured.</p></section>;

  const mime = bodyRef.contentType?.split(";", 1)[0].trim().toLowerCase() ?? "";
  const currentPayload = payload?.sha256 === bodyRef.sha256 ? payload : null;
  const text = currentPayload?.text ?? null;
  const base64 = currentPayload?.base64 ?? null;
  const protocol = mime === "application/grpc" || mime.startsWith("application/grpc+") || ["application/protobuf", "application/x-protobuf", "application/vnd.google.protobuf"].includes(mime);
  const raster = RASTER_TYPES.has(mime) && base64 !== null && bodyRef.byteSize <= 5 * 1024 * 1024;
  let parseText = bodyRef.isBinary ? null : text;
  if (parseText === null && mime === "multipart/form-data" && base64 && base64.length <= MAX_PREVIEW * 1.4) {
    try { parseText = atob(base64); }
    catch { /* Keep the raw base64 view. */ }
  }
  const formatted = parseText === null ? null : formatText(parseText, mime, bodyRef.contentType ?? "", !bodyRef.isBinary);
  const canFormat = raster || formatted !== null || protocol;
  const activeDecode = decoded?.sha256 === bodyRef.sha256 ? decoded.result : null;
  const raw = bodyRef.isBinary ? (base64 ? `Base64\n${base64}` : "Binary body unavailable") : text ?? (base64 ? `Base64\n${base64}` : "Loading body…");

  async function loadDescriptor(file: File | null) {
    if (!file) return;
    setDecoded(null);
    setDescriptorBase64(null);
    setDescriptorName("");
    if (file.size > MAX_DESCRIPTOR) { setDecodeError("Descriptor must be 2 MiB or smaller."); return; }
    try {
      const dataBase64 = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => typeof reader.result === "string" ? resolve(reader.result.split(",", 2)[1] ?? "") : reject(new Error("Could not read descriptor."));
        reader.onerror = () => reject(reader.error ?? new Error("Could not read descriptor."));
        reader.readAsDataURL(file);
      });
      setDescriptorBase64(dataBase64);
      setDescriptorName(file.name);
      setDecodeError(null);
    } catch (error) { setDecodeError(errorMessage(error)); }
  }

  async function decode() {
    if (!bodyRef || decoding) return;
    setDecoding(true); setDecodeError(null); setDecoded(null);
    try {
      const result = await invoke<DecodeResult>("decode_protocol_body", {
        sha256: bodyRef.sha256,
        contentType: bodyRef.contentType ?? "",
        descriptorBase64,
        messageType: messageType.trim() || null,
      });
      setDecoded({ sha256: bodyRef.sha256, result });
    } catch (error) { setDecodeError(errorMessage(error)); }
    finally { setDecoding(false); }
  }

  return <section className="inspector-section">
    <div className="section-title-row"><h3>{title}</h3><span>{bodyRef.contentType ?? "unknown"} · {bodyRef.byteSize} bytes{bodyRef.isTruncated ? " · capture truncated" : ""}</span></div>
    {canFormat ? <div className="inspector-actions" aria-label={`${title} view`}>
      <button className="secondary compact" aria-pressed={view === "formatted"} onClick={() => setView("formatted")}>Formatted</button>
      <button className="secondary compact" aria-pressed={view === "raw"} onClick={() => setView("raw")}>Raw</button>
    </div> : null}
    {view === "raw" || !canFormat ? <pre>{limited(raw)}</pre>
      : raster ? <img src={`data:${mime};base64,${base64}`} alt={`${title} raster preview`} style={{ maxWidth: "100%", maxHeight: 320, objectFit: "contain" }} />
        : protocol ? <>
          <p className="muted-copy">Decode captured gRPC or Protobuf bytes. Raw base64 remains available above.</p>
          <label className="field-label">FileDescriptorSet (2 MiB max)<input type="file" onChange={(event) => { const file = event.currentTarget.files?.[0] ?? null; event.currentTarget.value = ""; void loadDescriptor(file); }} /></label>
          {descriptorName ? <p className="muted-copy">Descriptor: {descriptorName}</p> : null}
          <label className="field-label">Full message type<input className="text-input" value={messageType} onChange={(event) => { setMessageType(event.target.value); setDecoded(null); }} placeholder="demo.Widget" /></label>
          <button className="secondary compact" onClick={() => void decode()} disabled={decoding || !currentPayload}>{decoding ? "Decoding…" : "Decode body"}</button>
          {decodeError ? <p role="alert">{decodeError}</p> : null}
          {activeDecode ? <>
            <p className="muted-copy">{activeDecode.kind === "grpc" ? "gRPC frames" : activeDecode.kind === "protobuf" ? "Protobuf message" : "Raw body"} · {activeDecode.byteSize} bytes</p>
            {activeDecode.error ? <p role="alert">{activeDecode.error}</p> : null}
            {activeDecode.messages.map((message) => <div key={message.index}>
              <p>Message {message.index + 1} · offset {message.offset} · {message.length} bytes{message.compressed ? " · compressed" : ""}</p>
              {message.decodeError ? <p role="status">{message.decodeError}</p> : null}
              <pre>{limited(message.decoded !== null ? JSON.stringify(message.decoded, null, 2) : `Base64\n${message.rawBase64}`)}</pre>
            </div>)}
            {!activeDecode.messages.length ? activeDecode.byteSize === 0 ? <p className="muted-copy">Empty body.</p> : <pre>{limited(`Base64\n${activeDecode.rawBase64}`)}</pre> : null}
          </> : null}
        </> : formatted ? <>
          {formatted.label ? <p className="muted-copy">{formatted.label}</p> : null}
          <pre>{limited(formatted.value)}</pre>
        </> : <pre>{limited(raw)}</pre>}
  </section>;
}

function formatText(text: string, mime: string, contentType: string, showFieldValues: boolean): { label: string; value: string } | null {
  if (text.length > MAX_PREVIEW) return null;
  if (mime === "application/graphql") {
    const query = formatGraphql(text);
    return { label: `GraphQL operation: ${query.operation}`, value: query.formatted };
  }
  if (mime === "application/json" || mime.endsWith("+json")) {
    try {
      const value: unknown = JSON.parse(text);
      if (value && typeof value === "object" && !Array.isArray(value) && "query" in value && typeof value.query === "string") {
        const query = formatGraphql(value.query);
        const operation = "operationName" in value && typeof value.operationName === "string" && value.operationName ? value.operationName : query.operation;
        const extras = ["variables", "extensions"].flatMap((field) => field in value ? [`${field[0].toUpperCase()}${field.slice(1)}\n${JSON.stringify(value[field as keyof typeof value], null, 2)}`] : []);
        return { label: `GraphQL operation: ${operation}`, value: [query.formatted, ...extras].join("\n\n") };
      }
      return { label: "JSON", value: JSON.stringify(value, null, 2) };
    } catch { return null; }
  }
  if (mime === "application/xml" || mime === "text/xml" || mime.endsWith("+xml")) {
    if (/<!DOCTYPE|<!ENTITY/i.test(text)) return null;
    const document = new DOMParser().parseFromString(text, "application/xml");
    if (document.getElementsByTagName("parsererror").length) return null;
    const serialized = new XMLSerializer().serializeToString(document);
    let depth = 0;
    const lines = serialized.replace(/>\s*</g, ">\n<").split("\n").map((line) => {
      if (line.startsWith("</")) depth = Math.max(0, depth - 1);
      const formatted = `${"  ".repeat(Math.min(depth, 32))}${line}`;
      if (/^<[^!?/][^>]*[^/]>$/.test(line)) depth++;
      return formatted;
    });
    return { label: "XML", value: lines.join("\n") };
  }
  if (mime === "application/x-www-form-urlencoded") {
    const fields = [...new URLSearchParams(text)].slice(0, 100);
    return { label: "Form fields", value: fields.map(([name, value]) => `${name}: ${value.slice(0, 1000)}`).join("\n") || "(empty form)" };
  }
  if (mime === "multipart/form-data") {
    const boundaryMatch = /boundary=(?:"([^"]+)"|([^;]+))/i.exec(contentType);
    const boundary = boundaryMatch?.[1] ?? boundaryMatch?.[2];
    if (!boundary || boundary.length > 200) return null;
    const parts = text.split(`--${boundary}`).slice(1, 21).filter((part) => part.trim() && part.trim() !== "--");
    if (!parts.length) return null;
    const lines = parts.map((part, index) => {
      const split = part.indexOf("\r\n\r\n");
      if (split < 0) return `Part ${index + 1}: malformed headers`;
      const headers = part.slice(0, split);
      const disposition = /content-disposition:[^\r\n]*/i.exec(headers)?.[0] ?? "";
      const name = /name="([^"]*)"/i.exec(disposition)?.[1] ?? `part ${index + 1}`;
      const filename = /filename="([^"]*)"/i.exec(disposition)?.[1];
      const body = part.slice(split + 4).replace(/\r\n$/, "");
      return `${name}${filename ? ` (${filename}, ${body.length} bytes)` : showFieldValues ? `: ${body.slice(0, 1000)}` : ` (${body.length} bytes; binary body)`}`;
    });
    return { label: `Multipart parts (showing up to 20)`, value: lines.join("\n") };
  }
  if (mime.startsWith("text/")) return { label: "Text", value: text };
  return null;
}

function formatGraphql(query: string): { formatted: string; operation: string } {
  let formatted = "";
  let searchable = "";
  let depth = 0;
  let quoted = false;
  let block = false;
  let comment = false;
  const newline = () => { formatted = `${formatted.trimEnd()}\n${"  ".repeat(Math.min(depth, 32))}`; };
  for (let index = 0; index < query.length; index++) {
    const char = query[index];
    if (comment) {
      searchable += char === "\n" ? "\n" : " ";
      if (char === "\n") { comment = false; newline(); } else formatted += char;
    } else if (block) {
      if (char === "\\" && query.startsWith('"""', index + 1)) { formatted += '\\"""'; searchable += "    "; index += 3; }
      else if (query.startsWith('"""', index)) { formatted += '"""'; searchable += "   "; index += 2; block = false; }
      else { formatted += char; searchable += " "; }
    } else if (quoted) {
      formatted += char; searchable += " ";
      if (char === "\\" && index + 1 < query.length) { formatted += query[++index]; searchable += " "; }
      else if (char === '"') quoted = false;
    } else if (query.startsWith('"""', index)) {
      formatted += '"""'; searchable += "   "; index += 2; block = true;
    } else if (char === '"') { formatted += char; searchable += " "; quoted = true; }
    else if (char === "#") { formatted += char; searchable += " "; comment = true; }
    else if (char === "{") { searchable += char; formatted = `${formatted.trimEnd()} {`; depth++; newline(); }
    else if (char === "}") { searchable += char; depth = Math.max(0, depth - 1); newline(); formatted += "}"; }
    else if (char === "\n" || char === "\r") { searchable += "\n"; if (char === "\n") newline(); }
    else { searchable += char; formatted += char; }
  }
  const operation = /\b(?:query|mutation|subscription)\s+([_A-Za-z][_0-9A-Za-z]*)/.exec(searchable)?.[1] ?? "unnamed";
  return { formatted: formatted.trim(), operation };
}

function limited(value: string) { return value.length > MAX_PREVIEW ? `${value.slice(0, MAX_PREVIEW)}\n… UI preview truncated …` : value; }
function errorMessage(value: unknown) { return typeof value === "string" ? value : value && typeof value === "object" && "message" in value ? String(value.message) : String(value); }
