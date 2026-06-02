import { SymbolView } from 'expo-symbols';
import { Link, Tabs } from 'expo-router';
import { Pressable } from 'react-native';

import Colors from '@/constants/Colors';
import { useColorScheme } from '@/components/useColorScheme';
import { useClientOnlyValue } from '@/components/useClientOnlyValue';
import { useAppStore } from '@/src/store';

export default function TabLayout() {
  const colorScheme = useColorScheme();
  const isPaired = useAppStore((s) => s.isPaired);

  return (
    <Tabs
      screenOptions={{
        tabBarActiveTintColor: Colors[colorScheme].tint,
        headerShown: useClientOnlyValue(false, true),
      }}>
      <Tabs.Screen
        name="index"
        options={{
          title: 'Capture',
          tabBarIcon: ({ color }) => (
            <SymbolView
              name={{
                ios: 'square.and.pencil',
                android: 'edit',
                web: 'edit',
              }}
              tintColor={color}
              size={28}
            />
          ),
          headerRight: () => (
            <Link href="/modal" asChild>
              <Pressable style={{ marginRight: 15 }}>
                {({ pressed }) => (
                  <SymbolView
                    name={{ ios: 'info.circle', android: 'info', web: 'info' }}
                    size={25}
                    tintColor={Colors[colorScheme].text}
                    style={{ opacity: pressed ? 0.5 : 1 }}
                  />
                )}
              </Pressable>
            </Link>
          ),
        }}
      />
      <Tabs.Screen
        name="two"
        options={{
          title: isPaired ? 'Graph' : 'Graph (locked)',
          tabBarIcon: ({ color }) => (
            <SymbolView
              name={{
                ios: isPaired
                  ? 'chevron.left.forwardslash.chevron.right'
                  : 'lock.fill',
                android: isPaired ? 'code' : 'lock',
                web: isPaired ? 'code' : 'lock',
              }}
              tintColor={color}
              size={28}
            />
          ),
          tabBarItemStyle: isPaired ? undefined : { opacity: 0.4 },
          ...(isPaired
            ? {}
            : {
                listeners: {
                  tabPress: (e: { preventDefault: () => void }) => {
                    e.preventDefault();
                  },
                },
              }),
        }}
      />
      <Tabs.Screen
        name="settings"
        options={{
          title: 'Settings',
          tabBarIcon: ({ color }) => (
            <SymbolView
              name={{ ios: 'gearshape', android: 'settings', web: 'settings' }}
              tintColor={color}
              size={28}
            />
          ),
        }}
      />
    </Tabs>
  );
}
