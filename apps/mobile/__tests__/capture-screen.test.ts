import fs from "node:fs";
import path from "node:path";

describe("Capture screen send behavior", () => {
  it("uses Expo Crypto.randomUUID for capture ids instead of browser global crypto", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("from 'expo-crypto'");
    expect(source).toContain("Crypto.randomUUID()");
    expect(source).not.toContain("crypto.randomUUID()");
  });

  it("wires recent outbox status metadata into the paired capture screen", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("getRecentOutboxStatuses");
    expect(source).toContain("subscribeToOutboxChanges");
    expect(source).toContain("Recent sends");
    expect(source).toContain("outboxStatusRows");
    expect(source).not.toContain("encrypted_blob");
  });
});
