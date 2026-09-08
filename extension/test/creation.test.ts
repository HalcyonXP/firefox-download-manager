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

describe("optional expected SHA-256", () => {
  const valid = {
    url: "https://example.test/file",
    destination: "C:\\Downloads",
    filename: "hash.bin",
    workers: 4,
  };
  it("omits blank values and normalizes a valid expectation", () => {
    expect(creationPayload({ ...valid, checksum: "" }).checksum).toBeUndefined();
    expect(creationPayload({ ...valid, checksum: `  ${"A1".repeat(32)}  ` }).checksum).toEqual({
      algorithm: "sha256",
      digest: "a1".repeat(32),
    });
  });
  it.each(["g".repeat(64), "a".repeat(63), "a".repeat(65), "é".repeat(32)])(
    "rejects malformed digests",
    (checksum) => {
      expect(() => creationPayload({ ...valid, checksum })).toThrow("64 hexadecimal");
    },
  );
});
