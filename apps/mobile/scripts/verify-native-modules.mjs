import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const mobileRoot = path.resolve(scriptDir, "..");
const strictGenerated = process.argv.includes("--strict-generated");

const iosPermissionKeys = [
  "NSCameraUsageDescription",
  "NSPhotoLibraryUsageDescription",
  "NSMicrophoneUsageDescription",
];

const androidPermissions = [
  "android.permission.CAMERA",
  "android.permission.RECORD_AUDIO",
];

const requiredDependencies = [
  "expo-image-picker",
  "expo-image-manipulator",
  "expo-camera",
  "expo-audio",
  "syncmind-device-identity",
];

const requiredPlugins = [
  "expo-image-picker",
  "expo-camera",
  "expo-audio",
  "./plugins/with-expo-image-picker-linker-fix.cjs",
];

const requiredIosModules = [
  ["expo-image-picker", "ImagePickerModule"],
  ["expo-image-manipulator", "ImageManipulatorModule"],
  ["expo-camera", "CameraViewModule"],
  ["expo-audio", "AudioModule"],
  ["syncmind-device-identity", "SyncMindDeviceIdentityModule"],
];

const requiredAndroidModules = [
  ["expo-image-picker", "expo.modules.imagepicker.ImagePickerModule"],
  ["expo-image-manipulator", "expo.modules.imagemanipulator.ImageManipulatorModule"],
  ["expo-camera", "expo.modules.camera.CameraViewModule"],
  ["expo-audio", "expo.modules.audio.AudioModule"],
  [
    "syncmind-device-identity",
    "expo.modules.syncminddeviceidentity.SyncMindDeviceIdentityModule",
  ],
];

function main() {
  const packageJson = readJson(path.join(mobileRoot, "package.json"));
  const expoConfig = runJson("pnpm", ["exec", "expo", "config", "--json"]);
  const iosAutolinking = runJson("pnpm", [
    "exec",
    "expo-modules-autolinking",
    "resolve",
    "--platform",
    "ios",
    "--json",
  ]);
  const androidAutolinking = runJson("pnpm", [
    "exec",
    "expo-modules-autolinking",
    "resolve",
    "--platform",
    "android",
    "--json",
  ]);

  verifyPackageJson(packageJson);
  verifyExpoConfig(expoConfig);
  verifyIosAutolinking(iosAutolinking);
  verifyAndroidAutolinking(androidAutolinking);

  if (strictGenerated) {
    verifyGeneratedIosProject();
    verifyGeneratedAndroidProject();
  }

  console.log(
    strictGenerated
      ? "Native module verification passed, including generated native projects."
      : "Native module verification passed.",
  );
}

function verifyPackageJson(packageJson) {
  for (const dependency of requiredDependencies) {
    assert(
      packageJson.dependencies?.[dependency] || packageJson.devDependencies?.[dependency],
      `Missing ${dependency} in apps/mobile/package.json.`,
    );
  }
}

function verifyExpoConfig(config) {
  const pluginNames = new Set(config.plugins.map(getPluginName));

  for (const plugin of requiredPlugins) {
    assert(pluginNames.has(plugin), `Missing Expo config plugin: ${plugin}.`);
  }

  for (const key of iosPermissionKeys) {
    assert(
      typeof config.ios?.infoPlist?.[key] === "string" &&
        config.ios.infoPlist[key].length > 0,
      `Missing iOS permission string in Expo config: ${key}.`,
    );
  }

  for (const permission of androidPermissions) {
    assert(
      config.android?.permissions?.includes(permission),
      `Missing Android permission in Expo config: ${permission}.`,
    );
  }
}

function verifyIosAutolinking(result) {
  for (const [packageName, className] of requiredIosModules) {
    const module = findAutolinkedPackage(result, packageName);
    assert(module, `iOS autolinking did not resolve ${packageName}.`);
    assert(
      module.modules?.some((candidate) => candidate.class === className),
      `iOS autolinking resolved ${packageName} without ${className}.`,
    );
  }
}

