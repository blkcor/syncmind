import { create } from "zustand";

export interface AppState {
  isPaired: boolean;
  peerDeviceFingerprint: string | null;
  connectionStatus: "disconnected" | "connecting" | "connected" | "error";
}

interface AppActions {
  setPaired: (fingerprint: string) => void;
  setUnpaired: () => void;
  setConnectionStatus: (status: AppState["connectionStatus"]) => void;
  reset: () => void;
}

const initialState: AppState = {
  isPaired: false,
  peerDeviceFingerprint: null,
  connectionStatus: "disconnected",
};

export const useAppStore = create<AppState & AppActions>()((set) => ({
  ...initialState,
  setPaired: (fingerprint) =>
    set({ isPaired: true, peerDeviceFingerprint: fingerprint }),
  setUnpaired: () =>
    set({ isPaired: false, peerDeviceFingerprint: null, connectionStatus: "disconnected" }),
  setConnectionStatus: (status) => set({ connectionStatus: status }),
  reset: () => set(initialState),
}));
