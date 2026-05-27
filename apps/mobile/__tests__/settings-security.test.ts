import { shouldConfirmBiometricDisable } from "../src/settings/security";

describe("shouldConfirmBiometricDisable", () => {
  it("returns true when disabling an already enabled biometric setting", () => {
    expect(shouldConfirmBiometricDisable(true, false)).toBe(true);
  });

  it("returns false when enabling biometric protection", () => {
    expect(shouldConfirmBiometricDisable(false, true)).toBe(false);
  });

  it("returns false when the value does not change", () => {
    expect(shouldConfirmBiometricDisable(true, true)).toBe(false);
    expect(shouldConfirmBiometricDisable(false, false)).toBe(false);
  });
});
