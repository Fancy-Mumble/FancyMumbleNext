import { describe, it, expect } from "vitest";
import { maskSensitive } from "../maskSensitive";

describe("maskSensitive", () => {
  it("masks each character in a short string", () => {
    expect(maskSensitive("abc")).toBe("***");
  });

  it("caps mask length at 12 characters", () => {
    expect(maskSensitive("a-very-long-host-name.example.com")).toBe("************");
  });

  it("returns empty string for nullish or empty input", () => {
    expect(maskSensitive(null)).toBe("");
    expect(maskSensitive(undefined)).toBe("");
    expect(maskSensitive("")).toBe("");
  });

  it("masks numeric values", () => {
    expect(maskSensitive(64738)).toBe("*****");
  });
});
