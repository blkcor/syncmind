import { CameraView, useCameraPermissions, type BarcodeScanningResult } from "expo-camera";
import * as Linking from "expo-linking";
import { useCallback, useEffect, useState } from "react";
import {
  ActivityIndicator,
  Button,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { startPairingFlow, pairingFlowErrorMessage } from ".";
import {
  PairingPayloadError,
  parsePairingPayload,
  validatePairingPayload,
  validationErrorMessage,
} from "./payload";

interface PairingScannerProps {
  onPaired?: () => void;
}

export function PairingScanner({ onPaired }: PairingScannerProps) {
  const [permission, requestPermission] = useCameraPermissions();
  const [manualPayload, setManualPayload] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isPairing, setIsPairing] = useState(false);
  const [didScan, setDidScan] = useState(false);

  useEffect(() => {
    if (!permission) {
      void requestPermission();
    }
  }, [permission, requestPermission]);

  const submitPayload = useCallback(
    async (input: string) => {
      setError(null);
      setIsPairing(true);
      try {
        const payload = parsePairingPayload(input);
        const validation = validatePairingPayload(payload, {
          allowHttp: Boolean(__DEV__),
        });
        if (validation) {
          throw new PairingPayloadError(validation);
        }
        await startPairingFlow(payload);
        onPaired?.();
      } catch (caught) {
        setDidScan(false);
        if (caught instanceof PairingPayloadError) {
          setError(validationErrorMessage(caught));
        } else {
          setError(pairingFlowErrorMessage(caught));
        }
      } finally {
        setIsPairing(false);
      }
    },
    [onPaired],
  );

  const handleScan = useCallback(
    ({ data }: BarcodeScanningResult) => {
      if (didScan || isPairing) {
        return;
      }
      setDidScan(true);
      void submitPayload(data);
    },
    [didScan, isPairing, submitPayload],
  );

  if (!permission) {
    return (
      <View style={styles.center}>
        <ActivityIndicator />
      </View>
    );
  }

  if (!permission.granted) {
    return (
      <ManualPayloadFallback
        blocked={!permission.canAskAgain}
        error={error}
        isPairing={isPairing}
        manualPayload={manualPayload}
        onChangePayload={setManualPayload}
        onOpenSettings={() => {
          void Linking.openSettings();
        }}
        onRequestPermission={() => {
          void requestPermission();
        }}
        onSubmit={() => {
          void submitPayload(manualPayload);
        }}
      />
    );
  }

  return (
    <View style={styles.container}>
      <CameraView
        style={styles.camera}
        facing="back"
        barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
        onBarcodeScanned={didScan || isPairing ? undefined : handleScan}
      />
      <View style={styles.overlay}>
        {isPairing ? <ActivityIndicator color="#fff" /> : null}
        {error ? <Text style={styles.error}>{error}</Text> : null}
      </View>
    </View>
  );
}

function ManualPayloadFallback({
  blocked,
  error,
  isPairing,
  manualPayload,
  onChangePayload,
  onOpenSettings,
  onRequestPermission,
  onSubmit,
}: {
  blocked: boolean;
  error: string | null;
  isPairing: boolean;
  manualPayload: string;
  onChangePayload: (value: string) => void;
  onOpenSettings: () => void;
  onRequestPermission: () => void;
  onSubmit: () => void;
}) {
  return (
    <View style={styles.fallback}>
      {blocked ? (
        <>
          <Text style={styles.title}>Camera access is blocked</Text>
          <Button title="Open Settings" onPress={onOpenSettings} />
        </>
      ) : (
        <Button title="Allow Camera" onPress={onRequestPermission} />
      )}
      <TextInput
        accessibilityLabel="Paste pairing payload"
        autoCapitalize="none"
        autoCorrect={false}
        multiline
        onChangeText={onChangePayload}
        placeholder="Paste pairing payload"
        style={styles.input}
        value={manualPayload}
      />
      <Button title={isPairing ? "Pairing..." : "Submit"} onPress={onSubmit} />
      {error ? <Text style={styles.error}>{error}</Text> : null}
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: "#111",
  },
  camera: {
    flex: 1,
  },
  center: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
  },
  overlay: {
    position: "absolute",
    left: 16,
    right: 16,
    bottom: 32,
    gap: 12,
  },
  fallback: {
    flex: 1,
    justifyContent: "center",
    padding: 20,
    gap: 16,
  },
  title: {
    fontSize: 18,
    fontWeight: "600",
    textAlign: "center",
  },
  input: {
    minHeight: 160,
    borderWidth: 1,
    borderColor: "#8a8f98",
    borderRadius: 6,
    padding: 12,
    textAlignVertical: "top",
  },
  error: {
    color: "#b00020",
    fontSize: 14,
    textAlign: "center",
  },
});
