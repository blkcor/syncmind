provider_path = ARGV.fetch(0)

unless File.exist?(provider_path)
  warn "  [ExpoImagePicker fix] Missing ExpoModulesProvider.swift at #{provider_path}"
  exit 0
end

content = File.read(provider_path)
exit 0 if content.include?('// SyncMind: force conformance reference')

unless content.include?('ImagePickerModule.self')
  warn '  [ExpoImagePicker fix] ExpoModulesProvider.swift does not reference ImagePickerModule'
  exit 0
end

sentinel = <<~SWIFT

  // SyncMind: force conformance reference to prevent linker dead-strip.
  // This is never executed; it only exists to emit a linker reference.
  private func _syncmind_forceImagePickerConformance() -> [any Module.Type] {
    var modules: [any Module.Type] = []
    if #available(iOS 9999, *) {
      modules.append(ImagePickerModule.self)
    }
    return modules
  }
SWIFT

patched = content.sub(
  '@objc(ExpoModulesProvider)',
  "#{sentinel}\n@objc(ExpoModulesProvider)",
)
exit 0 if patched == content

File.write(provider_path, patched)
puts '  [ExpoImagePicker fix] Patched ExpoModulesProvider.swift with conformance reference'
