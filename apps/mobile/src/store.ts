import { create } from "zustand";

export interface AppState {
  isPaired: boolean;
  isFirstPairing: boolean;
  peerDeviceFingerprint: string | null;
  connectionStatus: "disconnected" | "connecting" | "connected" | "error";
  showFirstCaptureGuide: boolean;
}

interface AppActions {
  setPaired: (fingerprint: string, isFirstPairing?: boolean) => void;
  dismissFirstCaptureGuide: () => void;
  setUnpaired: () => void;
  setConnectionStatus: (status: AppState["connectionStatus"]) => void;
  reset: () => void;
}

const initialState: AppState = {
  isPaired: false,
  isFirstPairing: false,
  peerDeviceFingerprint: null,
  connectionStatus: "disconnected",
  showFirstCaptureGuide: false,
};

export const useAppStore = create<AppState & AppActions>()((set) => ({
  ...initialState,
  setPaired: (fingerprint, isFirstPairing = false) =>
    set({
      isPaired: true,
      isFirstPairing,
      peerDeviceFingerprint: fingerprint,
      connectionStatus: "connected",
      showFirstCaptureGuide: isFirstPairing,
    }),
  dismissFirstCaptureGuide: () => set({ showFirstCaptureGuide: false }),
  setUnpaired: () =>
    set({
      isPaired: false,
      isFirstPairing: false,
      peerDeviceFingerprint: null,
      connectionStatus: "disconnected",
      showFirstCaptureGuide: false,
    }),
  setConnectionStatus: (status) => set({ connectionStatus: status }),
  reset: () => set(initialState),
}));
