# Mobile Hold 录音按钮触发 iOS Crash 排查复盘

日期：2026-06-06

## 现象

移动端切到 voice mode 后，按住 `Hold` 录音按钮时 App 直接崩溃。

崩溃不是普通 JS 异常：`VoiceRecorder` 里的 `try/catch` 没有机会接住，React error boundary 也不会显示错误页。表现更接近 native 层主动终止进程。

## 背景

US-044 语音捕获使用 Expo SDK 56 的 `expo-audio`：

- `VoiceRecorder` 在 mount 时调用 `setAudioModeAsync({ allowsRecording: true, playsInSilentMode: true })`
- 按下 `Hold` 时走 `handleVoicePressIn`
- `handleVoicePressIn` 创建 audio capture session
- session start 第一阶段调用 `AudioModule.requestRecordingPermissionsAsync()`
- 权限通过后再 `prepareToRecordAsync()` 和 `record({ forDuration: 60 })`

触发路径：

```text
Hold onPressIn
  -> handleVoicePressIn
  -> createAudioCaptureSession().start()
  -> AudioModule.requestRecordingPermissionsAsync()
  -> iOS native microphone permission requester
```

## 排查过程

1. 先检查 `VoiceRecorder` 和 `audio.ts`，确认按钮按下后第一步是申请麦克风权限，而不是直接开始读写文件。

2. 检查 `apps/mobile/app.json`，发现 tracked Expo config 中已经新增：

   ```json
   {
     "ios": {
       "infoPlist": {
         "NSMicrophoneUsageDescription": "SyncMind uses the microphone to record audio captures you send to your paired desktop."
       }
     },
     "plugins": [
       [
         "expo-audio",
         {
           "microphonePermission": "SyncMind uses the microphone to record audio captures you send to your paired desktop.",
           "recordAudioAndroid": true
         }
       ]
     ]
   }
   ```

3. 继续检查本机已经生成的 native iOS 工程，发现 `apps/mobile/ios/mobile/Info.plist` 里没有 `NSMicrophoneUsageDescription`。

   这个目录被 `apps/mobile/.gitignore` 忽略：

   ```text
   /ios
   /android
   ```

   因此 `app.json` 改对了，并不代表当前已安装的 dev build 已经包含新的 Info.plist key。

4. 查本地安装的 `expo-audio` iOS 源码，确认缺少该 key 时会直接 native fatal：

   ```swift
   guard (Bundle.main.infoDictionary?["NSMicrophoneUsageDescription"]) != nil else {
     RCTFatal(RCTErrorWithMessage("""
       This app is missing NSMicrophoneUsageDescription, so audio services will fail.
       Add one of these keys to your bundle's Info.plist.
     """))
     return ["status": EXPermissionStatusDenied]
   }
   ```

5. 跑 `plutil -p apps/mobile/ios/mobile/Info.plist` 复核，修复前没有 `NSMicrophoneUsageDescription`，修复后确认存在。

## 根因

根因是：当前安装到 iOS 的 native build 使用的 `Info.plist` 缺少 `NSMicrophoneUsageDescription`。

`Hold` 按钮按下后会请求麦克风权限。`expo-audio` 的 iOS 权限 requester 在读取权限状态时会先检查 `Bundle.main.infoDictionary["NSMicrophoneUsageDescription"]`。如果缺失，它调用 `RCTFatal`，导致 native 层直接终止 App。

所以这个问题不能只靠 JS 热更新或 Metro reload 修复。`NSMicrophoneUsageDescription` 是 iOS native bundle metadata，必须重新生成/重新安装 native app。

## 修复

修复分为 durable config 和本地 generated native build 两层。

### 1. 持久化 Expo 配置

在 `apps/mobile/app.json` 中声明 iOS microphone usage description：

```json
"ios": {
  "supportsTablet": true,
  "bundleIdentifier": "com.blkcor.syncmind",
  "infoPlist": {
    "NSMicrophoneUsageDescription": "SyncMind uses the microphone to record audio captures you send to your paired desktop."
  }
}
```

同时保留 `expo-audio` config plugin 配置：

```json
[
  "expo-audio",
  {
    "microphonePermission": "SyncMind uses the microphone to record audio captures you send to your paired desktop.",
    "recordAudioAndroid": true
  }
]
```

这保证以后重新 prebuild 或 run native app 时，生成的 iOS app bundle 会带上麦克风权限说明。

### 2. 修复当前本机 generated iOS 工程

本机已有的 `apps/mobile/ios/mobile/Info.plist` 是 ignored generated 文件，但当前 dev build 会使用它。为避免继续安装旧 plist，本地同步补上：

```xml
<key>NSMicrophoneUsageDescription</key>
<string>SyncMind uses the microphone to record audio captures you send to your paired desktop.</string>
```

### 3. 增加回归测试

新增 `apps/mobile/__tests__/app-config.test.ts`，覆盖两件事：

- `expo-audio` plugin 声明了 `microphonePermission`
- `expo.ios.infoPlist.NSMicrophoneUsageDescription` 存在并等于预期文案

这个测试避免之后只配置 Android `RECORD_AUDIO` 或只配置 `expo-audio` plugin，却遗漏 iOS Info.plist。

## 验证

配置验证：

```bash
pnpm --dir apps/mobile exec expo config --json
```

结果中包含：

```json
"ios": {
  "infoPlist": {
    "NSMicrophoneUsageDescription": "SyncMind uses the microphone to record audio captures you send to your paired desktop."
  }
}
```

本机 generated plist 验证：

```bash
plutil -p apps/mobile/ios/mobile/Info.plist
```

结果包含：

```text
"NSMicrophoneUsageDescription" => "SyncMind uses the microphone to record audio captures you send to your paired desktop."
```

测试验证：

```bash
pnpm --dir apps/mobile test -- app-config audio-capture capture-screen
pnpm --dir apps/mobile typecheck
```

结果：

- `app-config` / `audio-capture` / `capture-screen` 共 3 个 test suite、22 个测试通过
- TypeScript typecheck 通过

## 运行侧修复步骤

如果设备或 simulator 上已经安装过旧 build，只跑 Metro 不会修复这个问题：

```bash
pnpm --dir apps/mobile start
```

上面的命令只更新 JS bundle，不会更新 iOS app bundle 里的 `Info.plist`。

需要删除旧 App 或直接重新安装 native build：

```bash
pnpm --dir apps/mobile ios
```

真机测试可用：

```bash
pnpm --dir apps/mobile exec expo run:ios --device
```

重新安装后再按 `Hold`，iOS 应该弹出麦克风权限请求，不应再因为缺少 usage description 直接崩溃。

## 经验教训

- iOS 权限类崩溃要优先检查实际安装 app bundle 的 Info.plist，而不是只看 `app.json`。
- Expo config plugin 的改动需要重新生成/重新安装 native build；Metro reload 只能更新 JS。
- 对被 ignore 的 generated native 目录，tracked source of truth 应该是 `app.json`，但调试当前设备时也要检查 generated plist 是否过期。
- `try/catch` 不能兜住 native `RCTFatal`，看到“按按钮直接退 App”时要沿 native permission/requester 路径查证。
