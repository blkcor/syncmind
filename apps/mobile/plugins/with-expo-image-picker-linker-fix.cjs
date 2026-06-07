/* eslint-disable @typescript-eslint/no-require-imports */

const {
  createRunOncePlugin,
  withDangerousMod,
  withInfoPlist,
  withPodfile,
} = require("expo/config-plugins");
const fs = require("node:fs");
const path = require("node:path");

const PLUGIN_NAME = "with-expo-image-picker-linker-fix";
const PLUGIN_VERSION = "1.0.0";
const HELPER_MARKER = "# SyncMind: ExpoImagePicker linker helper";
const CALL_MARKER = "# SyncMind: apply ExpoImagePicker linker fix";
const INTEGRATE_MARKER =
  "# SyncMind: apply ExpoImagePicker linker fix after integration";
const XCODE_MARKER =
  "# SyncMind: patch ExpoImagePicker provider after Expo configure";
const EXPO_CONFIGURE_PROJECT_SCRIPT = "expo-configure-project.sh";
const IOS_PERMISSIONS = {
  NSCameraUsageDescription:
    "SyncMind uses the camera to scan desktop pairing QR codes and take photo captures you send to your paired desktop.",
  NSPhotoLibraryUsageDescription:
    "SyncMind uses your photo library to pick image captures you send to your paired desktop.",
  NSMicrophoneUsageDescription:
    "SyncMind uses the microphone to record audio captures you send to your paired desktop.",
};

function withExpoImagePickerLinkerFix(config) {
  config = withInfoPlist(config, (config) => {
    Object.assign(config.modResults, IOS_PERMISSIONS);
    return config;
  });

  config = withPodfile(config, (config) => {
    config.modResults.contents = patchPodfile(config.modResults.contents);
    return config;
  });

  return withDangerousMod(config, [
    "ios",
    async (config) => {
      const xcodeProjectPath = getXcodeProjectPath(
        config.modRequest.platformProjectRoot,
      );
      const projectFilePath = path.join(xcodeProjectPath, "project.pbxproj");
      const contents = fs.readFileSync(projectFilePath, "utf8");
      const patched = patchXcodeProject(contents);
      if (patched !== contents) {
        fs.writeFileSync(projectFilePath, patched);
      }
      return config;
    },
  ]);
}

function patchPodfile(contents) {
  let patched = contents;

  if (!patched.includes(HELPER_MARKER)) {
    const targetMarker = "target 'mobile' do";
    if (!patched.includes(targetMarker)) {
      throw new Error(
        `${PLUGIN_NAME} could not find the mobile target in the generated Podfile.`,
      );
    }

    patched = patched.replace(targetMarker, `${buildHelper()}\n${targetMarker}`);
  }

  if (!patched.includes(CALL_MARKER)) {
    const postInstallCall =
      /(\n {4}react_native_post_install\(\n[\s\S]*?\n {4}\)\n)/;
    const match = patched.match(postInstallCall);
    if (!match) {
      throw new Error(
        `${PLUGIN_NAME} could not find react_native_post_install() in the generated Podfile.`,
      );
    }

    patched = patched.replace(match[1], `${match[1]}${buildPostInstallCall()}`);
  }

  if (!patched.includes(INTEGRATE_MARKER)) {
    patched = `${patched.trimEnd()}\n\n${buildPostIntegrateHook()}\n`;
  }

  return patched;
}

function buildHelper() {
  return `${HELPER_MARKER}
def syncmind_patch_expo_image_picker_provider(project_dir)
  provider_path = File.join(
    project_dir,
    'Pods',
    'Target Support Files',
    'Pods-mobile',
    'ExpoModulesProvider.swift'
  )
  patch_script = File.expand_path('../scripts/patch-expo-image-picker-provider.rb', project_dir)
  system('ruby', patch_script, provider_path)
end

def syncmind_patch_expo_configure_project_phase(project_dir)
  project_path = Dir.glob(File.join(project_dir, '*.xcodeproj', 'project.pbxproj')).first
  return unless project_path

  content = File.read(project_path)
  return if content.include?('${XCODE_MARKER}')
  return unless content.include?('${EXPO_CONFIGURE_PROJECT_SCRIPT}')

  patched = content.sub(
    /(${EXPO_CONFIGURE_PROJECT_SCRIPT}.*?\\\\n)/,
    "\\\\1${XCODE_MARKER}\\\\n" \
      "ruby \\\\\\"$SRCROOT\\/..\\/scripts\\/patch-expo-image-picker-provider.rb\\\\\\" " \
      "\\\\\\"$SRCROOT\\/Pods\\/Target Support Files\\/Pods-mobile\\/ExpoModulesProvider.swift\\\\\\"\\\\n"
  )

  File.write(project_path, patched) if patched != content
end
`;
}

function buildPostInstallCall() {
  return `
    ${CALL_MARKER}
    syncmind_patch_expo_image_picker_provider(__dir__)
    syncmind_patch_expo_configure_project_phase(__dir__)
`;
}

function buildPostIntegrateHook() {
  return `post_integrate do |_installer|
  ${INTEGRATE_MARKER}
  syncmind_patch_expo_image_picker_provider(__dir__)
  syncmind_patch_expo_configure_project_phase(__dir__)
end`;
}

function patchXcodeProject(contents) {
  if (contents.includes(XCODE_MARKER)) {
    return contents;
  }

  const configureCall =
    'bash -l -c \\"./Pods/Target\\\\ Support\\\\ Files/Pods-mobile/expo-configure-project.sh\\"\\n';
  if (!contents.includes(configureCall)) {
    return contents;
  }

  return contents.replace(
    configureCall,
    `${configureCall}${XCODE_MARKER}\\n` +
      'ruby \\"$SRCROOT/../scripts/patch-expo-image-picker-provider.rb\\" ' +
      '\\"$SRCROOT/Pods/Target Support Files/Pods-mobile/ExpoModulesProvider.swift\\"\\n',
  );
}

function getXcodeProjectPath(platformProjectRoot) {
  const project = fs
    .readdirSync(platformProjectRoot)
    .find((entry) => entry.endsWith(".xcodeproj"));
  if (!project) {
    throw new Error(`${PLUGIN_NAME} could not find an Xcode project.`);
  }
  return path.join(platformProjectRoot, project);
}

module.exports = createRunOncePlugin(
  withExpoImagePickerLinkerFix,
  PLUGIN_NAME,
  PLUGIN_VERSION,
);
module.exports.patchPodfile = patchPodfile;
module.exports.patchXcodeProject = patchXcodeProject;