function verifyAndroidAutolinking(result) {
  for (const [packageName, classifier] of requiredAndroidModules) {
    const module = findAutolinkedPackage(result, packageName);
    assert(module, `Android autolinking did not resolve ${packageName}.`);
    assert(
      module.projects?.some((project) =>
        project.modules?.some((candidate) => candidate.classifier === classifier),
      ),
      `Android autolinking resolved ${packageName} without ${classifier}.`,
    );
  }
}

function verifyGeneratedIosProject() {
  const iosRoot = path.join(mobileRoot, "ios");
  assert(fs.existsSync(iosRoot), "Missing generated apps/mobile/ios directory.");
  readRequiredText(path.join(mobileRoot, "scripts", "patch-expo-image-picker-provider.rb"));

  const podfile = readRequiredText(path.join(iosRoot, "Podfile"));
  assertIncludes(podfile, "syncmind_patch_expo_image_picker_provider", "iOS Podfile");
  assertIncludes(podfile, "# SyncMind: apply ExpoImagePicker linker fix", "iOS Podfile");
  assertIncludes(
    podfile,
    "# SyncMind: apply ExpoImagePicker linker fix after integration",
    "iOS Podfile",
  );

  const projectFile = readRequiredText(
    path.join(iosRoot, "mobile.xcodeproj", "project.pbxproj"),
  );
  assertIncludes(
    projectFile,
    "# SyncMind: patch ExpoImagePicker provider after Expo configure",
    "iOS Xcode project",
  );

  const podfileLock = readRequiredText(path.join(iosRoot, "Podfile.lock"));
  assertIncludes(podfileLock, "ExpoImagePicker", "iOS Podfile.lock");
  assertIncludes(podfileLock, "ExpoImageManipulator", "iOS Podfile.lock");

  const infoPlist = readRequiredText(path.join(iosRoot, "mobile", "Info.plist"));
  for (const key of iosPermissionKeys) {
    assertIncludes(infoPlist, key, "iOS Info.plist");
  }

  const provider = readRequiredText(
    path.join(
      iosRoot,
      "Pods",
      "Target Support Files",
      "Pods-mobile",
      "ExpoModulesProvider.swift",
    ),
  );
  assertIncludes(provider, "internal import ExpoImagePicker", "ExpoModulesProvider.swift");
  assertIncludes(provider, "ImagePickerModule.self", "ExpoModulesProvider.swift");
  assertIncludes(
    provider,
    "// SyncMind: force conformance reference",
    "ExpoModulesProvider.swift",
  );
}

function verifyGeneratedAndroidProject() {
  const androidRoot = path.join(mobileRoot, "android");
  assert(fs.existsSync(androidRoot), "Missing generated apps/mobile/android directory.");

  const manifest = readRequiredText(
    path.join(androidRoot, "app", "src", "main", "AndroidManifest.xml"),
  );
  for (const permission of androidPermissions) {
    assertIncludes(manifest, permission, "AndroidManifest.xml");
  }
}

function findAutolinkedPackage(result, packageName) {
  return result.modules?.find((module) => module.packageName === packageName);
}

function getPluginName(plugin) {
  return Array.isArray(plugin) ? plugin[0] : plugin;
}

function runJson(command, args) {
  const output = run(command, args);
  const firstJsonChar = output.indexOf("{");
  assert(firstJsonChar >= 0, `${command} ${args.join(" ")} did not print JSON.`);
  return JSON.parse(output.slice(firstJsonChar));
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: mobileRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      EXPO_NO_TELEMETRY: "1",
    },
  });

  if (result.error) {
    throw result.error;
  }

  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed with exit code ${result.status}.\n` +
        `${result.stdout}${result.stderr}`,
    );
  }

  return result.stdout;
}

function readJson(filePath) {
  return JSON.parse(readRequiredText(filePath));
}

function readRequiredText(filePath) {
  assert(fs.existsSync(filePath), `Missing file: ${relative(filePath)}.`);
  return fs.readFileSync(filePath, "utf8");
}

function assertIncludes(content, expected, label) {
  assert(content.includes(expected), `${label} does not include ${expected}.`);
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function relative(filePath) {
  return path.relative(mobileRoot, filePath);
}

try {
  main();
} catch (error) {
  console.error("Native module verification failed.");
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
