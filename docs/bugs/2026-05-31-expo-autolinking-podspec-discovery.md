# Expo Autolinking 无法发现根目录下的 .podspec 文件

日期：2026-05-31

## 现象

自定义 Expo 原生模块 `syncmind-device-identity` 构建成功（`npx expo run:ios` 0 errors），但运行时 `requireOptionalNativeModule("SyncMindDeviceIdentity")` 返回 `null`。

App 打开 Settings 页面时报错：

> SyncMindDeviceIdentity native module is unavailable. Use a development build or rebuilt native app; Expo Go cannot load this local native module.

Pod 已正确通过 CocoaPods 链接（`Podfile.lock` 中有 `SyncMindDeviceIdentity (0.0.1)`），原生 Swift 源码编译无报错，但 JS 侧无法获取到原生模块实例。

## 背景

`syncmind-device-identity` 是 SyncMind 移动端的自定义原生模块，用于通过 iOS Keychain / Android Keystore 管理 Ed25519 设备身份。模块结构：

```
modules/syncmind-device-identity/
  SyncMindDeviceIdentity.podspec    ← 根目录
  expo-module.config.json
  index.js
  package.json
  ios/
    SyncMindDeviceIdentityModule.swift
  android/
    ...
```

`expo-module.config.json` 最初配置：

```json
{
  "platforms": ["apple", "android"],
  "apple": {
    "modules": ["SyncMindDeviceIdentityModule"]
  }
}
```

注意 `podspecPath` 未显式声明。

## 排查过程

1. 确认 `Podfile.lock` 包含 `SyncMindDeviceIdentity` pod，排除 CocoaPods 安装问题。

2. 确认 Xcode 构建 0 errors，排除编译问题。

3. 发现 `ExpoModulesProvider.swift` 是 Expo autolinking 自动生成的 Swift 模块注册文件，其中**缺少** `SyncMindDeviceIdentityModule`：

   ```swift
   // 生成的 ExpoModulesProvider.swift 模块列表中只有标准模块
   (module: CryptoModule.self, name: nil),
   (module: SecureStoreModule.self, name: nil),
   ...
   // 缺少: (module: SyncMindDeviceIdentityModule.self, name: nil),
   ```

4. 手动运行 autolinking resolve 命令确认范围：

   ```bash
   node --no-warnings --eval "require('expo/bin/autolinking')" \
     expo-modules-autolinking resolve --platform ios --json
   ```

   输出 26 个模块，`syncmind-device-identity` 不在其中。

   但 `react-native-config` 命令能找到该模块：

   ```bash
   node --no-warnings --eval "require('expo/bin/autolinking')" \
     expo-modules-autolinking react-native-config --json --platform ios
   ```

   输出中包含 `syncmind-device-identity`。说明模块在 CocoaPods 层面被发现，但在 Expo 模块提供者生成阶段被过滤掉。

5. 追踪 autolinking 代码路径：

   - `findModulesAsync`（`findModules.js:37`）→ 正确找到 28 个模块（包括 `syncmind-device-identity`）
   - `resolveModulesAsync`（`resolveModules.js:8`）→ 解析后只剩 27 个，`syncmind-device-identity` 被丢弃
   - `resolveModuleAsync`（`apple.js:41`）→ 调用 `findPodspecFiles(revision)`，返回空数组 → `return null`

6. 定位到 `findPodspecFiles`（`apple.js:24`）的查找逻辑：

   ```javascript
   async function findPodspecFiles(revision) {
       const configPodspecPaths = revision.config?.applePodspecPaths();
       if (configPodspecPaths && configPodspecPaths.length) {
           return configPodspecPaths;   // 路径 1: config 显式声明
       }
       return await listFilesInDirectories(  // 路径 2: 扫描子目录
           revision.path,
           (basename) => basename.endsWith('.podspec')
       );
   }
   ```

   路径 1 要求 `expo-module.config.json` 中的 `apple.podspecPath` 字段被设置，但初始配置中缺少。

   路径 2 调用 `listFilesInDirectories`（`utils.js`），其实现只扫描**子目录**内的文件：

   ```javascript
   async function listFilesInDirectories(targetPath, filter) {
       return (await Promise.all((await fs.readdir(targetPath, ...))
           .filter((entry) => entry.isDirectory() && entry.name !== 'node_modules')
           //     ^^^^^^^^^^^^^^^^ 只检查目录，不检查根目录下的文件
           ...
       ))).flat(1);
   }
   ```

