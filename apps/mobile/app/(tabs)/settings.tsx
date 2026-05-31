import { useState, useEffect } from "react";
import {
  StyleSheet,
  Switch,
  Alert,
  ActivityIndicator,
  Platform,
} from "react-native";
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

type IdentityStatus = "loading" | "active" | "unpaired" | "error";

function StatusDot({ status }: { status: IdentityStatus }) {
  const color =
    status === "active"
      ? "#22c55e"
      : status === "error"
        ? "#ef4444"
        : status === "loading"
          ? "#94a3b8"
          : "#f59e0b";

  return (
    <View style={[statusDotStyles.dot, { backgroundColor: color }]}>
      {status === "loading" && (
        <ActivityIndicator size={6} color="#fff" style={{ marginTop: -1 }} />
      )}
    </View>
  );
}

const statusDotStyles = StyleSheet.create({
  dot: {
    width: 8,
    height: 8,
    borderRadius: 4,
    marginRight: 8,
  },
});

export default function SettingsScreen() {
  const colorScheme = useColorScheme();
  const tint = Colors[colorScheme].tint;
  const [biometricEnabled, setBiometricEnabled] = useState<boolean | null>(null);
  const [fingerprint, setFingerprint] = useState<string | null>(null);
  const [identityError, setIdentityError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      try {
        const fp = await ensureIdentity();
        setFingerprint(fp);
        setBiometricEnabled(isAuthenticationRequired());
        setIdentityError(null);
      } catch (error) {
        setFingerprint(null);
        setIdentityError(
          error instanceof Error
            ? error.message
            : "Device identity is unavailable.",
        );
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const identityStatus: IdentityStatus = loading
    ? "loading"
    : identityError
      ? "error"
      : fingerprint
        ? "active"
        : "unpaired";

  const handleBiometricToggle = async (enabled: boolean) => {
    if (biometricEnabled === null) return;

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

  return (
    <View style={styles.container}>
      {/* ── Header ── */}
      <View style={styles.header}>
        <Text style={styles.headerTitle}>Settings</Text>
        <Text style={styles.headerSubtitle}>Device identity &amp; security</Text>
      </View>

      {/* ── Device Identity Card ── */}
      <View style={styles.card} lightColor="#f8fafc" darkColor="#0f172a">
        <View style={styles.cardHeader}>
          <StatusDot status={identityStatus} />
          <Text style={styles.cardTitle}>Device Identity</Text>
          <View style={styles.statusBadge} lightColor="#e2e8f0" darkColor="#1e293b">
            <Text
              style={[
                styles.statusText,
                {
                  color:
                    identityStatus === "active"
                      ? "#22c55e"
                      : identityStatus === "error"
                        ? "#ef4444"
                        : identityStatus === "unpaired"
                          ? "#f59e0b"
                          : "#94a3b8",
                },
              ]}
            >
              {identityStatus === "active"
                ? "Active"
                : identityStatus === "error"
                  ? "Error"
                  : identityStatus === "unpaired"
                    ? "Unpaired"
                    : "Checking"}
            </Text>
          </View>
        </View>

        {loading ? (
          <View style={styles.identityBody}>
            <View style={styles.skeletonLine} lightColor="#e2e8f0" darkColor="#334155" />
            <View
              style={[styles.skeletonLine, { width: "60%" }]}
              lightColor="#e2e8f0"
              darkColor="#334155"
            />
          </View>
        ) : identityError ? (
          <View style={styles.identityBody}>
            <Text style={styles.errorText}>{identityError}</Text>
          </View>
        ) : fingerprint ? (
          <View style={styles.identityBody}>
            <Text style={styles.fingerprintLabel}>Fingerprint</Text>
            <View style={styles.fingerprintRow} lightColor="#e2e8f0" darkColor="#1e293b">
              <Text style={styles.fingerprint} selectable>
                {fingerprint}
              </Text>
            </View>
            <Text style={styles.hint}>
              Ed25519 &middot; SHA-256 &middot; stored in{" "}
              {Platform.OS === "ios" ? "Keychain" : "Keystore"}
            </Text>
          </View>
        ) : (
          <View style={styles.identityBody}>
            <Text style={styles.muted}>
              No device identity. Pair with a desktop to get started.
            </Text>
          </View>
        )}
      </View>

      {/* ── Security Card ── */}
      <View style={styles.card} lightColor="#f8fafc" darkColor="#0f172a">
        <View style={styles.cardHeader}>
          <Text style={styles.cardTitle}>Security</Text>
        </View>

        <View style={styles.row}>
          <View style={styles.rowLabel}>
            <Text style={styles.rowTitle}>
              Biometric Protection
            </Text>
            <Text style={styles.rowHint}>
              {biometricEnabled === null
                ? "Loading security settings..."
                : biometricEnabled
                  ? "Requires Face ID or fingerprint to unlock your device key"
                  : "Device key accessible without biometric check"}
            </Text>
          </View>
          {biometricEnabled !== null && (
            <Switch
              value={biometricEnabled}
              onValueChange={handleBiometricToggle}
              disabled={!fingerprint}
              trackColor={{ false: "#94a3b8", true: tint }}
              ios_backgroundColor="#94a3b8"
            />
          )}
        </View>
      </View>

      {/* ── Danger Zone ── */}
      <View style={styles.card} lightColor="#fef2f2" darkColor="#1a0f0f">
        <View style={styles.cardHeader}>
          <Text style={[styles.cardTitle, { color: "#ef4444" }]}>
            Danger Zone
          </Text>
        </View>

        <Text
          style={styles.resetButton}
          onPress={handleDeviceReset}
        >
          Reset Device Identity
        </Text>
        <Text style={styles.resetHint}>
          Clears local keys, unpairs from desktop, and deletes pending captures.
          This action cannot be undone.
        </Text>
      </View>

      {/* ── Footer ── */}
      <Text style={styles.footer}>
        SyncMind &middot; All data stays on device
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    paddingHorizontal: 20,
    paddingTop: 60,
    paddingBottom: 40,
  },

  /* ── Header ── */
  header: {
    marginBottom: 28,
    paddingHorizontal: 4,
  },
  headerTitle: {
    fontSize: 32,
    fontWeight: "700",
    letterSpacing: -0.5,
    marginBottom: 4,
  },
  headerSubtitle: {
    fontSize: 15,
    opacity: 0.45,
    fontWeight: "400",
  },

  /* ── Card ── */
  card: {
    borderRadius: 16,
    padding: 20,
    marginBottom: 16,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: "rgba(148,163,184,0.2)",
  },
  cardHeader: {
    flexDirection: "row",
    alignItems: "center",
    marginBottom: 16,
  },
  cardTitle: {
    fontSize: 13,
    fontWeight: "600",
    letterSpacing: 0.8,
    textTransform: "uppercase",
    opacity: 0.5,
    flex: 1,
  },
  statusBadge: {
    borderRadius: 6,
    paddingHorizontal: 10,
    paddingVertical: 4,
  },
  statusText: {
    fontSize: 11,
    fontWeight: "700",
    letterSpacing: 0.4,
    textTransform: "uppercase",
  },

  /* ── Identity body ── */
  identityBody: {
    gap: 10,
  },
  fingerprintLabel: {
    fontSize: 12,
    fontWeight: "500",
    opacity: 0.35,
    textTransform: "uppercase",
    letterSpacing: 1,
  },
  fingerprintRow: {
    borderRadius: 10,
    paddingHorizontal: 14,
    paddingVertical: 14,
  },
  fingerprint: {
    fontSize: 12,
    fontFamily: Platform.OS === "ios" ? "Menlo" : "monospace",
    lineHeight: 20,
    opacity: 0.8,
  },
  errorText: {
    fontSize: 14,
    color: "#ef4444",
    lineHeight: 22,
    fontWeight: "500",
  },
  muted: {
    fontSize: 14,
    opacity: 0.4,
    lineHeight: 22,
  },
  hint: {
    fontSize: 11,
    opacity: 0.3,
    marginTop: 2,
  },

  /* ── Skeleton ── */
  skeletonLine: {
    height: 14,
    borderRadius: 7,
    opacity: 0.4,
  },

  /* ── Security row ── */
  row: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 16,
  },
  rowLabel: {
    flex: 1,
    gap: 4,
  },
  rowTitle: {
    fontSize: 15,
    fontWeight: "600",
  },
  rowHint: {
    fontSize: 12,
    opacity: 0.4,
    lineHeight: 17,
  },

  /* ── Danger zone ── */
  resetButton: {
    fontSize: 15,
    color: "#ef4444",
    fontWeight: "700",
    marginBottom: 8,
  },
  resetHint: {
    fontSize: 12,
    opacity: 0.5,
    color: "#ef4444",
    lineHeight: 18,
  },

  /* ── Footer ── */
  footer: {
    textAlign: "center",
    fontSize: 11,
    opacity: 0.2,
    marginTop: 40,
    letterSpacing: 0.3,
  },
});
