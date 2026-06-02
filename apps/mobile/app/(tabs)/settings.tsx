import { useState, useEffect } from "react";
import {
  StyleSheet,
  Switch,
  Alert,
  ActivityIndicator,
  Platform,
  ScrollView,
} from "react-native";
import { Text, View } from "@/components/Themed";
import { useColorScheme } from "@/components/useColorScheme";
import Colors from "@/constants/Colors";
import {
  ensureIdentity,
  isAuthenticationRequired,
  setAuthenticationRequirement,
  device_reset,
  unpair,
} from "@/src/crypto/identity";
import { shouldConfirmBiometricDisable } from "@/src/settings/security";
import {
  getRestoredPairingState,
  getLastSeenAt,
} from "@/src/spine/session";
import { useAppStore } from "@/src/store";

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

export function fingerprintShort(fp: string): string {
  const prefix = "sha256:";
  const hex = fp.startsWith(prefix) ? fp.slice(prefix.length) : fp;
  if (hex.length <= 12) return fp;
  return `sha256:${hex.slice(0, 8)}…${hex.slice(-4)}`;
}

export function relativeTime(iso: string | null): string {
  if (!iso) return "Never";
  const now = Date.now();
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "Unknown";
  const diffSec = Math.floor((now - then) / 1000);
  if (diffSec < 60) return "Just now";
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < 30) return `${diffDay}d ago`;
  const diffMo = Math.floor(diffDay / 30);
  return `${diffMo}mo ago`;
}

export default function SettingsScreen() {
  const colorScheme = useColorScheme();
  const tint = Colors[colorScheme].tint;
  const isPaired = useAppStore((s) => s.isPaired);
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
        setBiometricEnabled(!enabled);
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

  const handleUnpair = () => {
    const state = getRestoredPairingState();
    const peerFp = state?.pairedPeerFingerprint ?? "the paired desktop";
    Alert.alert(
      "Unpair from Desktop",
      `This will disconnect from ${fingerprintShort(peerFp)}. Your device identity will be preserved so you can re-pair without losing your device ID.`,
      [
        { text: "Cancel", style: "cancel" },
        {
          text: "Unpair",
          style: "destructive",
          onPress: async () => {
            try {
              const result = await unpair();
              if (result.revokeWarning === "network_error") {
                Alert.alert(
                  "Unpaired Locally",
                  "Could not notify desktop — unpaired locally. You can re-pair when the desktop is available.",
                );
              }
            } catch {
              Alert.alert("Error", "Unpair failed. Try again.");
            }
          },
        },
      ],
    );
  };

  const handleDeviceReset = () => {
    Alert.alert(
      "Reset Device",
      "This will destroy your device identity, unpair from your desktop, and delete pending captures. This cannot be undone.",
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
    <ScrollView
      style={[
        styles.container,
        { backgroundColor: Colors[colorScheme].background },
      ]}
      contentContainerStyle={styles.scrollContent}
      showsVerticalScrollIndicator={false}
    >
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

      {/* ── Paired Desktop Card ── */}
      {isPaired && (() => {
        const state = getRestoredPairingState();
        const lastSeen = getLastSeenAt();
        return (
          <View style={styles.card} lightColor="#f8fafc" darkColor="#0f172a">
            <View style={styles.cardHeader}>
              <StatusDot status="active" />
              <Text style={styles.cardTitle}>Paired Desktop</Text>
              <View style={[styles.statusBadge, { backgroundColor: "#22c55e20" }]}>
                <Text style={[styles.statusText, { color: "#22c55e" }]}>
                  {state?.pairedPeerDeviceType ?? "Desktop"}
                </Text>
              </View>
            </View>

            <View style={styles.pairedBody}>
              {state?.pairedPeerFingerprint ? (
                <>
                  <Text style={styles.pairedLabel}>Peer Fingerprint</Text>
                  <View style={styles.fingerprintRow} lightColor="#e2e8f0" darkColor="#1e293b">
                    <Text style={styles.fingerprint} selectable>
                      {state.pairedPeerFingerprint}
                    </Text>
                  </View>
                  <Text style={styles.hint}>
                    {fingerprintShort(state.pairedPeerFingerprint)} &middot; tap to copy full
                  </Text>
                </>
              ) : null}

              <View style={styles.pairedMetaRow}>
                <View style={styles.pairedMetaItem}>
                  <Text style={styles.pairedMetaLabel}>Paired</Text>
                  <Text style={styles.pairedMetaValue}>
                    {state?.pairedAt ? relativeTime(state.pairedAt) : "—"}
                  </Text>
                </View>
                <View style={styles.pairedMetaItem}>
                  <Text style={styles.pairedMetaLabel}>Last Seen</Text>
                  <Text style={styles.pairedMetaValue}>
                    {relativeTime(lastSeen)}
                  </Text>
                </View>
              </View>

              {state?.spineUrl ? (
                <View style={styles.pairedMetaItem}>
                  <Text style={styles.pairedMetaLabel}>Spine</Text>
                  <Text style={styles.pairedMetaValue} numberOfLines={1}>
                    {state.spineUrl}
                  </Text>
                </View>
              ) : null}
            </View>

            <Text
              style={styles.unpairButton}
              onPress={handleUnpair}
            >
              Unpair
            </Text>
            <Text style={styles.unpairHint}>
              Disconnects from this desktop but preserves your device identity for re-pairing.
            </Text>
          </View>
        );
      })()}

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
          Destroys your device identity, unpairs from desktop, and deletes pending captures. Unlike Unpair, this cannot be undone and you will lose your device ID.
        </Text>
      </View>

      {/* ── Footer ── */}
      <Text style={styles.footer}>
        SyncMind &middot; All data stays on device
      </Text>
    </ScrollView>
  );
}

export const settingsStyles = StyleSheet.create({
  container: {
    flex: 1,
  },
  scrollContent: {
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

  /* ── Paired desktop ── */
  pairedBody: {
    gap: 10,
    marginBottom: 8,
  },
  pairedLabel: {
    fontSize: 12,
    fontWeight: "500",
    opacity: 0.35,
    textTransform: "uppercase",
    letterSpacing: 1,
  },
  pairedMetaRow: {
    flexDirection: "row",
    gap: 16,
    marginTop: 4,
  },
  pairedMetaItem: {
    flex: 1,
    gap: 2,
  },
  pairedMetaLabel: {
    fontSize: 11,
    fontWeight: "500",
    opacity: 0.35,
    textTransform: "uppercase",
    letterSpacing: 0.8,
  },
  pairedMetaValue: {
    fontSize: 13,
    fontWeight: "500",
    opacity: 0.7,
  },
  unpairButton: {
    fontSize: 15,
    color: "#ef4444",
    fontWeight: "700",
    marginBottom: 8,
  },
  unpairHint: {
    fontSize: 12,
    opacity: 0.5,
    color: "#ef4444",
    lineHeight: 18,
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

const styles = settingsStyles;
