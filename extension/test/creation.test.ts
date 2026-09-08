import { describe, expect, it } from "vitest";
import {
  connectionMessage,
  creationPayload,
  directUrl,
  safeFilename,
  suggestedFilename,
} from "../src/creation";
import { NativeConnectionError } from "../src/native-connection";

describe("explicit creation boundary", () => {
  it.each([
    "ftp://example.test/x",
    "javascript:alert(1)",
    "file:///C:/test",
    "https://u:p@example.test/x",
    "https://example.test/\nprivate",
    "/relative",
  ])("rejects %s", (url) => {
    expect(() => directUrl(url)).toThrow();
  });
  it("keeps signed URL bytes unchanged and never uses queries as names", () => {
    const url = "https://example.test/a%20b.zip?sig=a%2Fb%2BC&x=2&x=1";
    expect(
      creationPayload({ url, filename: "a b.zip", destination: "C:\\Downloads", workers: 4 }).url,
    ).toBe(url);
    expect(suggestedFilename(url)).toBe("a b.zip");
  });
  it.each([
    ["../CON", ".._CON"],
    ["NUL.zip", "_NUL.zip"],
    ["a:stream", "a_stream"],
    ["..", "download"],
    ["a. ", "a"],
  ])("sanitizes %s", (input, expected) => {
    expect(safeFilename(input!)).toBe(expected);
  });
  it("rejects invalid destinations, filenames, and workers before submission", () => {
    const valid = {
      url: "https://example.test/a",
      destination: "C:\\Downloads",
      filename: "a",
      workers: 4,
    };
    for (const patch of [{ destination: "../escape" }, { filename: "a:ads" }, { workers: 3 }])
      expect(() => creationPayload({ ...valid, ...patch })).toThrow();
  });
  it("distinguishes incompatible versions and uncertain native availability without echo", () => {
    expect(
      connectionMessage(new NativeConnectionError("helper_error", "PROTOCOL_UNSUPPORTED_VERSION")),
    ).toContain("matching versions");
    expect(connectionMessage(new NativeConnectionError("disconnected"))).toContain(
      "may have succeeded",
    );
    expect(connectionMessage(new Error("cookie=secret"))).not.toContain("secret");
  });
});
