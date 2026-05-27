export function shouldConfirmBiometricDisable(
  currentValue: boolean,
  nextValue: boolean,
): boolean {
  return currentValue && !nextValue;
}

