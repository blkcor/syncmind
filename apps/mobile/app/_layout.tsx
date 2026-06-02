import { useFonts } from 'expo-font';
import { DarkTheme, DefaultTheme, Stack, ThemeProvider } from 'expo-router';
import * as SplashScreen from 'expo-splash-screen';
import { useEffect, useState } from 'react';
import 'react-native-reanimated';

import { useColorScheme } from '@/components/useColorScheme';
import { checkCurrentDevicePairing, UnpairedError } from '@/src/spine/client';
import { restorePairingState } from '@/src/spine/session';
import { useAppStore } from '@/src/store';
import { ensureIdentity } from '@/src/crypto/identity';

const PAIRING_HEALTH_CHECK_MS = 1500;

export {
  // Catch any errors thrown by the Layout component.
  ErrorBoundary,
} from 'expo-router';

export const unstable_settings = {
  // Ensure that reloading on `/modal` keeps a back button present.
  initialRouteName: '(tabs)',
};

// Prevent the splash screen from auto-hiding before asset loading is complete.
SplashScreen.preventAutoHideAsync();

export default function RootLayout() {
  const [pairingRestored, setPairingRestored] = useState(false);
  const [loaded, error] = useFonts({
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    SpaceMono: require('../assets/fonts/SpaceMono-Regular.ttf'),
  });

  // Expo Router uses Error Boundaries to catch errors in the navigation tree.
  useEffect(() => {
    if (error) throw error;
  }, [error]);

  useEffect(() => {
    if (loaded) {
      SplashScreen.hideAsync();
    }
  }, [loaded]);

  useEffect(() => {
    if (!loaded) {
      return;
    }

    let cancelled = false;

    async function restore() {
      try {
        const state = await restorePairingState();
        if (cancelled) return;

        if (state) {
          useAppStore.getState().setPaired(state.pairedPeerFingerprint, false);

          // Startup health check: verify device is still valid on Spine.
          try {
            await ensureIdentity();
            await checkCurrentDevicePairing({ allowJwtMint: true });
            if (!cancelled) {
              useAppStore.getState().setConnectionStatus("connected");
            }
          } catch (err) {
            if (err instanceof UnpairedError) {
              // authenticatedFetch already cleared pairing state and set unpaired.
            } else if (!cancelled) {
              useAppStore.getState().setConnectionStatus("error");
            }
          }
        } else {
          useAppStore.getState().setUnpaired();
        }
      } catch {
        if (!cancelled) {
          useAppStore.getState().setUnpaired();
        }
      } finally {
        if (!cancelled) {
          setPairingRestored(true);
        }
      }
    }

    restore();

    return () => {
      cancelled = true;
    };
  }, [loaded]);

  useEffect(() => {
    if (!loaded || !pairingRestored) {
      return;
    }

    const timer = setInterval(() => {
      if (!useAppStore.getState().isPaired) {
        return;
      }

      void checkCurrentDevicePairing()
        .then(() => {
          useAppStore.getState().setConnectionStatus("connected");
        })
        .catch((err) => {
          if (err instanceof UnpairedError) {
            return;
          }
          useAppStore.getState().setConnectionStatus("error");
        });
    }, PAIRING_HEALTH_CHECK_MS);

    return () => clearInterval(timer);
  }, [loaded, pairingRestored]);

  if (!loaded || !pairingRestored) {
    return null;
  }

  return <RootLayoutNav />;
}

function RootLayoutNav() {
  const colorScheme = useColorScheme();

  return (
    <ThemeProvider value={colorScheme === 'dark' ? DarkTheme : DefaultTheme}>
      <Stack>
        <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
        <Stack.Screen name="modal" options={{ presentation: 'modal' }} />
      </Stack>
    </ThemeProvider>
  );
}
