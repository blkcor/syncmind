import { StyleSheet, TouchableOpacity } from "react-native";
import { router } from "expo-router";

import { Text, View } from "@/components/Themed";
import { useAppStore } from "@/src/store";

export default function TabTwoScreen() {
  const isPaired = useAppStore((s) => s.isPaired);

  if (!isPaired) {
    return (
      <View style={styles.container}>
        <Text style={styles.lockIcon}>🔒</Text>
        <Text style={styles.title}>Graph Locked</Text>
        <Text style={styles.subtitle}>
          Pair with a desktop to search your knowledge
        </Text>
        <TouchableOpacity
          style={styles.button}
          onPress={() => router.navigate("/(tabs)/settings")}
        >
          <Text style={styles.buttonText}>Go to Settings</Text>
        </TouchableOpacity>
      </View>
    );
  }

  return (
    <View style={styles.container}>
      <Text style={styles.title}>Knowledge Graph</Text>
      <View
        style={styles.separator}
        lightColor="#eee"
        darkColor="rgba(255,255,255,0.1)"
      />
      <Text style={styles.placeholder}>
        Knowledge graph visualization coming in Phase 5.
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 24,
    gap: 12,
  },
  lockIcon: {
    fontSize: 40,
    marginBottom: 8,
  },
  title: {
    fontSize: 20,
    fontWeight: "bold",
  },
  subtitle: {
    fontSize: 14,
    opacity: 0.5,
    textAlign: "center",
    marginBottom: 12,
  },
  separator: {
    marginVertical: 30,
    height: 1,
    width: "80%",
  },
  placeholder: {
    fontSize: 14,
    opacity: 0.4,
    textAlign: "center",
  },
  button: {
    borderRadius: 8,
    backgroundColor: "#1f6feb",
    paddingHorizontal: 20,
    paddingVertical: 12,
  },
  buttonText: {
    color: "#fff",
    fontSize: 15,
    fontWeight: "600",
  },
});
