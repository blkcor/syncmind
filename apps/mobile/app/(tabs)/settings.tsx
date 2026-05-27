import { useState, useEffect } from "react";
import { StyleSheet, Switch, Alert } from "react-native";
import { Text, View } from "@/components/Themed";
import { useColorScheme } from "@/components/useColorScheme";
import Colors from "@/constants/Colors";
import {
  ensureIdentity,
  isAuthenticationRequired,
  setAuthenticationRequirement,
  device_reset,
} from "@/src/crypto/identity";
import { shouldConfirmBiometricDisable } from "@/src/settings/security";

export default function SettingsScreen() {
  const colorScheme = useColorScheme();
  const [biometricEnabled, setBiometricEnabled] = useState(false);
  const [fingerprint, setFingerprint] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      try {
        const fp = await ensureIdentity();
        setFingerprint(fp);
        setBiometricEnabled(isAuthenticationRequired());
      } catch {
        setFingerprint(null);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const handleBiometricToggle = async (enabled: boolean) => {
    const applyToggle = async () => {
      try {
        await setAuthenticationRequirement(enabled);
        setBiometricEnabled(enabled);
      } catch {
        Alert.alert(
          "Error",
          "Failed to update biometric protection. Try again.",
        );
      }
    };

    if (shouldConfirmBiometricDisable(biometricEnabled, enabled)) {
      Alert.alert(
        "Disable Biometric Protection",
        "This will re-store your device key without biometric protection.",
        [
          { text: "Cancel", style: "cancel" },
          {
            text: "Disable",
            style: "destructive",
            onPress: () => {
              void applyToggle();
            },
          },
        ],
      );
      return;
    }

    try {
      await applyToggle();
    } catch {}
  };

  const handleDeviceReset = () => {
    Alert.alert(
      "Reset Device",
      "This will clear your device identity, unpair from your desktop, and delete pending captures. This cannot be undone.",
      [
        { text: "Cancel", style: "cancel" },
        {
          text: "Reset",
          style: "destructive",
          onPress: async () => {
            try {
              await device_reset();
              setFingerprint(null);
              setBiometricEnabled(false);
            } catch {
              Alert.alert("Error", "Reset failed. Try again.");
            }
          },
        },
      ],
    );
  };

  if (loading) {
    return (
      <View style={styles.container}>
        <Text>Loading...</Text>
      </View>
    );
  }

  return (
    <View style={styles.container}>
      <Text style={styles.title}>Settings</Text>
      <View style={styles.section}>
        <Text style={styles.sectionTitle}>Device Identity</Text>
        {fingerprint ? (
          <Text style={styles.fingerprint}>
            Fingerprint: {fingerprint.slice(0, 15)}...
          </Text>
        ) : (
          <Text style={styles.muted}>No identity — pair a desktop to begin</Text>
        )}
      </View>

      <View
        style={styles.section}
        lightColor="#eee"
        darkColor="rgba(255,255,255,0.1)"
      >
        <Text style={styles.sectionTitle}>Security</Text>
        <View style={styles.row}>
          <Text style={styles.label}>Biometric Protection</Text>
          <Switch
            value={biometricEnabled}
            onValueChange={handleBiometricToggle}
            disabled={!fingerprint}
            trackColor={{
              true: Colors[colorScheme].tint,
            }}
          />
        </View>
        <Text style={styles.hint}>
          When enabled, Face ID / fingerprint is required to access your device
          key.
        </Text>
      </View>

      <View style={styles.section}>
        <Text
          style={styles.resetButton}
          onPress={handleDeviceReset}
        >
          Reset Device Identity
        </Text>
        <Text style={styles.hint}>
          Clears local keys, unpairs, and deletes pending captures.
        </Text>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    padding: 20,
  },
  title: {
    fontSize: 24,
    fontWeight: "bold",
    marginBottom: 24,
  },
  section: {
    marginBottom: 24,
    padding: 16,
    borderRadius: 12,
  },
  sectionTitle: {
    fontSize: 16,
    fontWeight: "600",
    marginBottom: 8,
  },
  fingerprint: {
    fontSize: 14,
    fontFamily: "monospace",
    opacity: 0.7,
  },
  muted: {
    fontSize: 14,
    fontStyle: "italic",
    opacity: 0.5,
  },
  row: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: 4,
  },
  label: {
    fontSize: 16,
  },
  hint: {
    fontSize: 12,
    opacity: 0.5,
    marginTop: 4,
  },
  resetButton: {
    fontSize: 16,
    color: "#ff4444",
    fontWeight: "600",
  },
});
