/* eslint-disable @typescript-eslint/no-require-imports */

const { patchPodfile, patchXcodeProject } = require(
  "../plugins/with-expo-image-picker-linker-fix.cjs",
);

const configureCall =
  'bash -l -c \\"./Pods/Target\\\\ Support\\\\ Files/Pods-mobile/expo-configure-project.sh\\"\\n';

describe("Expo image picker linker plugin", () => {
  it("does not fail when clean prebuild has not added the Expo configure build phase yet", () => {
    const project = 'shellScript = "echo prebuild-only";';

    expect(patchXcodeProject(project)).toBe(project);
  });

  it("patches the Expo configure build phase when it exists", () => {
    const project = `shellScript = "${configureCall}";`;

    expect(patchXcodeProject(project)).toContain(
      "# SyncMind: patch ExpoImagePicker provider after Expo configure",
    );
    expect(patchXcodeProject(project)).toContain(
      'patch-expo-image-picker-provider.rb\\"',
    );
  });

  it("adds Podfile hooks that patch both provider and Xcode configure phase", () => {
    const podfile = [
      "target 'mobile' do",
      "  post_install do |installer|",
      "    react_native_post_install(",
      "      installer,",
      "      config[:reactNativePath],",
      "    )",
      "  end",
      "end",
      "",
    ].join("\n");

    const patched = patchPodfile(podfile);

    expect(patched).toContain("syncmind_patch_expo_image_picker_provider");
    expect(patched).toContain("syncmind_patch_expo_configure_project_phase");
    expect(patched).toContain("post_integrate do |_installer|");
  });
});
