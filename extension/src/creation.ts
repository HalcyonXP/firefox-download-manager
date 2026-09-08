/* eslint-disable no-control-regex -- Untrusted input must reject or sanitize control characters. */
import { NativeConnectionError } from "./native-connection";

export interface CreationInput {
  url: string;
  destination: string;
  filename: string;
  workers: number;
}
export interface AddInput {
  url: string;
  destination: string;
  suggested_filename: string;
  workers: 1 | 2 | 4 | 8;
}

export function directUrl(input: string): URL {
  if (input.length > 16_384 || /[\u0000-\u0020\u007f]/u.test(input))
    throw new Error("Enter a direct HTTP or HTTPS URL without spaces or control characters.");
  let url: URL;
  try {
    url = new URL(input);
  } catch {
    throw new Error("Enter an absolute HTTP or HTTPS URL.");
  }
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password)
    throw new Error("Only HTTP(S) URLs without embedded credentials are supported.");
  return url;
}

export function suggestedFilename(input: string): string {
  try {
    const path = directUrl(input).pathname.split("/").at(-1) ?? "";
    return safeFilename(decodeURIComponent(path));
  } catch {
    return "download";
  }
}

// Matches the helper's Windows filename policy, including UTF-16 length.
export function safeFilename(input: string): string {
  let output = input.replace(/[\u0000-\u001f\u007f-\u009f<>:"/\\|?*]+/gu, "_");
  output = [...output].reduce(
    (text, char) => (text.length + char.length <= 180 ? text + char : text),
    "",
  );
  output = output.replace(/^ +|[ .]+$/gu, "") || "download";
  if (/^(con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)/iu.test(output))
    output = `_${output.slice(0, 179)}`;
  return output;
}

export function creationPayload(input: CreationInput): AddInput {
  directUrl(input.url);
  if (
    !/^[a-z]:\\/iu.test(input.destination) ||
    input.destination.length > 32_767 ||
    /[\u0000-\u001f]/u.test(input.destination)
  )
    throw new Error(
      "Enter an existing local folder as an absolute Windows path, such as C:\\Users\\you\\Downloads.",
    );
  if (![1, 2, 4, 8].includes(input.workers)) throw new Error("Choose 1, 2, 4, or 8 connections.");
  if (!input.filename || input.filename !== safeFilename(input.filename))
    throw new Error("Choose a Windows-safe filename (no path, device name, or trailing dot).");
  return {
    url: input.url,
    destination: input.destination,
    suggested_filename: input.filename,
    workers: input.workers as 1 | 2 | 4 | 8,
  };
}

export function connectionMessage(error: unknown): string {
  if (!(error instanceof NativeConnectionError))
    return "The manager could not complete this action. Reconnect and check the queue before trying again.";
  if (error.helperCode === "PROTOCOL_UNSUPPORTED_VERSION" || error.failure === "protocol_error")
    return "Extension and helper are incompatible. Install matching versions, then reconnect.";
  if (["AUTH_REQUIRED", "AUTH_EXPIRED"].includes(error.helperCode ?? ""))
    return "Sign in, then explicitly add a fresh download with session handoff. Old partials cannot receive refreshed credentials.";
  if (error.helperCode === "INVALID_SETTINGS")
    return "Settings were not applied. Pause active tasks and check the existing destination, connection caps, and retry limit.";
  if (error.helperCode)
    return `Helper rejected the action (${error.helperCode}). Check the folder and task state before retrying.`;
  return "Native helper unavailable or disconnected. Install the helper, then reconnect. Check the queue before resubmitting: an interrupted command may have succeeded.";
}