## 根因

**Expo autolinking 的 `findPodspecFiles` 使用了 `listFilesInDirectories`，该函数仅扫描子目录中的 `.podspec` 文件（匹配标准 Expo 模块结构如 `expo-crypto/ios/ExpoCrypto.podspec`），不会检查模块根目录下的 `.podspec` 文件。**

`syncmind-device-identity` 的 podspec 文件 `SyncMindDeviceIdentity.podspec` 放在模块根目录，不在子目录 `ios/` 中，因此 autolinking 找不到任何 podspec，`resolveModuleAsync` 返回 `null`，模块在 Expo 模块提供者生成阶段被静默丢弃。

标准 Expo 模块（如 `expo-crypto`）的 `.podspec` 在 `ios/` 子目录中，因此不受影响。

## 修复

在 `expo-module.config.json` 中显式声明 `apple.podspecPath`，跳过 `listFilesInDirectories` 的目录扫描：

```json
{
  "platforms": ["apple", "android"],
  "apple": {
    "modules": ["SyncMindDeviceIdentityModule"],
    "podspecPath": "SyncMindDeviceIdentity.podspec",
    "swiftModuleName": "SyncMindDeviceIdentity"
  }
}
```

三个字段的作用：
- `podspecPath` — autolinking 直接使用此路径，不再扫描子目录
- `swiftModuleName` — 指定生成的 `import` 语句中的模块名
- `modules` — 指定要注册的 Swift module 类名

修复后 autolinking resolve 输出 28 个模块，`ExpoModulesProvider.swift` 自动生成了正确的 `SyncMindDeviceIdentityModule` 注册代码。

## 验证

```bash
# 确认 autolinking 发现模块
node --no-warnings --eval "require('expo/bin/autolinking')" \
  expo-modules-autolinking resolve --platform ios --json |
  rg syncmind

# Clean rebuild
npx expo prebuild --platform ios --clean
npx expo run:ios

# 确认 ExpoModulesProvider.swift 包含模块注册
rg "SyncMind" ios/Pods/Target\ Support\ Files/Pods-mobile/ExpoModulesProvider.swift
```

结果：
- autolinking resolve 输出包含 `syncmind-device-identity`（`swiftModuleNames: ["SyncMindDeviceIdentity"]`，`modules: [{"class": "SyncMindDeviceIdentityModule"}]`）
- `ExpoModulesProvider.swift` 包含 `internal import SyncMindDeviceIdentity` 和 `(module: SyncMindDeviceIdentityModule.self, name: nil)`
- 应用启动后 Settings 页面正常显示设备指纹，不再报原生模块不可用错误

## 经验教训

- **自定义 Expo 模块的 `.podspec` 放在根目录时，必须在 `expo-module.config.json` 中显式声明 `podspecPath`。** 原因是 `listFilesInDirectories` 只扫描子目录，不检查根目录文件。
- **`react-native-config` 能找到模块 ≠ resolve 能找到模块。** 前者使用 CocoaPods 的发现机制（扫描所有 `.podspec`），后者使用 Expo 自己的 `listFilesInDirectories`（只扫描子目录）。
- **运行时 `requireOptionalNativeModule` 返回 `null` 而构建日志 0 errors 时，问题在 autolinking 注册阶段**，需要检查 `ExpoModulesProvider.swift` 是否包含目标模块。
- **调试 autolinking 问题时，分三步排查**：① `findModulesAsync` 能否找到（目录扫描层面）② `resolveModuleAsync` 能否解析（podspec/config 层面）③ `ExpoModulesProvider.swift` 是否正确生成（代码生成层面）。
